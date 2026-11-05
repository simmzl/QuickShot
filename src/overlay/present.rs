//! Frame presentation for the overlay window.
//!
//! macOS: the softbuffer CoreGraphics backend allocates a fresh full-screen
//! `Vec<u32>` every frame and hands it to CoreAnimation wrapped in a new
//! `CGImage`. On a 6016×3384 Retina screen that costs ~40 ms per frame —
//! drags were capped at ~25 fps and felt laggy. Here we render straight into
//! one of two `IOSurface`s and set it as the layer contents, which is
//! zero-copy for the compositor (present ≈ 0.02 ms).
//!
//! Other platforms keep using softbuffer.

use anyhow::Result;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use winit::window::Window;

pub struct Presenter {
    inner: imp::Presenter,
}

impl Presenter {
    pub fn new(window: Rc<Window>) -> Result<Self> {
        Ok(Self { inner: imp::Presenter::new(window)? })
    }

    /// Begin a frame of `w × h` physical pixels. The returned buffer's
    /// contents are undefined (callers repaint every pixel), layout is
    /// row-major `0x00RRGGBB`, stride == `w`.
    pub fn frame(&mut self, w: u32, h: u32) -> Result<Frame<'_>> {
        Ok(Frame { inner: self.inner.frame(w, h)? })
    }
}

pub struct Frame<'a> {
    inner: imp::Frame<'a>,
}

impl Frame<'_> {
    pub fn present(self) -> Result<()> {
        self.inner.present()
    }
}

impl Deref for Frame<'_> {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.inner
    }
}

impl DerefMut for Frame<'_> {
    fn deref_mut(&mut self) -> &mut [u32] {
        &mut self.inner
    }
}

// ================================================================ macOS ====

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{anyhow, Context, Result};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint};
    use objc2_io_surface::{
        kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
        kIOSurfacePixelFormat, kIOSurfaceWidth, IOSurfaceLockOptions, IOSurfaceRef,
    };
    use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
    use std::ops::{Deref, DerefMut};
    use std::rc::Rc;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// 'BGRA' — matches softbuffer's little-endian 0x00RRGGBB u32 layout.
    const PIXEL_FORMAT_BGRA: i64 = 0x4247_5241;

    struct Surface {
        surface: CFRetained<IOSurfaceRef>,
        /// Fallback staging buffer when the surface's row stride isn't
        /// exactly `w * 4` (rendering assumes a tightly packed buffer).
        staging: Option<Vec<u32>>,
    }

    pub struct Presenter {
        window: Rc<Window>,
        root: Retained<CALayer>,
        layer: Retained<CALayer>,
        surfaces: Vec<Surface>,
        size: (u32, u32),
        next: usize,
    }

    fn new_surface(w: u32, h: u32) -> Result<Surface> {
        let num = |v: i64| CFNumber::new_i64(v);
        let keys: [&CFString; 5] = unsafe {
            [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfaceBytesPerRow,
                kIOSurfacePixelFormat,
            ]
        };
        let vals = [
            num(w as i64),
            num(h as i64),
            num(4),
            num(w as i64 * 4),
            num(PIXEL_FORMAT_BGRA),
        ];
        let vals_ref: Vec<&CFType> = vals.iter().map(|v| v.as_ref()).collect();
        let props = CFDictionary::from_slices(&keys, &vals_ref);
        let surface = unsafe { IOSurfaceRef::new(props.as_opaque()) }
            .ok_or_else(|| anyhow!("IOSurfaceCreate({w}x{h}) failed"))?;
        let staging = (surface.bytes_per_row() != w as usize * 4)
            .then(|| vec![0u32; w as usize * h as usize]);
        Ok(Surface { surface, staging })
    }

    impl Presenter {
        pub fn new(window: Rc<Window>) -> Result<Self> {
            let handle = window.window_handle().context("window handle")?;
            let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
                return Err(anyhow!("not an AppKit window"));
            };
            // SAFETY: winit keeps the NSView alive for the window's lifetime.
            let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
            view.setWantsLayer(true);
            let root = view.layer().context("view has no layer")?;
            let layer = CALayer::new();
            // Top-left origin, no implicit scaling: the IOSurface is already
            // at physical resolution.
            layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
            layer.setGeometryFlipped(true);
            layer.setContentsGravity(unsafe { kCAGravityTopLeft });
            // Our pixels carry alpha = 0 (0x00RRGGBB); opaque tells the
            // compositor to ignore it (and skips blending — cheaper).
            layer.setOpaque(true);
            root.addSublayer(&layer);
            Ok(Self { window, root, layer, surfaces: Vec::new(), size: (0, 0), next: 0 })
        }

        pub fn frame(&mut self, w: u32, h: u32) -> Result<Frame<'_>> {
            if self.size != (w, h) || self.surfaces.is_empty() {
                // Double buffering: write into the surface the compositor is
                // NOT currently showing.
                self.surfaces = vec![new_surface(w, h)?, new_surface(w, h)?];
                self.size = (w, h);
                self.next = 0;
            }
            // Keep the layer glued to the view (window may have moved to a
            // display with a different size / scale).
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.layer.setFrame(self.root.bounds());
            self.layer.setContentsScale(self.window.scale_factor());
            CATransaction::commit();

            let idx = self.next;
            let s = &mut self.surfaces[idx];
            unsafe { s.surface.lock(IOSurfaceLockOptions::empty(), std::ptr::null_mut()) };
            let len = w as usize * h as usize;
            let pixels: &mut [u32] = match s.staging.as_mut() {
                Some(v) => v.as_mut_slice(),
                None => unsafe {
                    std::slice::from_raw_parts_mut(s.surface.base_address().as_ptr().cast(), len)
                },
            };
            // SAFETY: `pixels` borrows either `staging` or the locked
            // surface memory, both owned by `self.surfaces[idx]`, which the
            // Frame keeps exclusively borrowed through `self`.
            let pixels: &mut [u32] = unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr(), len) };
            Ok(Frame { presenter: self, idx, pixels })
        }
    }

    pub struct Frame<'a> {
        presenter: &'a mut Presenter,
        idx: usize,
        pixels: &'a mut [u32],
    }

    impl Frame<'_> {
        pub fn present(self) -> Result<()> {
            let p = self.presenter;
            let (w, h) = p.size;
            let s = &p.surfaces[self.idx];
            if let Some(staging) = s.staging.as_ref() {
                let bpr = s.surface.bytes_per_row();
                let base = s.surface.base_address().as_ptr().cast::<u8>();
                for y in 0..h as usize {
                    let src = &staging[y * w as usize..(y + 1) * w as usize];
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            src.as_ptr().cast::<u8>(),
                            base.add(y * bpr),
                            w as usize * 4,
                        );
                    }
                }
            }
            unsafe { s.surface.unlock(IOSurfaceLockOptions::empty(), std::ptr::null_mut()) };

            CATransaction::begin();
            CATransaction::setDisableActions(true);
            // SAFETY: IOSurfaceRef is toll-free bridged to the IOSurface
            // ObjC class, a valid CALayer `contents` value.
            let obj: &AnyObject = unsafe { &*(&*s.surface as *const IOSurfaceRef).cast() };
            unsafe { p.layer.setContents(Some(obj)) };
            CATransaction::commit();
            p.next = (self.idx + 1) % p.surfaces.len();
            Ok(())
        }
    }

    impl Deref for Frame<'_> {
        type Target = [u32];
        fn deref(&self) -> &[u32] {
            self.pixels
        }
    }

    impl DerefMut for Frame<'_> {
        fn deref_mut(&mut self) -> &mut [u32] {
            self.pixels
        }
    }
}

