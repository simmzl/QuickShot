# quickshot Iter 3 — Config, File Save, Autostart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a TOML-backed config file, optional PNG-to-disk mode with templated filenames, a macOS LaunchAgent-based autostart via CLI subcommands, and hotkey rebinding via config — all without a GUI.

**Architecture:** Three new modules (`config.rs`, `file_save.rs`, `autostart.rs`); `hotkey.rs` and `app.rs` gain `Config` parameters; `main.rs` dispatches on CLI flags or loads config + starts the daemon. No existing feature behavior changes when config is absent — defaults match the pre-Iter-3 hardcoded values.

**Tech Stack:**
- Rust 2021 (existing toolchain 1.86+)
- `serde = { version = "1", features = ["derive"] }` — new
- `toml = "0.8"` — new
- `time = { version = "0.3", features = ["formatting", "local-offset"] }` — new (already transitive)
- All existing crates unchanged

**Spec:** `docs/superpowers/specs/2026-04-19-quickshot-iter3-design.md`

**Scope for this plan (Iter 3 only):**
- Config file at `~/.config/quickshot/config.toml` with hotkey/save/general sections
- Hotkey string parsing (modifier+modifier+...+key) → `Modifiers` + `Code`
- File-save mode with template interpolation + uniquifier
- Autostart install/uninstall via CLI subcommands
- CLI flags: `--install-autostart`, `--uninstall-autostart`, `--help`, `-h`

**Not in this plan (deferred):**
- egui settings window (intentionally omitted — binary bloat vs value tradeoff)
- Hot config reload (restart required)
- Cross-platform autostart (Windows/Linux)
- Iter 2c polish items (TTF subset, coord helper extraction, estimate_text_width)

---

## File Structure

```
quickshot/
├── Cargo.toml                          (modified — add serde, toml, time)
├── README.md                           (modified — Iter 3 status + config docs)
├── src/
│   ├── app.rs                          (modified — take Config, wire save + notif toggle)
│   ├── autostart.rs                    (new, ~80 lines — macOS LaunchAgent management)
│   ├── capture.rs                      (unchanged)
│   ├── clipboard.rs                    (unchanged)
│   ├── config.rs                       (new, ~240 lines — Config + parsing + tests)
│   ├── crop.rs                         (unchanged)
│   ├── file_save.rs                    (new, ~160 lines — template + PNG + tests)
│   ├── hotkey.rs                       (modified — accept ParsedHotkey params)
│   ├── main.rs                         (modified — CLI dispatch + config load)
│   ├── notification.rs                 (unchanged)
│   ├── overlay/                        (unchanged)
│   ├── permission.rs                   (unchanged)
│   ├── text.rs                         (unchanged)
│   └── tray.rs                         (unchanged)
```

**Responsibilities:**
- `config.rs` — single source of truth for user-configurable settings. Pure logic: IO + parsing in `load()`, pure helpers in `parse_hotkey_string()`, `expand_home()`, `default_toml_contents()`. No other module touches the filesystem for config.
- `file_save.rs` — accepts an `RgbaImage` + save settings + capture mode, writes to disk. Pure helpers: `interpolate`, `uniquify`, plus `save_png` that orchestrates.
- `autostart.rs` — builds the LaunchAgent plist and shells out to `launchctl`. Pure template rendering is testable; IO is behind thin public fns.
- `hotkey.rs` — takes `ParsedHotkey` structs (modifiers + code + raw string) from config and registers them. No string parsing here.
- `main.rs` — CLI dispatch, config load, daemon startup. Three early-exit branches for the autostart subcommands; otherwise flows into the existing event-loop path.

---

## Task 1: Config module (types, parsing, load with defaults)

Add the `config.rs` module with the full `Config` struct, TOML parsing, hotkey-string grammar, and tests. Do not wire it into anything yet.

**Files:**
- Modify: `Cargo.toml` (add `serde`, `toml`, `time`)
- Create: `src/config.rs`
- Modify: `src/main.rs` (declare `mod config;`)

- [ ] **Step 1: Add dependencies**

Edit `Cargo.toml`. In the `[dependencies]` block, add `serde`, `toml`, and `time` alphabetically. After the edit the block reads:
```toml
[dependencies]
anyhow = "1"
arboard = "3.4"
fontdue = "0.9"
global-hotkey = "0.6"
image = { version = "0.25", default-features = false, features = ["png"] }
notify-rust = "4"
serde = { version = "1", features = ["derive"] }
softbuffer = "0.4"
time = { version = "0.3", features = ["formatting", "local-offset"] }
toml = "0.8"
tray-icon = "0.19"
winit = "0.30"
xcap = "0.9"
```

Note: the `tray-icon` version already pinned from Iter 2b (actual `0.22`). Preserve whatever is currently there; don't force it back to `0.19`.

- [ ] **Step 2: Verify dependencies resolve**

Run:
```bash
cargo check
```
Expected: downloads serde, toml (and transitives), time re-export. Finished with at most unused-import warnings.

- [ ] **Step 3: Write `src/config.rs`**

