# quickshot Iter 3 — Config, File Save, Autostart (Design Spec)

**Date:** 2026-04-19
**Status:** Design approved (auto by user delegation); ready for implementation plan.
**Predecessor:** Iter 2b (merge `78e1fb6`, tag `v0.3.0-iter2b`) — system integration.
**Successor:** TBD (maybe Iter 3.5 for optional egui settings UI if ever wanted).

## Goal

Make quickshot useful as a daily-driver background daemon by adding:
1. **Config file** at `~/.config/quickshot/config.toml` — controls hotkeys, save behavior, and notifications. Loaded at startup; no hot reload.
2. **File-save mode** — when enabled in config, each successful capture also writes a PNG to disk with a templated filename.
3. **Autostart** on macOS — CLI subcommands to install/uninstall a `LaunchAgent` plist so the daemon launches at login.
4. **Hotkey rebinding** via config — users can change the two hotkeys by editing the TOML file; no GUI required.

## Non-Goals (explicitly deferred)

- **egui settings window** — intentionally skipped. Adding egui + wgpu would bloat the binary from ~1.2 MB to ~8 MB+, which violates the project's "small, fast, pure-Rust" ethos. Config file is adequate for the target user (developers on macOS). If a UI is ever needed, it's a separate iteration (Iter 3.5).
- **Hot config reload** — changing `config.toml` while the daemon runs does not take effect until restart. Simplifies the implementation dramatically.
- **Cross-platform autostart** — Windows autostart uses a different mechanism (registry Run key or startup folder). macOS-only for Iter 3.
- **Config migration / versioning** — first config format; no upgrade path needed yet.
- **Per-monitor config** — one set of settings for all monitors.
- **Capture delay / timer mode** — Iter 4+.
- **TTF subset / coord-helper extraction / estimate_text_width real advance** — still Iter 2c polish items.
- **Tray menu debounce** — Iter 2c.

---

## UX Specification

### Config file format

Path: `~/.config/quickshot/config.toml` (resolved via `$XDG_CONFIG_HOME/quickshot/config.toml` if set, else `$HOME/.config/quickshot/config.toml`).

