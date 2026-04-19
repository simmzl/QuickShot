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
        unsafe { std::env::set_var("HOME", "/Users/tester") };
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
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
        assert_eq!(c.save.filename_template, "Screenshot_{datetime}.png");
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
        let c = Config::parse(src).unwrap();
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
    }

    #[test]
    fn default_toml_is_valid() {
        let c = Config::parse(DEFAULT_TOML).unwrap();
        assert_eq!(c.hotkey.region.raw, "Cmd+Shift+A");
    }
}
