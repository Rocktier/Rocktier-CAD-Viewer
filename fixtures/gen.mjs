/**
 * fixtures/gen.mjs — generates developer test DXF files.
 * Covers: multi-layer ACI colors, bulge polyline, ellipse arc, spline,
 * two-level block nesting, MTEXT stacked fraction, layout (paper space).
 * Run: node fixtures/gen.mjs
 */
import { writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = __dirname;
mkdirSync(OUT, { recursive: true });

// ---- tiny DXF helpers --------------------------------------------------
// DXF group codes and values must start at column 0 — no indentation.

function gc(code, value) {
  return code + "\n" + value + "\n";
}

function section(type, body) {
  return "0\nSECTION\n2\n" + type + "\n" + body + "0\nENDSEC\n";
}

function header(vars) {
  // DXF HEADER: var name preceded by group code 9.
  // Most string values use code 1, but $DWGCODEPAGE uses code 3.
  let body = "";
  for (const [name, val] of vars) {
    const vcode = name === "$DWGCODEPAGE" ? 3 : 1;
    body += gc(9, name) + gc(vcode, val);
  }
  return section("HEADER", body);
}

function layerTable(layers) {
  // DXF requires ENDTAB after each TABLE.
  let body = "0\nTABLE\n2\nLAYER\n70\n" + layers.length + "\n";
  for (const [name, color] of layers) {
    body += "0\nLAYER\n2\n" + name + "\n70\n0\n62\n" + color + "\n6\nContinuous\n";
  }
  body += "0\nENDTAB\n";
  return body;
}

function entsSection(items) {
  return section("ENTITIES", items.join(""));
}

function blocksSection(items) {
  return section("BLOCKS", items.join(""));
}

function eof() { return "0\nEOF\n"; }

function wrap(...sections) {
  return sections.join("") + eof();
}

function row(group, value) { return gc(group, value); }

function entLayer(l) { return gc(8, l); }
function entColor(c) { return gc(62, String(c)); }

function ent(type, attrs) {
  return "0\n" + type + "\n" + attrs.join("");
}

// ---- geometry builders -------------------------------------------------

const aci = {
  red: 1, green: 3, cyan: 4, blue: 5, magenta: 6,
  yellow: 2, white: 7, dkGray: 8,
  green2: 96, blue2: 130, red2: 216,
};

function line(x1, y1, x2, y2, layer = "0", colorStr = "") {
  return ent("LINE", [entLayer(layer), colorStr,
    gc(10, x1), gc(20, y1), gc(30, "0.0"),
    gc(11, x2), gc(21, y2), gc(31, "0.0"),
  ]);
}

function circle(cx, cy, r, layer = "0", colorStr = "") {
  return ent("CIRCLE", [entLayer(layer), colorStr,
    gc(10, cx), gc(20, cy), gc(30, "0.0"), gc(40, r),
  ]);
}

function arc(cx, cy, r, start, end, layer = "0", colorStr = "") {
  return ent("ARC", [entLayer(layer), colorStr,
    gc(10, cx), gc(20, cy), gc(30, "0.0"), gc(40, r),
    gc(50, start), gc(51, end),
  ]);
}

function lwpolyline(points, layer = "0", colorStr = "") {
  const head = [entLayer(layer), colorStr,
    gc(90, points.length), gc(70, "0"),
  ];
  for (const p of points) {
    head.push(gc(10, p.x), gc(20, p.y));
    if (p.b !== undefined) head.push(gc(42, p.b));
  }
  return ent("LWPOLYLINE", head);
}

function ellipse(cx, cy, mvec, ratio, layer = "0", colorStr = "") {
  return ent("ELLIPSE", [entLayer(layer), colorStr,
    gc(10, cx), gc(20, cy), gc(30, "0.0"),
    gc(11, mvec[0]), gc(21, mvec[1]), gc(31, "0.0"),
    gc(40, ratio),
  ]);
}

function spline(ctlPts, knots, layer = "0", colorStr = "", degree = 3) {
  const kc = knots.length, pc = ctlPts.length;
  const head = [entLayer(layer), colorStr,
    gc(70, "10"), gc(71, degree), gc(72, kc), gc(73, pc), gc(74, "0"),
    gc(42, (knots[0] - 1e-9).toFixed(6)), gc(43, (knots[0] - 1e-9).toFixed(6)),
    gc(44, (knots[kc - 1] + 1e-9).toFixed(6)),
  ];
  for (const k of knots) head.push(gc(40, k));
  for (const p of ctlPts) {
    head.push(gc(10, p.x), gc(20, p.y), gc(30, "0.0"));
  }
  return ent("SPLINE", head);
}

function point(x, y, layer = "0", colorStr = "") {
  return ent("POINT", [entLayer(layer), colorStr,
    gc(10, x), gc(20, y), gc(30, "0.0"),
  ]);
}

function text(x, y, h, content, layer = "0", colorStr = "") {
  return ent("TEXT", [entLayer(layer), colorStr,
    gc(10, x), gc(20, y), gc(30, "0.0"), gc(40, h), gc(1, content),
  ]);
}

function mtext(x, y, h, content, layer = "0", colorStr = "") {
  return ent("MTEXT", [entLayer(layer), colorStr,
    gc(10, x), gc(20, y), gc(30, "0.0"), gc(40, h), gc(1, content),
  ]);
}

function insert(blockName, x, y, layer = "0", colorStr = "") {
  return ent("INSERT", [entLayer(layer), colorStr,
    gc(2, blockName), gc(10, x), gc(20, y), gc(30, "0.0"),
  ]);
}

function solid(x1, y1, x2, y2, x3, y3, x4, y4, layer = "0", colorStr = "") {
  return ent("SOLID", [entLayer(layer), colorStr,
    gc(10, x1), gc(20, y1), gc(30, "0.0"),
    gc(11, x2), gc(21, y2), gc(31, "0.0"),
    gc(12, x4), gc(22, y4), gc(32, "0.0"),
    gc(13, x3), gc(23, y3), gc(33, "0.0"),
  ]);
}

// ---- DXF files ---------------------------------------------------------

// 1. Multi-layer ACI color fan
const f1 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([
    ["WALL", aci.red], ["DOOR", aci.green], ["WINDOW", aci.cyan],
    ["DIM", aci.magenta], ["FURN", aci.yellow],
  ])),
  entsSection([
    line(-100, -80, 100, -80, "WALL", entColor(aci.red)),
    line(100, -80, 100, 80, "WALL", entColor(aci.red)),
    line(100, 80, -100, 80, "WALL", entColor(aci.red)),
    line(-100, 80, -100, -80, "WALL", entColor(aci.red)),
    arc(0, -80, 38, 180, 0, "DOOR", entColor(aci.green)),
    line(-38, -80, 38, -80, "DOOR", entColor(aci.green)),
    line(-60, -80, -20, -80, "WINDOW", entColor(aci.cyan)),
    line(20, -80, 60, -80, "WINDOW", entColor(aci.cyan)),
    line(-80, 0, -80, 40, "WINDOW", entColor(aci.cyan)),
    line(-110, -90, -110, 90, "DIM", entColor(aci.magenta)),
    circle(60, 40, 15, "FURN", entColor(aci.yellow)),
    text(0, -110, 5, "Rocktier CAD Viewer", "FURN"),
  ]),
);

