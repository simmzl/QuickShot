# quickshot

Small, fast screenshot tool for macOS and Windows. Pure Rust.

## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

## Run

    ./target/release/quickshot

Press `Cmd+Shift+A` (region) or `Cmd+Shift+S` (full screen) to capture. The
screenshot is placed on your clipboard; full-screen captures also show a
system notification.

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

## Status (Iter 2b)

- Region capture via `Cmd+Shift+A` with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- Full-screen capture via `Cmd+Shift+S` (cursor's monitor, clipboard + notification)
- Menu-bar tray icon with Capture Region / Capture Screen / Quit
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No settings window / file saving yet (Iter 3)
- No cross-screen selection

Release binary size on this machine: 1.2M.
Dominant contributors: std (40.7% of .text), image crate (13.0%), winit (10.6%).