Create `src/config.rs`:
```rust
//! User-configurable daemon settings, loaded from
//! `~/.config/quickshot/config.toml` at startup. Never panics on bad input;
//! failed fields fall back to defaults with a stderr warning.

use anyhow::{anyhow, bail, Result};
use global_hotkey::hotkey::{Code, Modifiers};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub save: SaveConfig,
    pub general: GeneralConfig,
}

#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    pub region: ParsedHotkey,
    pub fullscreen: ParsedHotkey,
}

#[derive(Debug, Clone)]
pub struct ParsedHotkey {
    pub modifiers: Modifiers,
    pub code: Code,
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct SaveConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub filename_template: String,
}

#[derive(Debug, Clone)]
pub struct GeneralConfig {
    pub notification_on_fullscreen: bool,
}

impl Config {
    /// Load the config. Always returns a Config — errors are logged and
    /// defaults are substituted.
    pub fn load() -> Config {
        let path = match config_path() {
            Some(p) => p,
            None => {
                eprintln!("config: could not resolve config dir; using defaults");
                return Config::defaults();
            }
        };
        if !path.exists() {
            if let Some(parent) = path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("config: failed to create {parent:?}: {e}");
                    return Config::defaults();
                }
            }
            if let Err(e) = fs::write(&path, DEFAULT_TOML) {
                eprintln!("config: failed to write defaults to {path:?}: {e}");
            }
            return Config::defaults();
        }
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("config: failed to read {path:?}: {e}; using defaults");
                return Config::defaults();
            }
        };
        match Self::parse(&source) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("config: parse error in {path:?}: {e}; using defaults");
                Config::defaults()
            }
        }
    }

    pub fn defaults() -> Config {
        Config {
            hotkey: HotkeyConfig {
                region: default_region_hotkey(),
                fullscreen: default_fullscreen_hotkey(),
            },
            save: SaveConfig {
                enabled: false,
                directory: expand_home("~/Desktop"),
                filename_template: "Screenshot_{datetime}.png".to_string(),
            },
            general: GeneralConfig {
                notification_on_fullscreen: true,
            },
        }
    }

    pub fn parse(source: &str) -> Result<Config> {
        let raw: RawConfig = toml::from_str(source)?;
        let mut cfg = Config::defaults();

        if let Some(h) = raw.hotkey {
            if let Some(s) = h.region.as_deref() {
                match parse_hotkey_string(s) {
                    Ok(p) => cfg.hotkey.region = p,
                    Err(e) => eprintln!("config: hotkey.region {s:?} invalid: {e}; using default"),
                }
            }
            if let Some(s) = h.fullscreen.as_deref() {
                match parse_hotkey_string(s) {
                    Ok(p) => cfg.hotkey.fullscreen = p,
                    Err(e) => {
                        eprintln!("config: hotkey.fullscreen {s:?} invalid: {e}; using default");
                    }
                }
            }
        }
        if let Some(s) = raw.save {
            if let Some(v) = s.enabled {
                cfg.save.enabled = v;
            }
            if let Some(d) = s.directory.as_deref() {
                cfg.save.directory = expand_home(d);
            }
            if let Some(t) = s.filename_template {
                cfg.save.filename_template = t;
            }
        }
        if let Some(g) = raw.general {
            if let Some(v) = g.notification_on_fullscreen {
                cfg.general.notification_on_fullscreen = v;
            }
        }
        Ok(cfg)
    }
}

#[derive(Deserialize, Default)]
struct RawConfig {
    hotkey: Option<RawHotkey>,
    save: Option<RawSave>,
    general: Option<RawGeneral>,
}

#[derive(Deserialize, Default)]
struct RawHotkey {
    region: Option<String>,
    fullscreen: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawSave {
    enabled: Option<bool>,
    directory: Option<String>,
    filename_template: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawGeneral {
    notification_on_fullscreen: Option<bool>,
}

pub const DEFAULT_TOML: &str = r#"# quickshot config — edit and restart the daemon to apply changes.
# Regenerate defaults by deleting this file and re-launching quickshot.

[hotkey]
# Format: modifiers joined by "+", ending with a key. Modifiers (case-insensitive):
#   Cmd (macOS) / Ctrl / Alt / Opt / Shift / Meta / Super
# Keys: A-Z, 0-9, F1-F24, or named keys (Space, Enter, Tab, Backspace, Escape).
region = "Cmd+Shift+A"
fullscreen = "Cmd+Shift+S"

[save]
# When true, every successful capture also writes a PNG to `directory`.
enabled = false
# `~` is expanded to $HOME. Missing directories are created.
directory = "~/Desktop"
# Available placeholders: {date}, {time}, {datetime}, {w}, {h}, {mode}
#   {date}     2026-04-19
#   {time}     15-04-30
#   {datetime} 2026-04-19_15-04-30
#   {w}, {h}   1920, 1080 (physical pixels)
#   {mode}     region | fullscreen
filename_template = "Screenshot_{datetime}.png"

[general]
# Show a system notification after a successful full-screen capture.
# Region captures never show one (overlay dismissal is already feedback).
notification_on_fullscreen = true
"#;

fn config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("quickshot").join("config.toml"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("quickshot")
            .join("config.toml"),
    )
}

pub fn expand_home(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if input == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(input)
}

pub fn parse_hotkey_string(input: &str) -> Result<ParsedHotkey> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("empty hotkey string");
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.len() < 2 {
        bail!("hotkey must have at least one modifier and a key");
    }
    let (key_str, modifier_strs) = parts.split_last().unwrap();
    let mut modifiers = Modifiers::empty();
    for m in modifier_strs {
        let bit = parse_modifier(m).ok_or_else(|| anyhow!("unknown modifier: {m}"))?;
        if modifiers.contains(bit) {
            bail!("duplicate modifier: {m}");
        }
        modifiers |= bit;
    }
    if modifiers.is_empty() {
        bail!("at least one modifier required");
    }
    let code = parse_code(key_str).ok_or_else(|| anyhow!("unknown key: {key_str}"))?;
    Ok(ParsedHotkey {
        modifiers,
        code,
        raw: trimmed.to_string(),
    })
}

fn parse_modifier(s: &str) -> Option<Modifiers> {
    match s.to_ascii_lowercase().as_str() {
        "cmd" | "meta" | "super" => Some(Modifiers::META),
        "ctrl" | "control" => Some(Modifiers::CONTROL),
        "alt" | "opt" | "option" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
        _ => None,
    }
}

fn parse_code(s: &str) -> Option<Code> {
    let upper = s.to_ascii_uppercase();
    // Single letters A-Z
    if upper.len() == 1 {
        let b = upper.as_bytes()[0];
        if b.is_ascii_uppercase() {
            return match b {
                b'A' => Some(Code::KeyA),
                b'B' => Some(Code::KeyB),
                b'C' => Some(Code::KeyC),
                b'D' => Some(Code::KeyD),
                b'E' => Some(Code::KeyE),
                b'F' => Some(Code::KeyF),
                b'G' => Some(Code::KeyG),
                b'H' => Some(Code::KeyH),
                b'I' => Some(Code::KeyI),
                b'J' => Some(Code::KeyJ),
                b'K' => Some(Code::KeyK),
                b'L' => Some(Code::KeyL),
                b'M' => Some(Code::KeyM),
                b'N' => Some(Code::KeyN),
                b'O' => Some(Code::KeyO),
                b'P' => Some(Code::KeyP),
                b'Q' => Some(Code::KeyQ),
                b'R' => Some(Code::KeyR),
                b'S' => Some(Code::KeyS),
                b'T' => Some(Code::KeyT),
                b'U' => Some(Code::KeyU),
                b'V' => Some(Code::KeyV),
                b'W' => Some(Code::KeyW),
                b'X' => Some(Code::KeyX),
                b'Y' => Some(Code::KeyY),
                b'Z' => Some(Code::KeyZ),
                _ => None,
            };
        }
        if b.is_ascii_digit() {
            return match b {
                b'0' => Some(Code::Digit0),
                b'1' => Some(Code::Digit1),
                b'2' => Some(Code::Digit2),
                b'3' => Some(Code::Digit3),
                b'4' => Some(Code::Digit4),
                b'5' => Some(Code::Digit5),
                b'6' => Some(Code::Digit6),
                b'7' => Some(Code::Digit7),
                b'8' => Some(Code::Digit8),
                b'9' => Some(Code::Digit9),
                _ => None,
            };
        }
    }
    // F1 .. F24
    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u8>() {
            return match n {
                1 => Some(Code::F1),
                2 => Some(Code::F2),
                3 => Some(Code::F3),
                4 => Some(Code::F4),
                5 => Some(Code::F5),
                6 => Some(Code::F6),
                7 => Some(Code::F7),
                8 => Some(Code::F8),
                9 => Some(Code::F9),
                10 => Some(Code::F10),
                11 => Some(Code::F11),
                12 => Some(Code::F12),
                13 => Some(Code::F13),
                14 => Some(Code::F14),
                15 => Some(Code::F15),
                16 => Some(Code::F16),
                17 => Some(Code::F17),
                18 => Some(Code::F18),
                19 => Some(Code::F19),
                20 => Some(Code::F20),
                21 => Some(Code::F21),
                22 => Some(Code::F22),
                23 => Some(Code::F23),
                24 => Some(Code::F24),
                _ => None,
            };
        }
    }
    // Named keys
    match upper.as_str() {
        "SPACE" => Some(Code::Space),
        "ENTER" | "RETURN" => Some(Code::Enter),
        "TAB" => Some(Code::Tab),
        "BACKSPACE" => Some(Code::Backspace),
        "ESCAPE" | "ESC" => Some(Code::Escape),
        _ => None,
    }
}

fn default_region_hotkey() -> ParsedHotkey {
    parse_hotkey_string("Cmd+Shift+A").expect("default region hotkey must parse")
}

fn default_fullscreen_hotkey() -> ParsedHotkey {
    parse_hotkey_string("Cmd+Shift+S").expect("default fullscreen hotkey must parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let p = parse_hotkey_string("Cmd+Shift+A").unwrap();
        assert!(p.modifiers.contains(Modifiers::META));
        assert!(p.modifiers.contains(Modifiers::SHIFT));
        assert_eq!(p.code, Code::KeyA);
        assert_eq!(p.raw, "Cmd+Shift+A");
    }

    #[test]
    fn parse_case_insensitive() {
        let p = parse_hotkey_string("cmd+SHIFT+a").unwrap();
        assert!(p.modifiers.contains(Modifiers::META));
        assert!(p.modifiers.contains(Modifiers::SHIFT));
        assert_eq!(p.code, Code::KeyA);
    }

    #[test]
    fn parse_opt_alias() {
        let p = parse_hotkey_string("Opt+Shift+B").unwrap();
        assert!(p.modifiers.contains(Modifiers::ALT));
    }

    #[test]
    fn parse_super_alias() {
        let p = parse_hotkey_string("Super+X").unwrap();
        assert!(p.modifiers.contains(Modifiers::META));
    }

    #[test]
    fn parse_digit() {
        let p = parse_hotkey_string("Ctrl+5").unwrap();
        assert_eq!(p.code, Code::Digit5);
    }

    #[test]
    fn parse_fn_key() {
        let p = parse_hotkey_string("Cmd+F7").unwrap();
        assert_eq!(p.code, Code::F7);
    }

    #[test]
    fn parse_named_key() {
        let p = parse_hotkey_string("Ctrl+Space").unwrap();
        assert_eq!(p.code, Code::Space);
    }

    #[test]
    fn parse_empty_errors() {
        assert!(parse_hotkey_string("").is_err());
        assert!(parse_hotkey_string("   ").is_err());
    }

    #[test]
    fn parse_missing_modifier_errors() {
        assert!(parse_hotkey_string("A").is_err());
    }

    #[test]
    fn parse_unknown_modifier_errors() {
        assert!(parse_hotkey_string("Hyper+A").is_err());
    }

    #[test]
    fn parse_duplicate_modifier_errors() {
        assert!(parse_hotkey_string("Cmd+Cmd+A").is_err());
    }

    #[test]
    fn parse_unknown_key_errors() {
        assert!(parse_hotkey_string("Cmd+Banana").is_err());
    }

    #[test]
    fn expand_home_basic() {
        std::env::set_var("HOME", "/Users/tester");
        assert_eq!(expand_home("~/Desktop"), PathBuf::from("/Users/tester/Desktop"));
        assert_eq!(expand_home("~"), PathBuf::from("/Users/tester"));
        assert_eq!(expand_home("/absolute/path"), PathBuf::from("/absolute/path"));
        assert_eq!(expand_home("relative/path"), PathBuf::from("relative/path"));
    }

    #[test]
    fn defaults_parse() {
        let c = Config::defaults();
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
        assert_eq!(c.hotkey.fullscreen.raw, "Cmd+Shift+S");
        assert!(!c.save.enabled);
        assert_eq!(c.save.filename_template, "Screenshot_{datetime}.png");
        assert!(c.general.notification_on_fullscreen);
    }

    #[test]
    fn parse_full_toml() {
        let src = r#"
            [hotkey]
            region = "Ctrl+Shift+R"
            fullscreen = "Ctrl+Shift+F"
            [save]
            enabled = true
            directory = "/tmp/shots"
            filename_template = "s_{datetime}.png"
            [general]
            notification_on_fullscreen = false
        "#;
        let c = Config::parse(src).unwrap();
        assert_eq!(c.hotkey.region.raw, "Ctrl+Shift+R");
        assert_eq!(c.hotkey.region.code, Code::KeyR);
        assert!(c.save.enabled);
        assert_eq!(c.save.directory, PathBuf::from("/tmp/shots"));
        assert!(!c.general.notification_on_fullscreen);
    }

    #[test]
    fn parse_partial_toml_uses_defaults() {
        let src = r#"
            [save]
            enabled = true
        "#;
        let c = Config::parse(src).unwrap();
        assert!(c.save.enabled);
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A"); // default
        assert_eq!(c.save.filename_template, "Screenshot_{datetime}.png"); // default
    }

    #[test]
    fn parse_malformed_returns_err() {
        let src = "this is not toml [[ ";
        assert!(Config::parse(src).is_err());
    }

    #[test]
    fn parse_invalid_hotkey_falls_back_to_default() {
        let src = r#"
            [hotkey]
            region = "Hyper+Z"
        "#;
        // Should not error out — invalid hotkey falls back to default.
        let c = Config::parse(src).unwrap();
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
    }

    #[test]
    fn default_toml_is_valid() {
        // The embedded default must parse without errors.
        let c = Config::parse(DEFAULT_TOML).unwrap();
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
    }
}
```

