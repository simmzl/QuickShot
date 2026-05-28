# quickshot

Small, fast screenshot daemon for **macOS** and **Windows**. Pure Rust, ~2-3 MB binary, lives in the system tray.

[![Release](https://img.shields.io/github/v/release/simmzl/quickshot)](https://github.com/simmzl/quickshot/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

> English | [简体中文](README.zh-CN.md)

## Features

- **Region + fullscreen capture** through configurable global hotkeys.
- **Drag → anchor-adjust → confirm** flow; `Esc` cancels at any point.
- **Annotation tools**: Arrow, Rectangle, Ellipse, Mosaic, Pen, Text, Move — plus undo / redo.
- **Mini toolbar** under the selection: switch tool, pin the result as a floating preview, or save-as PNG.
- **4× magnifier** with crosshair, hex / coord readout, and live W × H size label during selection.
- **System tray**: Capture Region / Screen, Edit Config, Start at Login, Quit.
- **PNG save-to-disk** with templated filenames (date / time / dimensions / mode placeholders).
- **Live config reload** — edits to `config.toml` are applied within 1 second; hotkeys and tray labels refresh in place, no restart required.

## Install

Pre-built binaries are attached to each [GitHub Release](https://github.com/simmzl/quickshot/releases/latest).

### macOS

Download `quickshot-<VERSION>.dmg`. Double-click → drag `quickshot.app` to `Applications` → eject the DMG.

First launch: Finder will warn the app is from an "unidentified developer" (normal for open-source apps without an Apple Developer ID signature). Work around it once:

1. Open `Applications` in Finder.
2. **Right-click** `quickshot.app` → **Open** → click **Open** in the confirmation sheet.

macOS remembers the override; future launches (including autostart) work without the prompt.

On the first capture, macOS will ask for **Screen Recording** permission. Grant it in **System Settings → Privacy & Security → Screen Recording**, then relaunch.

### Windows

Download `quickshot-<VERSION>-windows-x64.zip`. Extract anywhere and double-click `quickshot.exe`. The app lives in the system tray — no console window appears.

On first launch SmartScreen may show *"Windows protected your PC"*; click **More info → Run anyway** once. Windows remembers the override.

## Use

Default hotkeys:

| Action | macOS | Windows |
|---|---|---|
| Region capture | `Cmd+Shift+A` | `Ctrl+Shift+A` |
| Fullscreen capture | `Cmd+Shift+S` | `Ctrl+Shift+S` |

A successful capture is placed on the clipboard. Optionally also saved as PNG to disk (see [Config](#config)).

### Region capture — adjusting state

After the initial drag, the selection enters an *adjusting* state where you can resize / re-anchor by the edges and add annotations.

| Key | Action |
|---|---|
| `Enter` or double-click | Confirm — copy to clipboard |
| `Esc` | Cancel |
| `A` / `R` / `E` / `B` | Arrow / Rectangle / Ellipse / Mosaic |
| `P` / `T` | Pen / Text |
| `M` | Move existing annotation |
| `Cmd/Ctrl + Z` | Undo |
| `Cmd/Ctrl + Shift + Z` | Redo |

The mini toolbar below the selection mirrors these tools and adds:

- **Pin** — turn the cropped image into a floating, always-on-top preview window. Multiple pins can coexist; drag to reposition, double-click to close.
- **Save as…** — open a native save dialog and write the cropped + annotated image to your chosen path.

### Tray menu

Right-click the tray icon for: **Capture Region**, **Capture Screen**, **Edit Config…**, **Start at Login**, **Quit**.

## Config

Config path:

- macOS: `~/.config/quickshot/config.toml`
- Windows: `%APPDATA%\quickshot\config.toml`

Defaults are written on first run. **Edits are auto-reloaded within 1 second** — quickshot polls the file's mtime; on change it rebinds hotkeys, updates the tray menu labels, and refreshes save / notification settings live. No restart needed.

```toml
[hotkey]
# Format: modifiers joined by "+", ending with a key. Modifiers (case-insensitive):
#   Cmd / Meta / Super (= Win key on Windows) / Ctrl / Alt / Opt / Shift
# Keys: A-Z, 0-9, F1-F24, or named keys (Space, Enter, Tab, Backspace, Escape).
region = "Cmd+Shift+A"          # Windows default: "Ctrl+Shift+A"
fullscreen = "Cmd+Shift+S"      # Windows default: "Ctrl+Shift+S"

[save]
# When true, every successful capture also writes a PNG to `directory`.
enabled = false
# `~` expands to $HOME on macOS, %USERPROFILE% on Windows. Missing dirs are created.
directory = "~/Desktop"         # Windows default: "~/Pictures"
# Placeholders: {date} {time} {datetime} {w} {h} {mode}
filename_template = "Screenshot_{datetime}.png"

[general]
# Show a system notification after a successful full-screen capture.
notification_on_fullscreen = true
```

**Note (Windows)**: `Win+Shift+S` is already reserved by the built-in Snipping Tool. Avoid binding `Super+Shift+S` or `Meta+Shift+S` for the fullscreen hotkey — registration will fail and the change rolls back. Stick to `Ctrl` / `Alt` based combos.

Filename template placeholders:

| Token | Example |
|---|---|
| `{date}` | `2026-04-19` |
| `{time}` | `15-04-30` |
| `{datetime}` | `2026-04-19_15-04-30` |
| `{w}`, `{h}` | `1920`, `1080` (physical pixels) |
| `{mode}` | `region` or `fullscreen` |

## Autostart

### macOS

Tray menu → **Start at Login**, or from CLI:

```
/Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart
/Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart
```

Installs `~/Library/LaunchAgents/com.quickshot.daemon.plist`. Takes effect at next login. For bare-binary users (running `target/release/quickshot` directly), the same flags work — the plist points at whatever path the running binary lives at.

### Windows

Tray menu → **Start at Login**. Writes a string value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\quickshot` pointing at the running exe. Takes effect at next login. Verify or remove from PowerShell:

```powershell
# Check
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v quickshot

# Remove (toggle from the tray is the normal way)
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v quickshot /f
```

## Build

Requires Rust stable (1.75+).

```
cargo build --release
```

Binary lands at `target/release/quickshot[.exe]`. The release profile is size-optimized (`opt-level="z"`, `lto=true`, `codegen-units=1`, `strip=true`, `panic="abort"`); the macOS universal binary is ~3.2 MB, the Windows binary ~2.1 MB.

Windows builds also require the MSVC linker (`link.exe`). Install **Visual Studio Build Tools** with the **Desktop development with C++** workload, or set up the MSVC env via `vcvars64.bat` before invoking `cargo`.

### Packaging

| Platform | Script | Produces |
|---|---|---|
| macOS | `bash scripts/package.sh` | `dist/quickshot.app` (universal x86_64 + aarch64, ad-hoc signed) + `dist/quickshot-<VERSION>.dmg` |
| Windows | `pwsh scripts/package.ps1` (run from a Developer PowerShell) | `dist/quickshot-<VERSION>-windows-x64/` (exe + README) + `dist/quickshot-<VERSION>-windows-x64.zip` |

macOS env overrides for `package.sh`:

- `BUNDLE_ID` (default `com.quickshot.app`)
- `SIGN_IDENTITY` (default `-` for ad-hoc; pass an Apple Developer ID identity string for full signing)

### Release CI

Pushing a `v*` git tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml):

1. `build-macos` (runs on `macos-latest`) and `build-windows` (runs on `windows-latest`) execute in parallel — each runs `cargo test`, packages its artifact, and uploads it.
2. The `release` job downloads both artifacts and publishes a single GitHub Release containing the dmg and the zip.

To cut a release:

```
# 1. Bump version in Cargo.toml, commit.
# 2. Tag and push.
git tag v1.2.3
git push origin master v1.2.3
```

## Uninstall

### macOS

```
# Disable autostart (only if you enabled it):
/Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart

# Remove the app:
rm -rf /Applications/quickshot.app

# Optional: remove config + cached state:
rm -rf ~/.config/quickshot
```

### Windows

1. **Quit** from the tray menu.
2. Delete the extracted folder.
3. (Optional) Disable autostart if you had enabled it:
   ```powershell
   reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v quickshot /f
   ```
4. (Optional) Remove the config dir:
   ```powershell
   Remove-Item -Recurse "$env:APPDATA\quickshot"
   ```

## License

MIT OR Apache-2.0 — choose whichever fits your project.
