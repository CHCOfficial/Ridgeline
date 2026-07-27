#!/bin/zsh
set -euo pipefail

ROOT_DIR="${0:A:h:h}"
VARIANT="${1:-classic}"
VARIANT="${VARIANT:l}"
DIST_DIR="$ROOT_DIR/dist/macos"
APP_BUNDLE="$DIST_DIR/RIDGELINE.app"
ZIP_ARCHIVE="$DIST_DIR/RIDGELINE-macOS-arm64.zip"
STAGING_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ridgeline-package.XXXXXX")"
STAGED_APP="$STAGING_ROOT/RIDGELINE.app"
STAGED_ZIP="$STAGING_ROOT/RIDGELINE-macOS-arm64.zip"
trap 'rm -rf "$STAGING_ROOT"' EXIT

case "$VARIANT" in
  classic) ICON_NAME="Ridgeline-Classic.icns" ;;
  party) ICON_NAME="Ridgeline-Party.icns" ;;
  contour) ICON_NAME="Ridgeline-Contour.icns" ;;
  *)
    print -u2 "Usage: $0 [classic|party|contour]"
    exit 2
    ;;
esac

ICON_PATH="$ROOT_DIR/assets/icons/macos/$ICON_NAME"
if [[ ! -f "$ICON_PATH" ]]; then
  print -u2 "Missing $ICON_PATH. Run scripts/build_macos_icons.sh first."
  exit 1
fi

cd "$ROOT_DIR"
cargo build --release

mkdir -p "$STAGED_APP/Contents/MacOS" "$STAGED_APP/Contents/Resources"
cp "$ROOT_DIR/target/release/ridgeline" "$STAGED_APP/Contents/MacOS/RIDGELINE"
cp "$ROOT_DIR/packaging/macos/Info.plist" "$STAGED_APP/Contents/Info.plist"
cp "$ICON_PATH" "$STAGED_APP/Contents/Resources/AppIcon.icns"
cp "$ROOT_DIR/LICENSE" "$STAGED_APP/Contents/Resources/LICENSE.txt"
mkdir -p "$STAGED_APP/Contents/Resources/music"
/bin/cp -X "$ROOT_DIR/music/"*.mp3 "$STAGED_APP/Contents/Resources/music/"
chmod 755 "$STAGED_APP/Contents/MacOS/RIDGELINE"

mkdir -p "$DIST_DIR/Icon Choices"
cp "$ROOT_DIR/assets/icons/macos/Ridgeline-Classic.icns" "$DIST_DIR/Icon Choices/"
cp "$ROOT_DIR/assets/icons/macos/Ridgeline-Party.icns" "$DIST_DIR/Icon Choices/"
cp "$ROOT_DIR/assets/icons/macos/Ridgeline-Contour.icns" "$DIST_DIR/Icon Choices/"

plutil -lint "$STAGED_APP/Contents/Info.plist" >/dev/null
/usr/bin/xattr -cr "$STAGED_APP"
codesign --force --deep --sign - "$STAGED_APP"
codesign --verify --deep --strict "$STAGED_APP"

rm -rf "$APP_BUNDLE"
rm -f "$ZIP_ARCHIVE"
ditto --norsrc --noextattr --noqtn --noacl "$STAGED_APP" "$APP_BUNDLE"
ditto -c -k --norsrc --noextattr --noqtn --noacl --keepParent "$STAGED_APP" "$STAGED_ZIP"
cp "$STAGED_ZIP" "$ZIP_ARCHIVE"

# Synced workspace providers may attach Finder/provenance metadata during the final copy. Strip it
# from the deliverable (the staged ZIP is already clean), then verify the actual bundle users open.
/usr/bin/xattr -cr "$APP_BUNDLE"
codesign --verify --deep --strict "$APP_BUNDLE"
unzip -tq "$ZIP_ARCHIVE"

print "Packaged RIDGELINE.app with the $VARIANT icon."
print "$APP_BUNDLE"
print "$ZIP_ARCHIVE"
