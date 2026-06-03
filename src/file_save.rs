//! Write captured frames to disk with templated filenames.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::fs;
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
    fs::create_dir_all(directory).with_context(|| {
        format!("create save directory {}", directory.display())
    })?;
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| {
        eprintln!("file_save: local time unavailable; using UTC");
        OffsetDateTime::now_utc()
    });
    let warn_cell = std::cell::Cell::new(false);
    let ctx = TemplateContext {
        width: frame.width(),
        height: frame.height(),
        mode,
        now,
        warn_unknown: &warn_cell,
    };
    let filename = interpolate(template, &ctx);
    let candidate = directory.join(&filename);
    let target = uniquify(&candidate);
    frame
        .save(&target)
        .with_context(|| format!("write PNG to {}", target.display()))?;
    Ok(target)
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

pub fn uniquify(candidate: &Path) -> PathBuf {
    if !candidate.exists() {
        return candidate.to_path_buf();
    }
    let parent = candidate.parent().unwrap_or_else(|| Path::new(""));
    let stem = candidate
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = candidate
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 1..=99 {
        let new_name = if ext.is_empty() {
            format!("{stem}_{n}")
        } else {
            format!("{stem}_{n}.{ext}")
        };
        let candidate = parent.join(new_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use time::macros::datetime;

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
    fn uniquify_no_collision() {
        let dir = std::env::temp_dir().join("QuickShot_uniquify_test_noclash");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("fresh.png");
        assert_eq!(uniquify(&p), p);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn uniquify_adds_suffix_on_collision() {
        let dir = std::env::temp_dir().join("QuickShot_uniquify_test_clash");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("shot.png");
        fs::write(&existing, b"x").unwrap();
        let next = uniquify(&existing);
        assert_eq!(next, dir.join("shot_1.png"));
        fs::write(&next, b"x").unwrap();
        let next2 = uniquify(&existing);
        assert_eq!(next2, dir.join("shot_2.png"));
        fs::remove_dir_all(&dir).ok();
    }
}