// ======================================================= other platforms ====

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::{anyhow, Result};
    use softbuffer::{Context, Surface};
    use std::num::NonZeroU32;
    use std::ops::{Deref, DerefMut};
    use std::rc::Rc;
    use winit::window::Window;

    pub struct Presenter {
        surface: Surface<Rc<Window>, Rc<Window>>,
        size: (u32, u32),
    }

    impl Presenter {
        pub fn new(window: Rc<Window>) -> Result<Self> {
            let context = Context::new(window.clone()).map_err(|e| anyhow!("{e:?}"))?;
            let surface = Surface::new(&context, window).map_err(|e| anyhow!("{e:?}"))?;
            Ok(Self { surface, size: (0, 0) })
        }

        pub fn frame(&mut self, w: u32, h: u32) -> Result<Frame<'_>> {
            if self.size != (w, h) {
                self.surface
                    .resize(NonZeroU32::new(w.max(1)).unwrap(), NonZeroU32::new(h.max(1)).unwrap())
                    .map_err(|e| anyhow!("{e:?}"))?;
                self.size = (w, h);
            }
            let buf = self.surface.buffer_mut().map_err(|e| anyhow!("{e:?}"))?;
            Ok(Frame { buf })
        }
    }

    pub struct Frame<'a> {
        buf: softbuffer::Buffer<'a, Rc<Window>, Rc<Window>>,
    }

    impl Frame<'_> {
        pub fn present(self) -> Result<()> {
            self.buf.present().map_err(|e| anyhow!("{e:?}"))
        }
    }

    impl Deref for Frame<'_> {
        type Target = [u32];
        fn deref(&self) -> &[u32] {
            &self.buf
        }
    }

    impl DerefMut for Frame<'_> {
        fn deref_mut(&mut self) -> &mut [u32] {
            &mut self.buf
        }
    }
}
