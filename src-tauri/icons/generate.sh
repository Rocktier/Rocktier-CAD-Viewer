#!/bin/bash
# Rocktier CAD Viewer — Icon generation script
# Generates all platform icon formats from icon.svg
#
# Dependencies:
#   - rsvg-convert  (brew install librsvg) OR Inkscape
#   - ImageMagick    (brew install imagemagick) for .ico
#   - iconutil (macOS built-in) for .icns
#
# Usage: cd src-tauri/icons && bash generate.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

SRC="icon.svg"
OUT_DIR="out"
ICONSET_DIR=".iconset"

echo "→ Generating Rocktier CAD Viewer icons..."

# Clean previous runs
rm -rf "$OUT_DIR" "$ICONSET_DIR"
mkdir -p "$OUT_DIR" "$ICONSET_DIR"

# ── 1. Render PNG at all required sizes ──
echo "  Rendering PNG sizes..."
SIZES=(16 24 32 48 64 128 256 512 1024)
for size in "${SIZES[@]}"; do
  if command -v rsvg-convert &>/dev/null; then
    rsvg-convert -w "$size" -h "$size" "$SRC" > "$OUT_DIR/icon-${size}.png"
  elif command -v inkscape &>/dev/null; then
    inkscape --export-type=png --export-width="$size" --export-height="$size" \
             --export-filename="$OUT_DIR/icon-${size}.png" "$SRC" 2>/dev/null
  else
    echo "ERROR: Need rsvg-convert or Inkscape to render PNG"
    exit 1
  fi
  echo "    ✓ icon-${size}.png"
done

# ── 2. macOS .icns (via iconutil) ──
echo "  Building macOS icon.icns..."
if [[ "$(uname)" == "Darwin" ]] && command -v iconutil &>/dev/null; then
  # iconutil requires @2x naming convention
  cp "$OUT_DIR/icon-16.png"    "$ICONSET_DIR/icon_16x16.png"
  cp "$OUT_DIR/icon-32.png"    "$ICONSET_DIR/icon_16x16@2x.png"
  cp "$OUT_DIR/icon-32.png"    "$ICONSET_DIR/icon_32x32.png"
  cp "$OUT_DIR/icon-64.png"    "$ICONSET_DIR/icon_32x32@2x.png"
  cp "$OUT_DIR/icon-128.png"   "$ICONSET_DIR/icon_128x128.png"
  cp "$OUT_DIR/icon-256.png"   "$ICONSET_DIR/icon_128x128@2x.png"
  cp "$OUT_DIR/icon-256.png"   "$ICONSET_DIR/icon_256x256.png"
  cp "$OUT_DIR/icon-512.png"   "$ICONSET_DIR/icon_256x256@2x.png"
  cp "$OUT_DIR/icon-512.png"   "$ICONSET_DIR/icon_512x512.png"
  cp "$OUT_DIR/icon-1024.png"  "$ICONSET_DIR/icon_512x512@2x.png"

  iconutil -c icns -o "icon.icns" "$ICONSET_DIR"
  echo "    ✓ icon.icns"
else
  echo "    ⚠ Skipped .icns (requires macOS iconutil)"
fi

# ── 3. Windows .ico ──
echo "  Building Windows icon.ico..."
if command -v convert &>/dev/null; then
  convert "$OUT_DIR/icon-256.png" \
          "$OUT_DIR/icon-128.png" \
          "$OUT_DIR/icon-64.png" \
          "$OUT_DIR/icon-48.png" \
          "$OUT_DIR/icon-32.png" \
          "$OUT_DIR/icon-24.png" \
          "$OUT_DIR/icon-16.png" \
          "icon.ico"
  echo "    ✓ icon.ico"
else
  echo "    ⚠ Skipped .ico (requires ImageMagick)"
fi

# ── 4. Store logo ──
echo "  Copying store logo..."
cp "$OUT_DIR/icon-1024.png" "StoreLogo.png"
echo "    ✓ StoreLogo.png"

# ── 5. Cleanup ──
rm -rf "$ICONSET_DIR"
echo ""
echo "Done. Output in $SCRIPT_DIR/"
echo "  icon.icns     — macOS app bundle"
echo "  icon.ico       — Windows app + NSIS installer"
echo "  StoreLogo.png  — Microsoft Store"
echo "  out/           — all PNG sizes for inspection"