Default content (written on first run if the file doesn't exist):

```toml
# quickshot config — edit and restart the daemon to apply changes.
# Regenerate defaults by deleting this file and re-launching quickshot.

[hotkey]
# Format: modifiers joined by "+", ending with a key. Modifiers (case-insensitive):
#   Cmd (macOS) / Ctrl / Alt / Opt / Shift / Meta / Super
# Keys: A-Z, 0-9, F1-F24, or any winit Code name (e.g. "Space", "Enter").
region = "Cmd+Shift+A"
fullscreen = "Cmd+Shift+S"

[save]
# When true, every successful capture also writes a PNG to `directory`.
enabled = false
# `~` is expanded to $HOME. Missing directories are created.
directory = "~/Desktop"
# Available placeholders: {date}, {time}, {datetime}, {w}, {h}, {mode}
#   {date}     → 2026-04-19
#   {time}     → 15-04-30
#   {datetime} → 2026-04-19_15-04-30
#   {w}, {h}   → 1920, 1080 (physical pixels)
#   {mode}     → region | fullscreen
filename_template = "Screenshot_{datetime}.png"

[general]
# Show a system notification after a successful full-screen capture.
# Region captures never show one (overlay dismissal is already feedback).
notification_on_fullscreen = true
```

### Configuration semantics

- **Missing config file:** create the directory (if missing), write defaults to `config.toml`, and continue with defaults. No error.
- **Malformed config (parse error):** log to stderr with a specific line/col diagnostic, continue with defaults. Do not crash. Do not overwrite the malformed file (user may want to fix by hand).
- **Invalid hotkey string:** log stderr error, fall back to that hotkey's default. Other config fields parse independently.
- **Invalid save directory:** on capture, if directory creation fails, log stderr, skip the file save, clipboard + notification still proceed.
- **Unknown table/key:** silently ignored (TOML parsing is lenient about unknown fields by default with serde).

### Hotkey string format

Grammar:
```
hotkey      = modifier ("+" modifier)* "+" key
modifier    = "Cmd" | "Ctrl" | "Alt" | "Opt" | "Shift" | "Meta" | "Super"
key         = letter | digit | fn-key | named-key
letter      = "A" .. "Z"
digit       = "0" .. "9"
fn-key      = "F1" .. "F24"
named-key   = "Space" | "Enter" | "Tab" | "Backspace" | "Escape" | ... (winit Code variants)
```

Parsing rules:
- Case-insensitive for modifiers and key names.
- `Cmd` and `Meta` map to the same modifier (`Modifiers::META`).
- `Alt` and `Opt` map to the same modifier (`Modifiers::ALT`).
- `Super` is a Linux alias for Meta; accept for compatibility.
- Minimum 1 modifier required (platform convention; protects against accidental triggers).
- Duplicate modifiers → parse error.
- Unknown modifier/key → parse error with the offending token in the message.

### Autostart CLI subcommands

```
quickshot                           # run the daemon (existing default)
quickshot --install-autostart       # install LaunchAgent, print status, exit 0
quickshot --uninstall-autostart     # remove LaunchAgent, print status, exit 0
quickshot --help                    # brief usage, exit 0
```

Subcommands do NOT start the daemon; they run their action and exit. Running `quickshot` without flags continues to launch the daemon and consume the hotkeys.

**Install behavior:**
1. Resolve the currently-running binary path via `std::env::current_exe()`.
2. Write the LaunchAgent plist to `~/Library/LaunchAgents/com.quickshot.daemon.plist`.
3. `launchctl load` the plist.
4. Print: `installed autostart → ~/Library/LaunchAgents/com.quickshot.daemon.plist`.
5. Exit 0.

**Uninstall behavior:**
1. `launchctl unload` the plist (errors non-fatal — may already be unloaded).
2. Remove the plist file (errors non-fatal — may already be gone).
3. Print: `removed autostart`.
4. Exit 0.

**Plist content (verbatim; Program points at the installed binary absolute path):**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.quickshot.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/absolute/path/to/quickshot</string>
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
```

`KeepAlive = false` means macOS won't auto-restart quickshot if the user Quits via the tray menu. That's deliberate: Quit should mean Quit.

### File save behavior

When `save.enabled = true`:
- On every successful capture (region or fullscreen), after clipboard write succeeds, also write a PNG to disk.
- Directory path: expand `~` to `$HOME`; resolve relative paths against `$HOME`.
- Ensure directory exists (`fs::create_dir_all`).
- Filename: interpolate the template against the current capture metadata.
- If the resolved filename exists, append `_1`, `_2`, … before the extension until a free slot is found (max 99 retries, else error).
- Encode the RGBA frame as PNG via the existing `image` crate.
- On save error: log stderr with the target path and error, skip the save (but clipboard + notification already succeeded).

Interpolation rules:
- `{date}` → `YYYY-MM-DD`
- `{time}` → `HH-MM-SS` (24h, zero-padded)
- `{datetime}` → `YYYY-MM-DD_HH-MM-SS`
- `{w}`, `{h}` → physical pixel dimensions of the saved PNG
- `{mode}` → `"region"` or `"fullscreen"`
- Unknown `{placeholder}` → left in place literally (with braces) + stderr warning on the first encounter.

---

## Architecture

### File layout

```
src/
├── app.rs              (modified — take config; wire save + notification toggles)
├── capture.rs          (unchanged)
├── clipboard.rs        (unchanged)
├── crop.rs             (unchanged)
├── config.rs           (new — Config struct + load/default; ~120 lines)
├── autostart.rs        (new, macOS only — install/uninstall; ~80 lines)
├── file_save.rs        (new — template interpolation + PNG write; ~100 lines)
├── hotkey.rs           (modified — accept parsed Hotkey from config)
├── main.rs             (modified — CLI parse, config load, dispatch)
├── notification.rs     (unchanged)
├── permission.rs       (unchanged)
├── text.rs             (unchanged)
├── tray.rs             (unchanged)
└── overlay/            (unchanged)
```

### `src/config.rs`

```rust
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub save: SaveConfig,
    pub general: GeneralConfig,
}

pub struct HotkeyConfig {
    pub region: ParsedHotkey,
    pub fullscreen: ParsedHotkey,
}

pub struct ParsedHotkey {
    pub modifiers: global_hotkey::hotkey::Modifiers,
    pub code: global_hotkey::hotkey::Code,
    pub raw: String,  // original string for diagnostics
}

