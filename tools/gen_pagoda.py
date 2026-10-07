#!/usr/bin/env python3
"""Generate examples/pagoda-garden: the "progada" voxel pagoda garden for Bozzard.

Ports the deterministic three.js generator of the original pagoda.html (LCG seed
1337) call for call, so terrain, pagoda, trees, props, clouds and koi land where
the original put them, then bakes the voxels for Bozzard's batch renderer:

* terrain, plaza, path, lantern posts, torii, stone lanterns, bridge, rocks and
  pond lanterns -> garden.gltf (exposed faces only, one primitive per colour);
* the five-tier pagoda -> pagoda.gltf;
* trees -> deterministic per-species variants, one single-primitive glTF per
  (variant, colour), instanced at every original tree position;
* flowers and petal carpets -> stock cubes with per-instance tint (glowing ones
  use two small emissive cube assets); koi -> scripted stock cubes;
* clouds -> translucent glTFs drifting on a script; petals -> particles.

Standard library only; run from anywhere. --check verifies the committed output
instead of writing it. Fixes for three bugs in the original (cherry trunks never
merged, terrain pits below y=-3, windows that never appear) are on by default and
draw from a separate random stream, so the shared layout stays identical;
--faithful reproduces the original's geometry exactly.
"""
import argparse
import base64
import json
import math
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = ROOT / "examples" / "pagoda-garden"
SEED = 1337
FIX_SEED = 0xC0FFEE


# --------------------------------------------------------------------------
# JavaScript semantics
# --------------------------------------------------------------------------
def js_round(x):
    """Math.round: halves round toward +infinity."""
    return math.floor(x + 0.5)


def smoothstep(x, lo, hi):
    """THREE.MathUtils.smoothstep."""
    if x <= lo:
        return 0.0
    if x >= hi:
        return 1.0
    x = (x - lo) / (hi - lo)
    return x * x * (3 - 2 * x)


class Lcg:
    """The original's `seed = (seed * 1664525 + 1013904223) >>> 0` stream."""

    def __init__(self, seed):
        self.seed = seed & 0xFFFFFFFF

    def rand(self):
        self.seed = (self.seed * 1664525 + 1013904223) & 0xFFFFFFFF
        return self.seed / 4294967296

    def rnd(self, a=1.0, b=None):
        return self.rand() * a if b is None else a + self.rand() * (b - a)

    def ri(self, a, b):
        return math.floor(self.rnd(a, b + 1))

    def chance(self, p):
        return self.rand() < p

    def pick(self, items):
        return items[math.floor(self.rand() * len(items))]


class Vox:
    """Insertion-ordered voxel map with the original's helpers. Every write can
    carry a category; the first non-tree voxel a tree overwrites is kept so the
    baked garden does not inherit holes from replaced tree shapes."""

    def __init__(self, rng):
        self.rng = rng
        self.map = {}
        self.category = {}
        self.under_trees = {}
        self.current = "terrain"

    def set(self, x, y, z, color):
        key = (js_round(x), js_round(y), js_round(z))
        old = self.category.get(key)
        if (self.current.startswith("tree:") and old is not None
                and not old.startswith("tree:") and key not in self.under_trees):
            self.under_trees[key] = (self.map[key], old)
        self.map[key] = color
        self.category[key] = self.current

    def has(self, x, y, z):
        return (js_round(x), js_round(y), js_round(z)) in self.map

    def box(self, x0, y0, z0, x1, y1, z1, color):
        x = x0
        while x <= x1:
            y = y0
            while y <= y1:
                z = z0
                while z <= z1:
                    self.set(x, y, z, color)
                    z += 1
                y += 1
            x += 1

    def disc(self, cx, cy, cz, r, color, h=0):
        r2 = r * r
        for x in range(math.ceil(cx - r), math.floor(cx + r) + 1):
            for z in range(math.ceil(cz - r), math.floor(cz + r) + 1):
                dx, dz = x - cx, z - cz
                if dx * dx + dz * dz <= r2:
                    for k in range(h + 1):
                        self.set(x, cy + k, z, color)

    def sphere(self, cx, cy, cz, r, color, fuzz=0.0):
        r2 = r * r
        for x in range(math.ceil(cx - r), math.floor(cx + r) + 1):
            for y in range(math.ceil(cy - r), math.floor(cy + r) + 1):
                for z in range(math.ceil(cz - r), math.floor(cz + r) + 1):
                    d = (x - cx) ** 2 + (y - cy) ** 2 + (z - cz) ** 2
                    if d <= r2 + (self.rng.rnd(0, fuzz) if fuzz else 0):
                        self.set(x, y, z, color)


# --------------------------------------------------------------------------
# Palette (sRGB hex, as authored)
# --------------------------------------------------------------------------
C = {
    "stone": [0x8f97a6, 0x9aa3b0, 0x7f8794, 0xa8b1bd],
    "stoneWarm": [0xb9a48a, 0xc7b39a, 0xa8937b],
    "wood": [0x8b3a2f, 0x9c4436, 0x7a2f26],
    "woodDark": [0x5d241d, 0x6b2a21],
    "roof": [0x2f4b7c, 0x37568d, 0x28406b],
    "roofEdge": [0xd9a441, 0xe8b552],
    "gold": [0xf2c14e, 0xffd76a, 0xe0ab35],
    "paper": [0xffd9e0, 0xffc2cf, 0xffe6ec],
    "pink1": [0xffb7ce, 0xffc9dc, 0xffa8c4, 0xf9c6d6],
    "pink2": [0xf28fab, 0xff9dbb, 0xe77fa0],
    "white": [0xffeef3, 0xfff6f8, 0xffe3ec],
    "leafD": [0x2f6b3a, 0x357a41, 0x28602f],
    "leafM": [0x4a9a4e, 0x57a95a, 0x3f8b45],
    "leafL": [0x7cc36a, 0x8fd07a, 0x6fb85e],
    "maple": [0xd9662f, 0xe07f3a, 0xc4501f],
    "trunk": [0x5a3a26, 0x6b452e, 0x4e3220],
    "trunkB": [0x8a6a44, 0x9c7a52],
    "grass": [0x4f8f4a, 0x5aa055, 0x458040, 0x63ab5c],
    "grassD": [0x3b6f38, 0x356232],
    "moss": [0x7fa65a, 0x8fb86a],
    "water": [0x2f7fbf, 0x3a90d0, 0x2a6fa8],
    "waterHi": [0x7fd0f0, 0x9adcf7],
    "sand": [0xd8c49a, 0xe4d2ab, 0xcbb58c],
    "gravel": [0xbfb6a6, 0xcdc4b4, 0xb0a694],
    "lantern": [0xff6b57, 0xff8a70, 0xf2503c],
    "lamp": [0xffe08a, 0xfff0b5],
    "red": [0xc0392b, 0xd04a3a],
    "cloud": [0xf4f7ff, 0xe8eefc, 0xffffff],
}
EMISSIVE = set(C["lamp"] + C["lantern"] + C["waterHi"] + C["gold"])
EMISSIVE_STRENGTH = 0.85
KOI_COLORS = [0xFF6B4A, 0xFFFFFF, 0xFFD98A]


