//! Write captured frames to disk with templated filenames.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy)]
pub enum CaptureMode {
    Region,
    Fullscreen,
}

impl CaptureMode {
    fn as_str(self) -> &'static str {
        match self {
            CaptureMode::Region => "region",
            CaptureMode::Fullscreen => "fullscreen",
        }
    }
}

pub struct TemplateContext<'a> {
    pub width: u32,
    pub height: u32,
    pub mode: CaptureMode,
    pub now: OffsetDateTime,
    pub warn_unknown: &'a std::cell::Cell<bool>,
}

/// Write `frame` as PNG into `directory` using the `template`. Returns the
/// final resolved path. Creates the directory if missing.
pub fn save_png(
    frame: &RgbaImage,
    directory: &Path,
    template: &str,
    mode: CaptureMode,
) -> Result<PathBuf> {
    save_png_at(frame, directory, template, mode, capture_time())
}

pub fn capture_time() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc())
}

pub fn save_png_at(
    frame: &RgbaImage,
    directory: &Path,
    template: &str,
    mode: CaptureMode,
    now: OffsetDateTime,
) -> Result<PathBuf> {
    fs::create_dir_all(directory)
        .with_context(|| format!("create save directory {}", directory.display()))?;
    let filename = screenshot_filename(template, frame.dimensions(), mode, now);
    let candidate = directory.join(filename);
    let mut png = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(png.as_file_mut());
        frame.write_to(&mut writer, image::ImageFormat::Png)?;
        writer.flush()?;
    }
    persist_png(png, &candidate)
}

pub fn screenshot_filename(
    template: &str, size: (u32, u32), mode: CaptureMode, now: OffsetDateTime,
) -> String {
    interpolate(template, &TemplateContext {
        width: size.0, height: size.1, mode, now,
        warn_unknown: &std::cell::Cell::new(false),
    })
}

/// Read the last successful Save As directory, falling back to the configured
/// folder. A missing configured folder can be created on first use.
pub fn dialog_directory(state: Option<&Path>, configured: &Path) -> Option<PathBuf> {
    if let Some(last) = state.and_then(|p| fs::read_to_string(p).ok()).map(PathBuf::from) {
        if last.is_absolute() && last.is_dir() {
            return Some(last);
        }
    }
    fs::create_dir_all(configured).ok()?;
    std::path::absolute(configured).ok()
}

