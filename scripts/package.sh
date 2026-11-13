#!/usr/bin/env bash
#
# QuickShot packaging script — produces dist/QuickShot.app and dist/QuickShot-<version>.dmg.
#
# Env overrides:
#   BUNDLE_ID      bundle identifier     (default: com.QuickShot.app)
#   SIGN_IDENTITY  codesign identity     (default: "-" — ad-hoc)
#
# Requirements:
#   macOS with Xcode Command Line Tools (lipo, sips, iconutil, codesign, hdiutil, plutil)
#   rustup + cargo
#   Rust targets: x86_64-apple-darwin, aarch64-apple-darwin (auto-installed)

set -euo pipefail

# --- config ----------------------------------------------------------------
# Preserve the capture app's shipping identity and its Screen Recording grant.
BUNDLE_ID="${BUNDLE_ID:-com.QuickShot.app}"
# A local install may already have a grant tied to another code signing identifier.
SIGNING_IDENTIFIER="${SIGNING_IDENTIFIER:-$BUNDLE_ID}"
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
    target/x86_64-apple-darwin/release/QuickShot \
    target/aarch64-apple-darwin/release/QuickShot \
    -output dist/QuickShot-universal

# --- build .icns -----------------------------------------------------------
echo "==> iconset → .icns"
ICONSET="dist/QuickShot.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for sz in 16 32 64 128 256 512 1024; do
    sips -z "$sz" "$sz" assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}.png" >/dev/null
done
for sz in 16 32 128 256 512; do
    dbl=$((sz * 2))
    sips -z "$dbl" "$dbl" assets/app-icon.png --out "$ICONSET/icon_${sz}x${sz}@2x.png" >/dev/null
done
iconutil -c icns -o dist/QuickShot.icns "$ICONSET"
rm -rf "$ICONSET"

# --- assemble .app --------------------------------------------------------
APP="dist/QuickShot.app"
echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp dist/QuickShot-universal "$APP/Contents/MacOS/QuickShot"
chmod +x "$APP/Contents/MacOS/QuickShot"
cp dist/QuickShot.icns "$APP/Contents/Resources/QuickShot.icns"

sed -e "s/{{VERSION}}/$VERSION/g" -e "s|{{BUNDLE_ID}}|$BUNDLE_ID|g" \
    scripts/Info.plist.in > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

# usernoted lowercases click destinations. Keep notifications in their own
# lowercase bundle so their callbacks work without migrating capture permissions.
HELPER="$APP/Contents/Helpers/QuickShot Notifications.app"
HELPER_ID=$(printf '%s.notifications' "$BUNDLE_ID" | LC_ALL=C tr '[:upper:]' '[:lower:]')
mkdir -p "$HELPER/Contents/MacOS" "$HELPER/Contents/Resources"
cp "$APP/Contents/Resources/QuickShot.icns" "$HELPER/Contents/Resources/"
cp "$APP/Contents/Info.plist" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleIdentifier -string "$HELPER_ID" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleExecutable -string QuickShotNotifications "$HELPER/Contents/Info.plist"
plutil -remove NSScreenCaptureUsageDescription "$HELPER/Contents/Info.plist"
xcrun clang -arch arm64 -arch x86_64 -mmacosx-version-min=11.0 -Os -fobjc-arc \
    -framework AppKit -framework UserNotifications src/macos_notifications.m \
    -o "$HELPER/Contents/MacOS/QuickShotNotifications"
# --- codesign --------------------------------------------------------------
# Default (SIGN_IDENTITY="-") is ad-hoc: unchanged behavior, no secrets needed.
# A real identity (passed via env, e.g. from the CI ephemeral keychain) enables
# hardened runtime + entitlements and signs the inner binary before the bundle.
if [ "$SIGN_IDENTITY" = "-" ]; then
    echo "==> codesigning ad-hoc"
    codesign --force --sign "-" "$HELPER"
    codesign --force --identifier "$SIGNING_IDENTIFIER" --sign "-" "$APP"
else
    echo "==> codesigning with real identity (value hidden)"
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$HELPER"
    ENTITLEMENTS="scripts/entitlements.mac.plist"
    codesign --force --identifier "$SIGNING_IDENTIFIER" --options runtime --entitlements "$ENTITLEMENTS" \
        --timestamp --sign "$SIGN_IDENTITY" "$APP/Contents/MacOS/QuickShot"
    codesign --force --identifier "$SIGNING_IDENTIFIER" --options runtime --entitlements "$ENTITLEMENTS" \
        --timestamp --sign "$SIGN_IDENTITY" "$APP"
fi
codesign --verify --verbose=2 "$APP"

# --- DMG -------------------------------------------------------------------
DMG="dist/QuickShot-$VERSION.dmg"
echo ""
echo "==> creating $DMG"
rm -f "$DMG"

STAGING="dist/dmg-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
    -volname "QuickShot" \
    -srcfolder "$STAGING" \
    -ov \
    -format UDZO \
    "$DMG" >/dev/null

rm -rf "$STAGING"

# --- cleanup ---------------------------------------------------------------
rm -f dist/QuickShot-universal dist/QuickShot.icns

# --- report ----------------------------------------------------------------
echo ""
echo "done:"
echo "  $APP"
echo "  $DMG  ($(du -h "$DMG" | cut -f1))"