- [ ] **Step 4: Declare the module in `src/main.rs`**

Edit `src/main.rs`. Add `mod config;` to the module list (alphabetical). After the edit the module list reads:
```rust
mod app;
mod capture;
mod clipboard;
mod config;
mod crop;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;
```

- [ ] **Step 5: Run the new tests**

Run:
```bash
cargo test config
```
Expected: all config tests pass (19 tests added).

Run the full test suite:
```bash
cargo test
```
Expected: 60 passed / 0 failed / 2 ignored (41 from Iter 2b + 19 new).

- [ ] **Step 6: Verify release build still clean**

Run:
```bash
cargo build --release
```
Expected: clean. `Config` and `parse_hotkey_string` will be dead-code (not yet called). One warning about unused functions is OK — they'll be wired in Task 4.

If the compiler warns about unused variants in `Config` / `RawConfig`, that's expected; ignore for now.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/config.rs src/main.rs
git commit -m "feat(config): TOML config loader with hotkey string parser"
```

---

## Task 2: File-save module with template interpolation

Add `file_save.rs` with pure helpers for template interpolation + uniquifier + PNG write. Wire into nothing yet.

**Files:**
- Create: `src/file_save.rs`
- Modify: `src/main.rs` (declare `mod file_save;`)

- [ ] **Step 1: Write `src/file_save.rs`**

Create `src/file_save.rs`:
```rust
//! Write captured frames to disk with templated filenames.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

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
    // Control: on stderr warning for unknown placeholder
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
    // Silence unused Rfc3339 import with a no-op reference:
    let _ = &Rfc3339;
    Ok(target)
}

