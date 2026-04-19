# quickshot Iter 4 — macOS .app Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce `dist/quickshot.app` and `dist/quickshot-{version}.dmg` via a single `bash scripts/package.sh` invocation. Bundle is a menu-bar-only universal binary (x86_64 + aarch64), ad-hoc signed, with embedded icon and proper Info.plist. No source code changes to `src/`.

**Architecture:** All new artifacts live under `scripts/` and `assets/`. `scripts/gen_app_icon.rs` is a one-off generator producing `assets/app-icon.png` (1024×1024 RGBA). `scripts/Info.plist.in` is a template substituted by `scripts/package.sh` at build time. The package script orchestrates `cargo build` (twice, once per target), `lipo`, `sips`, `iconutil`, `codesign`, and `hdiutil`. No existing code is touched.

**Tech Stack:**
- Existing: Rust 1.86+, all crates from Iter 3
- Tooling (macOS system): `rustup`, `cargo`, `lipo`, `sips`, `iconutil`, `codesign`, `hdiutil`, `plutil` (for verification), `sed`, `awk`

**Spec:** `docs/superpowers/specs/2026-04-20-quickshot-iter4-design.md`

**Scope for this plan (Iter 4 only):**
- App icon generator + committed 1024×1024 PNG
- Info.plist template with version/bundle-id substitution
- `scripts/package.sh` producing `.app` and `.dmg`
- Universal binary assembly (x86_64 + aarch64 via `lipo`)
- Ad-hoc codesign
- README updates (install, first launch, uninstall)
- `.gitignore` update for `/dist`

**Not in this plan:**
- Apple Developer ID signing (out of scope)
- Notarization (needs Developer ID)
- GitHub Actions CI
- Styled DMG backgrounds
- Any source-code changes — `src/` is frozen

---

## File Structure

```
quickshot/
├── .gitignore                       (modified — add /dist)
├── Cargo.toml                       (unchanged)
├── README.md                        (modified — install/uninstall/Gatekeeper)
├── assets/
│   ├── fonts/                       (unchanged)
│   ├── tray-icon.png                (unchanged — 22×22 menu-bar icon)
│   └── app-icon.png                 (new — 1024×1024 app icon source)
├── scripts/                         (new)
│   ├── gen_app_icon.rs              (one-shot; deleted after commit)
│   ├── Info.plist.in                (Info.plist template)
│   └── package.sh                   (packaging pipeline)
├── dist/                            (gitignored — build output)
└── src/                             (UNCHANGED this iteration)
```

---

## Task 1: Generate and commit the app-icon PNG

Produce `assets/app-icon.png` (1024×1024 RGBA) via a Rust generator. Same one-shot pattern as Iter 2b's tray-icon generator — create a temporary `src/bin/gen_app_icon.rs`, run it, commit the output PNG, remove the generator.

**Files:**
- Create: `assets/app-icon.png` (1024×1024 RGBA, ~10-40 KB)

- [ ] **Step 1: Create the temporary generator**

