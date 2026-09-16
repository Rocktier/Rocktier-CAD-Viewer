#!/usr/bin/env python3
"""Generate a synthetic demo floor plan for the Microsoft Store screenshots.

Why synthetic: real drawings in testdata/real/ are customer files and must not
end up in a public Store listing. This script draws a small office plan
(double-line walls, doors with swing arcs, windows, furniture, dimensions) using
only LINE / LWPOLYLINE / CIRCLE / ARC / TEXT, which is what the viewer's own
dxf_ascii parser handles best.

Run:  python3 store-assets/make-demo-plan.py
Out:  store-assets/office-plan.dxf
"""

import math
import os

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "office-plan.dxf")

W, H = 12000.0, 8000.0          # overall building envelope (mm)
T_EXT, T_INT = 240.0, 120.0     # wall thicknesses

ents = []


# ── DXF primitives ────────────────────────────────────────────────
def line(layer, x1, y1, x2, y2):
    ents.append(
        f"0\nLINE\n8\n{layer}\n"
        f"10\n{x1:.1f}\n20\n{y1:.1f}\n30\n0.0\n"
        f"11\n{x2:.1f}\n21\n{y2:.1f}\n31\n0.0\n"
    )


def lwpoly(layer, pts, closed=False):
    body = f"0\nLWPOLYLINE\n8\n{layer}\n90\n{len(pts)}\n70\n{1 if closed else 0}\n"
    for x, y in pts:
        body += f"10\n{x:.1f}\n20\n{y:.1f}\n"
    ents.append(body)


def circle(layer, cx, cy, r):
    ents.append(
        f"0\nCIRCLE\n8\n{layer}\n10\n{cx:.1f}\n20\n{cy:.1f}\n30\n0.0\n40\n{r:.1f}\n"
    )


def arc(layer, cx, cy, r, a0, a1):
    ents.append(
        f"0\nARC\n8\n{layer}\n10\n{cx:.1f}\n20\n{cy:.1f}\n30\n0.0\n"
        f"40\n{r:.1f}\n50\n{a0:.2f}\n51\n{a1:.2f}\n"
    )


def text(layer, x, y, h, value):
    ents.append(f"0\nTEXT\n8\n{layer}\n10\n{x:.1f}\n20\n{y:.1f}\n30\n0.0\n40\n{h:.1f}\n1\n{value}\n")


# ── Walls: a run of solid segments, gaps left for doors/windows ────
def wall(layer, x1, y1, x2, y2, t, gaps=()):
    """Axis-aligned wall as a double line, with `gaps` = [(from, to)] along its
    length (measured from the start point) left open for openings."""
    horizontal = abs(y2 - y1) < 1e-6
    length = (x2 - x1) if horizontal else (y2 - y1)

    runs, cursor = [], 0.0
    for a, b in sorted(gaps):
        if a > cursor:
            runs.append((cursor, a))
        cursor = max(cursor, b)
    if cursor < length:
        runs.append((cursor, length))

    half = t / 2.0
    for a, b in runs:
        if horizontal:
            ya, yb = y1 - half, y1 + half
            xa, xb = x1 + a, x1 + b
        else:
            xa, xb = x1 - half, x1 + half
            ya, yb = y1 + a, y1 + b
        if horizontal:
            line(layer, xa, ya, xb, ya)
            line(layer, xa, yb, xb, yb)
            line(layer, xa, ya, xa, yb)
            line(layer, xb, ya, xb, yb)
        else:
            line(layer, xa, ya, xa, yb)
            line(layer, xb, ya, xb, yb)
            line(layer, xa, ya, xb, ya)
            line(layer, xa, yb, xb, yb)


def door(px, py, width, layer="A-DOOR", swing="ccw", axis="h"):
    """Door leaf + quarter-circle swing inside the opening."""
    if axis == "h":
        arc(layer, px, py, width, 0, 90) if swing == "ccw" else arc(layer, px, py, width, 90, 180)
        line(layer, px, py, px, py + width)
    else:
        arc(layer, px, py, width, 0, 90) if swing == "ccw" else arc(layer, px, py, width, 270, 360)
        line(layer, px, py, px + width, py)


def window(layer, x1, y1, x2, y2, t):
    """Three thin parallel lines across an opening."""
    horizontal = abs(y2 - y1) < 1e-6
    if horizontal:
        for dy in (-t / 2 * 0.6, 0.0, t / 2 * 0.6):
            line(layer, x1, y1 + dy, x2, y2 + dy)
    else:
        for dx in (-t / 2 * 0.6, 0.0, t / 2 * 0.6):
            line(layer, x1 + dx, y1, x2 + dx, y2)


def dimension(x1, y1, x2, y2, offset, label):
    """Linear dimension with extension lines, ticks and text."""
    horizontal = abs(y2 - y1) < 1e-6
    if horizontal:
        y = y1 + offset
        line("A-DIMS", x1, y1, x1, y + 300)
        line("A-DIMS", x2, y2, x2, y + 300)
        line("A-DIMS", x1, y, x2, y)
        for x in (x1, x2):
            line("A-DIMS", x - 100, y - 100, x + 100, y + 100)
            line("A-DIMS", x - 100, y + 100, x + 100, y - 100)
        text("A-DIMS", (x1 + x2) / 2 - 600, y + 180, 300, label)
    else:
        x = x1 + offset
        line("A-DIMS", x1, y1, x + 300, y1)
        line("A-DIMS", x2, y2, x + 300, y2)
        line("A-DIMS", x, y1, x, y2)
        for y in (y1, y2):
            line("A-DIMS", x - 100, y - 100, x + 100, y + 100)
            line("A-DIMS", x - 100, y + 100, x + 100, y - 100)
        text("A-DIMS", x + 180, (y1 + y2) / 2 - 300, 300, label)


