# Rocktier CAD Viewer

Fast, private, offline CAD viewer for DXF and DWG drawings. Tauri 2 + Svelte 5,
WebGL hairlines with a 2D canvas overlay for text and measuring.

## Requirements

- Node + npm
- Rust toolchain
- **LibreDWG** (`brew install libredwg`) — needed at **runtime** for DWG, via
  its `dwg2dxf` CLI. A copy next to the executable wins over `PATH`, so a
  packaged build can ship one. Not needed for DXF, and not a build dependency.

## Develop

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run build          # frontend only (tsc + vite)
./build_app.sh         # release .app, re-signed with Entitlements.plist
```

## Tests

```bash
cd src-tauri && cargo check --tests
cd src-tauri && cargo run --bin test_e2e --features e2e-test
```

`test_e2e` parses the DXF fixtures in `fixtures/`, validates the binary blob
layout, checks block-INSERT placement and layer colours, and — when a real
`.dwg` is present in `~/Downloads` — exercises the DWG pipeline. Regenerate
fixtures with `npm run fixtures`.

## Architecture

- `src-tauri/src/drawing.rs` — the single "file → scene" entry point: size cap,
  UTF-8/GBK decoding, and DWG conversion through `dwg2dxf` followed by the same
  DXF parser. One pipeline for both formats keeps block definitions, INSERTs,
  LAYER colours and TEXT/MTEXT intact.
- `src-tauri/src/dxf_ascii.rs` — hand-written ASCII DXF extractor: LINE, ARC,
  CIRCLE, LWPOLYLINE/POLYLINE, ELLIPSE, SPLINE, TEXT/MTEXT, POINT, INSERT and
  DIMENSION block expansion, LAYER table colours.
- `src-tauri/src/lib.rs` — window setup, the `opened` event (macOS
  `RunEvent::Opened`, Windows/Linux argv via the single-instance plugin) and
  the queued hand-off to the frontend.
- `src/lib/` — scene blob decoding, WebGL renderer, camera, layer panel state.

## Opening files

`.dwg` and `.dxf` are registered as document types in `tauri.conf.json`, so the
installed app can be chosen in "Open with" and set as the default handler. Two
things to keep in mind:

- Registry/Info.plist entries are only written by real bundles (`tauri build`),
  not by `tauri dev`.
- On Windows/Linux the shell opens files by launching a second process; the
  single-instance plugin forwards its argv to the running window.

## Known gaps

- DWG needs a `dwg2dxf` binary (LibreDWG) next to the executable or on `PATH`;
  `build_app.sh` bundles it into the macOS `.app`, the Windows build ships it as
  a sidecar.
- Text is decoded as UTF-8 or GBK (the two encodings Chinese exports actually
  use); other 8-bit code pages (BIG5, Shift-JIS, …) are not transcoded.
- MULTILEADER geometry lives in embedded context data and is not drawn; `LEADER`
  is.  WIPEOUT masks are drawn (background colour, last pass), patterns are
  regenerated from the pattern definition — islands are honoured, but a pattern
  whose definition a writer omitted falls back to its boundary.
- Paper space: one layout per `*Paper_Space*` block, named `Layout N`, with model
  space projected through each VIEWPORT.  Layout *names* come from the block
  names, not from the LAYOUT objects in the OBJECTS section.
- RAY / XLINE (infinite construction lines) are approximated by their two
  definition points; unresolved external references are counted as unsupported.

## Diagnostics

Two tools make "the drawing looks incomplete" a measurable claim instead of an
impression:

```bash
# Per-layer coverage + a blob dump of exactly what the viewer would draw
cd src-tauri
cargo run --features tools --bin coverage -- ../testdata/real/<drawing>.dwg --dump /tmp/dump

# Rasterise the dump to compare against AutoCAD / another viewer
python3 tools/render_scene.py /tmp/dump -o /tmp/shot.png
```

Real drawings used for regression live in `testdata/real/` and are git-ignored
(they are customer files); the E2E test picks the first one it finds, or honours
`RCV_REAL_DWG`.

## License

GPL-3.0 (DXF/DWG parsing depends on free software).