pub struct SaveConfig {
    pub enabled: bool,
    pub directory: PathBuf,       // ~ expanded
    pub filename_template: String,
}

pub struct GeneralConfig {
    pub notification_on_fullscreen: bool,
}

impl Config {
    pub fn load() -> Config;                      // resolves path, reads, parses; never returns Err
    pub fn default_toml_contents() -> &'static str;
    fn parse(source: &str) -> Result<Config, ConfigError>;
}
```

Implementation approach:
- Use `serde` + `toml` for parsing.
- Use an internal `RawConfig` struct with all fields `Option<_>` for tolerant parsing, then fold into the strict `Config` with defaults.
- `load()` handles all IO and logging; returns `Config` by value. Internal failures are converted to warning logs + defaults.
- `~` expansion uses `std::env::var("HOME")` directly (no `dirs` crate needed for this one purpose).
- Config directory: `$XDG_CONFIG_HOME/quickshot/` if set, else `$HOME/.config/quickshot/`.

Unit tests:
- Parse valid config → expected struct.
- Parse missing section → defaults fill in.
- Parse malformed TOML → Err with line/col info.
- Parse invalid hotkey string → Err.
- Hotkey grammar: each modifier, letter A-Z, digits, F1-F24, named keys.
- `~` expansion.
- Template interpolation (tested in `file_save` tests).

### `src/autostart.rs` (macOS only)

```rust
#[cfg(target_os = "macos")]
pub fn install() -> anyhow::Result<()>;
#[cfg(target_os = "macos")]
pub fn uninstall() -> anyhow::Result<()>;

#[cfg(not(target_os = "macos"))]
pub fn install() -> anyhow::Result<()> { bail!("autostart is macOS-only") }
#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> anyhow::Result<()> { bail!("autostart is macOS-only") }
```

Implementation:
- Plist path: `$HOME/Library/LaunchAgents/com.quickshot.daemon.plist`.
- Plist content: format a `const` string template with the current executable path from `env::current_exe()`.
- `launchctl` invocation: `std::process::Command::new("launchctl").arg("load").arg(&plist_path)` (or `unload`). Non-zero exit from launchctl is a warning, not a hard error — the operation is declarative.
- Path escaping: `&` is legal in unix paths but XML-special; escape for plist via `str::replace` chain (replace `&` → `&amp;`, then `<` → `&lt;`, then `>` → `&gt;`). Most dev machines won't have these chars in their executable path, but be defensive.

Unit tests:
- Plist template substitution: given an executable path, produce a well-formed plist (assert the substring and XML escaping).

### `src/file_save.rs`

```rust
pub enum CaptureMode {
    Region,
    Fullscreen,
}

pub fn save_png(
    frame: &image::RgbaImage,
    directory: &Path,
    template: &str,
    mode: CaptureMode,
) -> anyhow::Result<PathBuf>;  // returns saved path

fn interpolate(template: &str, ctx: &TemplateContext) -> String;  // pure, unit-tested
fn uniquify(path: &Path) -> PathBuf;                              // _1 suffix logic
```

Implementation:
- `time` crate (already in `Cargo.lock` via `notify-rust`) for local-time formatting. Use `time::OffsetDateTime::now_local()` with a `UtcOffset` fallback to UTC if local-time lookup fails (`time` has soundness issues with local time at process startup; handle Err gracefully).
- PNG encoding: `image::RgbaImage::save(path)` auto-detects from extension.
- Uniquify: if the path exists, append `_1`, `_2`, … up to `_99` before the extension.

Unit tests:
- `interpolate` with each placeholder + unknown placeholder passthrough.
- `uniquify` with a pre-existing file in a tempdir.

### `src/hotkey.rs` (modified)

Replace the hardcoded modifier/code logic with an accepting-by-parameter API:

```rust
pub fn register(
    proxy: EventLoopProxy<UserEvent>,
    region: ParsedHotkey,
    fullscreen: ParsedHotkey,
) -> Result<HotkeyGuard>;
```

`ParsedHotkey` comes from `config.rs`. The rest of the function body (registering two hotkeys, spawning forwarder with press-state filter) is unchanged.

### `src/app.rs` (modified)

`App::new()` signature changes to take a `Config`:

```rust
pub struct App {
    overlay: Option<Overlay>,
    config: Config,
}

