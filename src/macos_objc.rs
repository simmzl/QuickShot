//! Minimal Objective-C runtime bindings for the few selectors quickshot needs.
//!
//! Why this exists: the obvious `extern "C" { fn objc_msgSend(..., ...) -> *mut c_void; }`
//! variadic declaration is **broken on Apple arm64**. The Darwin variadic ABI
//! puts arguments after `...` on the stack, but the real `_objc_msgSend` is a
//! non-variadic implementation that reads primitive args from x2/x3/.... The
//! result: `setLevel:`, `setCollectionBehavior:`, `setActivationPolicy:` etc.
//! all silently received whatever happened to be in x2, not our intended value.
//! That manifested as the screenshot overlay landing at the desktop layer with
//! a garbage NSWindow.level (e.g. -71374676).
//!
//! Fix: declare each call site with the exact non-variadic signature using
//! `#[link_name = "objc_msgSend"]`. All aliases resolve to the same symbol but
//! the Rust call-site uses the correct register-based ABI.
#![cfg(target_os = "macos")]

use std::ffi::{c_void, CStr};

extern "C" {
    pub fn sel_registerName(name: *const u8) -> *mut c_void;
    pub fn objc_getClass(name: *const u8) -> *mut c_void;

    /// `[obj sel]` returning an object pointer (id).
    #[link_name = "objc_msgSend"]
    pub fn msg_send_id(obj: *mut c_void, sel: *mut c_void) -> *mut c_void;

    /// `[obj sel]` returning NSInteger (i64 on macOS 64-bit).
    #[link_name = "objc_msgSend"]
    pub fn msg_send_int(obj: *mut c_void, sel: *mut c_void) -> i64;

    /// `[obj sel]` returning NSUInteger (u64 on macOS 64-bit).
    #[link_name = "objc_msgSend"]
    pub fn msg_send_uint(obj: *mut c_void, sel: *mut c_void) -> u64;

    /// `[obj sel: arg]` where arg is NSInteger.
    #[link_name = "objc_msgSend"]
    pub fn msg_send_set_int(obj: *mut c_void, sel: *mut c_void, arg: i64);

    /// `[obj sel: arg]` where arg is NSUInteger.
    #[link_name = "objc_msgSend"]
    pub fn msg_send_set_uint(obj: *mut c_void, sel: *mut c_void, arg: u64);

    /// `[obj sel: arg]` where arg is BOOL (signed char on Apple ABIs).
    #[link_name = "objc_msgSend"]
    pub fn msg_send_set_bool(obj: *mut c_void, sel: *mut c_void, arg: i8);

    /// `[obj sel: arg]` where arg is id (object pointer).
    #[link_name = "objc_msgSend"]
    pub fn msg_send_set_id(obj: *mut c_void, sel: *mut c_void, arg: *mut c_void);
}

/// Convenience: register a selector from a C-string literal.
#[inline]
pub fn sel(name: &CStr) -> *mut c_void {
    unsafe { sel_registerName(name.as_ptr().cast()) }
}

/// Convenience: look up a class from a C-string literal.
#[inline]
pub fn class(name: &CStr) -> *mut c_void {
    unsafe { objc_getClass(name.as_ptr().cast()) }
}
