#!/bin/bash
# Generate PNG + ICNS from the CV-branded SVG logo.
set -e
ICONS_DIR="$(cd "$(dirname "$0")/src-tauri/icons" && pwd)"
SVG="$ICONS_DIR/icon-cv.svg"
TMP="$ICONS_DIR/.iconset"
SRC_1024="$TMP/icon_512x512@2x_1024.png"

mkdir -p "$TMP"

echo "Generating PNGs from $SVG ..."

# 1024×1024 master (rsvg-convert uses points; at 72 dpi, 1024 pt = 1024 px)
rsvg-convert -w 1024 -h 1024 -o "$SRC_1024" "$SVG" 2>/dev/null
[ -f "$SRC_1024" ] || {
  # Fallback: rsvg-convert writes at 90 dpi — 1025pt ≈ 1281px; this is fine for the
  # iconutil pipeline because it just reads pixel dimensions. Skip unit fidgeting.
  rsvg-convert -w 1024 -h 1024 -o "$SRC_1024" "$SVG"
}

# Scale down to required PNG sizes
make_png() {
  local size="$1"
  local dst="$2"
  sips -Z "$size" "$SRC_1024" --out "$dst" >/dev/null 2>&1
}

make_png 32  "$ICONS_DIR/32x32.png"
make_png 128 "$ICONS_DIR/128x128.png"
make_png 256 "$ICONS_DIR/128x128@2x.png"
make_png 512 "$ICONS_DIR/icon.png"

# .ico for Windows (contains 32 + 128)
echo "Building icon.ico ..."
sips -Z 32  "$SRC_1024" --out "$TMP/icon_32x32.png" >/dev/null 2>&1
sips -Z 128 "$SRC_1024" --out "$TMP/icon_128x128.png" >/dev/null 2>&1
# .ico via iconutil isn't available; construct with python Pillow if ICNS path fails
python3 - <<PY 2>/dev/null || echo "(Pillow missing — .ico will be rebuilt at bundle time)"
import sys
from PIL import Image
ico = Image.open("$SRC_1024")
# Generate 32 + 128 + 256 multi-size .ico
sizes = [(32,32),(128,128),(256,256)]
frames = [ico.resize(s, Image.LANCZOS) for s in sizes]
frames[0].save("$ICONS_DIR/icon.ico", format="ICO", sizes=sizes, append_images=frames[1:])
print("icon.ico done")
PY

# .icns for macOS
echo "Building icon.icns ..."
rm -rf "$TMP.iconset"
mkdir -p "$TMP.iconset"
sips -Z 16   "$SRC_1024" --out "$TMP.iconset/icon_16x16.png"    >/dev/null 2>&1
sips -Z 32   "$SRC_1024" --out "$TMP.iconset/icon_16x16@2x.png" >/dev/null 2>&1
sips -Z 32   "$SRC_1024" --out "$TMP.iconset/icon_32x32.png"    >/dev/null 2>&1
sips -Z 64   "$SRC_1024" --out "$TMP.iconset/icon_32x32@2x.png" >/dev/null 2>&1
sips -Z 128  "$SRC_1024" --out "$TMP.iconset/icon_128x128.png"  >/dev/null 2>&1
sips -Z 256  "$SRC_1024" --out "$TMP.iconset/icon_128x128@2x.png" >/dev/null 2>&1
sips -Z 256  "$SRC_1024" --out "$TMP.iconset/icon_256x256.png"  >/dev/null 2>&1
sips -Z 512  "$SRC_1024" --out "$TMP.iconset/icon_256x256@2x.png" >/dev/null 2>&1
sips -Z 512  "$SRC_1024" --out "$TMP.iconset/icon_512x512.png"  >/dev/null 2>&1
sips -Z 1024 "$SRC_1024" --out "$TMP.iconset/icon_512x512@2x.png" >/dev/null 2>&1
iconutil -c icns "$TMP.iconset" -o "$ICONS_DIR/icon.icns"
rm -rf "$TMP.iconset"

# Windows Store tiles (optional PNGs used by some Windows installer bundles)
for dim in 30 44 71 89 107 142 150 284 310; do
  sips -Z "$dim" "$SRC_1024" --out "$ICONS_DIR/Square${dim}x${dim}Logo.png" >/dev/null 2>&1
done
cp "$ICONS_DIR/Square30x30Logo.png" "$ICONS_DIR/StoreLogo.png"

echo "All icons written to $ICONS_DIR"
rm -f "$SRC_1024"
ls "$ICONS_DIR"
