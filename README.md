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