// 2. Bulge polyline
const f2 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7]])),
  entsSection([
    lwpolyline([
      { x: 0, y: 30, b: 0.5 },
      { x: 0, y: 70 },
      { x: 30, y: 100, b: 0.5 },
      { x: 70, y: 100 },
      { x: 100, y: 70, b: 0.5 },
      { x: 100, y: 30 },
      { x: 70, y: 0, b: 0.5 },
      { x: 30, y: 0 },
    ], "0", entColor(aci.blue)),
    arc(0, 50, 20, 270, 90, "0", entColor(aci.red)),
  ]),
);

// 3. Ellipse arc
const f3 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7]])),
  entsSection([
    ellipse(0, 0, [80, 0], 0.5, "0", entColor(aci.red)),
    ellipse(0, 0, [0, 60], 0.4, "0", entColor(aci.green)),
    text(-120, 0, 5, "Ellipse major=80 ratio=0.5", "0"),
  ]),
);

// 4. Spline
const f4 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7]])),
  entsSection([
    spline(
      [
        { x: -100, y: 0 }, { x: -50, y: 80 }, { x: 0, y: 40 },
        { x: 50, y: 80 }, { x: 100, y: 0 },
      ],
      [0, 0, 0, 0, 1, 2, 3, 4, 4, 4],
      "0", entColor(aci.cyan),
    ),
    text(-100, -20, 3, "Spline (De Boor, deg=3)", "0"),
  ]),
);