Create `src/bin/gen_app_icon.rs`:
```rust
//! One-shot generator for `assets/app-icon.png` (1024×1024 app icon).
//! Run with `cargo run --release --bin gen_app_icon`. Output is committed;
//! this file is deleted in the next step.
//!
//! Design: rounded-corner slate-blue background with a thicker white
//! rounded-rectangle outline and a white center dot. Visually echoes the
//! tray icon (consistent brand) but scaled for app-icon presence.

use image::{Rgba, RgbaImage};

fn main() {
    const SIZE: i32 = 1024;
    const PADDING: i32 = 96; // distance from canvas edge to background square
    const BG_RADIUS: i32 = 180; // big rounded-corner background
    const INNER_PADDING: i32 = 220; // distance from canvas edge to inner rect
    const INNER_RADIUS: i32 = 72;
    const OUTLINE_THICK: i32 = 40;
    const DOT_DIAMETER: i32 = 104; // ~10% of canvas

    let transparent = Rgba([0u8, 0, 0, 0]);
    let slate = Rgba([59u8, 66, 82, 255]); // #3B4252
    let white = Rgba([255u8, 255, 255, 255]);

    let mut img = RgbaImage::from_pixel(SIZE as u32, SIZE as u32, transparent);

    // Background: slate-blue rounded square.
    let (bl, bt, br, bb) = (PADDING, PADDING, SIZE - PADDING - 1, SIZE - PADDING - 1);
    fill_rounded(&mut img, bl, bt, br, bb, BG_RADIUS, slate);

    // Inner white rounded-rectangle outline: draw filled then punch transparent.
    // Instead of transparent (which would re-hole the slate background), we
    // paint white then carve a slate-colored inner rect.
    let (il, it, ir, ib) = (
        INNER_PADDING,
        INNER_PADDING,
        SIZE - INNER_PADDING - 1,
        SIZE - INNER_PADDING - 1,
    );
    fill_rounded(&mut img, il, it, ir, ib, INNER_RADIUS, white);
    fill_rounded(
        &mut img,
        il + OUTLINE_THICK,
        it + OUTLINE_THICK,
        ir - OUTLINE_THICK,
        ib - OUTLINE_THICK,
        (INNER_RADIUS - OUTLINE_THICK).max(4),
        slate,
    );

    // Center dot: white filled circle.
    let cx = SIZE / 2;
    let cy = SIZE / 2;
    let dot_r = DOT_DIAMETER / 2;
    fill_circle(&mut img, cx, cy, dot_r, white);

    let out = "assets/app-icon.png";
    img.save(out).expect("save app icon");
    println!("wrote {out} ({SIZE}x{SIZE})");
}

fn fill_rounded(img: &mut RgbaImage, l: i32, t: i32, r: i32, b: i32, radius: i32, color: Rgba<u8>) {
    for y in t..=b {
        for x in l..=r {
            let cx = if x - l < radius {
                l + radius
            } else if r - x < radius {
                r - radius
            } else {
                x
            };
            let cy = if y - t < radius {
                t + radius
            } else if b - y < radius {
                b - radius
            } else {
                y
            };
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= radius * radius
                && x >= 0
                && y >= 0
                && x < img.width() as i32
                && y < img.height() as i32
            {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

fn fill_circle(img: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let r2 = radius * radius;
    for y in (cy - radius)..=(cy + radius) {
        for x in (cx - radius)..=(cx + radius) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= r2
                && x >= 0
                && y >= 0
                && x < img.width() as i32
                && y < img.height() as i32
            {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}
```

- [ ] **Step 2: Run the generator and verify**

```bash
mkdir -p src/bin
# (file already created in Step 1)
cargo run --release --bin gen_app_icon
file assets/app-icon.png
ls -l assets/app-icon.png
```

Expected:
- `wrote assets/app-icon.png (1024x1024)`
- `file` output contains `PNG image data, 1024 x 1024, 8-bit/color RGBA`
- size in range 8-60 KB (mostly solid colors compress well).

Optional: `open assets/app-icon.png` to visually inspect — should show a slate-blue rounded square with a white rounded-rectangle outline and center dot.

- [ ] **Step 3: Delete the generator and clean up**

```bash
rm src/bin/gen_app_icon.rs
rmdir src/bin 2>/dev/null || true
```

- [ ] **Step 4: Verify the main crate still builds**

```bash
cargo check
```
Expected: clean. No binary targets other than the default `quickshot`.

- [ ] **Step 5: Commit**

```bash
git add assets/app-icon.png
git commit -m "feat(assets): add 1024x1024 app icon PNG"
```

---

## Task 2: Info.plist template + .gitignore update

**Files:**
- Create: `scripts/Info.plist.in`
- Modify: `.gitignore`

- [ ] **Step 1: Create the scripts directory and Info.plist template**

```bash
mkdir -p scripts
```

Create `scripts/Info.plist.in`:
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

- [ ] **Step 2: Add `/dist` to `.gitignore`**

