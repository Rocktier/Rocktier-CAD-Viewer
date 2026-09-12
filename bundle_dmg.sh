#!/usr/bin/env bash
# Wrap a signed .app in a DMG without notarization.
# Output: $HOME/Downloads/Rocktier CAD Viewer.dmg
set -euo pipefail

APP="$(pwd)/src-tauri/target/release/bundle/macos/Rocktier CAD Viewer.app"
if [ ! -d "$APP" ]; then
  echo "▸ .app not found. Run ./build_app.sh first."
  exit 1
fi

# Verify ad-hoc signature + entitlements are present.
if ! codesign --verify --deep "$APP" 2>&1; then
  echo "▸ App signature invalid. Run ./build_app.sh first."
  exit 1
fi

CREATE_DMG="/opt/homebrew/bin/create-dmg"
if [ ! -x "$CREATE_DMG" ]; then
  echo "▸ create-dmg not found at $CREATE_DMG"
  exit 1
fi

VOL_NAME="Rocktier CAD Viewer"
OUT_DMG="$HOME/Downloads/Rocktier CAD Viewer.dmg"
TMP_DMG="$(pwd)/src-tauri/target/release/bundle/dmg/Rocktier_CAD_Viewer.tmp.dmg"

rm -f "$OUT_DMG"
mkdir -p "$(dirname "$TMP_DMG")"

echo "▸ Creating DMG via create-dmg..."
"$CREATE_DMG" \
  --volname "$VOL_NAME" \
  --window-pos 200 120 \
  --window-size 600 400 \
  --icon-size 110 \
  --icon "Rocktier CAD Viewer.app" 130 185 \
  --app-drop-link 450 185 \
  --format UDZO \
  --no-internet-enable \
  --skip-jenkins \
  "$TMP_DMG" \
  "$APP"

# create-dmg places the .dmg alongside the .app by default; move to Downloads.
FINAL_TMP="$(pwd)/src-tauri/target/release/bundle/dmg/Rocktier CAD Viewer.dmg"
if [ -f "$FINAL_TMP" ]; then
  mv -f "$FINAL_TMP" "$OUT_DMG"
fi
if [ -f "$TMP_DMG" ] && [ ! -f "$OUT_DMG" ]; then
  mv -f "$TMP_DMG" "$OUT_DMG"
fi
[ -f "$OUT_DMG" ] || { echo "▸ create-dmg did not produce .dmg"; exit 1; }

echo ""
echo "▸ DMG ready: $OUT_DMG"
echo "▸ Size: $(du -h "$OUT_DMG" | cut -f1)"
