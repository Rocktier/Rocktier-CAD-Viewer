#!/usr/bin/env node
/* CAD Viewer 多语言生成器（扁平结构版）。
 *
 * ── 为什么单独一个脚本而不是复用 PDF 的 ──
 * PDF 的字典是**嵌套对象**（`compress.profiles.web.label` 三层），
 * CAD 是**扁平 Record<string, string>** —— 键就是一行，没有嵌套。
 * 把两种结构塞进一个生成器会让两边都变复杂，所以分开。
 * 共享的是同一份家族术语表与同样的两道硬校验。
 *
 * ── 三道硬校验（都在生成期拦住）──
 * ① 缺键即拒绝生成 —— 不产出「界面一半英文」的包
 * ② 占位符逐字对齐 —— 丢了 {n} 界面上会直接露出来
 * ③ 结构对齐 —— 生成的字典必须与 `Dict` = Record<string,string> 一致
 *
 * 用法：
 *   node scripts/gen-i18n.mjs
 *   ROCKTIER_ROOT=/path node scripts/gen-i18n.mjs
 */

import { readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = process.env.ROCKTIER_ROOT || join(HERE, "..", "..", "..", "..");
const GLOSSARY = join(REPO, "docs", "rocktier", "i18n", "glossary.json");
const SRC = join(HERE, "..", "src", "lib", "i18n.svelte.ts");

const LANGS = ["ja", "ko", "de", "es", "pt", "ar"];
const args = process.argv.slice(2);
const only = args.includes("--lang") ? args[args.indexOf("--lang") + 1] : null;

if (!existsSync(GLOSSARY)) {
  console.error(`  ❌ 找不到家族术语表: ${GLOSSARY}\n     设 ROCKTIER_ROOT 指向家族根`);
  process.exit(2);
}
const G = JSON.parse(readFileSync(GLOSSARY, "utf8"));
const approved = new Map();
for (const [, loc] of Object.entries(G.terms)) {
  const en = loc.en?.value;
  if (!en) continue;
  const pack = {};
  for (const l of LANGS) if (loc[l]?.value) pack[l] = loc[l].value;
  if (Object.keys(pack).length) approved.set(en, pack);
}
console.error(`  术语表: ${approved.size} 条英文有家族批准译法`);

/* 用 esbuild 求值取 en 块的真实内容 —— 不用正则猜 TS 语法。
   手写解析器在 PDF 上错过三次（两层嵌套/跨行长文案/过度合并）。 */
/* esbuild 按绝对路径 require —— 本仓没装 esbuild（依赖在 PDF 仓里），
   `import "esbuild"` 会解析失败。用 createRequire 从 PDF 仓的
   node_modules 加载，比让每个产品仓都装一份 esbuild 干净。 */
/* 只抽取 `const en: Dict = { … };` 这一块再求值 ——
   直接 import 整个 i18n.svelte.ts 会连带加载 ./state.svelte（Svelte 运行时
   在 Node 里不存在），报 ERR_MODULE_NOT_FOUND。
   键名与字典值都是纯字面量，单独求值即可，不需要任何依赖。 */
const src = readFileSync(SRC, "utf8");
const enStart = src.indexOf("const en: Dict = {");
if (enStart < 0) {
  console.error("  ❌ 找不到 `const en: Dict = {` —— 生成器与源码结构脱节了");
  process.exit(2);
}
let depth = 0, enEnd = -1;
for (let i = src.indexOf("{", enStart); i < src.length; i++) {
  if (src[i] === "{") depth++;
  else if (src[i] === "}") { depth--; if (depth === 0) { enEnd = i + 1; break; } }
}
if (enEnd < 0) { console.error("  ❌ en 块花括号不配对"); process.exit(2); }

let EN;
const tmp = join(HERE, ".en.probe.mjs");
writeFileSync(tmp, `export default ${src.slice(src.indexOf("{", enStart), enEnd)};`);
try {
  EN = (await import(`${pathToFileURL(tmp).href}?t=${Date.now()}`)).default;
} finally {
  rmSync(tmp, { force: true });
}
const KEYS = Object.keys(EN);
console.error(`  en 块: ${KEYS.length} 个键`);

let missingTotal = 0;
const written = [];
for (const lang of only ? [only] : LANGS) {
  let ctx = {};
  try {
    ctx = (await import(`./i18n/${lang}.mjs`)).default || {};
  } catch { /* 该语言还没写 */ }

  const out = {};
  const missing = [];
  for (const k of KEYS) {
    const en = EN[k];
    const term = approved.get(en)?.[lang];
    const v = term ?? (typeof ctx[k] === "string" ? ctx[k] : null);
    if (v === null) { missing.push(`${k} = ${en.slice(0, 46)}`); continue; }
    const want = [...en.matchAll(/\{(\w+)\}/g)].map((x) => x[1]).sort().join(",");
    const got = [...v.matchAll(/\{(\w+)\}/g)].map((x) => x[1]).sort().join(",");
    if (want !== got) { missing.push(`${k} — 占位符不符（源 {${want}} / 译文 {${got}}）`); continue; }
    out[k] = v;
  }
  if (missing.length) {
    missingTotal += missing.length;
    console.error(`\n  ⚠️ ${lang}: 缺 ${missing.length} 条（不生成该语言）`);
    missing.forEach((m) => console.error(`      ${m}`));
    continue;
  }
  const content = [
    "/* 由 scripts/gen-i18n.mjs 生成 —— 请勿手改。",
    " * 术语取自 docs/rocktier/i18n/glossary.json（家族唯一真源）；",
    " * 其余取自 scripts/i18n/<lang>.mjs。",
    " * 改动流程：改术语表或译文表 → 重跑生成器。",
    ` * 语言: ${lang} · 键数: ${KEYS.length}`,
    " *",
    " * C 方案：机翻基线。接入翻译 API 后重跑生成器覆盖即可。 */",
    "",
    `export const ${lang}: Record<string, string> = {`,
    ...Object.entries(out).map(([k, v]) => `  ${k}: ${JSON.stringify(v)},`),
    "};",
    "",
  ].join("\n");
  writeFileSync(join(HERE, "..", "src", "lib", `i18n.${lang}.ts`), content);
  written.push(lang);
  console.error(`  ✅ ${lang}: ${KEYS.length} 条齐全 → src/lib/i18n.${lang}.ts`);
}
if (missingTotal) {
  console.error(`\n  ❌ 共 ${missingTotal} 条缺译文或占位符不符。补齐后重跑。\n`);
  process.exit(1);
}
console.error(`\n  完成：${written.join(", ") || "无"}\n`);