Edit `.gitignore`. After the edit it should read (preserving whatever was there before):
```
/target
.DS_Store
/.worktrees
/dist
```

- [ ] **Step 3: Verify**

```bash
cat .gitignore | grep -x '/dist' || echo MISSING
cat scripts/Info.plist.in | head -5
```
Expected: first command prints `/dist`; second prints the XML preamble.

Run `plutil -lint` to confirm the template is valid XML (placeholders don't break parsing because they're inside `<string>` tags):
```bash
plutil -lint scripts/Info.plist.in
```
Expected: `scripts/Info.plist.in: OK`.

- [ ] **Step 4: Commit**

```bash
git add scripts/Info.plist.in .gitignore
git commit -m "build: Info.plist template + gitignore dist/"
```

---

## Task 3: `package.sh` — phases 1 (build universal + bundle + sign)

Write the packaging script up through the codesign step. Output at end of this task: `dist/quickshot.app` exists, is signed (ad-hoc), and passes `codesign --verify`.

**Files:**
- Create: `scripts/package.sh`

- [ ] **Step 1: Write `scripts/package.sh`**

Create `scripts/package.sh`:
```bash
#!/usr/bin/env bash
#
# quickshot packaging script — produces dist/quickshot.app and dist/quickshot-<version>.dmg.
#
# Env overrides:
#   BUNDLE_ID      bundle identifier     (default: com.quickshot.app)
#   SIGN_IDENTITY  codesign identity     (default: "-" — ad-hoc)
#
# Requirements:
#   macOS with Xcode Command Line Tools (lipo, sips, iconutil, codesign, hdiutil, plutil)
#   rustup + cargo
#   Rust targets: x86_64-apple-darwin, aarch64-apple-darwin (auto-installed)

set -euo pipefail

# --- config ----------------------------------------------------------------
BUNDLE_ID="${BUNDLE_ID:-com.quickshot.app}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"

# --- derive version from Cargo.toml ---------------------------------------
VERSION=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)
if [ -z "$VERSION" ]; then
    echo "package.sh: could not read version from Cargo.toml" >&2
    exit 1
fi

# --- preflight -------------------------------------------------------------
if [ ! -f assets/app-icon.png ]; then
    echo "package.sh: assets/app-icon.png missing — Task 1 was supposed to generate + commit this." >&2
    exit 1
fi
if [ ! -f scripts/Info.plist.in ]; then
    echo "package.sh: scripts/Info.plist.in missing — Task 2 was supposed to create this." >&2
    exit 1
fi

for tool in cargo rustup lipo sips iconutil codesign hdiutil plutil sed awk; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "package.sh: required tool not found: $tool" >&2
        exit 1
    fi
done

# --- build universal binary ------------------------------------------------
echo "==> installing rust targets (idempotent)"
rustup target add x86_64-apple-darwin 2>/dev/null || true
rustup target add aarch64-apple-darwin 2>/dev/null || true

echo "==> cargo build x86_64"
cargo build --release --target x86_64-apple-darwin

echo "==> cargo build aarch64"
cargo build --release --target aarch64-apple-darwin

mkdir -p dist

echo "==> lipo: universal binary"
lipo -create \
    target/x86_64-apple-darwin/release/quickshot \
    target/aarch64-apple-darwin/release/quickshot \
    -output dist/quickshot-universal

# --- build .icns -----------------------------------------------------------
echo "==> iconset → .icns"
ICONSET="dist/quickshot.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for sz in 16 32 64 128 256 512 1024; do
    sips -z "$sz" "$sz" assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}.png" >/dev/null
done
for sz in 16 32 128 256 512; do
    dbl=$((sz * 2))
    sips -z "$dbl" "$dbl" assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}@2x.png" >/dev/null
done
iconutil -c icns -o dist/quickshot.icns "$ICONSET"
rm -rf "$ICONSET"

# --- assemble .app --------------------------------------------------------
APP="dist/quickshot.app"
echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp dist/quickshot-universal "$APP/Contents/MacOS/quickshot"
chmod +x "$APP/Contents/MacOS/quickshot"
cp dist/quickshot.icns "$APP/Contents/Resources/quickshot.icns"

sed -e "s/{{VERSION}}/$VERSION/g" -e "s|{{BUNDLE_ID}}|$BUNDLE_ID|g" \
    scripts/Info.plist.in > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

# --- codesign --------------------------------------------------------------
echo "==> codesigning with identity: $SIGN_IDENTITY"
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --verbose=2 "$APP"

echo ""
echo "phase-1 done:"
echo "  $APP"
ls -lh "$APP/Contents/MacOS/quickshot" | awk '{print "    binary:", $5}'
```