def linear(hex_color):
    def channel(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return [round(channel((hex_color >> shift) & 255), 6) for shift in (16, 8, 0)]


def hex_name(color):
    return f"{color:06x}"


# --------------------------------------------------------------------------
# The original scene, call for call
# --------------------------------------------------------------------------
W = 78
POND = (26, 20)
POND_R = 11


def in_pond(x, z):
    dx = math.hypot(x - POND[0], (z - POND[1]) * 1.25)
    wob = 1.25 * math.sin(math.atan2(z - POND[1], x - POND[0]) * 3)
    return dx < POND_R + wob


def terrain_height(x, z):
    h = 0.0
    h += 4.2 * math.sin(x * 0.055) * math.cos(z * 0.045)
    h += 2.6 * math.sin((x + z) * 0.09 + 1.3)
    h += 1.8 * math.cos((x - z) * 0.11 - 0.7)
    h += 0.9 * math.sin(x * 0.21) * math.sin(z * 0.19)
    d_plaza = math.hypot(x, z)
    if d_plaza < 27:
        h *= smoothstep(d_plaza, 19, 27)
    d_pond = math.hypot(x - POND[0], (z - POND[1]) * 1.25)
    if d_pond < POND_R + 9:
        h *= smoothstep(d_pond, POND_R + 1.5, POND_R + 9)
    d_hill = math.hypot(x + 30, z - 34)
    if d_hill < 26:
        h += 9 * math.pow(max(0.0, 1 - d_hill / 26), 1.7)
    return h


def surf_y(x, z):
    return max(0, js_round(terrain_height(x, z)))


def tree_canopy(rng, cx, cy, cz, r, palette, density=1.0, blob=0.9):
    canopy = Vox(rng)
    for x in range(math.ceil(cx - r), math.floor(cx + r) + 1):
        for y in range(math.ceil(cy - r * 0.75), math.floor(cy + r * 0.85) + 1):
            for z in range(math.ceil(cz - r), math.floor(cz + r) + 1):
                d = (x - cx) ** 2 + ((y - cy) / 0.8) ** 2 + (z - cz) ** 2
                # Both random terms are evaluated, as in the original expression.
                limit = r * r * (0.78 + rng.rnd(0, 0.22))
                limit += rng.rnd(0, blob * r)
                if d <= limit and rng.chance(density):
                    canopy.set(x, y, z, rng.pick(palette))
    return canopy


def palette_name(palette):
    return next(name for name, colors in C.items() if colors is palette)


def tree_cherry(rng, out, cx, cz, scale, y0, carpets, trunk_out):
    """`carpets` scatters petal voxels on the terrain around the tree (world
    only); `trunk_out` receives the trunk voxels the original never merged."""
    height = js_round(9 * scale + rng.rnd(0, 4))
    trunk = Vox(rng)
    for k in range(1, height + 1):
        lean = math.sin(k * 0.25 + cx) * 0.6
        trunk.set(js_round(cx + lean * 0.3), y0 + k, js_round(cz + math.cos(k * 0.3 + cz) * 0.3),
                  rng.pick(C["trunk"]))
        if k > height * 0.45 and rng.chance(0.5):
            direction = rng.ri(0, 3)
            dx, dz = [1, -1, 0, 0][direction], [0, 0, 1, -1][direction]
            for b in range(1, 4):
                trunk.set(js_round(cx + lean * 0.3 + dx * b), y0 + k + js_round(b * 0.6),
                          js_round(cz + dz * b), rng.pick(C["trunk"]))
    cy = y0 + height + 1
    r = 5.5 * scale + rng.rnd(0, 2.2)
    pal = C["pink1"] if rng.chance(0.55) else (C["pink2"] if rng.chance(0.5) else C["white"])
    pal2 = C["pink1"] if rng.chance(0.5) else C["pink2"]
    canopy = tree_canopy(rng, cx, cy, cz, r, pal, 0.92, 1.1)
    for (x, yy, z), c in canopy.map.items():
        out.set(x, yy, z, rng.pick(pal2) if rng.chance(0.25) else c)
    if trunk_out is not None:
        trunk_out.update(trunk.map)
    if carpets is not None:
        world, category = carpets
        saved = world.current
        world.current = category
        for _ in range(26):
            px, pz = cx + rng.ri(-8, 8), cz + rng.ri(-8, 8)
            if not in_pond(px, pz):
                world.set(px, surf_y(px, pz) + 1, pz,
                          rng.pick(C["pink1"]) if rng.chance(0.5) else rng.pick(C["white"]))
        world.current = saved
    return palette_name(pal)


def tree_pine(rng, out, cx, cz, scale, y0):
    height = js_round(7 * scale + rng.rnd(0, 3))
    for k in range(1, height + 1):
        out.set(cx, y0 + k, cz, rng.pick(C["trunk"]))
        if k > height * 0.4:
            r = max(1, js_round((height - k + 2) * 0.9 * scale))
            pal = C["leafD"] if rng.chance(0.3) else C["leafM"]
            for x in range(cx - r, cx + r + 1):
                for z in range(cz - r, cz + r + 1):
                    if math.hypot(x - cx, z - cz) <= r and rng.chance(0.85):
                        out.set(x, y0 + k + 1, z, rng.pick(pal))
    out.disc(cx, y0 + height + 1, cz, js_round(2.2 * scale), rng.pick(C["leafD"]))
    return "leaf"


def tree_maple(rng, out, cx, cz, scale, y0):
    height = js_round(8 * scale)
    for k in range(1, height + 1):
        out.set(cx + js_round(math.sin(k * 0.4) * 0.4), y0 + k, cz, rng.pick(C["trunk"]))
    cy, r = y0 + height + 1, 4.2 * scale + rng.rnd(0, 1.5)
    pal = C["maple"] if rng.chance(0.5) else C["gold"]
    canopy = tree_canopy(rng, cx, cy, cz, r, pal, 0.9, 1.0)
    for (x, yy, z), c in canopy.map.items():
        out.set(x, yy, z, rng.pick(C["maple"]) if rng.chance(0.3) else c)
    return palette_name(pal)


def tree_willow(rng, out, cx, cz, y0):
    height = 10
    for k in range(1, height + 1):
        out.set(cx, y0 + k, cz, rng.pick(C["trunkB"]))
    canopy = tree_canopy(rng, cx, y0 + height + 1, cz, 4.5, C["leafL"], 0.85, 1.2)
    for (x, yy, z), c in canopy.map.items():
        out.set(x, yy, z, c)
    for _ in range(26):
        a, r = rng.rnd(0, 6.28), rng.rnd(1.5, 4.5)
        px, pz = js_round(cx + math.cos(a) * r), js_round(cz + math.sin(a) * r)
        top = y0 + height + 1 if canopy.has(px, y0 + height + 1, pz) else y0 + height
        yy = top
        while yy > top - rng.ri(3, 6):  # re-evaluated per iteration, as in the original
            if canopy.has(px, yy, pz) or rng.chance(0.6):
                out.set(px, yy, pz, rng.pick(C["leafL"]))
            yy -= 1
    return "leafL"


def generate(fixes):
    rng = Lcg(SEED)
    fix = Lcg(FIX_SEED)
    world = Vox(rng)
    out = {"trees": [], "cherry_trunks": [], "tiers": []}

    # Terrain
    columns = []
    world.current = "terrain"
    for x in range(-W, W + 1):
        for z in range(-W, W + 1):
            h = terrain_height(x, z)
            if in_pond(x, z):
                depth = 3.2 * math.pow(max(0.0, 1 - math.hypot(x - POND[0], (z - POND[1]) * 1.25)
                                           / (POND_R + 1.2)), 0.7)
                top = js_round(-depth)
                for y in range(-4, top + 1):
                    world.set(x, y, z, rng.pick(C["sand"]) if y >= top - 1 else rng.pick(C["gravel"]))
                for y in range(top + 1, 1):
                    world.set(x, y, z, rng.pick(C["water"]))
                continue
            top = js_round(h)
            if top < -3:
                columns.append((x, z, top))
            for y in range(-3, top + 1):
                if y == top:
                    col = (rng.pick(C["moss"]) if rng.chance(0.14)
                           else (rng.pick(C["grassD"]) if rng.chance(0.2) else rng.pick(C["grass"])))
                elif y >= top - 2:
                    col = rng.pick(C["gravel"])
                else:
                    col = rng.pick(C["stoneWarm"])
                world.set(x, y, z, col)

    # Path from the gate to the plaza
    world.current = "path"
    for z in range(-W, -5):
        cx = js_round(2.2 * math.sin(z * 0.09))
        for x in range(cx - 2, cx + 3):
            if abs(x - cx) == 2 and rng.chance(0.6):
                continue
            y = surf_y(x, z)
            world.set(x, y, z, rng.pick(C["gravel"]) if rng.chance(0.25) else rng.pick(C["stoneWarm"]))
    # Plaza ring
    world.current = "plaza"
    for x in range(-19, 20):
        for z in range(-19, 20):
            d = math.hypot(x, z)
            if d <= 18.5:
                y = surf_y(x, z)
                world.set(x, y, z, rng.pick(C["stone"]) if d > 16 else
                          (rng.pick(C["stoneWarm"]) if max(abs(x), abs(z)) % 2 == 0 else rng.pick(C["gravel"])))

    # The pagoda (five tiers)
    pagoda = Vox(rng)
    pagoda.box(-13, -1, -13, 13, 0, 13, rng.pick(C["stone"]))
    pagoda.box(-12, 1, -12, 12, 1, 12, rng.pick(C["stoneWarm"]))
    half, floor_y = 10, 2
    for t in range(5):
        hgt = 6 if t == 0 else 5
        out["tiers"].append((floor_y, half, hgt))
        wall = rng.pick(C["wood"])
        for fy in range(hgt):
            yy = floor_y + fy
            for x in range(-half, half + 1):
                for z in range(-half, half + 1):
                    if not (abs(x) == half or abs(z) == half):
                        continue
                    corner = abs(x) == half and abs(z) == half
                    # The original window test can never pass on an edge voxel.
                    pagoda.set(x, yy, z, rng.pick(C["woodDark"]) if corner
                               else (wall if fy % 2 == 0 else rng.pick(C["wood"])))
        if t == 0:
            for x in range(-half + 1, half):
                for z in range(-half + 1, half):
                    pagoda.set(x, floor_y - 1, z, rng.pick(C["trunkB"]))
            pagoda.sphere(0, floor_y + 2, 0, 1.6, rng.pick(C["gold"]), 0.4)
            pagoda.box(-1, floor_y, -1, 1, floor_y, 1, rng.pick(C["gold"]))
            pagoda.set(0, floor_y + 4, 0, rng.pick(C["gold"]))
        if t > 0:
            ry = floor_y - 1
            for x in range(-half - 1, half + 2):
                for z in range(-half - 1, half + 2):
                    on_edge = max(abs(x), abs(z)) == half + 1
                    if on_edge and (abs(x) % 2 == 0 or abs(z) % 2 == 0):
                        pagoda.set(x, ry, z, rng.pick(C["red"]))
        over = half + 3
        ry = floor_y + hgt
        for s in range(4):
            w = over - s
            col = rng.pick(C["roofEdge"]) if s == 0 else rng.pick(C["roof"])
            for x in range(-w, w + 1):
                for z in range(-w, w + 1):
                    if max(abs(x), abs(z)) == w or s > 0:
                        pagoda.set(x, ry + s, z, rng.pick(C["roofEdge"]) if s == 3 else col)
            if s == 0:
                for k in (1, 2):
                    for sx in (-1, 1):
                        for sz in (-1, 1):
                            pagoda.set(sx * (over + k), ry + k, sz * (over + k), rng.pick(C["roofEdge"]))
                            pagoda.set(sx * (over + k), ry + k, sz * (over + k - 1), rng.pick(C["roof"]))
                            pagoda.set(sx * (over + k - 1), ry + k, sz * (over + k), rng.pick(C["roof"]))
                            pagoda.set(sx * over, ry + k, sz * over, rng.pick(C["roof"]))
        for sx in (-1, 1):
            for sz in (-1, 1):
                bx, bz = sx * (over + 1), sz * (over + 1)
                for k in (1, 2):
                    pagoda.set(bx, ry - k, bz, rng.pick(C["gold"]))
        for x in range(-half, half + 1, 4):
            for sz in (-1, 1):
                pagoda.set(x, floor_y + hgt - 1, sz * (half + 1), rng.pick(C["red"]))
        for z in range(-half, half + 1, 4):
            for sx in (-1, 1):
                pagoda.set(sx * (half + 1), floor_y + hgt - 1, z, rng.pick(C["red"]))
        floor_y = ry + 4 + 1
        half = max(3, half - 2)
    sy = floor_y
    pagoda.box(-1, sy, -1, 1, sy, 1, rng.pick(C["stoneWarm"]))
    for k in range(1, 10):
        pagoda.set(0, sy + k, 0, rng.pick(C["gold"]))
    for k in range(2, 8, 2):
        pagoda.disc(0, sy + k, 0, 2 if k <= 3 else 1, rng.pick(C["gold"]))
    pagoda.sphere(0, sy + 10, 0, 1.2, rng.pick(C["gold"]), 0.3)
    pagoda.set(0, sy + 12, 0, rng.pick(C["lamp"]))
    world.current = "pagoda"
    for (x, y, z), col in pagoda.map.items():
        world.set(x, y + 1, z, col)

    # Lanterns strung around the plaza
    world.current = "post"
    lanterns = []
    for i in range(12):
        a = i / 12 * math.pi * 2
        px, pz = js_round(math.cos(a) * 16.5), js_round(math.sin(a) * 16.5)
        py = surf_y(px, pz)
        for k in range(1, 5):
            world.set(px, py + k, pz, rng.pick(C["trunk"]))
        world.box(px - 1, py + 5, pz - 1, px + 1, py + 6, pz + 1, rng.pick(C["lantern"]))
        world.set(px, py + 7, pz, rng.pick(C["trunkB"]))
        lanterns.append((px, py + 5, pz))
    out["lanterns"] = lanterns

    # Torii gates at the path entrance
    world.current = "torii"
    for tz in (-W + 4, -W + 12):
        y = surf_y(0, tz)
        col = rng.pick(C["red"])
        world.box(-5, y + 1, tz, -4, y + 7, tz, col)
        world.box(4, y + 1, tz, 5, y + 7, tz, col)
        world.box(-7, y + 8, tz - 1, 7, y + 9, tz + 1, rng.pick(C["woodDark"]))
        world.box(-6, y + 6, tz, 6, y + 7, tz, col)
        world.set(0, y + 10, tz, rng.pick(C["gold"]))

    # Trees
    spots = []

    def plant(species, x, z, scale):
        index = len(out["trees"])
        world.current = f"tree:{index}"
        y0 = surf_y(x, z)
        if species == "cherry":
            trunk = {}
            pal = tree_cherry(rng, world, x, z, scale, y0, (world, "carpet"), trunk)
            out["cherry_trunks"].append(trunk)
        elif species == "pine":
            pal = tree_pine(rng, world, x, z, scale, y0)
        elif species == "maple":
            pal = tree_maple(rng, world, x, z, scale, y0)
        else:
            pal = tree_willow(rng, world, x, z, y0)
        out["trees"].append({"species": species, "x": x, "z": z, "y0": y0,
                             "scale": scale, "palette": pal})

    def place_tree(species, scale_of, min_r, max_r, count, avoid_plaza=True):
        placed = guard = 0
        while placed < count:
            guard += 1
            if guard > 4000:
                break
            a, r = rng.rnd(0, 6.283), rng.rnd(min_r, max_r)
            x, z = js_round(math.cos(a) * r), js_round(math.sin(a) * r)
            if abs(x) > W - 4 or abs(z) > W - 4:
                continue
            if avoid_plaza and math.hypot(x, z) < 21:
                continue
            if in_pond(x, z):
                continue
            if any(math.hypot(tx - x, tz - z) < 7 for tx, tz in spots):
                continue
            spots.append((x, z))
            plant(species, x, z, scale_of())
            placed += 1

    for _ in range(5):
        a, r = rng.rnd(0, 6.283), rng.rnd(22, 50)
        bx, bz = js_round(math.cos(a) * r), js_round(math.sin(a) * r)
        n = rng.ri(2, 4)
        for _ in range(n):
            x, z = bx + rng.ri(-6, 6), bz + rng.ri(-6, 6)
            if (in_pond(x, z) or math.hypot(x, z) < 21
                    or any(math.hypot(tx - x, tz - z) < 6 for tx, tz in spots)):
                continue
            if abs(x) > W - 4 or abs(z) > W - 4:
                continue
            spots.append((x, z))
            plant("cherry", x, z, 1.25 if rng.chance(0.4) else 1)
    place_tree("pine", lambda: 1.2 if rng.chance(0.5) else 0.9, 22, 70, 16)
    place_tree("maple", lambda: 1.2 if rng.chance(0.4) else 0.9, 24, 62, 9)
    place_tree("willow", lambda: 1, 24, 40, 5)
    place_tree("cherry", lambda: 0.9, 30, 70, 6)
    plant("cherry", 15, 30, 1.3)
    plant("willow", 36, 12, 1)
    plant("maple", 38, 28, 1.1)

    # Stone lanterns along the path
    world.current = "stonelantern"
    for z in range(-W + 8, -17, 7):
        x = js_round(2.2 * math.sin(z * 0.09)) + (4 if z % 14 == 0 else -4)
        y = surf_y(x, z)
        world.box(x - 1, y + 1, z - 1, x + 1, y + 1, z, rng.pick(C["stone"]))
        world.box(x, y + 2, z, x, y + 4, z, rng.pick(C["stone"]))
        world.box(x - 1, y + 5, z - 1, x + 1, y + 6, z + 1, rng.pick(C["lamp"]))
        world.box(x - 2, y + 7, z - 2, x + 2, y + 7, z + 2, rng.pick(C["stone"]))
        world.set(x, y + 8, z, rng.pick(C["stoneWarm"]))
    # Bridge
    world.current = "bridge"
    bx, bz = POND[0] - 9, POND[1] + 2
    for i in range(-4, 5):
        yy = 3 + js_round(2.2 * math.cos(i * math.pi / 10))
        for dz in range(-2, 3):
            world.set(bx + i, yy, bz + dz, rng.pick(C["red"]))
        if abs(i) <= 3:
            world.set(bx + i, yy + 1, bz - 2, rng.pick(C["woodDark"]))
            world.set(bx + i, yy + 1, bz + 2, rng.pick(C["woodDark"]))
    # Rocks
    world.current = "rock"
    for _ in range(10):
        a, r = rng.rnd(0, 6.283), rng.rnd(20, 34)
        x, z = js_round(math.cos(a) * r), js_round(math.sin(a) * r)
        if in_pond(x, z):
            continue
        y = surf_y(x, z)
        s = rng.ri(1, 3)
        world.sphere(x, y + s, z, s, rng.pick(C["stone"]), 0.5)
        world.set(x, y + s + 1, z, rng.pick(C["moss"]))
    # Flowers
    world.current = "flower"
    for _ in range(260):
        a, r = rng.rnd(0, 6.283), rng.rnd(19, 72)
        x, z = js_round(math.cos(a) * r), js_round(math.sin(a) * r)
        if in_pond(x, z) or abs(x) > W - 2 or abs(z) > W - 2:
            continue
        if world.has(x, surf_y(x, z) + 1, z):
            continue
        pal = (C["pink2"] if rng.chance(0.3)
               else ([C["gold"][0], C["lantern"][0], C["cloud"][0]] if rng.chance(0.5) else C["white"]))
        world.set(x, surf_y(x, z) + 1, z, rng.pick(pal))
    # Floating lanterns on the pond
    world.current = "floater"
    for _ in range(7):
        a, r = rng.rnd(0, 6.283), rng.rnd(3, POND_R - 3)
        x, z = POND[0] + math.cos(a) * r, POND[1] + math.sin(a) * r / 1.25
        rng.rnd(0, 6.28)  # bobbing phase
        world.box(js_round(x), 1, js_round(z), js_round(x), 2, js_round(z), rng.pick(C["lantern"]))
        world.set(js_round(x), 3, js_round(z), rng.pick(C["lamp"]))
    # Clouds
    clouds = []
    for _ in range(7):
        cloud = Vox(rng)
        n = rng.ri(5, 11)
        cx = cy = cz = 0.0
        for _ in range(n):
            radius = rng.rnd(2.2, 4)
            cloud.sphere(cx, cy, cz, radius, rng.pick(C["cloud"]), 0.6)
            cx += rng.rnd(3, 6) * rng.rnd(-1, 1)
            cy += rng.rnd(-1, 1)
            cz += rng.rnd(3, 6) * rng.rnd(-1, 1)
        clouds.append(cloud.map)
    # Koi
    koi = []
    for _ in range(11):
        a, r = rng.rnd(0, 6.283), rng.rnd(2, POND_R - 3)
        k = {"x": POND[0] + math.cos(a) * r, "z": POND[1] + math.sin(a) * r / 1.25, "a": a}
        k["s"] = rng.rnd(0, 6.28)
        k["sp"] = rng.rnd(0.02, 0.05)
        koi.append(k)
    # Water shimmer
    world.current = "shimmer"
    for _ in range(40):
        a, r = rng.rnd(0, 6.283), rng.rnd(2, POND_R - 1)
        x = js_round(POND[0] + math.cos(a) * r)
        z = js_round(POND[1] + math.sin(a) * r / 1.25)
        if in_pond(x, z):
            world.set(x, 0, z, rng.pick(C["waterHi"]))
    # Cloud group positions, then the 420 initial petals (consumed for the koi
    # colours, which the original draws lazily on the first frame).
    cloud_positions = [(rng.rnd(-60, 60), rng.rnd(46, 60), rng.rnd(-60, 60)) for _ in clouds]
    for _ in range(420):
        for low, high in ((0, 6.283), (5, 55), (28, 48), (0, 6.283), (5, 55), (-0.6, 1.1),
                          (-0.18, -0.06), (-0.5, 0.5), (0, 6.28), (1, 3)):
            rng.rnd(low, high)
        rng.ri(0, 2)
    for k in koi:
        k["color"] = rng.ri(0, 2)

    # Fixes, from an independent stream so the layout above stays identical.
    if fixes:
        world.current = "pagoda"
        for floor, half_size, _ in out["tiers"]:
            for fy in (2, 3):
                for u in range(-half_size + 1, half_size):
                    if abs(u) % 4 != 2:
                        continue
                    for x, z in ((u, half_size), (u, -half_size), (half_size, u), (-half_size, u)):
                        world.set(x, floor + fy + 1, z, fix.pick(C["paper"]))
        # Valleys below the original's y=-3 floor left empty columns; give
        # them the floor's grass so the garden has no holes.
        world.current = "terrain"
        for x, z, _ in columns:
            world.set(x, -3, z, fix.pick(C["moss"]) if fix.chance(0.14) else fix.pick(C["grass"]))
    return world, out, clouds, cloud_positions, koi


# --------------------------------------------------------------------------
# Tree variants
# --------------------------------------------------------------------------
def variant_key(tree):
    scale = f"{tree['scale']:g}".replace(".", "p")
    if tree["species"] in ("pine", "willow"):
        return f"{tree['species']}-{scale}"
    return f"{tree['species']}-{scale}-{tree['palette'].lower()}"


def fnv(text):
    value = 0x811C9DC5
    for byte in text.encode():
        value = ((value ^ byte) * 0x01000193) & 0xFFFFFFFF
    return value


def variant_shape(tree, fixes):
    """A tree of the same species, scale and canopy palette, generated at the
    origin from its own stream. The trunk column is voxel (0, *, 0)."""
    key = variant_key(tree)
    rng = Lcg(fnv(key))
    out = Vox(rng)
    species = tree["species"]
    if species == "cherry":
        # Regenerate until the main canopy palette matches the original tree.
        for _ in range(64):
            trial_rng = Lcg(rng.seed)
            trial = Vox(trial_rng)
            trunk = {}
            pal = tree_cherry(trial_rng, trial, 0, 0, tree["scale"], 0, None, trunk)
            rng.rand()
            if pal == tree["palette"]:
                out = trial
                if fixes:
                    for position, color in trunk.items():
                        out.map[position] = color
                break
    elif species == "pine":
        tree_pine(rng, out, 0, 0, tree["scale"], 0)
    elif species == "maple":
        for _ in range(64):
            trial_rng = Lcg(rng.seed)
            trial = Vox(trial_rng)
            pal = tree_maple(trial_rng, trial, 0, 0, tree["scale"], 0)
            rng.rand()
            if pal == tree["palette"]:
                out = trial
                break
    else:
        tree_willow(rng, out, 0, 0, 0)
    assert out.map, f"no {key} variant matched its palette"
    return key, out.map


# --------------------------------------------------------------------------
# Meshing and glTF
# --------------------------------------------------------------------------
# Each face: neighbour offset, outward normal and four corners, CCW from outside.
FACES = [
    ((1, 0, 0), [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)]),
    ((-1, 0, 0), [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)]),
    ((0, 1, 0), [(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)]),
    ((0, -1, 0), [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)]),
    ((0, 0, 1), [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)]),
    ((0, 0, -1), [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)]),
]


