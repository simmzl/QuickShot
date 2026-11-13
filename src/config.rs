//! User-configurable daemon settings, loaded from
//! `~/.config/QuickShot/config.toml` at startup. Never panics on bad input;
//! failed fields fall back to defaults with a stderr warning.

use anyhow::{anyhow, bail, Context, Result};
use global_hotkey::hotkey::{Code, Modifiers};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

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
    fallback_for: Option<String>,
}

impl ParsedHotkey {
    pub fn menu_label(&self) -> String {
        match &self.fallback_for {
            Some(key) => format!("{} (fallback for {})", self.raw, key),
            None => self.raw.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub filename_template: String,
}

#[derive(Debug, Clone)]
pub struct GeneralConfig {
    pub fullscreen_feedback: FullscreenFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenFeedback {
    FlashAndNotification,
    Flash,
    Off,
}

impl FullscreenFeedback {
    pub fn flashes(self) -> bool {
        matches!(self, Self::FlashAndNotification | Self::Flash)
    }

    pub fn notifies(self) -> bool {
        self == Self::FlashAndNotification
    }
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
        Self::read_from(&path).unwrap_or_else(|error| {
            eprintln!("config: {error:#}; using defaults");
            Config::defaults()
        })
    }

    pub fn read_existing() -> Result<Config> {
        let path = config_path().context("could not resolve config directory")?;
        Self::read_from(&path)
    }

    fn read_from(path: &Path) -> Result<Config> {
        let source = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&source).with_context(|| format!("parse {}", path.display()))
    }

    pub fn defaults() -> Config {
        Config {
            hotkey: HotkeyConfig {
                region: default_region_hotkey(),
                fullscreen: default_fullscreen_hotkey(),
            },
            save: SaveConfig {
                enabled: false,
                directory: expand_home(if cfg!(windows) { "~/Pictures" } else { "~/Desktop" }),
                filename_template: "QuickShot_{datetime}.png".to_string(),
            },
            general: GeneralConfig {
                fullscreen_feedback: FullscreenFeedback::FlashAndNotification,
            },
        }
    }

    pub fn parse(source: &str) -> Result<Config> {
        #[cfg(windows)]
        let raw = parse_windows_config(source)?;
        #[cfg(not(windows))]
        let raw: RawConfig = toml::from_str(source)?;
        let mut cfg = Config::defaults();

        if let Some(h) = raw.hotkey {
            for (configured, binding) in [
                (h.region, &mut cfg.hotkey.region),
                (h.fullscreen, &mut cfg.hotkey.fullscreen),
            ] {
                if let Some(key) = configured {
                    match parse_hotkey_string(&key) {
                        Ok(parsed) => *binding = parsed,
                        Err(error) => {
                            eprintln!("config: invalid shortcut {key:?}: {error}; using {}", binding.raw);
                            binding.fallback_for = Some(key);
                        }
                    }
                }
            }
            if cfg.hotkey.region.code == cfg.hotkey.fullscreen.code
                && cfg.hotkey.region.modifiers == cfg.hotkey.fullscreen.modifiers
            {
                eprintln!("config: both actions use the same shortcut; using default shortcuts");
                for (binding, mut default) in [
                    (&mut cfg.hotkey.region, default_region_hotkey()),
                    (&mut cfg.hotkey.fullscreen, default_fullscreen_hotkey()),
                ] {
                    default.fallback_for = binding.fallback_for.take()
                        .or_else(|| (binding.raw != default.raw).then(|| binding.raw.clone()));
                    *binding = default;
                }
            }
        }
        if let Some(s) = raw.save {
            if let Some(v) = s.enabled {
                cfg.save.enabled = v;
            }
            if let Some(d) = s.directory {
                cfg.save.directory = expand_home(d.get_ref());
            }
            // Older generated configs explicitly contained the old default.
            if let Some(t) = s.filename_template.filter(|t| t != "Screenshot_{datetime}.png") {
                cfg.save.filename_template = t;
            }
        }
        if let Some(g) = raw.general {
            if g.notification_on_fullscreen == Some(false) {
                cfg.general.fullscreen_feedback = FullscreenFeedback::Flash;
            }
            if let Some(mode) = g.fullscreen_feedback {
                match mode.as_str() {
                    "notification" => cfg.general.fullscreen_feedback = FullscreenFeedback::FlashAndNotification,
                    "flash" => cfg.general.fullscreen_feedback = FullscreenFeedback::Flash,
                    "off" => cfg.general.fullscreen_feedback = FullscreenFeedback::Off,
                    _ => eprintln!("config: unknown fullscreen_feedback {mode:?}; using fallback"),
                }
            }
        }
        Ok(cfg)
    }
}

// Explorer paths may contain invalid TOML escapes or valid ones such as \t.
// Let TOML identify the field by its source span so comments, other strings,
// and unrelated syntax errors keep their ordinary TOML semantics.
#[cfg(any(windows, test))]
fn parse_windows_config(source: &str) -> Result<RawConfig> {
    for (start, _) in source.match_indices('"') {
        let rest = &source[start + 1..];
        let bytes = rest.as_bytes();
        let drive = matches!(bytes, [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic());
        let home = rest.starts_with(r"~\") || rest.starts_with(r"～\");
        let unc = rest.starts_with(r"\\");
        if !(drive || home || unc) {
            continue;
        }
        // Already escaped roots (F:\\ or \\\\server) use standard TOML decoding.
        let after_root = if drive { &rest[2..] } else { rest.trim_start_matches(['~', '～']) };
        if (unc && after_root.starts_with(r"\\\\")) || (!unc && after_root.starts_with(r"\\")) {
            continue;
        }
        let Some(end) = rest.find(['"', '\n', '\r']) else { continue };
        if rest.as_bytes()[end] != b'"' { continue; }
        let mut candidate = source.to_owned();
        candidate.replace_range(start + 1..start + 1 + end, "");
        if let Ok(mut raw) = toml::from_str::<RawConfig>(&candidate) {
            if let Some(dir) = raw.save.as_mut().and_then(|save| save.directory.as_mut()) {
                if dir.span() == (start..start + 2) {
                    *dir.get_mut() = rest[..end].to_owned();
                    return Ok(raw);
                }
            }
        }
    }
    Ok(toml::from_str(source)?)
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
    directory: Option<toml::Spanned<String>>,
    filename_template: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawGeneral {
    fullscreen_feedback: Option<String>,
    notification_on_fullscreen: Option<bool>,
}

#[cfg(not(target_os = "windows"))]
pub const DEFAULT_TOML: &str = r#"# QuickShot config — changes apply automatically.
# Regenerate defaults by deleting this file and re-launching QuickShot.

[hotkey]
# Format: modifiers joined by "+", ending with a key. Modifiers (case-insensitive):
#   Cmd (macOS) / Ctrl / Alt / Opt / Shift / Meta / Super
# Function keys can be used alone, e.g. region = "F2".
# Keys: A-Z, 0-9, F1-F20, or named keys (Space, Enter, Tab, Backspace, Escape).
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
filename_template = "QuickShot_{datetime}.png"

[general]
# "notification" flashes and sends a notification, "flash" only flashes, "off" disables both.
fullscreen_feedback = "notification"
"#;

#[cfg(target_os = "windows")]
pub const DEFAULT_TOML: &str = r#"# QuickShot config — changes apply automatically.
# Regenerate defaults by deleting this file and re-launching QuickShot.

[hotkey]
# Format: modifiers joined by "+", ending with a key. Modifiers (case-insensitive):
#   Ctrl / Alt / Shift / Meta / Super (Win key) / Cmd (alias for Meta)
# Function keys can be used alone, e.g. region = "F2".
# Keys: A-Z, 0-9, F1-F11 or F13-F24 (Windows reserves F12), or named keys (Space, Enter, Tab, Backspace, Escape).
# Note: Win+Shift+S is reserved by Windows Snip & Sketch; avoid Super/Meta+Shift+S.
region = "Ctrl+Shift+A"
fullscreen = "Ctrl+Shift+S"

[save]
# When true, every successful capture also writes a PNG to `directory`.
enabled = false
# Set enabled = true to save PNGs. ~ and ～ expand to your user profile.
# Paste an Explorer path using either kind of quote, e.g. directory = "F:\images".
directory = '~/Pictures'
# Available placeholders: {date}, {time}, {datetime}, {w}, {h}, {mode}
#   {date}     2026-04-19
#   {time}     15-04-30
#   {datetime} 2026-04-19_15-04-30
#   {w}, {h}   1920, 1080 (physical pixels)
#   {mode}     region | fullscreen
filename_template = "QuickShot_{datetime}.png"

[general]
# "notification" flashes and sends a notification, "flash" only flashes, "off" disables both.
fullscreen_feedback = "notification"
"#;

pub fn config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("QuickShot").join("config.toml"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return Some(PathBuf::from(appdata).join("QuickShot").join("config.toml"));
            }
        }
        let home = home_dir()?;
        Some(home.join(".config").join("QuickShot").join("config.toml"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = home_dir()?;
        Some(home.join(".config").join("QuickShot").join("config.toml"))
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
        if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
            if !drive.is_empty() && !path.is_empty() {
                return Some(PathBuf::from(format!("{drive}{path}")));
            }
        }
    }
    std::env::var("HOME").ok().map(PathBuf::from)
}

pub fn expand_home(input: &str) -> PathBuf {
    let path = Path::new(input);
    let relative = path.strip_prefix("~");
    #[cfg(windows)]
    let relative = relative.or_else(|_| path.strip_prefix("～"));
    if let (Ok(rest), Some(home)) = (relative, home_dir()) {
        return home.join(rest);
    }
    path.to_owned()
}

pub fn parse_hotkey_string(input: &str) -> Result<ParsedHotkey> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("empty hotkey string");
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    let (key_str, modifier_strs) = parts.split_last().unwrap();
    let mut modifiers = Modifiers::empty();
    for m in modifier_strs {
        let bit = parse_modifier(m).ok_or_else(|| anyhow!("unknown modifier: {m}"))?;
        if modifiers.contains(bit) {
            bail!("duplicate modifier: {m}");
        }
        modifiers |= bit;
    }
    let code = parse_code(key_str).ok_or_else(|| anyhow!("unknown key: {key_str}"))?;
    #[cfg(windows)]
    if code == Code::F12 {
        bail!("Windows reserves F12 for the debugger");
    }
    #[cfg(target_os = "macos")]
    if matches!(code, Code::F21 | Code::F22 | Code::F23 | Code::F24) {
        bail!("macOS hotkeys support function keys F1 through F20");
    }
    // parse_code recognizes F1–F24 and the ordinary letter F.
    if modifiers.is_empty() && !(key_str.len() > 1 && key_str.starts_with(['F', 'f'])) {
        bail!("only function keys can be used without a modifier");
    }
    Ok(ParsedHotkey {
        modifiers,
        code,
        raw: trimmed.to_string(),
        fallback_for: None,
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

#[cfg(target_os = "windows")]
const DEFAULT_PRIMARY_MODIFIER: &str = "Ctrl";
#[cfg(not(target_os = "windows"))]
const DEFAULT_PRIMARY_MODIFIER: &str = "Cmd";

fn default_region_hotkey() -> ParsedHotkey {
    let s = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+A");
    parse_hotkey_string(&s).expect("default region hotkey must parse")
}

fn default_fullscreen_hotkey() -> ParsedHotkey {
    let s = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+S");
    parse_hotkey_string(&s).expect("default fullscreen hotkey must parse")
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
    fn parse_standalone_function_keys() {
        for key in ["F1", "f2", "F20"] {
            assert!(parse_hotkey_string(key).unwrap().modifiers.is_empty());
        }
        for key in ["F", "F25", "2", "Space", "Ctrl"] {
            assert!(parse_hotkey_string(key).is_err());
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn unavailable_function_keys_fall_back_without_losing_other_settings() {
        #[cfg(windows)]
        let unavailable = ["F12", "Ctrl+F12"];
        #[cfg(not(windows))]
        let unavailable = ["F21", "Cmd+F24"];
        for key in unavailable {
            let source = format!("[hotkey]\nregion = '{key}'\nfullscreen = 'F2'\n[save]\nenabled = true");
            let cfg = Config::parse(&source).unwrap();
            assert_eq!(cfg.hotkey.region.raw, default_region_hotkey().raw);
            assert!(cfg.hotkey.region.menu_label().contains(key));
            assert_eq!(cfg.hotkey.fullscreen.raw, "F2");
            assert!(cfg.save.enabled);
        }
        #[cfg(windows)]
        assert!(parse_hotkey_string("F24").is_ok());
    }

    #[test]
    fn fallback_cannot_leave_both_actions_on_the_same_key() {
        let region = default_region_hotkey().raw;
        let cfg = Config::parse(&format!("[hotkey]\nregion = 'invalid'\nfullscreen = '{region}'")).unwrap();
        assert_eq!(cfg.hotkey.region.raw, region);
        assert_eq!(cfg.hotkey.fullscreen.raw, default_fullscreen_hotkey().raw);
    }

    #[test]
    fn fullscreen_feedback_for_existing_and_new_configs() {
        for (source, flash, notify) in [
            ("", true, true),
            (DEFAULT_TOML, true, true),
            ("[general]\nnotification_on_fullscreen = true", true, true),
            ("[general]\nnotification_on_fullscreen = false", true, false),
            ("[general]\nfullscreen_feedback = 'notification'", true, true),
            ("[general]\nfullscreen_feedback = 'flash'", true, false),
            ("[general]\nfullscreen_feedback = 'off'", false, false),
        ] {
            let mode = Config::parse(source).unwrap().general.fullscreen_feedback;
            assert_eq!((mode.flashes(), mode.notifies()), (flash, notify), "{source}");
        }
    }

    #[test]
    fn feedback_overrides_legacy_setting() {
        let config = Config::parse("[general]\nnotification_on_fullscreen = false\nfullscreen_feedback = 'flash'").unwrap();
        assert_eq!(config.general.fullscreen_feedback, FullscreenFeedback::Flash);
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
    fn save_directory_accepts_literal_windows_paths() {
        for (value, expected) in [
            (r"'F:\images'", r"F:\images"),
            (r#""F:/images""#, "F:/images"),
            (r#""F:\\images""#, r"F:\images"),
            (r"'\\server\share\截图'", r"\\server\share\截图"),
        ] {
            let cfg = Config::parse(&format!("[save]\nenabled = true\ndirectory = {value}")).unwrap();
            assert!(cfg.save.enabled);
            assert_eq!(cfg.save.directory, Path::new(expected));
        }

    }

    #[test]
    fn explorer_paths_are_literal_only_in_the_save_directory() {
        for (value, expected) in [
            (r#""F:\images""#, r"F:\images"),
            (r#""F:\temp\new\测试's #1\""#, r"F:\temp\new\测试's #1\"),
            (r#""F:\""#, r"F:\"),
            (r#""\\server\share\new""#, r"\\server\share\new"),
            (r#""\\\\server\\share\\old""#, r"\\server\share\old"),
            (r#""F:\\images""#, r"F:\images"),
            (r"'F:\images'", r"F:\images"),
            (r#""～\截图""#, r"～\截图"),
        ] {
            let source = format!("# ignored: \"C:\\another\"\r\n[save]\r\n  directory = {value} # path\r\nenabled = true");
            let save = parse_windows_config(&source).unwrap().save.unwrap();
            assert_eq!(save.directory.unwrap().get_ref(), expected);
            assert_eq!(save.enabled, Some(true));
        }
        assert!(parse_windows_config(r#"[save]
filename_template = "F:\images""#).is_err());
        assert!(parse_windows_config(r#"[save]
directory = "F:\images"
enabled = invalid"#).is_err());
    }

    #[test]
    fn generated_legacy_filename_uses_the_new_default() {
        let cfg = Config::parse("[save]\nfilename_template = 'Screenshot_{datetime}.png'").unwrap();
        assert_eq!(cfg.save.filename_template, Config::defaults().save.filename_template);
        assert_eq!(Config::parse(DEFAULT_TOML).unwrap().save.filename_template, cfg.save.filename_template);
    }

    #[test]
    fn expand_home_basic() {
        let home = home_dir().unwrap();
        assert_eq!(expand_home("~/Desktop"), home.join("Desktop"));
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("relative/path"), Path::new("relative/path"));
        assert_eq!(expand_home("~other/path"), Path::new("~other/path"));
        #[cfg(windows)]
        for input in [r"~\Pictures", r"～\Pictures", "～/Pictures"] {
            let cfg = Config::parse(&format!("[save]\ndirectory = '{input}'")).unwrap();
            assert_eq!(cfg.save.directory, home.join("Pictures"));
        }
    }

    #[test]
    fn defaults_parse() {
        let c = Config::defaults();
        let expected_region = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+A");
        let expected_full = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+S");
        assert_eq!(c.hotkey.region.raw, expected_region);
        assert_eq!(c.hotkey.fullscreen.raw, expected_full);
        assert!(!c.save.enabled);
        assert_eq!(c.save.filename_template, "QuickShot_{datetime}.png");
        assert_eq!(c.general.fullscreen_feedback, FullscreenFeedback::FlashAndNotification);
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
        assert_eq!(c.general.fullscreen_feedback, FullscreenFeedback::Flash);
    }

    #[test]
    fn parse_partial_toml_uses_defaults() {
        let src = r#"
            [save]
            enabled = true
        "#;
        let c = Config::parse(src).unwrap();
        assert!(c.save.enabled);
        let expected_region = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+A");
        assert_eq!(c.hotkey.region.raw, expected_region);
        assert_eq!(c.save.filename_template, "QuickShot_{datetime}.png");
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
        let expected = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+A");
        assert_eq!(c.hotkey.region.raw, expected);
    }

    #[test]
    fn default_toml_is_valid() {
        let c = Config::parse(DEFAULT_TOML).unwrap();
        let expected = format!("{DEFAULT_PRIMARY_MODIFIER}+Shift+A");
        assert_eq!(c.hotkey.region.raw, expected);
    }
}