Make it executable:
```bash
chmod +x scripts/package.sh
```

- [ ] **Step 2: Run the script (phase-1-only output)**

```bash
bash scripts/package.sh
```

Expected:
- `==> cargo build x86_64` ... succeeds
- `==> cargo build aarch64` ... succeeds (may take longer on first run due to target install)
- `==> lipo: universal binary`
- `==> iconset → .icns`
- `==> assembling dist/quickshot.app`
- `dist/quickshot.app/Contents/Info.plist: OK`
- `==> codesigning with identity: -`
- `dist/quickshot.app: valid on disk`
- `dist/quickshot.app: satisfies its Designated Requirement`

If `cargo build --target` fails with "can't find crate for `std`" type errors, the target wasn't installed — run `rustup target add aarch64-apple-darwin` manually and re-run the script.

- [ ] **Step 3: Manually verify the bundle**

```bash
file dist/quickshot.app/Contents/MacOS/quickshot
lipo -info dist/quickshot.app/Contents/MacOS/quickshot
plutil -p dist/quickshot.app/Contents/Info.plist | head -20
codesign --verify --verbose=2 dist/quickshot.app
ls -lh dist/quickshot.app/Contents/MacOS/quickshot dist/quickshot.app/Contents/Resources/quickshot.icns
```

Expected:
- `file` reports universal Mach-O with two arches.
- `lipo -info` shows `x86_64 arm64`.
- `plutil -p` shows CFBundleExecutable = quickshot, LSUIElement = 1, version matches Cargo.toml, NSScreenCaptureUsageDescription present.
- codesign verify passes.

- [ ] **Step 4: Commit**

```bash
git add scripts/package.sh
git commit -m "build: package.sh phase 1 — universal binary + .app + ad-hoc sign"
```

---

## Task 4: `package.sh` — phase 2 (DMG + cleanup)

Extend `package.sh` with the DMG packaging step and intermediate-file cleanup. Output after this task: `dist/quickshot.app` and `dist/quickshot-{VERSION}.dmg`.

**Files:**
- Modify: `scripts/package.sh`

- [ ] **Step 1: Append DMG packaging + cleanup to `package.sh`**

At the end of `scripts/package.sh` (after the `phase-1 done:` block), append:

```bash

# --- DMG -------------------------------------------------------------------
DMG="dist/quickshot-$VERSION.dmg"
echo ""
echo "==> creating $DMG"
rm -f "$DMG"

STAGING="dist/dmg-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
    -volname "quickshot" \
    -srcfolder "$STAGING" \
    -ov \
    -format UDZO \
    "$DMG" >/dev/null

rm -rf "$STAGING"

# --- cleanup ---------------------------------------------------------------
rm -f dist/quickshot-universal dist/quickshot.icns

# --- report ----------------------------------------------------------------
echo ""
echo "done:"
echo "  $APP"
echo "  $DMG  ($(du -h "$DMG" | cut -f1))"
```

Also update the earlier `phase-1 done:` echo — remove it or keep it; redundancy is fine but cleaner to drop. Replace:
```
echo ""
echo "phase-1 done:"
echo "  $APP"
ls -lh "$APP/Contents/MacOS/quickshot" | awk '{print "    binary:", $5}'
```
with simply:
```
ls -lh "$APP/Contents/MacOS/quickshot" | awk '{print "    binary:", $5}'
```