pub fn remember_dialog_directory(state: &Path, saved_file: &Path) -> Result<()> {
    let saved_file = std::path::absolute(saved_file)?;
    let directory = saved_file.parent().context("missing save directory")?;
    let state_dir = state.parent().context("missing state directory")?;
    fs::create_dir_all(state_dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(state_dir)?;
    temp.write_all(directory.to_str().context("directory is not UTF-8")?.as_bytes())?;
    temp.persist(state)?;
    Ok(())
}

pub fn interpolate(template: &str, ctx: &TemplateContext<'_>) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else {
            out.push_str(rest);
            return out;
        };
        let placeholder = &rest[1..end];
        let replacement = match placeholder {
            "date" => format_date(ctx.now),
            "time" => format_time(ctx.now),
            "datetime" => format!("{}_{}", format_date(ctx.now), format_time(ctx.now)),
            "w" => ctx.width.to_string(),
            "h" => ctx.height.to_string(),
            "mode" => ctx.mode.as_str().to_string(),
            unknown => {
                if !ctx.warn_unknown.get() {
                    ctx.warn_unknown.set(true);
                    eprintln!("file_save: unknown placeholder {{{unknown}}}");
                }
                format!("{{{unknown}}}")
            }
        };
        out.push_str(&replacement);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn format_date(dt: OffsetDateTime) -> String {
    format!("{:04}-{:02}-{:02}", dt.year(), u8::from(dt.month()), dt.day())
}

fn format_time(dt: OffsetDateTime) -> String {
    format!("{:02}-{:02}-{:02}", dt.hour(), dt.minute(), dt.second())
}

// Publish only a complete PNG. Even when captures share a template, an old
// notification must continue to identify its original file.
fn persist_png(mut png: tempfile::NamedTempFile, candidate: &Path) -> Result<PathBuf> {
    let mut path = candidate.to_owned();
    for suffix in 1u64.. {
        match png.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                png = error.file;
            }
            Err(error) => return Err(error.error).with_context(|| format!("save {}", path.display())),
        }
        let mut name = candidate.file_stem().unwrap_or_default().to_owned();
        name.push(format!("_{suffix}"));
        if let Some(ext) = candidate.extension() {
            name.push(".");
            name.push(ext);
        }
        path = candidate.with_file_name(name);
    }
    anyhow::bail!("no available screenshot filename")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use time::macros::datetime;

    #[test]
    fn save_dialog_remembers_successful_directory_and_recovers_if_removed() {
        let root = tempfile::tempdir().unwrap();
        let configured = root.path().join("configured");
        let chosen = root.path().join("chosen");
        let state = root.path().join("settings/last-save-directory");
        assert_eq!(dialog_directory(Some(&state), &configured), Some(configured.clone()));
        fs::create_dir(&chosen).unwrap();
        remember_dialog_directory(&state, &chosen.join("shot.png")).unwrap();
        assert_eq!(dialog_directory(Some(&state), &configured), Some(chosen.clone()));
        fs::remove_dir(chosen).unwrap();
        assert_eq!(dialog_directory(Some(&state), &configured), Some(configured));
    }

    #[test]
    fn interpolate_date() {
        let now = datetime!(2026-04-19 15:04:30 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1920,
            height: 1080,
            mode: CaptureMode::Fullscreen,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(interpolate("shot_{date}.png", &ctx), "shot_2026-04-19.png");
    }

    #[test]
    fn interpolate_time() {
        let now = datetime!(2026-04-19 15:04:30 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1,
            height: 1,
            mode: CaptureMode::Region,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(interpolate("{time}", &ctx), "15-04-30");
    }

    #[test]
    fn interpolate_datetime() {
        let now = datetime!(2026-04-19 15:04:30 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1,
            height: 1,
            mode: CaptureMode::Region,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(interpolate("{datetime}.png", &ctx), "2026-04-19_15-04-30.png");
    }

    #[test]
    fn interpolate_wh_mode() {
        let now = datetime!(2026-04-19 00:00:00 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 2880,
            height: 1800,
            mode: CaptureMode::Fullscreen,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(
            interpolate("{mode}_{w}x{h}.png", &ctx),
            "fullscreen_2880x1800.png"
        );
    }

    #[test]
    fn interpolate_unknown_passthrough() {
        let now = datetime!(2026-04-19 00:00:00 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1,
            height: 1,
            mode: CaptureMode::Region,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(interpolate("x_{bogus}_y", &ctx), "x_{bogus}_y");
        assert!(warn.get());
    }

    #[test]
    fn interpolate_unmatched_brace() {
        let now = datetime!(2026-04-19 00:00:00 UTC);
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1,
            height: 1,
            mode: CaptureMode::Region,
            now,
            warn_unknown: &warn,
        };
        assert_eq!(interpolate("a_{date_unfinished", &ctx), "a_{date_unfinished");
    }

    #[test]
    fn notifications_keep_distinct_png_paths_after_many_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("shot.png");
        fs::write(&original, b"original").unwrap();
        for n in 1..=99 {
            fs::write(dir.path().join(format!("shot_{n}.png")), b"existing").unwrap();
        }
        let frame = RgbaImage::new(2, 3);
        let saved = save_png(&frame, dir.path(), "shot.png", CaptureMode::Fullscreen).unwrap();
        assert_eq!(saved, dir.path().join("shot_100.png"));
        assert_eq!(image::open(saved).unwrap().to_rgba8(), frame);
        assert_eq!(fs::read(original).unwrap(), b"original");
    }
}
