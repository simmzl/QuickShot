# quickshot Iter 4 — macOS .app Packaging (Design Spec)

**Date:** 2026-04-20
**Status:** Design approved (auto by user delegation); ready for implementation plan.
**Predecessor:** Iter 3 (merge `6c49de7`, tag `v0.4.0-iter3`) — config + file save + autostart.
**Successor:** Iter 2c polish (deferred) or Iter 5 feature work.

## Goal

Make quickshot distributable as a native macOS application. Produce a `.app` bundle wrapped in a `.dmg`, built as a universal binary (x86_64 + aarch64), with an embedded icon, correct `Info.plist` metadata, and ad-hoc code signing — everything needed for the author to install and use personally, and to share the binary with friends who can install via standard drag-to-Applications.

Specifically, by the end of this iteration:
1. `scripts/package.sh` produces `dist/quickshot.app` and `dist/quickshot-{version}.dmg` with one command.
2. The `.app` has its own icon visible in Finder.
3. The `.app` is a menu-bar only application (no Dock icon, not in Cmd+Tab).
4. macOS Screen Recording permission prompt has a quickshot-specific explanation string.
5. The existing `--install-autostart` subcommand correctly registers the bundled binary path when launched from `/Applications/quickshot.app/`.
6. README documents download + install + first-launch Gatekeeper workaround.

## Non-Goals (explicitly deferred)

- **Apple Developer ID code signing.** Requires a $99/year Apple Developer account. Ad-hoc signing is sufficient for personal use and friend-distribution (with a documented right-click-open workaround for Gatekeeper). If the user later obtains a Developer ID, a `--sign DEV_ID` flag to the package script is a one-line addition.
- **Notarization.** Requires Developer ID. Without it, Gatekeeper warns the first time; users can right-click → Open to bypass. Notarization can be added if/when Developer ID is configured.
- **Auto-update mechanism.** No Sparkle or similar. Users re-download and re-install for new versions.
- **Mac App Store submission.** Out of scope; would require sandboxing which conflicts with the global-hotkey and screen-capture use cases.
- **Windows/Linux packaging.** macOS-only for Iter 4. Windows `.msi` / Linux AppImage are future iterations.
- **CI / GitHub Actions release automation.** The script can be run locally; automating it in CI is a separate concern (and would need a macOS runner).
- **Iter 2c polish** (TTF subset, coord helper, estimate_text_width) — still deferred.

---

## UX Specification

### Build artifacts

Running `bash scripts/package.sh` from the project root produces:
```
dist/
├── quickshot.app/                      # installable app bundle
│   └── Contents/
│       ├── Info.plist
│       ├── MacOS/
│       │   └── quickshot               # universal binary (x86_64 + aarch64)
│       └── Resources/
│           └── quickshot.icns
└── quickshot-{VERSION}.dmg             # distributable disk image
```

Where `{VERSION}` is read from `Cargo.toml`'s `package.version` (e.g., `0.5.0`).

The DMG layout when mounted:
- `quickshot.app` on the left
- A symlink named `Applications` pointing to `/Applications` on the right
- Background: none (plain white; fancy background is polish)

### Install flow (user's perspective)

**Author's own machine (or after building from source):**
1. `bash scripts/package.sh`
2. Open `dist/quickshot-0.5.0.dmg` → drag `quickshot.app` to `Applications`
3. First launch: `open /Applications/quickshot.app` — macOS prompts for Screen Recording permission; grant it; relaunch; the tray icon appears.

**Someone else receiving the DMG:**
1. Download `quickshot-0.5.0.dmg`.
2. Double-click → drag to Applications.
3. First launch: Finder says "quickshot can't be opened because it's from an unidentified developer." Right-click the app → **Open** → "Open anyway" in the confirmation. macOS remembers this and subsequent launches are unprompted.
4. Grant Screen Recording permission → relaunch → done.

### Application behavior

- **No Dock icon.** The Info.plist sets `LSUIElement = true`, making quickshot a background-only app. It appears only in the menu bar via the tray icon.
- **No Cmd+Tab entry.** Also a consequence of `LSUIElement`.
- **Launch behavior** identical to CLI invocation (same code path, same hotkeys, same config load).
- **Autostart.** If the user wants the app to launch at login, they run from Terminal:
  ```
  /Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart
  ```
  This installs the LaunchAgent pointing at the bundled binary. Uninstall mirror:
  ```
  /Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart
  ```
  (The app path in the LaunchAgent plist is determined by `std::env::current_exe()` which correctly resolves to `/Applications/quickshot.app/Contents/MacOS/quickshot` when the user runs the bundled binary.)