So the final script has exactly one `done:` summary at the end.

- [ ] **Step 2: Re-run the script end-to-end**

```bash
bash scripts/package.sh
```

Expected output ends with:
```
done:
  dist/quickshot.app
  dist/quickshot-0.5.0.dmg  (~1.5M)
```

- [ ] **Step 3: Verify DMG**

```bash
hdiutil verify "dist/quickshot-$VERSION.dmg" 2>/dev/null || hdiutil verify dist/quickshot-*.dmg
```
Expected: `hdiutil: verify: checksum of "..." is VALID`.

Optional quick inspection:
```bash
hdiutil attach dist/quickshot-*.dmg
ls /Volumes/quickshot/
hdiutil detach /Volumes/quickshot/
```
Expected: `quickshot.app` and `Applications` symlink visible in `/Volumes/quickshot`.

- [ ] **Step 4: Verify intermediate files are cleaned up**

```bash
ls dist/
```
Expected: shows `quickshot.app/` and `quickshot-0.5.0.dmg` only. No `quickshot-universal`, no `quickshot.icns`, no `dmg-staging`.

- [ ] **Step 5: Commit**

```bash
git add scripts/package.sh
git commit -m "build: package.sh phase 2 — DMG + intermediate cleanup"
```

---

## Task 5: README install & uninstall docs

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Read the current README**

Run:
```bash
cat README.md
```
Review the existing structure: Build / Run / Status (Iter 3) / Config / Autostart sections.

- [ ] **Step 2: Prepend an "Install" section at the top**

Add a new `## Install (macOS)` section right BEFORE the `## Build` section. The new section reads:

```markdown
## Install (macOS)

Download the latest `quickshot-<VERSION>.dmg` from Releases (or build it yourself — see [Build](#build)). Double-click the DMG, drag `quickshot.app` to the `Applications` folder, eject the DMG.

First launch: Finder will warn that "quickshot.app can't be opened because it is from an unidentified developer." This is expected for open-source apps without Apple Developer ID signing. Work around it once:

1. Open `Applications` in Finder.
2. **Right-click** `quickshot.app` → **Open**.
3. Click **Open** in the confirmation sheet.

macOS remembers the override. Future launches (including autostart at login) work without the prompt.

On first capture, macOS will also ask for Screen Recording permission: System Settings → Privacy & Security → Screen Recording → enable `quickshot`, then relaunch the app.

### Uninstall

```bash
# If autostart was installed:
/Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart

# Remove the app itself:
rm -rf /Applications/quickshot.app

# Optional: remove config + saved screenshots config dir:
rm -rf ~/.config/quickshot
```
```

- [ ] **Step 3: Update the "Build" section to mention packaging**

The existing `## Build` section likely reads:
```
## Build

    cargo build --release

Binary lands at `target/release/quickshot`.
```

Replace with:
```markdown
## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

### Package as .app + .dmg

    bash scripts/package.sh

Produces:

- `dist/quickshot.app` — universal macOS bundle (x86_64 + aarch64), ad-hoc signed
- `dist/quickshot-<VERSION>.dmg` — distributable disk image

Environment overrides: `BUNDLE_ID` (default `com.quickshot.app`), `SIGN_IDENTITY` (default `-` ad-hoc; pass an Apple Developer ID identity string for full signing).
```

- [ ] **Step 4: Update the "Autostart (macOS)" section to mention the bundled path**

The existing section is currently:
```markdown
## Autostart (macOS)

    quickshot --install-autostart      # installs LaunchAgent + launches at login
    quickshot --uninstall-autostart    # removes it
```

Replace with:
```markdown
## Autostart (macOS)

When the binary is installed as an app bundle in `/Applications`:

    /Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart

To uninstall:

    /Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart

For bare-binary users (running from `target/release/quickshot`), the same flags apply to that binary path.
```

- [ ] **Step 5: Verify + commit**

```bash
grep -n "Install (macOS)" README.md
grep -n "package.sh" README.md
grep -n "Contents/MacOS/quickshot --install-autostart" README.md
```
Each should match at least once.

