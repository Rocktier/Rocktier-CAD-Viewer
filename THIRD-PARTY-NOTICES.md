# Third-party notices

Rocktier CAD Viewer is released under the **MIT License**. The full text lives in
[LICENSE](LICENSE). There is **no warranty** for this program, to the extent permitted
by law.

> **Why the application itself is not GPL.** An earlier release declared
> `GPL-3.0-or-later` for the whole application. That was a choice, not an obligation.
> The only copyleft component involved is LibreDWG's `dwg2dxf`, which this application
> runs as a **separate process** and talks to through files in a public format — it is
> not linked into this binary in any form (no static or dynamic linkage, no headers, no
> shared address space). Under the standard reading that is *mere aggregation*, and the
> obligations attach to `dwg2dxf` itself, which is met in full below: its licence is
> reproduced, its source is offered, and it is unmodified. The components we do link —
> Tauri, Svelte, the Rust crates — are all permissively licensed.

---

## Bundled components

### LibreDWG — `dwg2dxf`

| | |
|---|---|
| **Component** | LibreDWG, specifically its `dwg2dxf` command-line tool |
| **Licence** | GNU General Public License v3.0 or later (`GPL-3.0-or-later`) |
| **Copyright** | the LibreDWG contributors (GNU project) |
| **Upstream source** | https://github.com/LibreDWG/libredwg |
| **Version shipped** | 0.14 (Windows sidecar pinned to `0.14.8597`; macOS ships the Homebrew build of 0.14) |
| **Redistributed as** | Windows: `resources/libredwg/dwg2dxf.exe`, `libredwg-0.dll` (+ codec DLLs)<br>macOS: `Contents/MacOS/dwg2dxf`, `Contents/MacOS/libredwg.0.dylib` |

LibreDWG is invoked as a **separate process**; no LibreDWG code is linked into the
application binary. DWG drawings are converted to DXF by `dwg2dxf`, and that DXF is
then parsed by this application's own parser.

**Corresponding source.** The unmodified upstream source for the exact version shipped
is published by the LibreDWG project at the link above (releases are tagged, so the
pinned Windows version is reproducible). If you would prefer the source in another
form, or the link above ever stops working, write to hello@rocktier.com and we will
provide it.

---

## Trademarks

Rocktier CAD Viewer is an **independent product**. It is not affiliated with, endorsed
by, or sponsored by Autodesk, Inc.

DWG, DXF and AutoCAD are trademarks or registered trademarks of Autodesk, Inc. They
are named in this project only to describe the file formats the application can open —
a nominative use — and never as part of the product name, its logo, or its branding.

This application **reads** DWG and DXF drawings. It does not create, edit or write DWG
files, and it is not a substitute for a full CAD authoring suite.

All other trademarks are the property of their respective owners.

---

## Your drawings

Drawings are processed entirely on your machine. Nothing is uploaded, and the
application makes no network requests of its own — it declares no network capability
in its Windows package manifest.