pub fn interpolate(template: &str, ctx: &TemplateContext<'_>) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else {
            // unmatched '{' — emit the rest verbatim
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
    // If we hit 99 collisions, return the last candidate; the save will fail
    // on overwrite if the user actually has that much history.
    candidate.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use time::macros::datetime;

    fn ctx_for(ts: OffsetDateTime, mode: CaptureMode) -> (Cell<bool>, TemplateContext<'static>) {
        let warn = Cell::new(false);
        let ctx = TemplateContext {
            width: 1920,
            height: 1080,
            mode,
            now: ts,
            // SAFETY: we leak the reference by returning; the test uses it
            // and drops both at the end of scope. This is a test-only shim
            // and matches the lifetime-of-the-returned-tuple pattern.
            warn_unknown: Box::leak(Box::new(warn)),
        };
        (Cell::new(false), ctx) // caller ignores first element (legacy)
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
    fn uniquify_no_collision() {
        let dir = std::env::temp_dir().join("quickshot_uniquify_test_noclash");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("fresh.png");
        assert_eq!(uniquify(&p), p);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn uniquify_adds_suffix_on_collision() {
        let dir = std::env::temp_dir().join("quickshot_uniquify_test_clash");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("shot.png");
        fs::write(&existing, b"x").unwrap();
        let next = uniquify(&existing);
        assert_eq!(next, dir.join("shot_1.png"));
        // Create the _1 too and verify we get _2
        fs::write(&next, b"x").unwrap();
        let next2 = uniquify(&existing);
        assert_eq!(next2, dir.join("shot_2.png"));
        fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Declare the module in `src/main.rs`**

Add `mod file_save;` to the module list. After the edit:
```rust
mod app;
mod capture;
mod clipboard;
mod config;
mod crop;
mod file_save;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;
```

- [ ] **Step 3: Run tests**

Run:
```bash
cargo test file_save
```
Expected: 7 tests pass (`interpolate_date`, `interpolate_time`, `interpolate_datetime`, `interpolate_wh_mode`, `interpolate_unknown_passthrough`, `interpolate_unmatched_brace`, `uniquify_no_collision`, `uniquify_adds_suffix_on_collision` — actually 8 tests).

Full suite:
```bash
cargo test
```
Expected: 68 passed / 0 failed / 2 ignored (60 + 8 new).

- [ ] **Step 4: Verify release build**

Run:
```bash
cargo build --release
```
Expected: clean. `save_png` + `CaptureMode` + `interpolate` + `uniquify` will be dead-code warnings — expected; Task 5 wires them.

- [ ] **Step 5: Commit**

```bash
git add src/file_save.rs src/main.rs
git commit -m "feat(file_save): PNG save with template interpolation + uniquify"
```

---

## Task 3: Autostart module (macOS LaunchAgent)

Write `autostart.rs` with `install` and `uninstall` fns plus a plist-template unit test. No wiring yet.

**Files:**
- Create: `src/autostart.rs`
- Modify: `src/main.rs` (declare `mod autostart;`)

- [ ] **Step 1: Write `src/autostart.rs`**

Create `src/autostart.rs`:
```rust
//! macOS LaunchAgent install/uninstall for quickshot autostart.
//!
//! On non-macOS platforms the functions return an error — the plan
//! explicitly targets macOS for Iter 3.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

const LABEL: &str = "com.quickshot.daemon";

#[cfg(target_os = "macos")]
pub fn install() -> Result<()> {
    let bin = std::env::current_exe().context("resolve current executable path")?;
    let bin_str = bin.to_string_lossy().into_owned();
    let plist = render_plist(&bin_str);
    let plist_path = plist_path()?;
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create LaunchAgents dir {}", parent.display())
        })?;
    }
    std::fs::write(&plist_path, plist).with_context(|| {
        format!("write plist to {}", plist_path.display())
    })?;
    // launchctl load — non-zero exit is a warning, not fatal (the plist is already on disk).
    let status = std::process::Command::new("launchctl")
        .arg("load")
        .arg(&plist_path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "autostart: launchctl load exited with {s}; plist installed but not loaded"
        ),
        Err(e) => eprintln!("autostart: launchctl not runnable: {e}"),
    }
    println!("installed autostart → {}", plist_path.display());
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let plist_path = plist_path()?;
    // launchctl unload — errors non-fatal (agent may already be unloaded)
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .status();
    match std::fs::remove_file(&plist_path) {
        Ok(()) => println!("removed autostart"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("removed autostart (nothing to remove)");
        }
        Err(e) => {
            bail!("failed to remove {}: {e}", plist_path.display());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install() -> Result<()> {
    bail!("autostart is only supported on macOS");
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<()> {
    bail!("autostart is only supported on macOS");
}

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME env var not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

/// Render the plist with `bin` substituted in. Exposed for unit testing.
pub fn render_plist(bin: &str) -> String {
    let escaped = escape_xml(bin);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{escaped}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardErrorPath</key>
    <string>/tmp/quickshot.stderr.log</string>
    <key>StandardOutPath</key>
    <string>/tmp/quickshot.stdout.log</string>
</dict>
</plist>
"#
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_bin_path() {
        let out = render_plist("/Users/test/quickshot");
        assert!(out.contains("<string>/Users/test/quickshot</string>"));
        assert!(out.contains("<key>Label</key>"));
        assert!(out.contains("com.quickshot.daemon"));
        assert!(out.contains("<key>RunAtLoad</key>"));
        assert!(out.contains("<true/>"));
    }

    #[test]
    fn plist_escapes_xml_chars() {
        let out = render_plist("/home/user/dir with <brackets>&ampersands/bin");
        assert!(out.contains("&lt;brackets&gt;"));
        assert!(out.contains("&amp;ampersands"));
        assert!(!out.contains("<brackets>"));
    }

    #[test]
    fn escape_xml_cases() {
        assert_eq!(escape_xml("a&b"), "a&amp;b");
        assert_eq!(escape_xml("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(escape_xml("plain/path"), "plain/path");
    }
}
```

- [ ] **Step 2: Declare the module in `src/main.rs`**

Add `mod autostart;`. Module list becomes:
```rust
mod app;
mod autostart;
mod capture;
mod clipboard;
mod config;
mod crop;
mod file_save;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;
```

- [ ] **Step 3: Run tests**

Run:
```bash
cargo test autostart
```
Expected: 3 tests pass.

Full suite:
```bash
cargo test
```
Expected: 71 passed / 0 failed / 2 ignored (68 + 3 new).

- [ ] **Step 4: Verify release build**

Run:
```bash
cargo build --release
```
Expected: clean. `install`/`uninstall`/`plist_path` will be dead-code — expected; Task 4 wires CLI flags.

- [ ] **Step 5: Commit**

```bash
git add src/autostart.rs src/main.rs
git commit -m "feat(autostart): macOS LaunchAgent plist install/uninstall"
```

---

## Task 4: Wire config + CLI dispatch + hotkey rebinding

Now connect the modules written in Tasks 1–3 into the application startup. `hotkey::register` signature changes; `App::new` gains a `Config` parameter; `main.rs` parses CLI flags and dispatches.

**Files:**
- Modify: `src/hotkey.rs`
- Modify: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Change `hotkey::register` to accept `ParsedHotkey`**

Overwrite `src/hotkey.rs`:
```rust
use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use global_hotkey::hotkey::HotKey;
use std::thread;
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;
use crate::config::ParsedHotkey;

/// Keep the manager alive for the program's lifetime.
/// Dropping it unregisters both hotkeys.
pub struct HotkeyGuard {
    _manager: GlobalHotKeyManager,
}

pub fn register(
    proxy: EventLoopProxy<UserEvent>,
    region: ParsedHotkey,
    fullscreen: ParsedHotkey,
) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new().context("new GlobalHotKeyManager")?;

    let hk_region = HotKey::new(Some(region.modifiers), region.code);
    let hk_screen = HotKey::new(Some(fullscreen.modifiers), fullscreen.code);
    manager
        .register(hk_region)
        .with_context(|| format!("register region hotkey {}", region.raw))?;
    manager
        .register(hk_screen)
        .with_context(|| format!("register fullscreen hotkey {}", fullscreen.raw))?;

    let region_id = hk_region.id();
    let screen_id = hk_screen.id();

    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(event) = receiver.try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            let msg = if event.id == region_id {
                Some(UserEvent::CaptureRegion)
            } else if event.id == screen_id {
                Some(UserEvent::CaptureScreen)
            } else {
                None
            };
            if let Some(m) = msg {
                let _ = proxy.send_event(m);
            }
        }
        thread::sleep(Duration::from_millis(25));
    });

    Ok(HotkeyGuard { _manager: manager })
}
```

- [ ] **Step 2: Update `App` to hold `Config` and expose it through the existing flow**

In `src/app.rs`:

Change the `App` struct from:
```rust
pub struct App {
    overlay: Option<Overlay>,
}
```
to:
```rust
pub struct App {
    overlay: Option<Overlay>,
    config: crate::config::Config,
}
```

Change `App::new`:
```rust
impl App {
    pub fn new(config: crate::config::Config) -> Self {
        Self {
            overlay: None,
            config,
        }
    }
```

(Leave all other methods on `App` as they are for Task 4 — we'll wire save + notification toggles in Task 5.)

- [ ] **Step 3: Refactor `src/main.rs` to parse CLI and load config**

Overwrite `src/main.rs`:
```rust
mod app;
mod autostart;
mod capture;
mod clipboard;
mod config;
mod crop;
mod file_save;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--install-autostart") => {
            return autostart::install();
        }
        Some("--uninstall-autostart") => {
            return autostart::uninstall();
        }
        Some("--help") | Some("-h") => {
            print_usage();
            return Ok(());
        }
        Some(unknown) => {
            eprintln!("unknown argument: {unknown}");
            print_usage();
            std::process::exit(2);
        }
        None => {}
    }

    permission::preflight()?;
    let config = config::Config::load();

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let _hotkey_guard = hotkey::register(
        proxy.clone(),
        config.hotkey.region.clone(),
        config.hotkey.fullscreen.clone(),
    )?;
    let _tray_guard = tray::install(proxy.clone())?;

    println!(
        "quickshot running; {} (region), {} (fullscreen). Quit via tray or Ctrl+C.",
        config.hotkey.region.raw, config.hotkey.fullscreen.raw
    );

    let mut app = app::App::new(config);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn print_usage() {
    println!(
        "quickshot — small fast screenshot daemon\n\
         \n\
         USAGE:\n\
             quickshot                       run the daemon (default)\n\
             quickshot --install-autostart   install LaunchAgent (macOS)\n\
             quickshot --uninstall-autostart remove LaunchAgent (macOS)\n\
             quickshot --help                show this message\n\
         \n\
         Config: ~/.config/quickshot/config.toml\n"
    );
}
```

- [ ] **Step 4: Make `ParsedHotkey` implement `Clone`**

In `src/config.rs`, add `Clone` to the derives on `ParsedHotkey` (should already be `#[derive(Debug, Clone)]` per the plan — double-check, add if missing).

`Modifiers` and `Code` both implement `Copy` in `global-hotkey 0.6`, so `ParsedHotkey: Clone` is just a struct-level clone.

- [ ] **Step 5: Build + test**

Run:
```bash
cargo build --release
cargo test
```
Expected: clean build. 71 passed / 0 failed / 2 ignored (no new tests in this task — it's integration glue).

The dead-code warnings for `Config` should now be mostly gone (it's used in main + app). `file_save::save_png`, `CaptureMode`, and related interpolation helpers remain dead-code — Task 5 fixes those.

- [ ] **Step 6: Clippy**

Run:
```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. If clippy complains about the `ParsedHotkey::clone()` calls in `main.rs` ("redundant clone"), adjust by passing references into `hotkey::register` if the signature allows it, or add `#[allow(clippy::redundant_clone)]` with a justification if the clone is required for ownership.

Actually: `hotkey::register` takes `ParsedHotkey` by value; `main.rs` clones because `config.hotkey.region` is a field that needs to be readable AFTER `hotkey::register` (for the banner println). The clones are necessary. If clippy flags them, add `#[allow]` with a comment.

- [ ] **Step 7: Commit**

```bash
git add src/hotkey.rs src/app.rs src/main.rs src/config.rs
git commit -m "feat(config): wire config into hotkey registration and CLI dispatch"
```

---

## Task 5: Wire file-save into both capture flows

Add save-on-success to both `App::confirm` (region) and `App::capture_full_screen` (fullscreen). Also wire the notification toggle.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Update `App::capture_full_screen` to save + gate notification**

Find the `capture_full_screen` method:
```rust
    fn capture_full_screen(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        match capture::capture_at_cursor() {
            Ok((frame, _geom)) => {
                let (w, h) = frame.dimensions();
                if let Err(e) = clipboard::put_image(&frame) {
                    eprintln!("clipboard error: {e:?}");
                    return;
                }
                println!("copied {}x{} (full screen) to clipboard", w, h);
                if let Err(e) = crate::notification::screenshot_copied(w, h) {
                    eprintln!("notification error: {e:?}");
                }
            }
            Err(e) => {
                eprintln!("capture error: {e:?}");
            }
        }
    }
```

Replace with:
```rust
    fn capture_full_screen(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        match capture::capture_at_cursor() {
            Ok((frame, _geom)) => {
                let (w, h) = frame.dimensions();
                if let Err(e) = clipboard::put_image(&frame) {
                    eprintln!("clipboard error: {e:?}");
                    return;
                }
                println!("copied {}x{} (full screen) to clipboard", w, h);
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &frame,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Fullscreen,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
                if self.config.general.notification_on_fullscreen {
                    if let Err(e) = crate::notification::screenshot_copied(w, h) {
                        eprintln!("notification error: {e:?}");
                    }
                }
            }
            Err(e) => {
                eprintln!("capture error: {e:?}");
            }
        }
    }
```

- [ ] **Step 2: Update `App::confirm` to save on success (region)**

Find:
```rust
    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        if let Err(e) = clipboard::put_image(&cropped) {
            eprintln!("clipboard error: {e:?}");
        } else {
            println!("copied {}x{} to clipboard", cropped.width(), cropped.height());
        }
        drop(overlay);
    }
```

Replace with:
```rust
    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        match clipboard::put_image(&cropped) {
            Ok(()) => {
                println!("copied {}x{} to clipboard", cropped.width(), cropped.height());
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &cropped,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => {
                eprintln!("clipboard error: {e:?}");
            }
        }
        drop(overlay);
    }
```

- [ ] **Step 3: Build + run tests**

Run:
```bash
cargo build --release
cargo test
```
Expected: clean build. 71 passed / 0 failed / 2 ignored (unchanged test count). All the Iter 3 dead-code warnings should now be gone.

- [ ] **Step 4: Clippy**

Run:
```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): wire config.save + notification toggle into capture flows"
```

---

## Task 6: Polish — README, binary size, tag

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Full test + clippy run**

```bash
cargo test
cargo clippy --release --all-targets -- -D warnings
```
Expected: 71 passed / 0 failed / 2 ignored; clippy clean.

- [ ] **Step 2: Record binary size**

```bash
cargo build --release
ls -lh target/release/quickshot
```
Expected: 1.3–1.5 MB. Record the exact value.

- [ ] **Step 3: Update `README.md`**

In `README.md`, locate the `## Status (Iter 2b)` section. Replace its body and heading with:

```markdown
## Status (Iter 3)

- Region capture via configurable hotkey (default `Cmd+Shift+A`) with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- Full-screen capture via configurable hotkey (default `Cmd+Shift+S`) — cursor's monitor, clipboard + optional notification
- Menu-bar tray icon with Capture Region / Capture Screen / Quit
- Configurable save-to-disk with templated filenames (`~/.config/quickshot/config.toml`)
- macOS autostart via `quickshot --install-autostart` / `--uninstall-autostart`
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No cross-screen selection

Release binary size on this machine: <PASTE-SIZE-HERE>.
```

Replace `<PASTE-SIZE-HERE>` with the actual size from Step 2.

Then append a new `## Config` section right after Status:

```markdown
## Config

On first run, quickshot writes `~/.config/quickshot/config.toml` with defaults. Edit and restart to apply changes.

```toml
[hotkey]
region = "Cmd+Shift+A"
fullscreen = "Cmd+Shift+S"

[save]
enabled = false
directory = "~/Desktop"
filename_template = "Screenshot_{datetime}.png"

[general]
notification_on_fullscreen = true
```

Filename template placeholders: `{date}` `{time}` `{datetime}` `{w}` `{h}` `{mode}`.

## Autostart (macOS)

    quickshot --install-autostart      # installs LaunchAgent + launches at login
    quickshot --uninstall-autostart    # removes it
```

- [ ] **Step 4: Commit polish**

```bash
git add README.md
git commit -m "docs: record Iter 3 status, config schema, autostart usage"
```

- [ ] **Step 5: Tag**

```bash
git tag -a v0.4.0-iter3 -m "Iter 3: TOML config, file save, macOS autostart, hotkey rebinding"
```

---

## Manual verification checklist (whole plan)

1. Fresh terminal; delete `~/.config/quickshot/config.toml` if present; run `./target/release/quickshot`. Config file is created with defaults; banner mentions both hotkeys.
2. Press `Cmd+Shift+A` — overlay flow works as Iter 2b.
3. Press `Cmd+Shift+S` — full-screen capture + notification as Iter 2b.
4. Edit config: `save.enabled = true`, `save.directory = "~/tmp/quickshot"`, keep default template. Restart. Capture region → file appears at `~/tmp/quickshot/Screenshot_YYYY-MM-DD_HH-MM-SS.png`; clipboard also has it.
5. Trigger a full-screen capture → PNG lands in the same directory; filename has its own timestamp.
6. Capture twice within the same second → second file is `..._1.png`.
7. Edit `save.filename_template = "{mode}_{w}x{h}_{date}.png"`. Restart. Capture region → file like `region_1920x1080_2026-04-19.png`.
8. Edit `general.notification_on_fullscreen = false`. Restart. Full-screen capture → NO notification; clipboard still works.
9. Edit `hotkey.region = "Cmd+Shift+X"`. Restart. `Cmd+Shift+X` opens overlay; `Cmd+Shift+A` does nothing.
10. Edit `hotkey.region = "Hyper+A"` (bogus). Restart. Warning in terminal about invalid hotkey; daemon runs with region hotkey defaulted to `Cmd+Shift+A`.
11. Run `./target/release/quickshot --help` → usage prints; exits 0.
12. Run `./target/release/quickshot --install-autostart` → plist appears at `~/Library/LaunchAgents/com.quickshot.daemon.plist`. `launchctl list | grep quickshot` shows the agent. Log out / log in → quickshot is running (tray icon visible).
13. Run `./target/release/quickshot --uninstall-autostart` → plist removed. `launchctl list | grep quickshot` shows nothing.

Regression checks (Iter 2b invariants):
14. Region + full-screen capture still work end-to-end.
15. Multi-display follows cursor.
16. macOS overlay covers dock + menu bar.
17. Retina font sizing crisp.
18. Binary size ≤ 1.5 MB.

---

## Out of scope (deferred)

**Iter 2c polish** (parallel track, independent):
- Subset JetBrains Mono TTF
- Extract coord-scaling utility
- `estimate_text_width` real advance-width query
- Tray menu debounce

**Iter 3.5** (if ever needed):
- egui settings window for non-TOML-editors
- Hot config reload

**Iter 4**:
- Capture delay / timer mode
- Annotation tools post-capture
- OCR / text extraction
- Cloud upload integrations

**Never:**
- Cross-screen selection
