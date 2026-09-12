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

# ── 3. Claim third-party CAD UTIs ────────────────────────────────
# Finder's "Open With → recommended" matches the file's *resolved* UTI, not the
# extension.  Other CAD viewers already installed on a user's Mac (迷你CAD,
# AutoCAD for Mac, …) may own the .dwg/.dxf UTIs, so we import those types and
# list them among the content types we handle — otherwise we only show up
# under "all applications".  Must run BEFORE re-signing (plist is sealed).
echo "▸ Importing third-party CAD UTIs..."
# NOTE: deliberately NOT "$PLIST" — that name is Entitlements.plist (used by
# codesign below); shadowing it here once signed the Info.plist *as* the
# entitlements and AMFI refused to spawn the app ("Launch failed", errno 153).
APP_PLIST="$APP/Contents/Info.plist"
PB=/usr/libexec/PlistBuddy
$PB -c "Print :UTImportedTypeDeclarations" "$APP_PLIST" >/dev/null 2>&1 || \
  $PB -c "Add :UTImportedTypeDeclarations array" "$APP_PLIST"
idx=0
for spec in \
  "com.autodesk.dwg:dwg:AutoCAD DWG drawing" \
  "com.autodesk.dxf:dxf:AutoCAD DXF drawing" \
  "com.tianji.macminicad.dwg:dwg:CAD DWG drawing" \
  "com.tianji.macminicad.dxf:dxf:CAD DXF drawing"; do
  uti="${spec%%:*}"; rest="${spec#*:}"; ext="${rest%%:*}"; desc="${rest#*:}"
  $PB -c "Add :UTImportedTypeDeclarations:$idx dict" "$APP_PLIST" 2>/dev/null || { idx=$((idx+1)); continue; }
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeIdentifier string $uti" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeDescription string $desc" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeConformsTo array" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeConformsTo:0 string public.data" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeTagSpecification dict" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeTagSpecification:public.filename-extension array" "$APP_PLIST"
  $PB -c "Add :UTImportedTypeDeclarations:$idx:UTTypeTagSpecification:public.filename-extension:0 string $ext" "$APP_PLIST"
  idx=$((idx+1))
done
# Doc types: 0 = dwg, 1 = dxf (order set by tauri.conf.json fileAssociations).
$PB -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes:1 string com.autodesk.dwg" "$APP_PLIST" 2>/dev/null || true
$PB -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes:2 string com.tianji.macminicad.dwg" "$APP_PLIST" 2>/dev/null || true
$PB -c "Add :CFBundleDocumentTypes:1:LSItemContentTypes:1 string com.autodesk.dxf" "$APP_PLIST" 2>/dev/null || true
$PB -c "Add :CFBundleDocumentTypes:1:LSItemContentTypes:2 string com.tianji.macminicad.dxf" "$APP_PLIST" 2>/dev/null || true

# ── 4. Re-sign the app with entitlements ─────────────────────────
#    Tauri bundler does not inject the entitlements plist; without it
#    WKWebView fails with "Could not create a com.apple.webkit.mach-bootstrap
#    sandbox extension" and the app appears to do nothing on launch.
echo "▸ Re-signing with entitlements..."
codesign --force --deep --entitlements "$PLIST" -s - "$APP"

echo ""
echo "▸ Done. App at: $APP"
echo "▸ Next: ./bundle_dmg.sh"