impl App {
    pub fn new(config: Config) -> Self { Self { overlay: None, config } }
}
```

`capture_full_screen`:
- After clipboard write, if `config.save.enabled`, call `file_save::save_png(...)`.
- Notification: if `config.general.notification_on_fullscreen`, fire it (else skip). Default is `true` so behavior is unchanged for users who don't edit config.

Region capture (`App::confirm`):
- After clipboard write + stdout log, if `config.save.enabled`, call `file_save::save_png(...)` with mode=Region.

### `src/main.rs` (modified)

CLI parse (simple; no `clap` dep):
```rust
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--install-autostart") => {
            autostart::install()?;
            println!("installed autostart");
            return Ok(());
        }
        Some("--uninstall-autostart") => {
            autostart::uninstall()?;
            println!("removed autostart");
            return Ok(());
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
        None => {
            // normal daemon flow below
        }
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
        config.hotkey.region.raw,
        config.hotkey.fullscreen.raw,
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
             quickshot                     run the daemon (default)\n\
             quickshot --install-autostart  install LaunchAgent (macOS)\n\
             quickshot --uninstall-autostart remove LaunchAgent (macOS)\n\
             quickshot --help              show this message\n\
         \n\
         Config: ~/.config/quickshot/config.toml\n\
         "
    );
}
```

### Dependencies

Add to `Cargo.toml`:
```toml
serde = { version = "1", features = ["derive"] }
toml = "0.8"
time = { version = "0.3", features = ["formatting", "local-offset"] }
```

`time` is already in `Cargo.lock` as a transitive dep of `notify-rust`, but we add it to `[dependencies]` explicitly to pin usage. `local-offset` feature is needed for `OffsetDateTime::now_local`.

Binary size delta estimate: serde + toml + time together add roughly 100–200 KB after LTO and stripping. Target: ≤ 1.4 MB final.

---

## Data flow

### Startup

```
main()
  → permission::preflight()
  → Config::load()
       → read ~/.config/quickshot/config.toml (create if missing)
       → parse TOML → Config
       → on error: log + use defaults
  → EventLoop build
  → hotkey::register(proxy, config.hotkey.region, config.hotkey.fullscreen)
  → tray::install(proxy)
  → App::new(config)
  → event_loop.run_app(app)
```

### Region capture with save enabled

```
hotkey/tray → UserEvent::CaptureRegion
  → App::open_overlay → overlay runs
  → user confirms → App::confirm(rect)
       → crop::crop_rgba(frame, rect)
       → clipboard::put_image(cropped)
       → if config.save.enabled:
           file_save::save_png(&cropped, &config.save.directory, &config.save.filename_template, CaptureMode::Region)
```

### Full-screen capture

```
hotkey/tray → UserEvent::CaptureScreen
  → App::capture_full_screen()
       → capture::capture_at_cursor()
       → clipboard::put_image(frame)
       → if config.save.enabled: file_save::save_png(&frame, ...)
       → if config.general.notification_on_fullscreen:
           notification::screenshot_copied(w, h)
```

### Autostart install

```
main() with --install-autostart
  → autostart::install()
       → let bin = std::env::current_exe()
       → let plist = render_plist_template(&bin)
       → ensure ~/Library/LaunchAgents exists
       → write plist content to ~/Library/LaunchAgents/com.quickshot.daemon.plist
       → Command::new("launchctl").arg("load").arg(&plist_path).status()
  → print confirmation
  → exit 0
