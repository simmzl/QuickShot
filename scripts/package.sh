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
