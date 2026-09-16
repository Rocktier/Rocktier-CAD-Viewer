#!/usr/bin/env node
/**
 * Copies the canonical licence files into `src-tauri/resources/legal/` so they
 * actually travel with the app.
 *
 * Why copy instead of listing `"../LICENSE"` in `bundle.resources`: Tauri maps a
 * `..` segment to a `_up_/` folder. The macOS bundler keeps that
 * (`Contents/Resources/_up_/LICENSE`) but `tauri-windows-bundle` collects only
 * `resources/**` and silently drops everything else — the Microsoft Store
 * package would then redistribute LibreDWG (GPL-3.0-or-later) without its
 * licence or notices, which is exactly what GPL-3.0 forbids.
 *
 * Files under `resources/` land at `resources/legal/...` on both platforms, so
 * one mechanism covers macOS, MSI and MSIX.
 *
 * Runs from `beforeBuildCommand`, and from CI, so the copies can never drift
 * from the files at the repo root.
 */
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dest = join(root, "src-tauri", "resources", "legal");

mkdirSync(dest, { recursive: true });

for (const name of ["LICENSE", "THIRD-PARTY-NOTICES.md"]) {
  copyFileSync(join(root, name), join(dest, name));
  console.log(`legal -> src-tauri/resources/legal/${name}`);
}
