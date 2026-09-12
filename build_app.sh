#!/usr/bin/env bash
set -euo pipefail

APP="$(pwd)/src-tauri/target/release/bundle/macos/Rocktier CAD Viewer.app"
PLIST="$(pwd)/src-tauri/Entitlements.plist"

# ── 1. Build release .app (the DMG is made by bundle_dmg.sh) ─────
echo "▸ Building release..."
npm run tauri build -- --bundles app

# ── 2. Ship the LibreDWG CLI inside the app ──────────────────────
#    A Finder-launched .app does not inherit the shell PATH, so a
#    Homebrew `dwg2dxf` would never be found.  Both files go next to the
#    executable (Contents/MacOS) so `@loader_path` resolves the dylib.
echo "▸ Bundling LibreDWG CLI (dwg2dxf)..."
PREFIX="${LIBREDWG_DIR:-/opt/homebrew/opt/libredwg}"
SRC_BIN="$PREFIX/bin/dwg2dxf"
SRC_LIB="$PREFIX/lib/libredwg.0.dylib"
if [ ! -x "$SRC_BIN" ] || [ ! -f "$SRC_LIB" ]; then
  echo "▸ dwg2dxf / libredwg.0.dylib not found under $PREFIX — set LIBREDWG_DIR"
  exit 1
fi

MACOS_DIR="$APP/Contents/MacOS"
cp -f "$SRC_BIN" "$MACOS_DIR/dwg2dxf"
cp -f "$SRC_LIB" "$MACOS_DIR/libredwg.0.dylib"
chmod u+w "$MACOS_DIR/dwg2dxf" "$MACOS_DIR/libredwg.0.dylib"

# Repoint the loader at the copy that travelled with the app.
OLD_LIB="$(otool -L "$MACOS_DIR/dwg2dxf" | awk '/libredwg.*\.dylib/ {print $1; exit}')"
if [ -n "$OLD_LIB" ]; then
  install_name_tool -change "$OLD_LIB" "@loader_path/libredwg.0.dylib" "$MACOS_DIR/dwg2dxf"
fi

# install_name_tool invalidates signatures, and arm64 refuses to run
# unsigned code — re-sign both ad-hoc.
codesign --force -s - "$MACOS_DIR/libredwg.0.dylib"
codesign --force -s - "$MACOS_DIR/dwg2dxf"

# ── 3. Re-sign the app with entitlements ─────────────────────────
#    Tauri bundler does not inject the entitlements plist; without it
#    WKWebView fails with "Could not create a com.apple.webkit.mach-bootstrap
#    sandbox extension" and the app appears to do nothing on launch.
echo "▸ Re-signing with entitlements..."
codesign --force --deep --entitlements "$PLIST" -s - "$APP"

echo ""
echo "▸ Done. App at: $APP"
echo "▸ Next: ./bundle_dmg.sh"