### Permission prompts

- **Screen Recording:** triggered on first capture. The system dialog shows the `NSScreenCaptureUsageDescription` string from `Info.plist`: *"quickshot needs Screen Recording permission to capture screenshots. You can revoke this in System Settings → Privacy & Security → Screen Recording."*
- **Notifications:** triggered on first `Cmd+Shift+S` full-screen capture (via notify-rust → NSUserNotification). System dialog shows the app name "quickshot".

### Uninstall

No uninstaller needed — remove `/Applications/quickshot.app` and (if autostart was installed) also `~/Library/LaunchAgents/com.quickshot.daemon.plist`. Documented in README.

---

## Architecture

### File additions

```
quickshot/
├── .gitignore                       (modified — add /dist)
├── Cargo.toml                       (unchanged)
├── README.md                        (modified — install/uninstall/Gatekeeper)
├── assets/
│   ├── fonts/                       (existing)
│   ├── tray-icon.png                (existing)
│   └── app-icon.png                 (new — 1024×1024 source PNG for .icns)
├── scripts/                         (new directory)
│   ├── gen_app_icon.rs              (standalone Rust; produces assets/app-icon.png)
│   ├── package.sh                   (main packaging script)
│   └── Info.plist.in                (plist template with {{VERSION}} and {{BUNDLE_ID}} placeholders)
├── dist/                            (gitignored — build output)
└── src/                             (unchanged)
```

None of the source code in `src/` changes. The existing `--install-autostart` logic uses `std::env::current_exe()` which correctly resolves to `/Applications/quickshot.app/Contents/MacOS/quickshot` when the bundled binary is invoked.

### `scripts/gen_app_icon.rs`

Standalone Rust file — same approach as Iter 2b Task 1's `gen_tray_icon.rs`. Generates a 1024×1024 PNG with the same motif as the tray icon (rounded rectangle outline + center dot) but scaled up and visually richer. Committed to the repo at `assets/app-icon.png`; the generator is used once and deleted (or kept as `scripts/` for re-generation).

Design:
- Canvas: 1024×1024, RGBA.
- Background: fully transparent.
- Outer shape: rounded-corner square, ~80% of canvas, filled with a dark-to-lighter gradient (or solid color, simpler). Solid slate-blue fill (`#3B4252`) is fine and matches a "developer tool" aesthetic.
- Inner shape: a smaller rounded rectangle outline in white (1-px equivalent at 22px scales to ~46px thick at 1024px — use ~32px for visual weight).
- Center dot: 8% diameter white-filled circle.

This is a deterministic Rust snippet using the `image` crate. No external design tool required.

### `scripts/Info.plist.in`

Template with `{{VERSION}}` and `{{BUNDLE_ID}}` placeholders that `package.sh` substitutes:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>quickshot</string>
    <key>CFBundleExecutable</key>
    <string>quickshot</string>
    <key>CFBundleIconFile</key>
    <string>quickshot</string>
    <key>CFBundleIdentifier</key>
    <string>{{BUNDLE_ID}}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>quickshot</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{{VERSION}}</string>
    <key>CFBundleVersion</key>
    <string>{{VERSION}}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSScreenCaptureUsageDescription</key>
    <string>quickshot needs Screen Recording permission to capture screenshots. You can revoke this in System Settings → Privacy &amp; Security → Screen Recording.</string>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
