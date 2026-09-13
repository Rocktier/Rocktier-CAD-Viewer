#!/usr/bin/env python3
"""Rasterise a dumped scene so a human (or an agent) can compare it with a
reference viewer (AutoCAD / MiniCAD / …) without opening the GUI app.

Pipeline:

    cd src-tauri
    cargo run --features tools --bin coverage -- <file.dwg> --dump /tmp/dump
    python3 ../tools/render_scene.py /tmp/dump -o /tmp/shot.png

It mirrors what the WebGL renderer draws: hairlines first, then points, then the
canvas-2D text overlay.  Everything is drawn on the viewer's dark background
(#0a0a0a) because ACI 7 ("auto") resolves to white there.

This is a *diagnostic* renderer: no anti-aliasing tuning, no layer panel, no
camera state.  Its job is to make "the drawing looks incomplete" falsifiable.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

BG = (10, 10, 10)
AUTO = (255, 255, 255)
STRIDE = 12


def load_font(px: int) -> ImageFont.FreeTypeFont:
    for path in (
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ):
        try:
            return ImageFont.truetype(path, max(6, int(px)))
        except OSError:
            continue
    return ImageFont.load_default()


def color_of(r: int, g: int, b: int, a: int) -> tuple[int, int, int]:
    """a == 0 is the "auto" colour: white on the viewer's dark canvas."""
    return AUTO if a == 0 else (r, g, b)


def iter_vertices(blob: bytes):
    for off in range(0, len(blob) - STRIDE + 1, STRIDE):
        x, y = struct.unpack_from("<ff", blob, off)
        r, g, b, a = blob[off + 8 : off + 12]
        yield x, y, r, g, b, a


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dump", type=Path, help="目录，含 meta.json 与 geometry.bin")
    ap.add_argument("-o", "--out", type=Path, default=Path("scene.png"))
    ap.add_argument("--layout", type=int, default=0)
    ap.add_argument("--width", type=int, default=1800)
    ap.add_argument("--no-text", action="store_true", help="只画几何，不画文字")
    ap.add_argument("--hide", type=int, nargs="*", default=[], help="隐藏的图层序号")
    args = ap.parse_args()

    meta = json.loads((args.dump / "meta.json").read_text(encoding="utf-8"))
    geometry = (args.dump / "geometry.bin").read_bytes()
    layout = meta["layouts"][args.layout]

    minx, maxx = layout["min_x"], layout["max_x"]
    miny, maxy = layout["min_y"], layout["max_y"]
    spanx, spany = max(maxx - minx, 1e-9), max(maxy - miny, 1e-9)
    width = args.width
    height = max(1, int(width * spany / spanx))
    scale = min(width / spanx, height / spany) * 0.94

    def tf(x: float, y: float) -> tuple[float, float]:
        return (x - minx) * scale + (width - spanx * scale) / 2, (maxy - y) * scale + (
            height - spany * scale
        ) / 2

    img = Image.new("RGB", (width, height), BG)
    draw = ImageDraw.Draw(img)
    hidden = set(args.hide)

    # ---- filled triangles first (they are the background of the drawing)
    tris = geometry[layout["tris_offset"] : layout["tris_offset"] + layout["tris_len"]]
    for rng in layout.get("tri_ranges", []):
        if rng["layer"] in hidden:
            continue
        chunk = tris[rng["offset"] : rng["offset"] + rng["len"]]
        verts = list(iter_vertices(chunk))
        for i in range(0, len(verts) - 2, 3):
            a, b, c = verts[i : i + 3]
            draw.polygon(
                [tf(a[0], a[1]), tf(b[0], b[1]), tf(c[0], c[1])],
                fill=color_of(a[2], a[3], a[4], a[5]),
            )

    # ---- hairlines
    lines = geometry[
        layout["lines_offset"] : layout["lines_offset"] + layout["lines_len"]
    ]
    drawn = 0
    for rng in layout["line_ranges"]:
        if rng["layer"] in hidden:
            continue
        chunk = lines[rng["offset"] : rng["offset"] + rng["len"]]
        verts = list(iter_vertices(chunk))
        for i in range(0, len(verts) - 1, 2):
            x1, y1, r, g, b, a = verts[i]
            x2, y2, _, _, _, _ = verts[i + 1]
            draw.line([tf(x1, y1), tf(x2, y2)], fill=color_of(r, g, b, a), width=1)
            drawn += 1

    # ---- points
    points = geometry[
        layout["points_offset"] : layout["points_offset"] + layout["points_len"]
    ]
    for rng in layout["point_ranges"]:
        if rng["layer"] in hidden:
            continue
        chunk = points[rng["offset"] : rng["offset"] + rng["len"]]
        for x, y, r, g, b, a in iter_vertices(chunk):
            sx, sy = tf(x, y)
            col = color_of(r, g, b, a)
            draw.rectangle([sx - 1, sy - 1, sx + 1, sy + 1], fill=col)

    # ---- text overlay (canvas-2D in the app)
    texts = [t for t in meta["texts"] if t["layout"] == args.layout and t["layer"] not in hidden]
    if not args.no_text:
        for t in texts:
            px = abs(t["h"]) * scale
            if px < 4 or px > height * 0.6:
                continue
            font = load_font(px)
            col = color_of(t["r"], t["g"], t["b"], t["a"])
            anchor = ("l" if t["ha"] == 0 else "m" if t["ha"] == 1 else "r") + (
                "s" if t["va"] == 0 else "d" if t["va"] == 1 else "m" if t["va"] == 2 else "a"
            )
            x0, y0, x1, y1 = font.getbbox(t["text"], anchor=anchor)
            pad = 4
            cw = int(2 * max(abs(x0), abs(x1)) + 2 * pad + px)
            ch = int(2 * max(abs(y0), abs(y1)) + 2 * pad + px)
            tile = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
            ImageDraw.Draw(tile).text(
                (cw / 2, ch / 2), t["text"], font=font, fill=col + (255,), anchor=anchor
            )
            if abs(t["rot"]) > 0.5:
                tile = tile.rotate(t["rot"], resample=Image.BICUBIC, center=(cw / 2, ch / 2))
            sx, sy = tf(t["x"], t["y"])
            img.paste(tile, (int(sx - cw / 2), int(sy - ch / 2)), tile)

    # ---- WIPEOUT masks: background-coloured, painted over everything else
    masks = geometry[layout["masks_offset"] : layout["masks_offset"] + layout["masks_len"]]
    for rng in layout.get("mask_ranges", []):
        if rng["layer"] in hidden:
            continue
        chunk = masks[rng["offset"] : rng["offset"] + rng["len"]]
        verts = list(iter_vertices(chunk))
        for i in range(0, len(verts) - 2, 3):
            a, b, c = verts[i : i + 3]
            draw.polygon([tf(a[0], a[1]), tf(b[0], b[1]), tf(c[0], c[1])], fill=BG)

    img.save(args.out)
    print(
        f"{args.out}  {width}x{height}  ·  {drawn} 段 · {len(texts)} 文字"
        f"  ·  布局 {layout} [{layout['name']}]"
        f"  ·  跳过 {meta.get('skipped', 0)}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