// 5. Block with nested insert (ROOM contains WIN blocks and DOOR block, WIN draws a window cross)
function block(name, baseLayer, body) {
  return "0\nBLOCK\n2\n" + name + "\n8\n" + baseLayer + "\n10\n0.0\n20\n0.0\n30\n0.0\n70\n0\n"
    + body + "0\nENDBLK\n";
}

const winEntsLWP = ent("LWPOLYLINE", [entLayer("0"), gc(90, "4"), gc(70, "1"),
  gc(10, -10), gc(20, -10), gc(10, 10), gc(20, -10),
  gc(10, 10), gc(20, 10), gc(10, -10), gc(20, 10),
]);
const winEntsLINE1 = ent("LINE", [entLayer("0"), gc(10, -10), gc(20, 0), gc(30, "0.0"), gc(11, 10), gc(21, 0), gc(31, "0.0")]);
const winEntsLINE2 = ent("LINE", [entLayer("0"), gc(10, 0), gc(20, -10), gc(30, "0.0"), gc(11, 0), gc(21, 10), gc(31, "0.0")]);

const f5 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7]])),
  section("BLOCKS",
    block("WIN", "0", winEntsLWP + winEntsLINE1 + winEntsLINE2)
    + block("DOOR", "0", ent("LINE", [entLayer("0"), gc(10, -20), gc(20, 0), gc(30, "0.0"), gc(11, 20), gc(21, 0), gc(31, "0.0")]))
    + block("ROOM", "0",
        ent("LWPOLYLINE", [entLayer("0"), gc(90, "4"), gc(70, "1"),
          gc(10, -100), gc(20, -60), gc(10, 100), gc(20, -60),
          gc(10, 100), gc(20, 60), gc(10, -100), gc(20, 60),
        ])
        + insert("WIN", -50, 0, "0")
        + insert("WIN", 50, 0, "0")
        + insert("DOOR", 0, -60, "0")
      )
  ),
  entsSection([
    insert("ROOM", -100, 0, "0", entColor(aci.white)),
    insert("ROOM", 150, 0, "0", entColor(aci.white)),
  ]),
);

// 6. MTEXT with stacked fraction and special chars
const f6 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7]])),
  entsSection([
    text(0, 0, 3, "Hello DXF \\Pmulti\\Pline", "0", entColor(aci.red2)),
    text(0, 10, 3, "%%d=degree  %%c=dia  %%p=plus/minus", "0"),
    mtext(0, 25, 3, "Stacked\\S1/2; fraction", "0", entColor(aci.blue2)),
    point(0, 40, "0", entColor(aci.green)),
    circle(0, 40, 5, "0"),
    solid(-10, 50, 10, 50, 5, 60, -5, 60, "0", entColor(aci.dkGray)),
  ]),
);

// 7. Layouts (model + paper via *PAPER_SPACE block)
const f7 = wrap(
  header([["$ACADVER", "AC1027"]]),
  section("TABLES", layerTable([["0", 7], ["TITLE", aci.cyan]])),
  section("BLOCKS",
    block("*Paper_Space", "0",
      ent("TEXT", [entLayer("TITLE"), gc(1, "SHEET A1 标题栏"),
        gc(10, 0), gc(20, 80), gc(30, "0.0"), gc(40, 5),
      ])
      + ent("LINE", [entLayer("TITLE"),
        gc(10, -80), gc(20, 60), gc(30, "0.0"),
        gc(11, 80), gc(21, 60), gc(31, "0.0"),
      ])
    )
  ),
  entsSection([
    line(-50, -50, 50, 50, "0", entColor(aci.red)),
    line(50, -50, -50, 50, "0", entColor(aci.green)),
    circle(0, 0, 35, "0", entColor(aci.blue)),
    text(0, 60, 2.5, "MODEL SPACE 模型空间", "0"),
  ]),
);

    writeFileSync(join(OUT, "01_fan.dxf"), f1);
    writeFileSync(join(OUT, "02_bulge.dxf"), f2);
    writeFileSync(join(OUT, "03_ellipse.dxf"), f3);
    writeFileSync(join(OUT, "04_spline.dxf"), f4);
    writeFileSync(join(OUT, "05_nesting.dxf"), f5);
    writeFileSync(join(OUT, "06_mtext.dxf"), f6);
    writeFileSync(join(OUT, "07_layout.dxf"), f7);

console.log("OK 7 DXF fixtures written to " + OUT);
