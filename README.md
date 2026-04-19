# quickshot

Small, fast screenshot tool for macOS and Windows. Pure Rust.

## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

## Run

    ./target/release/quickshot

Press `Ctrl/Cmd+Shift+A`, drag a region, release — the PNG is now on your clipboard.

Quit with Ctrl+C in the launching terminal.

## macOS first run

The daemon requires Screen Recording permission. On first launch it will
detect the missing permission and print a guided prompt pointing you to
System Settings → Privacy & Security → Screen Recording. After granting,
relaunch.

## Status (Iter 2a)

- Primary-cursor monitor capture (multi-display: follows cursor's screen)
- Drag → draft → anchor-adjust → Enter/double-click confirm
- ESC cancels
- Live size label (W × H in physical pixels)
- Magnifier with 4× zoom, crosshair, hex + coord readout (visible while aiming/drafting)
- No system notification, no tray icon, no settings window yet (Iter 2b / Iter 3)
- No cross-screen selection
- Exit with Ctrl+C in the launching terminal

Release binary size on this machine: 855K.
Dominant contributors: std + winit (59.7% + 17.7% of .text); quickshot own code is ~8.9 KiB.