```

Bundle ID default: `com.quickshot.app`. Can be overridden by env var `BUNDLE_ID=...` at package time if the user wants a reverse-DNS matching their domain.

### `scripts/package.sh`

Single-file bash script, ~100 lines, with strict mode (`set -euo pipefail`):

1. **Parse env vars** for overrides:
   - `BUNDLE_ID` (default: `com.quickshot.app`)
   - `SIGN_IDENTITY` (default: `-` — ad-hoc). Users with a Developer ID can pass their identity string to upgrade from ad-hoc to full signing without script changes.

2. **Read version from `Cargo.toml`:**
   ```bash
   VERSION=$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)
   ```

3. **Build universal binary:**
   ```bash
   rustup target add x86_64-apple-darwin aarch64-apple-darwin 2>/dev/null || true
   cargo build --release --target x86_64-apple-darwin
   cargo build --release --target aarch64-apple-darwin
   mkdir -p dist
   lipo -create \
     target/x86_64-apple-darwin/release/quickshot \
     target/aarch64-apple-darwin/release/quickshot \
     -output dist/quickshot-universal
   ```

4. **Ensure the app icon exists.** If `assets/app-icon.png` is missing, run `cargo run --release --bin gen_app_icon` — but we do NOT want a permanent `bin` target cluttering `Cargo.toml`. Instead, the generator is a standalone script that's invoked ONCE during Iter 4 Task 1 and the resulting PNG is committed. `package.sh` checks the PNG exists and fails loudly if not.

5. **Build `.icns` from the 1024×1024 PNG:**
   ```bash
   ICONSET=dist/quickshot.iconset
   mkdir -p "$ICONSET"
   # Generate the required sizes for the iconset
   for sz in 16 32 64 128 256 512 1024; do
     sips -z $sz $sz assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}.png" >/dev/null
   done
   # Retina variants
   for sz in 16 32 128 256 512; do
     dbl=$((sz*2))
     sips -z $dbl $dbl assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}@2x.png" >/dev/null
   done
   iconutil -c icns -o dist/quickshot.icns "$ICONSET"
   rm -rf "$ICONSET"
   ```

6. **Assemble the `.app` bundle:**
   ```bash
   APP=dist/quickshot.app
   rm -rf "$APP"
   mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
   cp dist/quickshot-universal "$APP/Contents/MacOS/quickshot"
   chmod +x "$APP/Contents/MacOS/quickshot"
   cp dist/quickshot.icns "$APP/Contents/Resources/quickshot.icns"
   sed -e "s/{{VERSION}}/$VERSION/g" -e "s/{{BUNDLE_ID}}/$BUNDLE_ID/g" \
     scripts/Info.plist.in > "$APP/Contents/Info.plist"
   ```

7. **Code sign (ad-hoc by default):**
   ```bash
   codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
   codesign --verify --verbose=2 "$APP"
   ```

8. **Build DMG:**
   ```bash
   DMG=dist/quickshot-$VERSION.dmg
   rm -f "$DMG"
   STAGING=dist/dmg-staging
   rm -rf "$STAGING"
   mkdir -p "$STAGING"
   cp -R "$APP" "$STAGING/"
   ln -s /Applications "$STAGING/Applications"
   hdiutil create -volname "quickshot" -srcfolder "$STAGING" -ov -format UDZO "$DMG"
   rm -rf "$STAGING"
   ```

9. **Clean up intermediate artifacts** (but keep `.app` and `.dmg`):
   ```bash
   rm -f dist/quickshot-universal dist/quickshot.icns
   ```

10. **Report:**
    ```bash
    echo "done:"
    echo "  $APP"
    echo "  $DMG ($(du -h "$DMG" | cut -f1))"
    ```

The script is idempotent: running it twice produces identical output (minus timestamps in the DMG metadata).

### `.gitignore` additions

Add one line:
```
/dist
```

(We already have `/target`, `.DS_Store`, `/.worktrees`.)

---

## Data flow

```
scripts/package.sh
  → read VERSION from Cargo.toml
  → rustup add x86_64-apple-darwin + aarch64-apple-darwin targets (idempotent)
  → cargo build --release (per target, 2×)
  → lipo → dist/quickshot-universal
  → sips × 12 → iconset PNGs
  → iconutil → dist/quickshot.icns
  → cp binary + icns + subst Info.plist → dist/quickshot.app
  → codesign --sign "-" dist/quickshot.app
  → hdiutil create → dist/quickshot-{VERSION}.dmg
  → clean intermediates
  → stdout: paths + sizes
