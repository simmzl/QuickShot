# quickshot

Small, fast screenshot tool for macOS and Windows. Pure Rust.

## Install (macOS)

Download the latest `quickshot-<VERSION>.dmg` from Releases (or build it yourself — see [Build](#build)). Double-click the DMG, drag `quickshot.app` to the `Applications` folder, eject the DMG.

First launch: Finder will warn that "quickshot.app can't be opened because it is from an unidentified developer." This is expected for open-source apps without Apple Developer ID signing. Work around it once:

1. Open `Applications` in Finder.
2. **Right-click** `quickshot.app` → **Open**.
3. Click **Open** in the confirmation sheet.

macOS remembers the override. Future launches (including autostart at login) work without the prompt.

On first capture, macOS will also ask for Screen Recording permission: System Settings → Privacy & Security → Screen Recording → enable `quickshot`, then relaunch the app.

### Uninstall

    # If autostart was installed:
    /Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart

    # Remove the app itself:
    rm -rf /Applications/quickshot.app

    # Optional: remove config + saved screenshots config dir:
    rm -rf ~/.config/quickshot

## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

### Package as .app + .dmg

    bash scripts/package.sh

Produces:

- `dist/quickshot.app` — universal macOS bundle (x86_64 + aarch64), ad-hoc signed
- `dist/quickshot-<VERSION>.dmg` — distributable disk image

Environment overrides: `BUNDLE_ID` (default `com.quickshot.app`), `SIGN_IDENTITY` (default `-` ad-hoc; pass an Apple Developer ID identity string for full signing).

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

## Status (Iter 5a)

- Region capture via configurable hotkey (default `Cmd+Shift+A`) with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- **Annotation tools during region capture**: Arrow (A), Rectangle (R), Ellipse (E), Mosaic (B), Move (M) + Undo (Cmd+Z) / Redo (Cmd+Shift+Z)
- **Mini toolbar below selection**: click to switch tool or undo/redo
- Full-screen capture via configurable hotkey (default `Cmd+Shift+S`) — cursor's monitor, clipboard + optional notification
- Menu-bar tray icon with Capture Region / Capture Screen / Edit Config / Start at Login / Quit
- Configurable save-to-disk with templated filenames (`~/.config/quickshot/config.toml`)
- macOS autostart via tray menu or `quickshot --install-autostart`
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No cross-screen selection

Release binary size on this machine: 3.2M.

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

When the binary is installed as an app bundle in `/Applications`:

    /Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart

To uninstall:

    /Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart

For bare-binary users (running from `target/release/quickshot`), the same flags apply to that binary path.