```

---

## Testing strategy

### Unit tests

- **`config.rs`:** grammar parser (each modifier, each key variant, invalid strings), full TOML parse (valid, missing sections, malformed), `~` expansion.
- **`file_save.rs`:** `interpolate` with each placeholder, unknown placeholder passthrough, `uniquify` with pre-existing file (use `tempfile` crate OR manually create + cleanup in `std::env::temp_dir()`).
- **`autostart.rs`:** plist template substitution (no shelling out in tests).

Existing tests remain unchanged: 41 tests from Iter 2b continue to pass. New unit tests should bring the total to ~55–60.

### Manual verification (acceptance)

1. First run of the binary: `~/.config/quickshot/config.toml` is created with defaults; console prints the banner with the configured hotkey strings.
2. Edit `config.toml` to change `hotkey.region` to `"Cmd+Shift+X"`, restart the daemon; now `Cmd+Shift+X` opens the region overlay (and `Cmd+Shift+A` does nothing).
3. Edit `config.toml` to set `save.enabled = true`, restart. Capture a region. A PNG appears in `~/Desktop/Screenshot_2026-04-19_HH-MM-SS.png` with the correct dimensions. Clipboard also has the image.
4. Set `save.filename_template = "shot_{mode}_{w}x{h}_{datetime}.png"` and `save.directory = "~/tmp/quickshot"`, restart. Trigger a full-screen capture. A file like `shot_fullscreen_2880x1800_2026-04-19_15-04-30.png` lands in `~/tmp/quickshot/` (directory auto-created).
5. Set `general.notification_on_fullscreen = false`. Restart. Full-screen capture: no notification; clipboard still updated.
6. Run `./quickshot --help` → prints usage, exits 0.
7. Run `./quickshot --install-autostart` → prints `installed autostart → ~/Library/LaunchAgents/com.quickshot.daemon.plist` and exits 0. Check with `ls ~/Library/LaunchAgents/`. Log out and back in: quickshot is running (tray icon visible, hotkeys active).
8. Run `./quickshot --uninstall-autostart` → prints `removed autostart`, file gone from `~/Library/LaunchAgents/`.
9. Edit `config.toml` to set `hotkey.region = "Cmd+Shift+"` (malformed). Restart. Console prints a warning about the malformed hotkey, but the daemon runs with the region hotkey still defaulting to `Cmd+Shift+A`.
10. Delete `config.toml`, restart — fresh default file is written; behavior matches initial first run.

### Regression checks

- All Iter 2a + 2b invariants hold: region capture, anchors, magnifier, size label, DPI scaling, ESC in Idle, multi-display capture, Cmd+Shift+S full-screen, tray Quit, notification on success.
- Binary size target: ≤ 1.4 MB (Iter 2b was 1.2 MB; +100–200 KB budget for new deps).
- `cargo clippy --release --all-targets -- -D warnings` clean.

---

## Implementation order (for the plan)

1. **Config module** — write `config.rs` with types, grammar parser (ParsedHotkey), load/default, tests. Don't wire it yet.
2. **File-save module** — write `file_save.rs` with interpolation + uniquify + PNG write, tests.
3. **Autostart module** — write `autostart.rs` with install/uninstall + plist template, tests for the template.
4. **Wire config into hotkey + app + main** — change `hotkey::register` signature, change `App::new` signature, add config load + CLI parse in `main.rs`.
5. **Wire file-save into capture flows** — `App::confirm` and `capture_full_screen` both consult `config.save.enabled`.
6. **Polish** — README update with config schema + autostart usage, clippy clean, binary size record, tag `v0.4.0-iter3`.

Six tasks, parallel to Iter 2a/2b structure.

---

## Risks & mitigations

- **`time` crate local-offset soundness on macOS.** `time::OffsetDateTime::now_local()` can return Err if the process is multi-threaded when first invoked (tz-lookup is non-thread-safe on Unix). Mitigation: wrap in a fallback to `OffsetDateTime::now_utc()` and log a warning on Err.
- **`launchctl` version differences across macOS releases.** On macOS 14/15 the `launchctl load` subcommand still works but `bootstrap` / `bootout` are the modern equivalents. Using `load` / `unload` keeps backward compatibility with 11+. Prefer `load`/`unload` for simplicity.
- **Config file written by `chmod 000` / read-only filesystem.** Mitigation: on write failure, log and continue with in-memory defaults. Daemon still runs.
- **Invalid hotkey registration** after config change. If `global-hotkey` rejects a registration (e.g., hotkey already taken by another app), propagate the error at startup — this is the kind of thing the user needs to see and fix by editing their config.
- **Autostart binary path instability.** If the user moves the binary after installing, the LaunchAgent plist will point to a stale path. Mitigation: install is a user action; they rerun after moves. Document in README.
- **Binary size creep.** serde + toml + time can push binary past 1.4 MB. If it lands at 1.6 MB, accept and document; the features are worth it.

## Open questions

None blocking. All UX and architectural decisions are resolved per the design intent. Implementation details surfaced during plan writing will be resolved inline.