```

---

## Testing strategy

### Unit tests

None new — this is a packaging iteration with shell-script artifacts. All existing 71 tests continue to pass.

### Verification (manual, on macOS)

Run `bash scripts/package.sh`, then:

1. `file dist/quickshot.app/Contents/MacOS/quickshot` reports "Mach-O universal binary with 2 architectures: [x86_64:Mach-O 64-bit executable x86_64] [arm64:Mach-O 64-bit executable arm64]".
2. `lipo -info dist/quickshot.app/Contents/MacOS/quickshot` → "Architectures in the fat file: ... are: x86_64 arm64".
3. `codesign --verify --verbose=2 dist/quickshot.app` → "satisfies its Designated Requirement".
4. `plutil -lint dist/quickshot.app/Contents/Info.plist` → "OK".
5. `hdiutil verify dist/quickshot-*.dmg` → succeeds.
6. Open the DMG in Finder → visual check that `quickshot.app` + `Applications` shortcut are present.
7. Drag `quickshot.app` from the mounted DMG into `/Applications` (or to Desktop for testing).
8. Double-click `quickshot.app` → macOS Gatekeeper blocks with "cannot verify developer" → right-click → **Open** → "Open Anyway" → tray icon appears in menu bar.
9. Trigger `Cmd+Shift+A` → overlay appears. Trigger `Cmd+Shift+S` → notification appears (after granting notification permission on first run).
10. Open Tray menu → Quit → app exits.
11. `/Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart` → plist written at `~/Library/LaunchAgents/com.quickshot.daemon.plist`, `plutil -p` the plist confirms `ProgramArguments` points at the bundled binary path.
12. Log out / log in → quickshot tray icon visible before user opens anything.
13. `/Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart` → plist removed.

### Regression checks

- All existing Iter 1–3 features work identically inside the bundle.
- `cargo test` still 71 passed / 0 failed / 2 ignored on the source tree.
- `cargo clippy --release --all-targets -- -D warnings` still clean.

### Binary size expectations

- Iter 3 release binary (single architecture): ~1.4 MB.
- Universal binary (two architectures): ~2.8 MB.
- `.app` bundle (with icns): ~3.0 MB.
- DMG (compressed UDZO): ~1.5–2.0 MB (LZMA compression of the universal binary).

---

## Implementation order (for the plan)

1. **App-icon generator + 1024×1024 PNG asset** — write `scripts/gen_app_icon.rs`, run via `cargo run --bin` once (or as a standalone cargo-script), commit `assets/app-icon.png`. Remove the generator file or stash it in `scripts/` as a one-shot.
2. **Info.plist template + .gitignore update** — commit `scripts/Info.plist.in` + add `/dist` to `.gitignore`.
3. **Package script (phase 1: build + bundle + sign)** — write `scripts/package.sh` through the codesign step. At this point `dist/quickshot.app` exists but the DMG step isn't there yet.
4. **Package script (phase 2: DMG)** — add the `hdiutil` DMG step + cleanup. Now running the script produces both `.app` and `.dmg`.
5. **README update** — install / first-launch / Gatekeeper / uninstall instructions.
6. **Polish + tag** — confirm `bash scripts/package.sh` runs end-to-end on the host machine, record the artifact sizes in the README, tag `v0.5.0-iter4`.

Six tasks, matching the Iter 2a/2b/3 cadence. Each task is independently verifiable.

---

## Risks & mitigations

- **`rustup target add` network requirement.** Adding targets on a machine with spotty network will fail. Mitigation: the script suppresses failure of `rustup target add` with `|| true` because once a target is installed, repeated calls are fine; first-time installs need network and will be caught by the subsequent `cargo build` failing.
- **`sips` / `iconutil` / `hdiutil` availability.** These are all macOS-shipped tools (part of Xcode Command Line Tools). The script does not check for them explicitly — if any is missing, the pipeline will fail with a clear "command not found" error. Acceptable.
- **`codesign` with `-` identity (ad-hoc)** still produces a signature Gatekeeper treats as unsigned. This is the entire reason the README documents the right-click-Open workaround for first launch. Real Developer ID signing would fix this but is out of scope.
- **Universal binary size growth.** 2.8 MB is acceptable (the DMG compresses well). If this ever becomes a concern, a script flag could emit single-arch DMGs.
- **`NSHighResolutionCapable = true`** claims Retina support; we already paint into softbuffer with physical pixels and DPI-scale fonts via `window.scale_factor()`. Confirm via the manual test on Retina screens (should be unchanged from current behavior).
- **`LSMinimumSystemVersion = 11.0`.** Big Sur was the first macOS to support Apple Silicon universal binaries and winit 0.30 has the same floor. Lower versions probably won't work anyway.
- **`LSUIElement` and focus stealing.** With `LSUIElement = true`, the overlay window still activates its NSApplication via our existing `activateIgnoringOtherApps:` call (from Iter 2b fixup). This should still work — LSUIElement only hides Dock presence, not app activation. Verify in the smoke test.
- **DMG background / layout styling.** Currently the DMG is a plain two-icon layout. Most open-source macOS apps do this; styled backgrounds require additional tooling (`create-dmg` script) — deferred.

## Open questions

None blocking. All decisions are made. Implementation details that surface during plan writing will be resolved inline.
