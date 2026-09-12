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
  the build does not bundle it yet.
- Pre-R2007 DWG text/layer names may use a non-GBK 8-bit codepage that is not
  transcoded.
- Multi-layout (paper space) extraction: a single "Model" layout is emitted.
- HATCH / SOLID / LEADER / RAY / XLINE geometry is skipped (shown by the status
  bar's "unsupported entities" counter).

## License

GPL-3.0 (DXF/DWG parsing depends on free software).