def _check_winding():
    for normal, corners in FACES:
        a, b, c = corners[0], corners[1], corners[2]
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        assert sum(cross[i] * normal[i] for i in range(3)) > 0, normal


_check_winding()


def mesh_by_color(voxels, solid, offset=(0.0, 0.0, 0.0), skip_down=None):
    """Exposed faces of `voxels` against `solid`, grouped by colour (sorted)."""
    groups = {}
    for (x, y, z), color in voxels.items():
        for normal, corners in FACES:
            neighbour = (x + normal[0], y + normal[1], z + normal[2])
            if neighbour in solid:
                continue
            if normal[1] == -1 and skip_down is not None and skip_down((x, y, z)):
                continue
            positions, normals = groups.setdefault(color, ([], []))
            for corner in corners:
                positions.append((x + corner[0] + offset[0], y + corner[1] + offset[1],
                                  z + corner[2] + offset[2]))
                normals.append(normal)
    return dict(sorted(groups.items()))


class Gltf:
    """One mesh, one node; a primitive per colour; buffer external or embedded."""

    def __init__(self, name):
        self.name = name
        self.buffer = bytearray()
        self.views = []
        self.accessors = []
        self.materials = []
        self.primitives = []
        self.vertices = 0
        self.indices = 0

    def _view(self, data, target):
        while len(self.buffer) % 4:
            self.buffer.append(0)
        self.views.append({"buffer": 0, "byteOffset": len(self.buffer), "byteLength": len(data),
                           "target": target})
        self.buffer.extend(data)
        return len(self.views) - 1

    def add(self, color, positions, normals, alpha=None):
        count = len(positions)
        flat = [c for p in positions for c in p]
        position_view = self._view(struct.pack(f"<{len(flat)}f", *flat), 34962)
        flat_normals = [float(c) for n in normals for c in n]
        normal_view = self._view(struct.pack(f"<{len(flat_normals)}f", *flat_normals), 34962)
        indices = []
        for quad in range(count // 4):
            base = quad * 4
            indices.extend((base, base + 1, base + 2, base, base + 2, base + 3))
        wide = count > 65535
        index_view = self._view(struct.pack(f"<{len(indices)}{'I' if wide else 'H'}", *indices), 34963)
        lo = [min(p[i] for p in positions) for i in range(3)]
        hi = [max(p[i] for p in positions) for i in range(3)]
        self.accessors.extend([
            {"bufferView": position_view, "componentType": 5126, "count": count, "type": "VEC3",
             "min": lo, "max": hi},
            {"bufferView": normal_view, "componentType": 5126, "count": count, "type": "VEC3"},
            {"bufferView": index_view, "componentType": 5125 if wide else 5123,
             "count": len(indices), "type": "SCALAR"},
        ])
        material = {"name": f"c-{hex_name(color)}",
                    "pbrMetallicRoughness": {"baseColorFactor": [*linear(color), alpha or 1.0],
                                             "metallicFactor": 0.0, "roughnessFactor": 1.0}}
        if color in EMISSIVE:
            material["emissiveFactor"] = linear(color)
            material["extensions"] = {"KHR_materials_emissive_strength":
                                      {"emissiveStrength": EMISSIVE_STRENGTH}}
        if alpha is not None:
            material["alphaMode"] = "BLEND"
        self.materials.append(material)
        base = len(self.accessors) - 3
        self.primitives.append({"attributes": {"POSITION": base, "NORMAL": base + 1},
                                "indices": base + 2, "material": len(self.materials) - 1})
        self.vertices += count
        self.indices += len(indices)

    def document(self, buffer_uri):
        doc = {
            "asset": {"version": "2.0", "generator": "Bozzard tools/gen_pagoda.py"},
            "scene": 0, "scenes": [{"nodes": [0]}],
            "nodes": [{"name": self.name, "mesh": 0}],
            "meshes": [{"name": self.name, "primitives": self.primitives}],
            "materials": self.materials,
            "buffers": [{"uri": buffer_uri, "byteLength": len(self.buffer)}],
            "bufferViews": self.views, "accessors": self.accessors,
        }
        if any("extensions" in m for m in self.materials):
            doc["extensionsUsed"] = ["KHR_materials_emissive_strength"]
        return doc


LIMITS = {"vertices": 1_000_000, "indices": 3_000_000, "parts": 4096, "bytes": 32 * 1024 * 1024}


def check_limits(name, gltf, size):
    assert gltf.vertices <= LIMITS["vertices"], (name, gltf.vertices)
    assert gltf.indices <= LIMITS["indices"], (name, gltf.indices)
    assert len(gltf.primitives) < LIMITS["parts"], (name, len(gltf.primitives))
    assert size <= LIMITS["bytes"], (name, size)


# --------------------------------------------------------------------------
# Output
# --------------------------------------------------------------------------
def tf(translation, rotation=(0, 0, 0), scale=(1, 1, 1)):
    return {"translation": [round(v, 4) for v in translation],
            "rotation_degrees": [round(v, 4) for v in rotation],
            "scale": [round(v, 4) for v in scale]}


def drawable(mesh, color=(1, 1, 1), extra=None):
    value = {"layer": "3d", "mesh": mesh, "texture": "white", "color": list(color), "uv_scale": [1, 1]}
    value.update(extra or {})
    return value


def morton(x, z):
    def spread(v):
        v &= 0xFFFF
        v = (v | (v << 8)) & 0x00FF00FF
        v = (v | (v << 4)) & 0x0F0F0F0F
        v = (v | (v << 2)) & 0x33333333
        return (v | (v << 1)) & 0x55555555
    return spread(x) | (spread(z) << 1)


# The original draws a flat 0x9ec9f0 background and lights surfaces with a sky
# (0xbfe3ff) / ground (0x50613a) hemisphere. Bozzard's procedural sky does both:
# its below-horizon band stays the background blue (the default camera looks
# down on the garden) and the zenith carries the hemisphere's sky colour.
SKY_DAY = {"zenith": linear(0xBFE3FF), "horizon": linear(0x9EC9F0), "ground": linear(0x9EC9F0)}
SKY_NIGHT = {"zenith": linear(0x0B1430), "horizon": linear(0x121A33), "ground": linear(0x121A33)}


def garden_script(lights):
    names = ", ".join(f'"{name}"' for name, _ in lights["lanterns"])
    d, n = SKY_DAY, SKY_NIGHT
    return f"""// Generated by tools/gen_pagoda.py. Space or N toggles night; lanterns flicker.

fn apply_daylight(night) {{
    if night {{
        set_sun_light({linear(0x9FB4FF)}, 0.18);
        set_ambient_light({linear(0x6F86C8)}, 0.03);
        set_environment({n['zenith']}, {n['horizon']}, {n['ground']}, 0.6);
        set_star_intensity(1.5);
        set_fog({n['horizon']}, 0.008);
        set_bloom_intensity(0.5);
    }} else {{
        set_sun_light({linear(0xFFF2D8)}, 1.35);
        set_ambient_light([1.0, 1.0, 1.0], 0.057);
        set_environment({d['zenith']}, {d['horizon']}, {d['ground']}, 1.0);
        set_star_intensity(0.0);
        set_fog({d['horizon']}, 0.008);
        set_bloom_intensity(0.12);
    }}
}}

fn on_update(me, dt) {{
    let night = get_object_variable("night");
    if input_pressed("N") || input_pressed("Space") {{
        night = !night;
        set_object_variable("night", night);
        apply_daylight(night);
    }}
    let t = elapsed_time();
    let lantern = if night {{ {lights['lantern_night']} }} else {{ {lights['lantern_day']} }};
    let i = 0.0;
    for name in [{names}] {{
        set_light_intensity(name, lantern * (1.0 + 0.18 * sin(t * 3.0 + i) + random(0.0, 0.08)));
        i += 1.0;
    }}
    let pond = if night {{ {lights['pond_night']} }} else {{ {lights['pond_day']} }};
    set_light_intensity("ac-light-pond", pond * (1.0 + 0.12 * sin(t * 1.4)));
}}
"""


KOI_SCRIPT = f"""// Generated by tools/gen_pagoda.py: the original koi swim loop, per second.

fn on_update(me, dt) {{
    let t = elapsed_time();
    let s = get_object_variable("s");
    let sp = get_object_variable("sp") * 60.0;
    let a = get_object_variable("a") + sin(t * 0.9 + s) * 2.1 * dt;
    let p = get_position(me);
    let x = p[0] + cos(a) * sp * dt;
    let z = p[2] + sin(a) * sp * 0.8 * dt;
    let dx = x - {POND[0]}.0;
    let dz = (z - {POND[1]}.0) * 1.25;
    if sqrt(dx * dx + dz * dz) > {POND_R - 2.5} {{
        a = atan2(-(z - {POND[1]}.0), -(x - {POND[0]}.0));
    }}
    set_object_variable("a", a);
    set_position(me, [x, 1.08 + sin(t * 2.0 + s) * 0.06, z]);
    set_rotation(me, [0.0, -to_degrees(a), to_degrees(sin(t * 4.0 + s) * 0.25)]);
}}
"""

CLOUD_SCRIPT = """// Generated by tools/gen_pagoda.py: clouds drift east and wrap around.

fn on_update(me, dt) {
    let p = get_position(me);
    let x = p[0] + get_object_variable("speed") * dt;
    if x > 120.0 {
        x = -120.0;
    }
    let bob = sin(elapsed_time() * 0.2 + get_object_variable("phase")) * 0.24 * dt;
    set_position(me, [x, p[1] + bob, p[2]]);
}
"""


def build(fixes):
    world, out, clouds, cloud_positions, koi = generate(fixes)
    files = {}
    assets = {}
    objects = []

    def emit_json(path, value):
        files[path] = (json.dumps(value, indent=1) + "\n").encode()

    # Partition the final voxel state.
    garden, pagoda, cubes = {}, {}, {}
    for position, color in world.map.items():
        category = world.category[position]
        if category.startswith("tree:"):
            shadowed = world.under_trees.get(position)
            if shadowed is None:
                continue
            color, category = shadowed
        if category == "pagoda":
            pagoda[position] = color
        elif category in ("flower", "carpet"):
            cubes[position] = color
        else:
            garden[position] = color
    solid = set(garden) | set(pagoda) | set(cubes)

    def terrain_floor(position):
        return world.category.get(position) in ("terrain", "path", "plaza")

    for name, voxels in (("garden", garden), ("pagoda", pagoda)):
        gltf = Gltf(name)
        for color, (positions, normals) in mesh_by_color(voxels, solid, skip_down=terrain_floor).items():
            gltf.add(color, positions, normals)
        binary = bytes(gltf.buffer)
        files[f"scenes/assets/{name}.bin"] = binary
        check_limits(name, gltf, len(binary))
        emit_json(f"scenes/assets/{name}.gltf", gltf.document(f"{name}.bin"))
        assets[name] = {"kind": "mesh", "path": f"assets/{name}.gltf"}

    # Tree variants: one single-primitive glTF per (variant, colour).
    variants = {}
    for tree in out["trees"]:
        key = variant_key(tree)
        if key not in variants:
            variants[key] = variant_shape(tree, fixes)[1]
    variant_assets = {}
    for key, voxels in sorted(variants.items()):
        groups = mesh_by_color(voxels, set(voxels), offset=(-0.5, 0.0, -0.5))
        variant_assets[key] = []
        for color, (positions, normals) in groups.items():
            asset = f"tree-{key}-{hex_name(color)}"
            gltf = Gltf(asset)
            gltf.add(color, positions, normals)
            uri = "data:application/octet-stream;base64," + base64.b64encode(bytes(gltf.buffer)).decode()
            check_limits(asset, gltf, len(gltf.buffer))
            emit_json(f"scenes/assets/trees/{asset}.gltf", gltf.document(uri))
            assets[asset] = {"kind": "mesh", "path": f"assets/trees/{asset}.gltf"}
            variant_assets[key].append(asset)

    # Emissive flower cubes and clouds.
    for color, name in ((C["gold"][0], "flower-gold"), (C["lantern"][0], "flower-lantern")):
        gltf = Gltf(name)
        positions, normals = mesh_by_color({(0, 0, 0): color}, set(), offset=(-0.5, -0.5, -0.5))[color]
        gltf.add(color, positions, normals)
        uri = "data:application/octet-stream;base64," + base64.b64encode(bytes(gltf.buffer)).decode()
        emit_json(f"scenes/assets/{name}.gltf", gltf.document(uri))
        assets[name] = {"kind": "mesh", "path": f"assets/{name}.gltf"}
    for index, voxels in enumerate(clouds):
        name = f"cloud-{index}"
        gltf = Gltf(name)
        for color, (positions, normals) in mesh_by_color(voxels, set(voxels)).items():
            gltf.add(color, positions, normals, alpha=0.92)
        uri = "data:application/octet-stream;base64," + base64.b64encode(bytes(gltf.buffer)).decode()
        emit_json(f"scenes/assets/{name}.gltf", gltf.document(uri))
        assets[name] = {"kind": "mesh", "path": f"assets/{name}.gltf"}

    for name, source in (("garden-script", garden_script), ("koi-script", KOI_SCRIPT),
                         ("cloud-script", CLOUD_SCRIPT)):
        assets[name] = {"kind": "script", "path": f"scripts/{name.removesuffix('-script')}.rhai"}

    # Objects; render order follows object IDs, so IDs group batch keys.
    theta, phi, radius = -0.6, 0.95, 118.0
    objects.append({"id": "aa-camera-rig", "name": "Camera rig / slow orbit in Play",
                    "transform": tf((0, 16, 0), (0, math.degrees(theta), 0)),
                    "spin": [0, round(math.degrees(0.0012 * 60), 4), 0]})
    pitch = -math.degrees(math.atan2(radius * math.cos(phi), radius * math.sin(phi)))
    objects.append({"id": "aa-camera", "name": "Camera", "parent": "aa-camera-rig",
                    "transform": tf((0, radius * math.cos(phi), radius * math.sin(phi)), (pitch, 0, 0)),
                    "camera": {"projection": "perspective", "vertical_fov_degrees": 52,
                               "near": 0.5, "far": 600}})
    objects.append({"id": "ab-controller", "name": "Garden controller / Space or N toggles night",
                    "transform": tf((0, 0, 0)),
                    "blackboard": {"night": {"scalar": {"bool": False}}},
                    "script_manager": {"scripts": [{"enabled": True, "script": "garden-script"}]}})
    # The original's day/night ratios (1.2 / 2.6 lanterns, 0.9 / 1.8 pond) at a
    # brightness that reads as warm pools in Bozzard's local light units.
    lights = {"lanterns": [], "lantern_day": 6.0, "lantern_night": 30.0, "pond_day": 4.5,
              "pond_night": 20.0}
    for i, (x, y, z) in enumerate(out["lanterns"]):
        if i % 3:
            continue
        name = f"ac-light-lantern-{i // 3}"
        lights["lanterns"].append((name, (x, y, z)))
        objects.append({"id": name, "name": f"Lantern light {i // 3 + 1}",
                        "transform": tf((x + 0.5, y + 1, z + 0.5)),
                        "light": {"kind": "point", "color": linear(0xFF9A6A),
                                  "intensity": lights["lantern_day"], "range": 34}})
    objects.append({"id": "ac-light-pond", "name": "Pond light",
                    "transform": tf((POND[0], 4, POND[1])),
                    "light": {"kind": "point", "color": linear(0xFFCF8A),
                              "intensity": lights["pond_day"], "range": 30}})
    for i, (x, z) in enumerate(((-25, -25), (25, -25), (-25, 25), (25, 25))):
        objects.append({"id": f"ad-petals-{i}", "name": f"Falling petals {i + 1}",
                        "transform": tf((x, 26, z)),
                        "particle_emitter": {"enabled": True, "kind": "ash", "rate": 22, "lifetime": 14,
                                             "radius": 20, "speed": 0.2, "spread": 1.0,
                                             "start_size": 0.3, "end_size": 0.22,
                                             "color": linear(0xFFC2CF), "opacity": 0.9,
                                             "wind": [0.5, 0, 0.12], "turbulence": 1.2,
                                             "gravity": -1.0, "drag": 0.5, "softness": 0.15,
                                             "trail_length": 0, "max_particles": 512, "seed": 7 + i}})
    objects.append({"id": "b-garden", "name": "Garden / terrain, plaza, path, props",
                    "transform": tf((0, 0, 0)), "drawable": drawable({"asset": "garden"})})
    objects.append({"id": "c-pagoda", "name": "Pagoda / five tiers",
                    "transform": tf((0, 0, 0)), "drawable": drawable({"asset": "pagoda"})})
    # Trees: a root per original tree, children grouped by asset for batching.
    children = []
    for index, tree in enumerate(out["trees"]):
        root = f"t-{index:03d}"
        turn = fnv(f"{index}") % 4 * 90
        objects.append({"id": root, "name": f"Tree {index + 1} / {tree['species']}",
                        "transform": tf((tree["x"] + 0.5, tree["y0"], tree["z"] + 0.5), (0, turn, 0))})
        for asset in variant_assets[variant_key(tree)]:
            children.append((asset, index, root))
    for asset, index, root in sorted(children):
        objects.append({"id": f"d-{asset}-{index:03d}", "name": asset, "parent": root,
                        "transform": tf((0, 0, 0)), "drawable": drawable({"asset": asset})})
    # Flowers and petal carpets in spatial order: emissive assets, then tinted cubes.
    glowing = {C["gold"][0]: "flower-gold", C["lantern"][0]: "flower-lantern"}
    flowers = sorted(cubes.items(), key=lambda item: (morton(item[0][0] + 128, item[0][2] + 128), item[0]))
    for (x, y, z), color in flowers:
        code = morton(x + 128, z + 128)
        if color in glowing:
            objects.append({"id": f"e-{glowing[color]}-{code:05d}-{y + 8:02d}", "name": "Glowing flower",
                            "transform": tf((x + 0.5, y + 0.5, z + 0.5)),
                            "drawable": drawable({"asset": glowing[color]})})
        else:
            objects.append({"id": f"f-cube-a-{code:05d}-{y + 8:02d}", "name": "Flower",
                            "transform": tf((x + 0.5, y + 0.5, z + 0.5)),
                            "drawable": drawable("cube", linear(color), {"roughness": 1.0})})
    for i, k in enumerate(koi):
        objects.append({"id": f"f-cube-b-koi-{i:02d}", "name": f"Koi {i + 1}",
                        "transform": tf((k["x"], 1.08, k["z"]), (0, -math.degrees(k["a"]), 0),
                                        (1.1, 0.16, 0.42)),
                        "drawable": drawable("cube", linear(KOI_COLORS[k["color"]]), {"roughness": 0.6,
                                                                                    "gi_static": False}),
                        "blackboard": {"a": {"scalar": {"number": round(k["a"], 6)}},
                                       "s": {"scalar": {"number": round(k["s"], 6)}},
                                       "sp": {"scalar": {"number": round(k["sp"], 6)}}},
                        "script_manager": {"scripts": [{"enabled": True, "script": "koi-script"}]}})
    for i, (x, y, z) in enumerate(cloud_positions):
        objects.append({"id": f"g-cloud-{i}", "name": f"Cloud {i + 1}",
                        "transform": tf((x, y, z)),
                        "drawable": drawable({"asset": f"cloud-{i}"}, extra={"gi_static": False}),
                        "blackboard": {"speed": {"scalar": {"number": round(0.72 + 0.12 * i, 4)}},
                                       "phase": {"scalar": {"number": float(i)}}},
                        "script_manager": {"scripts": [{"enabled": True, "script": "cloud-script"}]}})

    sun = [60, 90, -40]
    length = math.sqrt(sum(v * v for v in sun))
    scene = {
        "version": 1,
        "name": "Pagoda Garden / progada",
        "views": {"3d": "aa-camera"},
        "assets": dict(sorted(assets.items())),
        "lighting": {"shadows": True, "shadow_resolution": 2048, "shadow_bias": 0.005,
                     "shadow_normal_bias": 0.02,
                     "sun_direction": [round(v / length, 5) for v in sun],
                     "sun_color": linear(0xFFF2D8), "sun_intensity": 1.35,
                     "ambient_color": [1, 1, 1], "ambient_intensity": 0.057},
        "environment": {**SKY_DAY, "intensity": 1.0, "star_intensity": 0, "background": True},
        "fog": {"enabled": True, "color": SKY_DAY["horizon"], "distance_density": 0.008,
                "start_distance": 130, "height_density": 0, "base_height": 0, "height_falloff": 1},
        "display": {"exposure_ev": 0, "tone_mapping": False,
                    "bloom": {"enabled": True, "intensity": 0.12, "threshold": 1.0, "scatter": 0.7,
                              "anamorphic": 0},
                    "temporal_aa": {"enabled": True}},
        "objects": objects,
    }
    emit_json("scenes/pagoda.json", scene)
    files["scenes/scripts/garden.rhai"] = garden_script(lights).encode()
    files["scenes/scripts/koi.rhai"] = KOI_SCRIPT.encode()
    files["scenes/scripts/cloud.rhai"] = CLOUD_SCRIPT.encode()
    emit_json("bozzard.project.json", {"version": 1, "name": "Pagoda Garden",
                                       "start_scene": "scenes/pagoda.json", "view": "3d"})
    stats = {
        "world_voxels": len(world.map),
        "garden_voxels": len(garden),
        "pagoda_voxels": len(pagoda),
        "flower_cubes": len(cubes),
        "trees": len(out["trees"]),
        "tree_variants": len(variants),
        "tree_assets": sum(len(v) for v in variant_assets.values()),
        "objects": len(objects),
        "koi_colors": [k["color"] for k in koi],
    }
    return files, stats


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--check", action="store_true", help="verify files instead of writing")
    parser.add_argument("--faithful", action="store_true",
                        help="reproduce the original geometry, including its three bugs")
    args = parser.parse_args(argv)
    files, stats = build(fixes=not args.faithful)
    out = args.out.resolve()
    stale = []
    for path, data in sorted(files.items()):
        target = out / path
        if args.check:
            if not target.exists() or target.read_bytes() != data:
                stale.append(path)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.exists() or target.read_bytes() != data:
            target.write_bytes(data)
    if not args.check:
        tree_dir = out / "scenes" / "assets" / "trees"
        for old in tree_dir.glob("*.gltf"):
            if f"scenes/assets/trees/{old.name}" not in files:
                old.unlink()
    print(json.dumps(stats))
    if stale:
        print("stale: " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
