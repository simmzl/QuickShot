# quickshot

Small, fast screenshot tool for macOS and Windows. Pure Rust.

## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

## Run

    ./target/release/quickshot

Press `Cmd+Shift+A` (region) or `Cmd+Shift+S` (full screen) to capture. The
screenshot is placed on your clipboard; full-screen captures also show a
system notification by default (toggle `notification_on_fullscreen` in the
config).

Quit via the menu-bar tray icon → Quit, or Ctrl+C in the launching terminal.

## macOS first run

The daemon requires Screen Recording permission. On first launch it will
detect the missing permission and print a guided prompt pointing you to
System Settings → Privacy & Security → Screen Recording. After granting,
relaunch.

On the first full-screen capture (`Cmd+Shift+S`), macOS may also prompt for
notification permission ("quickshot wants to send you notifications"). Allow
it so successful captures can show a confirmation banner. Subsequent
captures are silent in terms of prompts.

## Status (Iter 3)

- Region capture via configurable hotkey (default `Cmd+Shift+A`) with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- Full-screen capture via configurable hotkey (default `Cmd+Shift+S`) — cursor's monitor, clipboard + optional notification
- Menu-bar tray icon with Capture Region / Capture Screen / Quit
- Configurable save-to-disk with templated filenames (`~/.config/quickshot/config.toml`)
- macOS autostart via `quickshot --install-autostart` / `--uninstall-autostart`
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No cross-screen selection

Release binary size on this machine: 1.4M.

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