# ── Building shell ────────────────────────────────────────────────
wall("A-WALL", 0, 0, W, 0, T_EXT, [(2900, 3900)])                 # south, main entry
wall("A-WALL", W, 0, W, H, T_EXT, [(5000, 6800)])                 # east, window
wall("A-WALL", W, H, 0, H, T_EXT, [(1600, 3400), (4300, 6100)])   # north, two windows
wall("A-WALL", 0, H, 0, 0, T_EXT, [(1700, 3300)])                 # west, window

# Interior partitions (gaps are the door openings)
wall("A-WALL", 7600, 0, 7600, H, T_INT, [(5400, 6300)])           # vertical spine
wall("A-WALL", 0, 4600, 7600, 4600, T_INT, [(2700, 3600)])        # office A / B
wall("A-WALL", 7600, 3000, W, 3000, T_INT, [(9000, 9800)])        # meeting / storage

# ── Openings ──────────────────────────────────────────────────────
door(2900, 120, 1000, swing="ccw")                                # main entrance
door(7480, 5400, 900, swing="ccw", axis="v")                      # into meeting room
door(9000, 3060, 800, swing="ccw")                                # into storage
door(2700, 4480, 900, swing="ccw")                                # between offices

window("A-WINDOW", 1600, H - T_EXT / 2, 3400, H - T_EXT / 2, T_EXT)
window("A-WINDOW", 4300, H - T_EXT / 2, 6100, H - T_EXT / 2, T_EXT)
window("A-WINDOW", W - T_EXT / 2, 5000, W - T_EXT / 2, 6800, T_EXT)
window("A-WINDOW", T_EXT / 2, 1700, T_EXT / 2, 3300, T_EXT)

# ── Furniture ─────────────────────────────────────────────────────
def desk(x, y, w=1400, d=700):
    lwpoly("A-FURN", [(x, y), (x + w, y), (x + w, y + d), (x, y + d)], closed=True)
    circle("A-FURN", x + w / 2, y - 420, 260)
    arc("A-FURN", x + w / 2, y - 420, 420, 200, 340)


for i in range(4):                                                # OFFICE A desks
    desk(600 + i * 1750, 6900)

for i in range(2):                                                # OFFICE B desks
    desk(900 + i * 1900, 3400)

# Meeting room table + chairs
lwpoly("A-FURN", [(8300, 4300), (11200, 4300), (11200, 6500), (8300, 6500)], closed=True)
for i in range(3):
    circle("A-FURN", 8700 + i * 1000, 4050, 250)
    circle("A-FURN", 8700 + i * 1000, 6750, 250)

# Storage shelving
for i in range(5):
    lwpoly("A-FURN", [(7900, 400 + i * 480), (11600, 400 + i * 480),
                      (11600, 750 + i * 480), (7900, 750 + i * 480)], closed=True)

# ── Room labels ───────────────────────────────────────────────────
text("A-TEXT", 2600, 7200, 420, "OFFICE A")
text("A-TEXT", 2800, 6850, 240, "42.5 m2")

text("A-TEXT", 2600, 3800, 420, "OFFICE B")
text("A-TEXT", 2800, 3450, 240, "35.0 m2")

text("A-TEXT", 8700, 7300, 420, "MEETING ROOM")
text("A-TEXT", 8900, 6950, 240, "28.0 m2")

text("A-TEXT", 8400, 2000, 420, "STORAGE")
text("A-TEXT", 8600, 1650, 240, "18.4 m2")

text("A-TEXT", 240, 150, 260, "DEMO FLOOR PLAN  |  SCALE 1:50  |  UNITS: mm")

# ── Dimensions ────────────────────────────────────────────────────
dimension(0, 0, W, 0, -900, "12000")
dimension(0, 0, 0, H, -900, "8000")

# ── Column grid (structural, dashed feel via short segments) ──────
for gx in (0, 4000, 7600, W):
    for gy in (0, 3000, H):
        if 0 <= gx <= W and 0 <= gy <= H:
            circle("A-GRID", gx, gy, 260)
            line("A-GRID", gx - 420, gy, gx + 420, gy)
            line("A-GRID", gx, gy - 420, gx, gy + 420)


# ── Assemble the DXF ──────────────────────────────────────────────
LAYERS = [
    ("A-WALL", 7), ("A-DOOR", 3), ("A-WINDOW", 4),
    ("A-FURN", 5), ("A-TEXT", 6), ("A-DIMS", 2), ("A-GRID", 1),
]

layer_table = "0\nTABLE\n2\nLAYER\n70\n%d\n" % len(LAYERS)
for name, color in LAYERS:
    layer_table += f"0\nLAYER\n2\n{name}\n70\n0\n62\n{color}\n6\nCONTINUOUS\n"
layer_table += "0\nENDTAB\n"

doc = (
    "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n"
    "9\n$INSUNITS\n70\n4\n0\nENDSEC\n"
    "0\nSECTION\n2\nTABLES\n" + layer_table + "0\nENDSEC\n"
    "0\nSECTION\n2\nENTITIES\n" + "".join(ents) + "0\nENDSEC\n"
    "0\nEOF\n"
)

with open(OUT, "w", encoding="utf-8") as f:
    f.write(doc)

print(f"✅ {OUT}  ({len(ents)} entities, {len(doc) / 1024:.1f} KB)")