```bash
git add README.md
git commit -m "docs: install, build .app/.dmg, and bundle-path autostart instructions"
```

---

## Task 6: Polish pass + tag

**Files:** none modified; verification + tag only.

- [ ] **Step 1: Full test + clippy run**

```bash
cargo test
cargo clippy --release --all-targets -- -D warnings
```
Expected: 71 passed / 0 failed / 2 ignored; clippy clean. No changes required — `src/` is frozen this iteration.

- [ ] **Step 2: Full end-to-end package run**

```bash
rm -rf dist
bash scripts/package.sh
```
Expected: clean end-to-end pipeline; `dist/quickshot.app` and `dist/quickshot-<VERSION>.dmg` produced; summary printed.

- [ ] **Step 3: Manual smoke test (deferred to user)**

Controller note: this requires actually launching the bundled app, granting Screen Recording, exercising the hotkeys, and confirming the tray icon appears. Not subagent-friendly.

Record the bundle size for the README update (optional — can be added in-place):

```bash
ls -lh dist/
du -sh dist/quickshot.app dist/quickshot-*.dmg
```

- [ ] **Step 4: Tag**

```bash
git tag -a v0.5.0-iter4 -m "Iter 4: .app bundle + .dmg packaging (macOS)"
```

Verify:
```bash
git tag -l 'v0.*'
```
Expected: shows `v0.1.0-mvp`, `v0.2.0-iter2a`, `v0.3.0-iter2b`, `v0.4.0-iter3`, `v0.5.0-iter4`.

- [ ] **Step 5: Final check of repository state**

```bash
git status --short
```
Expected: clean working tree, only `dist/` present and ignored (no staged or unstaged changes).

---

## Manual verification checklist (whole plan)

Run on a real macOS host. `dist/` populated from `bash scripts/package.sh`.

1. `file dist/quickshot.app/Contents/MacOS/quickshot` → `Mach-O universal binary with 2 architectures`.
2. `lipo -info dist/quickshot.app/Contents/MacOS/quickshot` → includes `x86_64 arm64`.
3. `plutil -lint dist/quickshot.app/Contents/Info.plist` → `OK`.
4. `codesign --verify --verbose=2 dist/quickshot.app` → `satisfies its Designated Requirement`.
5. `hdiutil verify dist/quickshot-*.dmg` → valid.
6. Double-click the DMG → Finder shows `quickshot.app` + `Applications` symlink.
7. Drag `quickshot.app` into `Applications`. Eject the DMG.
8. Double-click `/Applications/quickshot.app` → Gatekeeper blocks. Right-click → Open → "Open Anyway".
9. Tray icon appears in menu bar. NO entry in Dock. NO entry in Cmd+Tab.
10. `Cmd+Shift+A` → overlay works as Iter 2a.
11. `Cmd+Shift+S` → full-screen capture + notification.
12. Tray → Quit → app exits cleanly.
13. `/Applications/quickshot.app/Contents/MacOS/quickshot --install-autostart` → plist at `~/Library/LaunchAgents/com.quickshot.daemon.plist`; `plutil -p` the plist → `ProgramArguments` shows `/Applications/quickshot.app/Contents/MacOS/quickshot`.
14. Log out / log in → tray icon reappears.
15. `/Applications/quickshot.app/Contents/MacOS/quickshot --uninstall-autostart` → LaunchAgent removed.
16. Remove `/Applications/quickshot.app`; `rm -rf ~/.config/quickshot` — clean uninstall.

Regression:
17. All 71 unit tests still pass on the source tree.

---

## Out of scope (deferred)

- Apple Developer ID signing (requires $99/year account)
- Notarization (requires Developer ID)
- Mac App Store distribution
- Windows `.msi` / Linux AppImage
- GitHub Actions CI for automated release builds
- Styled DMG backgrounds / installer UX (create-dmg tool integration)
- Sparkle auto-update integration
- Iter 2c polish items still pending (TTF subset, coord helper, estimate_text_width)
