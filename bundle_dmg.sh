#!/usr/bin/env bash
# Wrap the signed .app in a DMG.
# 签名与公证分工：.app 的签名在 build_app.sh（CI 里有证书时是 Developer ID），
# DMG 的签名 + notarytool 公证在 workflow 的 "Sign + notarize the DMG" 那步。
# Output: ${CAD_DMG_OUT:-$HOME/Downloads/Rocktier CAD Viewer.dmg}
#   CI 传 CAD_DMG_OUT 指到 bundle/dmg，避免依赖 $HOME/Downloads；本地不传则维持旧行为。
set -euo pipefail

APP="$(pwd)/src-tauri/target/release/bundle/macos/Rocktier CAD Viewer.app"
if [ ! -d "$APP" ]; then
  echo "▸ .app not found. Run ./build_app.sh first."
  exit 1
fi

# Verify the .app signature is valid and entitlements survived the re-sign.
if ! codesign --verify --deep "$APP" 2>&1; then
  echo "▸ App signature invalid. Run ./build_app.sh first."
  exit 1
fi

CREATE_DMG="$(command -v create-dmg || true)"
if [ -z "$CREATE_DMG" ] || [ ! -x "$CREATE_DMG" ]; then
  echo "▸ create-dmg not found on PATH"
  exit 1
fi

VOL_NAME="Rocktier CAD Viewer"
OUT_DMG="${CAD_DMG_OUT:-$HOME/Downloads/Rocktier CAD Viewer.dmg}"
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
if [ -f "$FINAL_TMP" ] && [ "$FINAL_TMP" != "$OUT_DMG" ]; then
  mv -f "$FINAL_TMP" "$OUT_DMG"
fi
if [ -f "$TMP_DMG" ] && [ ! -f "$OUT_DMG" ]; then
  mv -f "$TMP_DMG" "$OUT_DMG"
fi
[ -f "$OUT_DMG" ] || { echo "▸ create-dmg did not produce .dmg"; exit 1; }

echo ""
echo "▸ DMG ready: $OUT_DMG"
echo "▸ Size: $(du -h "$OUT_DMG" | cut -f1)"
