#!/usr/bin/env python3
"""Generate examples/sand-island: a first-person voxel castaway game for Bozzard.

The player is stranded on a small sand island with one palm tree in an endless ocean.
Far out to sea a steamship sails away under voxel smoke, and far to the south-west a
lighthouse stands on a rocky islet. Gather driftwood, palm fronds and coconuts, craft
rope, a paddle and a sail, build a raft in five stages, and sail it to the lighthouse.
A day/night cycle runs at eight game seconds per real second.

Everything you can see is voxels except the ocean, which is a shaded plane:

* the island and its seafloor -> island.gltf, a greedy-meshed heightfield of 0.25 m
  voxels with one primitive per colour; the lighthouse islet is meshed the same way;
* the palm, driftwood, coconuts, the raft's five stages and their translucent
  previews, the steamship, the lighthouse and its keeper's cottage -> voxel glTFs;
* smoke puffs, clouds, the sun, the moon, the lighthouse lamp and beam -> small
  translucent voxel glTFs that cast no shadows.

This script writes the meshes, the inventory icons, the scene with its HUD and menus,
and scripts/island.rhai: the island's shape, the game's layout (raft site, launch
point, beach spawn spots, lighthouse) and its recipe tables, shared by the scripts.
The gameplay scripts beside it (player, game, hud, world, ...) are written by hand.

Standard library only; run from anywhere. --check verifies the committed output
instead of writing it.
"""
import argparse
import base64
import json
import math
from pathlib import Path
import struct
import sys
import zlib

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = ROOT / "examples" / "sand-island"
VOXEL = 0.25
TIME_SCALE = 8.0
START_HOUR = 8.0
SKY_INTENSITY = 0.55
SUN_INTENSITY = 2.4
TONE_MAPPER = "filmic"


def linear(hex_color):
    def channel(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return [round(channel((hex_color >> shift) & 255), 5) for shift in (16, 8, 0)]


def clamp(x, lo, hi):
    return lo if x < lo else hi if x > hi else x


def smoothstep(e0, e1, x):
    t = clamp((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def hash01(*values):
    """Deterministic 0..1 hash of integers."""
    h = 0x811C9DC5
    for v in values:
        h = ((h ^ (v & 0xFFFFFFFF)) * 0x01000193) & 0xFFFFFFFF
        h ^= h >> 15
        h = (h * 0x2C1B3C6D) & 0xFFFFFFFF
        h ^= h >> 12
    return (h & 0xFFFFFF) / 0x1000000


def value_noise(x, z, seed):
    xi, zi = math.floor(x), math.floor(z)
    fx, fz = x - xi, z - zi
    fx, fz = fx * fx * (3 - 2 * fx), fz * fz * (3 - 2 * fz)
    a, b = hash01(xi, zi, seed), hash01(xi + 1, zi, seed)
    c, d = hash01(xi, zi + 1, seed), hash01(xi + 1, zi + 1, seed)
    return (a + (b - a) * fx) + ((c + (d - c) * fx) - (a + (b - a) * fx)) * fz


class Lcg:
    def __init__(self, seed):
        self.seed = seed & 0xFFFFFFFF

    def rand(self):
        self.seed = (self.seed * 1664525 + 1013904223) & 0xFFFFFFFF
        return self.seed / 4294967296

    def rnd(self, a, b):
        return a + self.rand() * (b - a)

    def pick(self, items):
        return items[min(len(items) - 1, int(self.rand() * len(items)))]


# --------------------------------------------------------------------------
# The island's shape. scripts/island.rhai evaluates the same functions, so the
# player walks on the surface the voxels were cut from.
# --------------------------------------------------------------------------
COAST = 16.5
PEAK = 2.3
PALM = (3.0, -2.0)
SPAWN = (-5.5, 7.0)
SWIM_DEPTH = 1.25
BOUNDARY = 33.0
GRID_RADIUS = 37.0


def coast_radius(x, z):
    a = math.atan2(z, x)
    return COAST + 2.2 * math.sin(2 * a + 0.7) + 1.4 * math.sin(3 * a + 2.1) + 0.7 * math.sin(5 * a + 0.3)


def ground(x, z):
    s = math.hypot(x, z) / coast_radius(x, z)
    if s < 1:
        dunes = 0.32 * math.sin(x * 0.45 + 0.3) * math.cos(z * 0.38 - 0.8) + 0.16 * math.sin((x + z) * 0.9)
        return PEAK * (1 - s * s) + dunes * (1 - s)
    return -1.6 * (1 - math.exp(-(s - 1) * 4)) - 3.5 * smoothstep(1.35, 1.9, s)


# The ship starts north-east of the island and steams away at an angle, so its
# profile shows while the distance keeps growing.
SHIP_BEARING = (0.6, -0.8)
SHIP_START = 1350.0
SHIP_SPEED = 2.6
SHIP_TURN = math.radians(40)
SHIP_HEADING = (SHIP_BEARING[0] * math.cos(SHIP_TURN) - SHIP_BEARING[1] * math.sin(SHIP_TURN),
                SHIP_BEARING[0] * math.sin(SHIP_TURN) + SHIP_BEARING[1] * math.cos(SHIP_TURN))
SHIP_YAW = math.degrees(math.atan2(-SHIP_HEADING[0], -SHIP_HEADING[1]))
FUNNEL_TOP = (0.0, 25.0, 13.0)  # ship-local, bow toward -Z


# --------------------------------------------------------------------------
# Meshing
# --------------------------------------------------------------------------
def quad(corners, normal):
    """Order four corners counter-clockwise when seen from the side `normal` points to."""
    a, b, c = corners[0], corners[1], corners[2]
    u = [b[i] - a[i] for i in range(3)]
    v = [c[i] - a[i] for i in range(3)]
    cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
    if sum(cross[i] * normal[i] for i in range(3)) < 0:
        corners = [corners[0], corners[3], corners[2], corners[1]]
    return corners


class Mesh:
    """Quads grouped by colour; positions in metres."""

    def __init__(self):
        self.groups = {}

    def add(self, color, corners, normal):
        positions, normals = self.groups.setdefault(color, ([], []))
        for corner in quad(corners, normal):
            positions.append(corner)
            normals.append(normal)

    def quads(self):
        return sum(len(p) // 4 for p, _ in self.groups.values())


def greedy_rectangles(cells):
    """Merge a {(u, v): key} mask into maximal rectangles (u0, v0, du, dv, key)."""
    done = set()
    for (u, v) in sorted(cells):
        if (u, v) in done:
            continue
        key = cells[(u, v)]
        du = 1
        while (u + du, v) in cells and (u + du, v) not in done and cells[(u + du, v)] == key:
            du += 1
        dv = 1
        while all((u + i, v + dv) in cells and (u + i, v + dv) not in done
                  and cells[(u + i, v + dv)] == key for i in range(du)):
            dv += 1
        for i in range(du):
            for j in range(dv):
                done.add((u + i, v + j))
        yield u, v, du, dv, key


AXES = [(0, 1, 2), (1, 2, 0), (2, 0, 1)]


def mesh_voxels(voxels, size, offset=(0.0, 0.0, 0.0), mesh=None, inflate=0.0):
    """Greedy-mesh exposed faces of {(x, y, z): colour}; a voxel spans `size` metres.
    `inflate` pushes each face out along its normal, so an overlay (lit windows,
    lamps) covers the voxels it decorates without fighting them for depth."""
    mesh = mesh or Mesh()
    for axis, ua, va in AXES:
        for sign in (1, -1):
            planes = {}
            for p, color in voxels.items():
                n = list(p)
                n[axis] += sign
                if tuple(n) in voxels:
                    continue
                planes.setdefault(p[axis], {})[(p[ua], p[va])] = color
            normal = [0, 0, 0]
            normal[axis] = sign
            for layer, cells in planes.items():
                level = layer + (1 if sign > 0 else 0)
                for u, v, du, dv, color in greedy_rectangles(cells):
                    corners = []
                    for cu, cv in ((u, v), (u + du, v), (u + du, v + dv), (u, v + dv)):
                        c = [0.0, 0.0, 0.0]
                        c[axis], c[ua], c[va] = level, cu, cv
                        corners.append(tuple(c[i] * size + offset[i] + normal[i] * inflate for i in range(3)))
                    mesh.add(color, corners, tuple(normal))
    return mesh


# --------------------------------------------------------------------------
# glTF output
# --------------------------------------------------------------------------
class Gltf:
    """One mesh, one node; a primitive per colour."""

    def __init__(self, name):
        self.name = name
        self.buffer = bytearray()
        self.views, self.accessors, self.materials, self.primitives = [], [], [], []
        self.vertices = 0

    def _view(self, data, target):
        while len(self.buffer) % 4:
            self.buffer.append(0)
        self.views.append({"buffer": 0, "byteOffset": len(self.buffer), "byteLength": len(data), "target": target})
        self.buffer.extend(data)
        return len(self.views) - 1

    def add_mesh(self, mesh, alpha=None, emissive=None):
        for color in sorted(mesh.groups):
            positions, normals = mesh.groups[color]
            self.add(color, positions, normals, alpha, (emissive or {}).get(color))

    def add(self, color, positions, normals, alpha=None, emissive=None):
        count = len(positions)
        flat = [round(c, 5) for p in positions for c in p]
        position_view = self._view(struct.pack(f"<{len(flat)}f", *flat), 34962)
        flat_normals = [float(c) for n in normals for c in n]
        normal_view = self._view(struct.pack(f"<{len(flat_normals)}f", *flat_normals), 34962)
        indices = []
        for base in range(0, count, 4):
            indices.extend((base, base + 1, base + 2, base, base + 2, base + 3))
        wide = count > 65535
        index_view = self._view(struct.pack(f"<{len(indices)}{'I' if wide else 'H'}", *indices), 34963)
        lo = [min(p[i] for p in positions) for i in range(3)]
        hi = [max(p[i] for p in positions) for i in range(3)]
        self.accessors.extend([
            {"bufferView": position_view, "componentType": 5126, "count": count, "type": "VEC3",
             "min": lo, "max": hi},
            {"bufferView": normal_view, "componentType": 5126, "count": count, "type": "VEC3"},
            {"bufferView": index_view, "componentType": 5125 if wide else 5123, "count": len(indices),
             "type": "SCALAR"},
        ])
        material = {"name": f"c-{color:06x}",
                    "pbrMetallicRoughness": {"baseColorFactor": [*linear(color), alpha or 1.0],
                                             "metallicFactor": 0.0, "roughnessFactor": 0.95}}
        if emissive:
            material["emissiveFactor"] = linear(color)
            material["extensions"] = {"KHR_materials_emissive_strength": {"emissiveStrength": emissive}}
        if alpha is not None:
            material["alphaMode"] = "BLEND"
        self.materials.append(material)
        base = len(self.accessors) - 3
        self.primitives.append({"attributes": {"POSITION": base, "NORMAL": base + 1},
                                "indices": base + 2, "material": len(self.materials) - 1})
        self.vertices += count

    def document(self, buffer_uri):
        doc = {
            "asset": {"version": "2.0", "generator": "Bozzard tools/gen_island.py"},
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


def png_rgba(width, height, pixel):
    rows = b"".join(b"\x00" + bytes(c for x in range(width) for c in pixel(x, y)) for y in range(height))

    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


# --------------------------------------------------------------------------
# The island: a heightfield of voxel columns, greedy-meshed
# --------------------------------------------------------------------------
SAND_DRY = [0xe4cc98, 0xdcc289, 0xe9d4a6, 0xd5b981]
SAND_DAMP = [0xc6aa74, 0xbb9f69]
SHELLS = [0xfbf7ee, 0xf3c9c0, 0xf7ead2]


def sand_color(x, z, h, i, k):
    if h > 0.42:
        if hash01(i, k, 7) < 0.012:
            return SHELLS[int(hash01(i, k, 8) * len(SHELLS))]
        ripple = 0.5 + 0.5 * math.sin(x * 2.6 + z * 0.9 + 1.8 * value_noise(x * 0.4, z * 0.4, 5))
        n = value_noise(x * 0.55, z * 0.55, 3) * 0.75 + ripple * 0.25
        return SAND_DRY[min(3, int(n * 4))]
    if h > 0.04:
        return SAND_DAMP[0 if value_noise(x * 0.8, z * 0.8, 4) < 0.55 else 1]
    # Under water: darker with depth, with ripples on the lagoon floor.
    depth = -h
    band = min(5, int(depth / 0.9))
    ripple = 1 if math.sin(x * 1.7 + z * 0.6 + value_noise(x * 0.3, z * 0.3, 6) * 3) > 0.35 else 0
    base = [(0xdcc595, 0xd3bb88), (0xcdb582, 0xc4ab78), (0xbea873, 0xb59f6c),
            (0xab9767, 0xa18d60), (0x96865c, 0x8d7d55), (0x80734f, 0x786b4a)][band]
    return base[ripple]


def heightfield_mesh(radius, voxel, height, color):
    """Greedy-mesh voxel columns cut from `height(x, z)` within `radius` metres."""
    n = int(math.ceil(radius / voxel))
    top, colors = {}, {}
    for i in range(-n, n):
        for k in range(-n, n):
            x, z = (i + 0.5) * voxel, (k + 0.5) * voxel
            if math.hypot(x, z) > radius:
                continue
            t = round(height(x, z) / voxel)
            top[(i, k)] = t
            colors[(i, k)] = color(x, z, t * voxel, i, k)
    mesh = Mesh()
    # Tops: merge equal height and colour.
    for u, v, du, dv, (t, c) in greedy_rectangles({p: (top[p], colors[p]) for p in top}):
        y = t * voxel
        corners = [(u * voxel, y, v * voxel), ((u + du) * voxel, y, v * voxel),
                   ((u + du) * voxel, y, (v + dv) * voxel), (u * voxel, y, (v + dv) * voxel)]
        mesh.add(c, corners, (0, 1, 0))
    # Sides: where a column stands above its neighbour, merged along the edge.
    for di, dk in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        runs = {}
        for (i, k), t in top.items():
            nb = top.get((i + di, k + dk))
            if nb is None or nb >= t:
                continue
            line = (i if di else k)
            along = k if di else i
            runs.setdefault(line, {})[(along, 0)] = (t, nb, colors[(i, k)])
        for line, cells in runs.items():
            for along, _, length, _, (t, nb, c) in greedy_rectangles(cells):
                edge = (line + (1 if (di + dk) > 0 else 0)) * voxel
                a0, a1 = along * voxel, (along + length) * voxel
                y0, y1 = nb * voxel, t * voxel
                if di:
                    corners = [(edge, y0, a0), (edge, y0, a1), (edge, y1, a1), (edge, y1, a0)]
                else:
                    corners = [(a0, y0, edge), (a1, y0, edge), (a1, y1, edge), (a0, y1, edge)]
                mesh.add(c, corners, (di, 0, dk))
    return mesh


def island_mesh():
    return heightfield_mesh(GRID_RADIUS, VOXEL, ground, sand_color)


# --------------------------------------------------------------------------
# The palm tree (0.25 m voxels): a leaning ringed trunk and a crown of fronds
# --------------------------------------------------------------------------
TRUNK = [0x8a6d47, 0x7b5f3c, 0x9a7d52, 0x6b5233]
LEAF = [0x3f8f3a, 0x4fa244, 0x2f7a33, 0x5cb04a]
LEAF_TIP = [0x86b84a, 0x9cc455]
RIB = 0x6f8c36
DEAD = [0x9b7b45, 0x8a6a3a]
COCONUT = [0x5e7a2c, 0x6b4a2a, 0x54401f]
PALM_HEIGHT = 27  # voxels
PALM_LEAN = (0.75, 0.66)


def palm_trunk_center(level):
    u = level / PALM_HEIGHT
    bend = 1.7 * u ** 1.8 / VOXEL
    return PALM_LEAN[0] * bend, PALM_LEAN[1] * bend


def palm_voxels():
    rng = Lcg(0xBEEF)
    trunk, crown = {}, {}
    # The trunk starts two voxels into the sand so it meets the slope everywhere,
    # flares at the foot and narrows toward the crown.
    for y in range(-2, PALM_HEIGHT + 1):
        level = max(0, y)
        cx, cz = palm_trunk_center(level)
        r = 1.05 - 0.25 * level / PALM_HEIGHT + 0.7 * max(0.0, 1 - level / 3)
        ring = TRUNK[1] if y % 3 == 2 else TRUNK[3] if y % 6 == 5 else TRUNK[0]
        for x in range(math.floor(cx - r), math.ceil(cx + r) + 1):
            for z in range(math.floor(cz - r), math.ceil(cz + r) + 1):
                if (x - cx) ** 2 + (z - cz) ** 2 <= r * r:
                    trunk[(x, y, z)] = ring if rng.rand() > 0.15 else TRUNK[2]
    top_x, top_z = palm_trunk_center(PALM_HEIGHT)
    origin = (round(top_x), PALM_HEIGHT + 1, round(top_z))

    def put(px, py, pz, color):
        crown[(round(px), round(py), round(pz))] = color

    # The heart of the crown.
    for x in range(-1, 2):
        for z in range(-1, 2):
            for y in range(-1, 2):
                if abs(x) + abs(z) + abs(y) <= 2:
                    put(x, y, z, RIB if y >= 0 else TRUNK[3])
    # Fronds: a rib that arches up and out and droops to the tip, with leaflets
    # swept toward the tip on both sides, hanging lower the farther they reach.
    for f in range(10):
        angle = f * 2 * math.pi / 10 + rng.rnd(-0.18, 0.18)
        dx, dz = math.cos(angle), math.sin(angle)
        px, pz = -dz, dx
        length = rng.rnd(19, 24)
        lift = rng.rnd(2.5, 4.5)
        droop = rng.rnd(9, 13)
        steps = int(length * 2)
        for s in range(steps + 1):
            t = s / steps
            d = t * length
            y = lift * math.sin(math.pi * t * 0.6) - droop * t ** 2.1
            rx, rz = d * dx, d * dz
            put(rx, y, rz, RIB)
            if t < 0.1 or s % 2:
                continue
            width = 4.6 * math.sin(math.pi * min(1.0, t * 0.98)) ** 0.6
            for w in range(1, int(width) + 1):
                sweep = 0.45 * w
                hang = -0.7 * w - 0.5 * t * w
                tip = t > 0.86 or (w == int(width) and rng.rand() < 0.35)
                for side in (1, -1):
                    if rng.rand() < 0.2:
                        continue
                    color = rng.pick(LEAF_TIP) if tip else rng.pick(LEAF)
                    put(rx + side * px * w + dx * sweep, y + hang, rz + side * pz * w + dz * sweep, color)
    # Two dead fronds hanging down the trunk.
    for f in range(2):
        angle = f * math.pi + 0.6
        dx, dz = math.cos(angle), math.sin(angle)
        for s in range(14):
            put(dx * (1.2 + s * 0.22), -s * 0.85 - 1, dz * (1.2 + s * 0.22), DEAD[s % 2])
            if s % 2 == 0:
                put(dx * (1.2 + s * 0.22) - dz, -s * 0.85 - 1.6, dz * (1.2 + s * 0.22) + dx, DEAD[1])
                put(dx * (1.2 + s * 0.22) + dz, -s * 0.85 - 1.6, dz * (1.2 + s * 0.22) - dx, DEAD[0])
    # Coconuts: 2x2x2 clusters tucked under the crown.
    for c in range(5):
        angle = c * 2 * math.pi / 5 + 0.4
        bx, bz = round(math.cos(angle) * 1.7), round(math.sin(angle) * 1.7)
        color = COCONUT[c % 3]
        for x in range(2):
            for y in range(2):
                for z in range(2):
                    put(bx + x - 1 + (x == 1 and bx < 0), -3 + y - (c % 2), bz + z - 1, color)
    return trunk, crown, origin


# --------------------------------------------------------------------------
# The steamship (1 m voxels), bow toward -Z, waterline at y = 0
# --------------------------------------------------------------------------
HULL_RED, HULL_BLACK, RAIL, DECK = 0x8e2d24, 0x1f2226, 0xd9d7cf, 0x6c6257
WHITE, WINDOW, FUNNEL, FUNNEL_RED, MAST = 0xe9e6de, 0x2b3644, 0x24282d, 0xb3261e, 0x3a3d42
CONTAINERS = [0xb5462f, 0x2f5f8a, 0xc89b2e, 0x3f7a4a, 0x8a8f96, 0x9c3b5a]
LAMP_WHITE, LAMP_RED, LAMP_GREEN, WINDOW_LIT = 0xfff4d0, 0xff3b2e, 0x3bff6a, 0xffcf7a


def hull_half_width(z):
    if z < -24:
        return 6.0 * math.sqrt(max(0.0, (z + 37) / 13))
    if z > 30:
        return 6.0 - (z - 30) * 0.55
    return 6.0


def ship_voxels():
    rng = Lcg(0x5A11)
    ship, lights = {}, {}
    for z in range(-37, 37):
        w = hull_half_width(z + 0.5)
        for x in range(-7, 7):
            if abs(x + 0.5) > w:
                continue
            for y in range(0, 7):
                if y <= 1:
                    color = HULL_RED
                elif y < 6:
                    color = HULL_BLACK
                else:
                    color = RAIL if abs(x + 0.5) > w - 1 else DECK
                ship[(x, y, z)] = color
    # Raised forecastle at the bow.
    for z in range(-37, -26):
        w = hull_half_width(z + 0.5)
        for x in range(-7, 7):
            if abs(x + 0.5) <= w - 0.5:
                ship[(x, 7, z)] = RAIL if abs(x + 0.5) > w - 1.5 else DECK
    # Bridge superstructure near the stern.
    for z in range(18, 30):
        for x in range(-5, 5):
            for y in range(7, 15):
                ship[(x, y, z)] = WINDOW if y in (9, 12) and (x + z) % 2 == 0 else WHITE
    for z in range(19, 26):
        for x in range(-7, 7):
            for y in (15, 16):
                ship[(x, y, z)] = WINDOW if y == 16 and z == 19 else WHITE
    # Funnel.
    for z in range(11, 16):
        for x in range(-2, 2):
            for y in range(15, 25):
                ship[(x, y, z)] = FUNNEL if y >= 23 else FUNNEL_RED if 19 <= y <= 21 else 0x2c3036
    # Containers on deck, two bays either side of the centre line.
    for bay in range(5):
        z0 = -24 + bay * 7
        for side in (-1, 1):
            height = 2 + int(rng.rand() * 3)
            for level in range(height):
                color = rng.pick(CONTAINERS)
                for x in range(0, 5):
                    xx = x if side > 0 else -x - 1
                    for z in range(z0, z0 + 6):
                        for y in range(7 + level * 2, 9 + level * 2):
                            ship[(xx, y, z)] = color
    # Masts.
    for y in range(8, 23):
        ship[(0, y, -30)] = MAST
    for x in range(-4, 4):
        ship[(x, 19, -30)] = MAST
    for y in range(17, 22):
        ship[(0, y, 22)] = MAST
    # Night lights: masthead, sidelights, stern light and lit windows.
    # Navigation lights are 2 m blocks, oversized so they read from the island.
    for (x, y, z), color in (((0, 23, -30), LAMP_WHITE), ((0, 22, 22), LAMP_WHITE), ((-8, 17, 21), LAMP_RED),
                             ((6, 17, 21), LAMP_GREEN), ((0, 7, 36), LAMP_WHITE)):
        for dx in range(2):
            for dy in range(2):
                for dz in range(2):
                    lights[(x + dx, y + dy, z + dz)] = color
    for z in range(18, 30):
        for y in (9, 12):
            if (z % 3) and z % 2 == 0:
                lights[(-6, y, z)] = WINDOW_LIT
                lights[(5, y, z)] = WINDOW_LIT
    for x in range(-4, 4):
        if x % 2 == 0:
            lights[(x, 12, 17)] = WINDOW_LIT
    return ship, lights


# --------------------------------------------------------------------------
# Sky voxels: sun, moon, clouds and smoke puffs (unit voxels, scaled in the scene)
# --------------------------------------------------------------------------
def disc(radius, colors, rim=None, craters=()):
    voxels = {}
    c = radius - 0.5
    for x in range(int(radius * 2)):
        for y in range(int(radius * 2)):
            d = math.hypot(x - c, y - c)
            if d <= radius - 0.25:
                color = colors[0]
                if rim and d > radius - 1.6:
                    color = rim
                for (cx, cy, cr) in craters:
                    if math.hypot(x - cx, y - cy) <= cr:
                        color = colors[1]
                voxels[(x - int(radius), y - int(radius), 0)] = color
    return voxels


def cloud_voxels(seed):
    rng = Lcg(seed)
    voxels = {}
    blobs = int(rng.rnd(4, 8))
    cx = cz = 0.0
    for _ in range(blobs):
        rx, ry, rz = rng.rnd(3.0, 6.0), rng.rnd(1.2, 2.2), rng.rnd(2.5, 4.5)
        for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
            for y in range(0, int(ry) + 2):
                for z in range(int(cz - rz) - 1, int(cz + rz) + 2):
                    if ((x - cx) / rx) ** 2 + (y / ry) ** 2 + ((z - cz) / rz) ** 2 <= 1:
                        voxels[(x, y, z)] = 0xffffff if y > 0 else 0xe3e9f2
        cx += rng.rnd(-5, 7)
        cz += rng.rnd(-3, 3)
    return voxels


def smoke_voxels(seed):
    rng = Lcg(seed)
    voxels = {}
    for x in range(-2, 3):
        for y in range(-2, 3):
            for z in range(-2, 3):
                d = math.sqrt(x * x + y * y * 1.3 + z * z)
                if d <= 2.3 + rng.rnd(-0.6, 0.4):
                    voxels[(x, y, z)] = 0xffffff
    return voxels


# --------------------------------------------------------------------------
# The game's layout: the raft site faces the lighthouse across the sea
# --------------------------------------------------------------------------
LIGHTHOUSE = (-505.0, 485.0)
LIGHT_DISTANCE = math.hypot(*LIGHTHOUSE)
TO_LIGHT = (LIGHTHOUSE[0] / LIGHT_DISTANCE, LIGHTHOUSE[1] / LIGHT_DISTANCE)


def radius_where(direction, level):
    """Distance from the island's centre along `direction` where the sand sinks to `level`."""
    lo, hi = 1.0, GRID_RADIUS
    for _ in range(40):
        mid = (lo + hi) / 2
        if ground(direction[0] * mid, direction[1] * mid) > level:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def _site():
    r = radius_where(TO_LIGHT, 0.7)
    x, z = TO_LIGHT[0] * r, TO_LIGHT[1] * r
    bow = ground(x + TO_LIGHT[0] * 1.6, z + TO_LIGHT[1] * 1.6)
    stern = ground(x - TO_LIGHT[0] * 1.6, z - TO_LIGHT[1] * 1.6)
    yaw = math.degrees(math.atan2(-TO_LIGHT[0], -TO_LIGHT[1]))
    pitch = math.degrees(math.atan2(bow - stern, 3.2))
    return (round(x, 3), round(ground(x, z) - 0.05, 3), round(z, 3), round(yaw, 3), round(pitch, 3))


SITE = _site()
_launch = radius_where(TO_LIGHT, -1.45) + 1.5
LAUNCH = (round(TO_LIGHT[0] * _launch, 3), round(TO_LIGHT[1] * _launch, 3))
SHADE = (PALM[0] - 1.4, PALM[1] + 1.1)
WOOD = 6
NUTS = 4
START_NUTS = [(PALM[0] + 1.6, PALM[1] - 1.1), (PALM[0] - 1.3, PALM[1] + 0.9)]


def _beach():
    spots = []
    for i in range(30):
        a = 2 * math.pi * i / 30 + 0.11
        d = (math.cos(a), math.sin(a))
        r = radius_where(d, 0.3)
        x, z = d[0] * r, d[1] * r
        if math.dist((x, z), SITE[0:3:2]) < 7 or math.dist((x, z), PALM) < 3:
            continue
        spots.append((round(x, 3), round(z, 3)))
    return spots


BEACH = _beach()
START_WOOD = [BEACH[i] for i in (2, 9, 15, 22)]

# Raft stages: what each costs and what it is called. The game and HUD scripts read
# these from scripts/island.rhai; tools/test_gen_island.py checks the totals.
STAGES = [
    {"name": "Lay the hull logs", "done": "The hull logs are laid", "cost": {"wood": 4.0}},
    {"name": "Lash the logs together", "done": "The logs are lashed together", "cost": {"rope": 3.0}},
    {"name": "Raise the mast", "done": "The mast is up", "cost": {"wood": 2.0, "rope": 1.0}},
    {"name": "Rig the frond sail", "done": "The sail is rigged", "cost": {"sail": 1.0}},
    {"name": "Stow the paddle and coconuts", "done": "Supplies stowed",
     "cost": {"paddle": 1.0, "coconuts": 2.0}},
]
RECIPES = {
    "rope": {"fronds": 2.0},
    "paddle": {"wood": 2.0, "rope": 1.0},
    "sail": {"fronds": 4.0, "rope": 2.0},
}
ITEMS = ["wood", "fronds", "rope", "coconuts", "sail", "paddle"]
NAMES = {"wood": "driftwood", "fronds": "palm fronds", "rope": "rope", "coconuts": "coconuts",
         "sail": "frond sail", "paddle": "paddle"}


# --------------------------------------------------------------------------
# Game props (0.25 m voxels): driftwood, coconuts, the stakes and the raft
# --------------------------------------------------------------------------
DRIFT = [0x9a8c78, 0x8a7c68, 0xa89a84, 0x7f7262]
DRIFT_END = 0xc2b39a
DRIFT_DARK = 0x6e6252
ROPE = 0xc9a46a
MAST_WOOD = 0x7a5f3d
SAIL = [0x6fae4f, 0xc9c26a, 0x5d9a43]
GHOST = 0xd8ecff


def driftwood_voxels(seed):
    rng = Lcg(seed)
    length = int(rng.rnd(8, 12))
    voxels = {}
    for z in range(-length // 2, length - length // 2):
        for x in (-1, 0):
            for y in (0, 1):
                end = z in (-length // 2, length - length // 2 - 1)
                voxels[(x, y, z)] = DRIFT_END if end else rng.pick(DRIFT)
    # A broken-off branch.
    z = int(rng.rnd(-length // 3, length // 3))
    side = 1 if rng.rand() < 0.5 else -2
    voxels[(side, 1, z)] = DRIFT_DARK
    voxels[(side + (1 if side > 0 else -1), 2, z + 1)] = DRIFT_DARK
    return voxels


def coconut_voxels():
    voxels = {}
    for x in range(3):
        for y in range(3):
            for z in range(3):
                if sum(v in (0, 2) for v in (x, y, z)) < 3:
                    voxels[(x, y, z)] = COCONUT[2] if y == 2 and x == 1 and z == 1 else COCONUT[1]
    voxels[(1, 3, 1)] = COCONUT[0]
    return voxels


def line(voxels, a, b, color):
    steps = int(max(abs(b[i] - a[i]) for i in range(3)) * 2) + 1
    for s in range(steps + 1):
        t = s / steps
        p = tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))
        voxels.setdefault(p, color)


def stake_voxels():
    """Four stakes and a rope rectangle marking where the raft is built (raft-local)."""
    voxels = {}
    corners = [(-7, -9), (6, -9), (-7, 8), (6, 8)]
    for x, z in corners:
        for y in range(-3, 4):
            voxels[(x, y, z)] = DRIFT_DARK
    for z in (-9, 8):
        for x in range(-6, 6):
            voxels[(x, 3, z)] = ROPE
    for x in (-7, 6):
        for z in range(-8, 8):
            if z % 2 == 0:
                voxels[(x, 3, z)] = ROPE
    voxels[(6, 4, -9)] = SAIL[0]
    voxels[(6, 5, -9)] = SAIL[0]
    voxels[(5, 5, -9)] = SAIL[2]
    return voxels


def raft_stages():
    """The raft's five build stages in raft-local voxels; the bow points to -Z."""
    rng = Lcg(0x2AF7)
    hull, lash, mast, sail, supplies = ({} for _ in range(5))
    for j in range(5):
        x0 = -5 + 2 * j
        start, end = -7 + (j % 2), 7 - (1 - j % 2)
        for z in range(start, end):
            for x in (x0, x0 + 1):
                for y in (0, 1):
                    hull[(x, y, z)] = DRIFT_END if z in (start, end - 1) else rng.pick(DRIFT)
    for z in (-5, 0, 4):
        for x in range(-6, 6):
            lash[(x, 2, z)] = ROPE
        for y in (0, 1):
            lash[(-6, y, z)] = ROPE
            lash[(5, y, z)] = ROPE
    for z in (-6, 5):
        for x in range(-6, 6):
            lash.setdefault((x, 2, z), DRIFT_DARK)
    for y in range(3, 17):
        mast[(0, y, -1)] = MAST_WOOD
    for p in ((-1, 3, -1), (1, 3, -1), (0, 3, -2), (0, 3, 0)):
        mast[p] = MAST_WOOD
    for x in range(-4, 5):
        mast[(x, 15, -1)] = MAST_WOOD
    line(mast, (0, 16, -1), (0, 3, 6), ROPE)
    line(mast, (0, 16, -1), (0, 3, -7), ROPE)
    # The sail hangs high, so the castaway at the stern sees the sea under it.
    for x in range(-3, 4):
        for y in range(8, 15):
            belly = round(1.2 * math.sin(math.pi * (y - 8) / 7) * (1 - (x / 4.5) ** 2))
            sail[(x, y, -2 - belly)] = SAIL[2] if y == 14 else SAIL[(x + y) % 2]
    for x in range(-4, 2):
        supplies[(x, 2, 2)] = MAST_WOOD
    for x in range(2, 5):
        for z in range(1, 4):
            supplies[(x, 2, z)] = 0xb08a55
    for base in ((-4, -7), (3, -7)):
        for dx in range(2):
            for dy in range(2):
                for dz in range(2):
                    supplies[(base[0] + dx, 2 + dy, base[1] + dz)] = COCONUT[1]
    for x in range(-4, 0):
        supplies[(x, 2, 3)] = SAIL[0]
    return [hull, lash, mast, sail, supplies]


# --------------------------------------------------------------------------
# The lighthouse islet (0.5 m voxels), far out to the south-west
# --------------------------------------------------------------------------
LH_VOXEL = 0.5
ISLET_RADIUS = 22.0
ROCK = [0x7d7f86, 0x8b8d93, 0x6e7077, 0x9a9ca1]
GRASS = [0x5f8f45, 0x6fa052, 0x547f3c]
WHITE_PAINT, RED_PAINT, DARK_METAL = 0xeeeeea, 0xc0392b, 0x2a2c30
GLASS, LAMP, BEAM = 0x9fc3d8, 0xfff1c0, 0xfff3c4
TO_ISLAND = (-TO_LIGHT[0], -TO_LIGHT[1])


def islet_height(x, z):
    a = math.atan2(z, x)
    edge = 15.0 + 2.0 * math.sin(3 * a + 1.0) + 1.2 * math.sin(5 * a + 2.0)
    s = math.hypot(x, z) / edge
    if s < 1:
        return 1.2 + 6.0 * (1 - s ** 1.7) + 0.6 * math.sin(x * 0.5) * math.cos(z * 0.43) * (1 - s)
    return max(-4.0, 1.2 - (s - 1) * 30)


def islet_color(x, z, h, i, k):
    if h > 5.0:
        return ROCK[1] if hash01(i, k, 21) < 0.08 else GRASS[int(value_noise(x * 0.3, z * 0.3, 22) * 3) % 3]
    if h > 1.2:
        return ROCK[int(value_noise(x * 0.4, z * 0.4, 23) * 4) % 4]
    return 0x5c5e64 if h > -0.5 else 0x4a4c52


def lighthouse_voxels():
    """Tower, keeper's cottage and dock in islet-local voxel indices; plus glass,
    the lit windows and the tower's top, the lantern centre's voxel height."""
    solid, glass, windows = {}, {}, {}

    def top(x, z):
        return round(islet_height((x + 0.5) * LH_VOXEL, (z + 0.5) * LH_VOXEL) / LH_VOXEL)
    base = top(0, -1)
    for x in range(-6, 6):
        for z in range(-6, 6):
            r = math.hypot(x + 0.5, z + 0.5)
            if r > 5.2:
                continue
            for y in range(top(x, z) - 1, base):
                solid[(x, y, z)] = ROCK[2]
            for y in range(base, base + 40):
                level = y - base
                color = RED_PAINT if 12 <= level < 17 or 26 <= level < 31 else WHITE_PAINT
                if r > 4.2:
                    facing = ((x + 0.5) * TO_ISLAND[0] + (z + 0.5) * TO_ISLAND[1]) / r
                    if level < 4 and facing > 0.93:
                        color = 0x4a3423
                    elif level in (8, 9, 20, 21, 32, 33) and abs(facing) > 0.9:
                        color = 0x2b3644
                solid[(x, y, z)] = color
    for x in range(-8, 8):
        for z in range(-8, 8):
            r = math.hypot(x + 0.5, z + 0.5)
            if r <= 7.2:
                solid[(x, base + 40, z)] = 0x3a3d42
            if 6.3 < r <= 7.2:
                solid[(x, base + 41, z)] = DARK_METAL
            for y in range(base + 41, base + 46):
                if 2.3 < r <= 3.4:
                    angle = math.atan2(z + 0.5, x + 0.5)
                    if (angle * 8 / math.pi) % 2 < 0.5 or y in (base + 41, base + 45):
                        solid[(x, y, z)] = 0x1e1e1e
                    else:
                        glass[(x, y, z)] = GLASS
            for layer, radius in enumerate((4.2, 3.2, 2.2, 1.2)):
                if r <= radius:
                    solid[(x, base + 46 + layer, z)] = 0xa83a2c
    solid[(0, base + 50, 0)] = DARK_METAL
    solid[(-1, base + 50, -1)] = DARK_METAL
    # The keeper's cottage, a little way inland from the tower.
    x0, x1, z0, z1 = -17, -7, 3, 11
    floor = max(top(x, z) for x in range(x0, x1) for z in range(z0, z1))
    for x in range(x0 - 1, x1 + 1):
        for z in range(z0 - 1, z1 + 1):
            for y in range(top(x, z) - 1, floor):
                solid[(x, y, z)] = ROCK[0]
            wall = x in (x0, x1 - 1) or z in (z0, z1 - 1)
            inside = x0 <= x < x1 and z0 <= z < z1
            if inside:
                for y in range(floor, floor + 6):
                    if not wall and y > floor:
                        continue
                    color = WHITE_PAINT
                    if wall and y in (floor + 2, floor + 3) and (x in (-14, -13, -10, -9) or z in (6, 7)):
                        color = 0x2b3644
                        windows[(x, y, z)] = 0xffcf7a
                    if x == x1 - 1 and 6 <= z <= 7 and y < floor + 3:
                        color = 0x5a3a24
                    solid[(x, y, z)] = color
            roof = floor + 6 + min(z - (z0 - 1), (z1) - z)
            for y in range(floor + 6, roof + 1):
                if x0 <= x < x1 and z0 <= z < z1 and y < roof:
                    solid[(x, y, z)] = WHITE_PAINT
                elif y == roof:
                    solid[(x, y, z)] = 0xa83a2c
    for y in range(floor + 6, floor + 12):
        solid[(-15, y, 8)] = 0x7d5a48
    # The dock reaches out toward the island.
    edge = 15.0
    for step in range(0, 22):
        r = edge - 2 + step * 0.5
        cx, cz = TO_ISLAND[0] * r, TO_ISLAND[1] * r
        for w in (-1, 0, 1):
            px, pz = cx - TO_ISLAND[1] * w * LH_VOXEL, cz + TO_ISLAND[0] * w * LH_VOXEL
            i, k = math.floor(px / LH_VOXEL), math.floor(pz / LH_VOXEL)
            solid[(i, 2, k)] = 0x8a6a45 if (step + w) % 3 else 0x7a5a38
            if step % 4 == 0 and w != 0:
                for y in range(-6, 2):
                    solid.setdefault((i, y, k), 0x5a4128)
    return solid, glass, windows, base


def lamp_voxels():
    return {(x, y, z): LAMP for x in (-1, 0, 1) for y in (-1, 0, 1) for z in (-1, 0, 1)
            if abs(x) + abs(y) + abs(z) < 3}


def beam_voxels():
    """Two opposite light beams along X from the lamp (1.2 m voxels)."""
    voxels = {}
    for d in range(1, 42):
        half = (0.6 + d * 1.2 * 0.07) / 1.2
        for y in range(-5, 5):
            for z in range(-5, 5):
                if abs(y + 0.5) <= half and abs(z + 0.5) <= half:
                    voxels[(d, y, z)] = BEAM
                    voxels[(-d - 1, y, z)] = BEAM
    return voxels


# --------------------------------------------------------------------------
# Pixel-art inventory icons, one 32x32 cell per item in a 192x32 atlas
# --------------------------------------------------------------------------
ICONS = {
    "wood": (["......##", ".....#o#", "....###.", "...###..", "..###...", ".###....", "#o#.....", "##......"],
             {"#": 0x8a7a66, "o": 0xc9b79c}),
    "fronds": (["......g.", ".....gG.", "..g.gG.g", "...gGg..", "..gGg.g.", ".gGg....", "gG.g....", "b......."],
               {"g": 0x5cb04a, "G": 0x2f7a33, "b": 0x6f8c36}),
    "rope": (["..tttt..", ".tTTTTt.", "tT....Tt", "tT.tt.Tt", "tT.tt.Tt", "tT....Tt", ".tTTTTt.", "..tttt.."],
             {"t": 0xd8b47a, "T": 0x9c7a45}),
    "coconuts": (["...gg...", "..gbbg..", ".bbbbbb.", "bbBbbbbb", "bbbbbBbb", "bBbbbbbb", ".bbbbbb.", "..bbbb.."],
                 {"b": 0x7b5432, "B": 0x4e341b, "g": 0x6f8f35}),
    "sail": (["s.......", "sggyggy.", "sgyggyg.", "syggygg.", "sggyggy.", "sgyggyg.", "s.......", "s......."],
             {"s": 0x8b6a43, "g": 0x5aa64a, "y": 0xd3cc62}),
    "paddle": (["......pp", ".....ppp", "....ppp.", "...hh...", "..hh....", ".hh.....", "hh......", "h......."],
               {"p": 0xc49a62, "h": 0x8b6a43}),
}


def icons_png():
    cell = 32

    def pixel(x, y):
        art, palette = ICONS[ITEMS[x // cell]]
        ch = art[(y % cell) // 4][(x % cell) // 4]
        if ch == ".":
            return (0, 0, 0, 0)
        c = palette[ch]
        return ((c >> 16) & 255, (c >> 8) & 255, c & 255, 255)
    return png_rgba(cell * len(ITEMS), cell, pixel)


# --------------------------------------------------------------------------
# The ocean: a shader graph on a translucent plane. Wave normals are a sum of
# directional sines of world position and time; colour and opacity shift from
# a clear turquoise lagoon around the island to the deep blue open sea.
# --------------------------------------------------------------------------
class Graph:
    def __init__(self, name):
        self.name, self.nodes, self.wires = name, [], []

    VECTOR_PORTS = {"master": (0, 3, 5), "color": (0,), "vector": (0,), "split": (0,), "normalize": (0,),
                    "scale_vector": (0,), "lerp_vector": (0, 1), "add_vector": (0, 1),
                    "multiply_vectors": (0, 1), "dot": (0, 1)}

    def node(self, kind, *inputs, x=0.0, y=0.0):
        values = []
        links = []
        for port, value in enumerate(inputs):
            if isinstance(value, tuple):  # (node id, output port); the stored value is unused
                links.append((value, port))
                value = [0.0, 0.0, 0.0] if port in self.VECTOR_PORTS.get(kind, ()) else 0.0
            values.append({"vector": [float(v) for v in value]} if isinstance(value, list) else {"float": float(value)})
        node_id = len(self.nodes) + 1
        self.nodes.append({"id": node_id, "position": [x, y], "kind": kind, "inputs": values,
                           "slot": "base_color"})
        for (source, port_out), port in links:
            self.wires.append({"from": {"node": source, "port": port_out}, "to": {"node": node_id, "port": port}})
        return node_id

    def out(self, node_id, port=0):
        return (node_id, port)

    def document(self):
        assert len(self.nodes) <= 128 and len(self.wires) <= 512
        return {"version": 1, "name": self.name, "nodes": self.nodes, "wires": self.wires}


WAVES = [  # (kx, kz, omega, phase, slope x, slope z)
    (0.21, 0.09, 1.05, 0.0, 0.066, 0.028),
    (-0.12, 0.27, 1.27, 1.7, -0.025, 0.056),
    (0.55, 0.38, 2.1, 0.4, 0.034, 0.024),
    (-0.9, 0.7, 2.9, 2.6, -0.022, 0.017),
    (1.6, -1.1, 3.8, 4.1, 0.016, -0.011),
]
LAGOON = (14.0, 20.0)  # distance where the lagoon starts fading, and the fade length


def ocean_graph():
    g = Graph("Ocean")
    master = g.node("master", [0.0, 0.0, 0.0], 0.0, 0.05, [0.0, 0.0, 0.0], 1.0, [0.0, 1.0, 0.0], x=1400)
    time = g.node("time", x=-1200, y=0)
    split = g.node("split", g.out(g.node("world_position", x=-1400, y=300)), x=-1200, y=300)
    x, z = g.out(split, 0), g.out(split, 2)
    nx = nz = None
    for i, (kx, kz, omega, phase, sx, sz) in enumerate(WAVES):
        row = 160 * i
        a = g.node("add", g.out(g.node("multiply", x, kx, x=-1000, y=row)),
                   g.out(g.node("multiply", z, kz, x=-1000, y=row + 60)), x=-800, y=row)
        b = g.node("add", g.out(a), g.out(g.node("multiply", g.out(time), omega, x=-1000, y=row + 120)),
                   x=-600, y=row)
        # sin(phase + pi/2) is the cosine the slope needs.
        s = g.node("sine", g.out(g.node("add", g.out(b), phase + math.pi / 2, x=-400, y=row)), x=-200, y=row)
        px = g.node("multiply", g.out(s), -sx, x=0, y=row)
        pz = g.node("multiply", g.out(s), -sz, x=0, y=row + 60)
        nx = g.out(px) if nx is None else g.out(g.node("add", nx, g.out(px), x=200, y=row))
        nz = g.out(pz) if nz is None else g.out(g.node("add", nz, g.out(pz), x=200, y=row + 60))
    normal = g.node("normalize", g.out(g.node("append", nx, 1.0, nz, x=400, y=300)), x=600, y=300)
    g.wires.append({"from": {"node": normal, "port": 0}, "to": {"node": master, "port": 5}})
    # Distance from the island: 0 in the lagoon, 1 on the open sea.
    r2 = g.node("add", g.out(g.node("multiply", x, x, x=-1000, y=900)),
                g.out(g.node("multiply", z, z, x=-1000, y=960)), x=-800, y=900)
    r = g.node("power", g.out(r2), 0.5, x=-600, y=900)
    far = g.node("clamp", g.out(g.node("divide", g.out(g.node("subtract", g.out(r), LAGOON[0], x=-400, y=900)),
                                        LAGOON[1], x=-200, y=900)), 0.0, 1.0, x=0, y=900)
    shallow = g.node("color", linear(0x2fb7b0), x=200, y=760)
    deep = g.node("color", linear(0x0a3a5c), x=200, y=840)
    color = g.node("lerp_vector", g.out(shallow), g.out(deep), g.out(far), x=600, y=800)
    g.wires.append({"from": {"node": color, "port": 0}, "to": {"node": master, "port": 0}})
    alpha = g.node("lerp", 0.38, 1.0, g.out(g.node("power", g.out(far), 0.6, x=200, y=980)), x=600, y=980)
    g.wires.append({"from": {"node": alpha, "port": 0}, "to": {"node": master, "port": 4}})
    return g.document()


# --------------------------------------------------------------------------
# Scripts
# --------------------------------------------------------------------------
SKY = {
    "night": {"zenith": [0.0032, 0.0055, 0.018], "horizon": [0.011, 0.017, 0.042], "ground": [0.006, 0.01, 0.024]},
    "dusk": {"zenith": [0.05, 0.08, 0.2], "horizon": [0.92, 0.42, 0.19], "ground": [0.16, 0.11, 0.13]},
    "day": {"zenith": [0.07, 0.32, 0.95], "horizon": [0.62, 0.84, 1.0], "ground": linear(0x34607a)},
}
SKY_DAY = SKY["day"]
CLOUDS = 16
SMOKE = 40
SMOKE_LIFE = 30.0
BLOOM = 0.14


def rhai(value):
    """A Rhai literal for a Python value: maps, arrays, numbers and strings."""
    if isinstance(value, dict):
        return "#{ " + ", ".join(f"{k}: {rhai(v)}" for k, v in value.items()) + " }"
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(rhai(v) for v in value) + "]"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return repr(round(value, 6))
    return json.dumps(value)


def island_library(beacon):
    shared = {
        "VOXEL": VOXEL, "SWIM_DEPTH": SWIM_DEPTH, "BOUNDARY": BOUNDARY, "PALM": PALM, "SHADE": SHADE,
        "SITE": SITE, "LAUNCH": LAUNCH, "LIGHTHOUSE": LIGHTHOUSE, "BEACON": beacon, "BEACH": BEACH,
        "WOOD": WOOD, "NUTS": NUTS, "STAGES": STAGES, "RECIPES": RECIPES, "ITEMS": ITEMS, "NAMES": NAMES,
        "TIME_SCALE": TIME_SCALE, "SKY": SKY, "SKY_INTENSITY": SKY_INTENSITY, "SUN_INTENSITY": SUN_INTENSITY,
        "BLOOM": BLOOM, "CLOUDS": CLOUDS, "SMOKE": SMOKE, "SMOKE_LIFE": SMOKE_LIFE, "FUNNEL": FUNNEL_TOP,
    }
    constants = "\n".join(f"const {name} = {rhai(value)};" for name, value in shared.items())
    getters = "\n".join(f"fn {name.lower()}() {{ global::{name} }}" for name in shared)
    return f"""// Generated by tools/gen_island.py: the island's shape, the layout of the game and
// its tables, shared by every script. Edit the generator, not this file: it voxelizes
// the same shape and places the same objects.

{constants}

{getters}

fn smooth(e0, e1, x) {{
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}}

fn coast_radius(x, z) {{
    let a = atan2(z, x);
    {COAST} + 2.2 * sin(2.0 * a + 0.7) + 1.4 * sin(3.0 * a + 2.1) + 0.7 * sin(5.0 * a + 0.3)
}}

// Height of the sand at (x, z) in metres; the sea surface is y = 0.
fn ground(x, z) {{
    let s = sqrt(x * x + z * z) / coast_radius(x, z);
    if s < 1.0 {{
        let dunes = 0.32 * sin(x * 0.45 + 0.3) * cos(z * 0.38 - 0.8) + 0.16 * sin((x + z) * 0.9);
        {PEAK} * (1.0 - s * s) + dunes * (1.0 - s)
    }} else {{
        -1.6 * (1.0 - exp(-(s - 1.0) * 4.0)) - 3.5 * smooth(1.35, 1.9, s)
    }}
}}

// The top of the voxel column at (x, z): where small objects rest.
fn surface(x, z) {{
    round(ground(x, z) / global::VOXEL) * global::VOXEL
}}

// Where the ship is after `t` seconds of world time. It drops below the
// horizon with the Earth's curvature as it steams away.
fn ship_position(t) {{
    let x = {SHIP_BEARING[0] * SHIP_START:.3f} + {SHIP_HEADING[0]:.6f} * {SHIP_SPEED} * t;
    let z = {SHIP_BEARING[1] * SHIP_START:.3f} + {SHIP_HEADING[1]:.6f} * {SHIP_SPEED} * t;
    let drop = (x * x + z * z) / 12742000.0;
    [x, -drop, z]
}}

fn ship_yaw() {{ {SHIP_YAW:.4f} }}

// A ship-local point (bow toward -Z) in world space, ignoring the gentle roll.
fn ship_point(t, local) {{
    let p = ship_position(t);
    let a = to_radians(ship_yaw());
    let c = cos(a);
    let s = sin(a);
    [p[0] + local[0] * c + local[2] * s, p[1] + local[1], p[2] - local[0] * s + local[2] * c]
}}

// Inventory costs, shared by the camp, the raft and the HUD.
fn affordable(cost) {{
    for item in cost.keys() {{
        if get_scene_variable(item) < cost[item] {{
            return false;
        }}
    }}
    true
}}

fn pay(cost) {{
    for item in cost.keys() {{
        set_scene_variable(item, get_scene_variable(item) - cost[item]);
    }}
}}

fn cost_text(cost) {{
    let parts = [];
    for item in global::ITEMS {{
        if item in cost {{
            parts.push(`${{cost[item].to_int()}} ${{global::NAMES[item]}}`);
        }}
    }}
    parts.reduce(|text, part| if text == "" {{ part }} else {{ text + ", " + part }}, "")
}}

fn missing_text(cost) {{
    let parts = [];
    for item in global::ITEMS {{
        if item in cost {{
            let short = cost[item] - get_scene_variable(item);
            if short > 0.0 {{
                parts.push(`${{short.to_int()}} more ${{global::NAMES[item]}}`);
            }}
        }}
    }}
    parts.reduce(|text, part| if text == "" {{ part }} else {{ text + ", " + part }}, "")
}}

fn hours_text(hours) {{
    let h = floor(hours).to_int();
    let m = floor((hours - floor(hours)) * 60.0).to_int();
    let hh = if h < 10 {{ `0${{h}}` }} else {{ `${{h}}` }};
    let mm = if m < 10 {{ `0${{m}}` }} else {{ `${{m}}` }};
    `${{hh}}:${{mm}}`
}}

fn fixed(value, digits) {{
    let scale = if digits == 1 {{ 10 }} else {{ 100 }};
    let n = round(value * scale.to_float()).to_int();
    let whole = n / scale;
    let fraction = `${{n % scale}}`;
    while fraction.len() < digits {{
        fraction = `0${{fraction}}`;
    }}
    `${{whole}}.${{fraction}}`
}}
"""


# --------------------------------------------------------------------------
# HUD and settings widgets
# --------------------------------------------------------------------------
CLEAR = [0.0, 0.0, 0.0, 0.0]
PANEL = [0.025, 0.04, 0.07, 0.9]
CONTROL = [0.09, 0.14, 0.21, 0.92]
TEXT = [0.95, 0.97, 1.0, 1.0]
DIM = [0.62, 0.72, 0.82, 1.0]


def anchors(min_, max_, pivot, offset, size):
    return {"min": list(min_), "max": list(max_), "pivot": list(pivot), "offset": list(offset), "size": list(size)}


def widget(object_id, name, parent, **fields):
    value = {"kind": "label", "background": CLEAR, "text_color": TEXT, "padding": [0, 0, 0, 0]}
    value.update(fields)
    return {"id": object_id, "name": name, "parent": parent, "transform": tf((0, 0, 0)), "ui_widget": value}


def ui_objects():
    texts = ("clock_text", "compass_text", "fps_text", "sensitivity_text", "fov_text", "distance_text",
             "objective_text", "prompt_text", "site_text", "light_text") + tuple(f"count_{i}" for i in ITEMS)
    objs = [{"id": "ui", "name": "HUD and menus", "transform": tf((0, 0, 0)),
             "ui_canvas": {"layer": "3d", "reference": [1280, 720], "scaling": "fit", "order": 10},
             "blackboard": {key: text("") for key in texts} | {"fps_at": number(-1)},
             "script_manager": {"scripts": [{"enabled": True, "script": "hud-script"}]}}]
    shade = [0.0, 0.0, 0.0, 0.3]

    def at(min_, pivot, offset, size, max_=None):
        return anchors(min_, max_ or min_, pivot, offset, size)
    objs += [
        widget("ui-clock", "Clock", "ui", order=1, text="Day 1  ·  08:00  ·  Morning", font_size=21,
               background=shade, padding=[12, 7, 12, 7], anchors=at((0, 0), (0, 0), (20, 16), (300, 38))),
        widget("ui-objective", "Objective", "ui", order=2, text="", font_size=16, background=shade,
               padding=[12, 7, 12, 7], anchors=at((0, 0), (0, 0), (20, 60), (600, 58))),
        widget("ui-compass", "Compass", "ui", order=3, text="N", font_size=19, text_alignment="center",
               background=shade, padding=[14, 7, 14, 7], anchors=at((0.5, 0), (0.5, 0), (0, 16), (120, 36))),
        widget("ui-crosshair", "Crosshair", "ui", kind="panel", order=4, background=[1.0, 1.0, 1.0, 0.8],
               anchors=at((0.5, 0.5), (0.5, 0.5), (0, 0), (4, 4))),
        widget("ui-prompt", "Interaction prompt", "ui", order=5, text="", font_size=18, visible=False,
               text_alignment="center", background=[0.0, 0.0, 0.0, 0.5], padding=[12, 7, 12, 7],
               anchors=at((0.5, 0.5), (0.5, 0), (0, 34), (640, 36))),
        widget("ui-pickup", "Pickup", "ui", order=6, text="", font_size=17, visible=False, text_alignment="center",
               text_color=[1.0, 0.92, 0.62, 1.0], anchors=at((0.5, 0.5), (0.5, 0), (0, 78), (520, 28))),
        widget("ui-settings-button", "Settings button", "ui", kind="button", order=7, text="Settings  (Esc)",
               font_size=16, text_alignment="center", background=[0.03, 0.05, 0.08, 0.55], visible=False,
               padding=[8, 8, 8, 8], shortcuts=["Escape"], accessible_name="Open settings",
               anchors=at((1, 0), (1, 0), (-24, 16), (170, 38))),
        widget("ui-camp-button", "Camp button", "ui", kind="button", order=8, text="Camp  (C)", font_size=16,
               text_alignment="center", background=[0.03, 0.05, 0.08, 0.55], visible=False, padding=[8, 8, 8, 8],
               shortcuts=["C"], accessible_name="Open camp", anchors=at((1, 0), (1, 0), (-24, 60), (170, 38))),
        widget("ui-fps", "FPS", "ui", order=9, text="", font_size=16, text_alignment="right", visible=False,
               text_color=DIM, anchors=at((1, 0), (1, 0), (-24, 104), (240, 26))),
        widget("ui-hint", "Controls hint", "ui", order=10, font_size=16, text_alignment="center", visible=False,
               text="WASD move   Shift sprint   Space jump   E gather and build   C camp   Hold T to speed up time",
               background=[0.0, 0.0, 0.0, 0.38], padding=[10, 7, 10, 7],
               anchors=at((0.5, 1), (0.5, 1), (0, -122), (900, 34))),
        widget("ui-energy", "Energy", "ui", kind="panel", order=11, background=shade, visible=False,
               anchors=at((0, 1), (0, 1), (20, -20), (260, 50))),
        widget("ui-energy-label", "Energy label", "ui-energy", text="Energy", font_size=14, text_color=DIM,
               anchors=at((0, 0), (0, 0), (12, 5), (236, 20))),
        widget("ui-energy-bar", "Energy bar", "ui-energy", kind="panel", background=[0.0, 0.0, 0.0, 0.5],
               anchors=at((0, 0), (0, 0), (12, 28), (236, 12))),
        widget("ui-energy-fill", "Energy fill", "ui-energy-bar", kind="panel", background=[0.3, 0.85, 0.45, 1.0],
               anchors=at((0, 0), (0, 0), (0, 0), (236, 12))),
        widget("ui-hotbar", "Inventory", "ui", kind="panel", order=12, layout="row", gap=8, padding=[8, 8, 8, 8],
               background=shade, visible=False,
               anchors=at((0.5, 1), (0.5, 1), (0, -18), (len(ITEMS) * 88 + 8, 88))),
        widget("ui-label-site", "Raft site label", "ui", order=13, text="Raft", font_size=15, visible=False,
               text_alignment="center", background=[0.0, 0.0, 0.0, 0.4], padding=[8, 5, 8, 5],
               anchors=at((0, 0), (0.5, 1), (0, 0), (190, 28))),
        widget("ui-label-light", "Lighthouse label", "ui", order=14, text="Lighthouse", font_size=15,
               visible=False, text_alignment="center", background=[0.0, 0.0, 0.0, 0.4], padding=[8, 5, 8, 5],
               anchors=at((0, 0), (0.5, 1), (0, 0), (210, 28))),
    ]
    for index, item in enumerate(ITEMS):
        objs += [
            widget(f"ui-slot-{item}", NAMES[item], "ui-hotbar", kind="panel", order=index,
                   background=[0.06, 0.09, 0.13, 0.85], anchors=at((0, 0), (0, 0), (0, 0), (80, 72))),
            widget(f"ui-icon-{item}", f"{NAMES[item]} icon", f"ui-slot-{item}", kind="image", image="icons",
                   uv=[index / len(ITEMS), 0, 1 / len(ITEMS), 1], background=[1.0, 1.0, 1.0, 1.0],
                   anchors=at((0.5, 0), (0.5, 0), (0, 6), (36, 36))),
            widget(f"ui-count-{item}", f"{NAMES[item]} count", f"ui-slot-{item}", text=f"0 {NAMES[item]}",
                   font_size=12, text_alignment="center", anchors=at((0.5, 1), (0.5, 1), (0, -6), (80, 20))),
        ]

    def button(object_id, parent, label, order, size=(416, 46), color=CONTROL, shortcuts=(), font=17):
        return widget(object_id, label, parent, kind="button", order=order, text=label, font_size=font,
                      text_alignment="center", background=list(color), padding=[10, 10, 10, 10],
                      shortcuts=list(shortcuts), accessible_name=label,
                      anchors=at((0, 0), (0, 0), (0, 0), size))
    camp = [
        widget("ui-camp", "Camp panel", "ui", kind="panel", order=30, layout="column", visible=False,
               background=PANEL, padding=[22, 20, 22, 20], gap=10,
               anchors=at((1, 0.5), (1, 0.5), (-24, 0), (460, 470))),
        widget("ui-camp-title", "Camp title", "ui-camp", order=0, text="Camp", font_size=26,
               anchors=at((0, 0), (0, 0), (0, 0), (416, 34))),
        widget("ui-camp-note", "Camp note", "ui-camp", order=1, font_size=14, text_color=DIM,
               text="Craft, eat and rest. Greyed-out recipes need more materials.",
               anchors=at((0, 0), (0, 0), (0, 0), (416, 22))),
        button("ui-craft-rope", "ui-camp", "Twist rope  ·  2 palm fronds", 2),
        button("ui-craft-paddle", "ui-camp", "Carve a paddle  ·  2 driftwood, 1 rope", 3),
        button("ui-craft-sail", "ui-camp", "Weave a frond sail  ·  4 palm fronds, 2 rope", 4),
        button("ui-eat", "ui-camp", "Eat a coconut  ·  +30 energy", 5),
        button("ui-rest", "ui-camp", "Rest in the shade until dusk", 6),
        button("ui-camp-close", "ui-camp", "Close  (C)", 7, color=(0.13, 0.42, 0.33, 1.0), shortcuts=("C", "Escape")),
    ]

    def screen(object_id, name, order, visible, card_size, children):
        backdrop = widget(object_id, name, "ui", kind="panel", order=order, visible=visible,
                          background=[0.0, 0.01, 0.03, 0.55], anchors=anchors((0, 0), (1, 1), (0, 0), (0, 0), (0, 0)))
        card = widget(f"{object_id}-card", f"{name} card", object_id, kind="panel", layout="column",
                      background=PANEL, padding=[34, 30, 34, 30], gap=14,
                      anchors=at((0.5, 0.5), (0.5, 0.5), (0, 0), card_size))
        return [backdrop, card] + children(f"{object_id}-card", card_size[0] - 68)
    title = screen("ui-title", "Title screen", 50, True, (780, 470), lambda parent, width: [
        widget("ui-title-name", "Title", parent, order=0, text="SAND ISLAND", font_size=52,
               anchors=at((0, 0), (0, 0), (0, 0), (width, 64))),
        widget("ui-title-story", "Story", parent, order=1, font_size=19,
               text="You wake up on a tiny island with one palm tree. The ship that could have saved you "
                    "is already steaming away, and the current is too strong to swim.\n"
                    "Build a raft and find your way home.",
               anchors=at((0, 0), (0, 0), (0, 0), (width, 108))),
        widget("ui-title-controls", "Controls", parent, order=2, font_size=15, text_color=DIM,
               text="WASD walk   Shift sprint   Space jump   Mouse look\n"
                    "E gather and build   C camp: craft, eat and rest\nHold T to speed up time   Esc settings",
               anchors=at((0, 0), (0, 0), (0, 0), (width, 76))),
        widget("ui-title-buttons", "Title buttons", parent, kind="panel", order=3, layout="row", gap=16,
               background=CLEAR, anchors=at((0, 0), (0, 0), (0, 0), (width, 52))),
        button("ui-start", "ui-title-buttons", "Start  (Enter)", 0, size=(260, 52), color=(0.13, 0.42, 0.33, 1.0),
               shortcuts=("Enter", "Space", "E"), font=19),
        button("ui-title-quit", "ui-title-buttons", "Quit  (Esc)", 1, size=(180, 52), color=(0.35, 0.12, 0.12, 1.0),
               shortcuts=("Escape",), font=19),
    ])
    end = screen("ui-end", "Ending screen", 51, False, (720, 430), lambda parent, width: [
        widget("ui-end-name", "Ending title", parent, order=0, text="RESCUED", font_size=52,
               anchors=at((0, 0), (0, 0), (0, 0), (width, 64))),
        widget("ui-end-story", "Ending story", parent, order=1, font_size=19,
               text="The lighthouse keeper spots your frond sail and rows out to meet you. "
                    "After everything, you are going home.",
               anchors=at((0, 0), (0, 0), (0, 0), (width, 84))),
        widget("ui-end-stats", "Ending stats", parent, order=2, font_size=16, text_color=DIM, text="",
               anchors=at((0, 0), (0, 0), (0, 0), (width, 70))),
        widget("ui-end-buttons", "Ending buttons", parent, kind="panel", order=3, layout="row", gap=16,
               background=CLEAR, anchors=at((0, 0), (0, 0), (0, 0), (width, 52))),
        button("ui-again", "ui-end-buttons", "Play again  (Enter)", 0, size=(260, 52),
               color=(0.13, 0.42, 0.33, 1.0), shortcuts=("Enter",), font=19),
        button("ui-end-quit", "ui-end-buttons", "Quit  (Esc)", 1, size=(180, 52), color=(0.35, 0.12, 0.12, 1.0),
               shortcuts=("Escape",), font=19),
    ])
    overlays = [
        widget("ui-fade", "Fade", "ui", kind="panel", order=90, visible=False, background=[0.0, 0.0, 0.0, 1.0],
               anchors=anchors((0, 0), (1, 1), (0, 0), (0, 0), (0, 0))),
        widget("ui-toast", "Message", "ui", order=95, font_size=20, text_alignment="center", visible=False,
               background=[0.0, 0.0, 0.0, 0.45], padding=[12, 9, 12, 9],
               anchors=at((0.5, 0.3), (0.5, 0.5), (0, 0), (820, 42))),
    ]
    return objs + camp + settings_menu() + title + end + overlays


def settings_menu():
    menu = [
        widget("ui-menu", "Settings menu backdrop", "ui", kind="panel", order=40, visible=False,
               background=[0.0, 0.01, 0.03, 0.5], anchors=anchors((0, 0), (1, 1), (0, 0), (0, 0), (0, 0))),
        widget("ui-menu-panel", "Settings panel", "ui-menu", kind="panel", layout="column", background=PANEL,
               padding=[28, 24, 28, 24], gap=12,
               anchors=anchors((0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0, 0), (900, 694))),
        widget("ui-menu-title", "Title", "ui-menu-panel", order=0, text="Settings", font_size=30,
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 42))),
        widget("ui-menu-note", "Note", "ui-menu-panel", order=1, font_size=15, text_color=DIM,
               text="Time stands still while this menu is open.",
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 22))),
        widget("ui-menu-columns", "Columns", "ui-menu-panel", kind="panel", order=2, layout="row", gap=36,
               background=CLEAR, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 494))),
        widget("ui-col-controls", "Controls column", "ui-menu-columns", kind="panel", order=0, layout="column",
               gap=10, background=CLEAR, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 494))),
        widget("ui-col-graphics", "Graphics column", "ui-menu-columns", kind="panel", order=1, layout="column",
               gap=10, background=CLEAR, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 494))),
        widget("ui-menu-buttons", "Buttons", "ui-menu-panel", kind="panel", order=3, layout="row", gap=16,
               background=CLEAR, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 46))),
        widget("ui-resume", "Resume", "ui-menu-buttons", kind="button", order=0, text="Resume  (Esc)",
               font_size=18, text_alignment="center", background=[0.13, 0.42, 0.33, 1.0], padding=[10, 10, 10, 10],
               shortcuts=["Escape"], anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (220, 46))),
        widget("ui-quit", "Quit", "ui-menu-buttons", kind="button", order=1, text="Quit game", font_size=18,
               text_alignment="center", background=[0.35, 0.12, 0.12, 1.0], padding=[10, 10, 10, 10],
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (180, 46))),
    ]

    def header(object_id, parent, label, order):
        return widget(object_id, label, parent, order=order, text=label, font_size=15, text_color=DIM,
                      anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 22)))

    def slider(object_id, parent, name, order, value, lo, hi, step):
        return widget(object_id, name, parent, kind="slider", order=order, text=name, font_size=17,
                      value=value, min=lo, max=hi, step=step, background=CONTROL, padding=[12, 8, 12, 10],
                      accessible_name=name, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 56)))

    def toggle(object_id, parent, name, order, on):
        return widget(object_id, name, parent, kind="toggle", order=order, text=name, font_size=17,
                      value=1.0 if on else 0.0, background=CONTROL, padding=[12, 8, 12, 8],
                      accessible_name=name, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 40)))

    def button(object_id, parent, name, order):
        return widget(object_id, name, parent, kind="button", order=order, text=name, font_size=17,
                      background=CONTROL, padding=[12, 9, 12, 9], accessible_name=name,
                      anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 40)))
    return menu + [
        header("ui-head-controls", "ui-col-controls", "MOUSE AND VIEW", 0),
        slider("ui-opt-sensitivity", "ui-col-controls", "Mouse sensitivity", 1, 1.0, 0.1, 3.0, 0.05),
        toggle("ui-opt-invert", "ui-col-controls", "Invert mouse Y", 2, False),
        slider("ui-opt-fov", "ui-col-controls", "Field of view", 3, 75.0, 60.0, 110.0, 1.0),
        toggle("ui-opt-bob", "ui-col-controls", "Head bobbing", 4, True),
        widget("ui-help", "Help", "ui-col-controls", order=5, font_size=15, text_color=DIM,
               text="WASD walk, Shift sprint, Space jump, E gather.\nC opens camp. Hold T to speed up time.\n"
                    "Arrow keys adjust a focused slider; Tab moves focus.",
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 90))),
        header("ui-head-graphics", "ui-col-graphics", "GRAPHICS", 0),
        button("ui-opt-quality", "ui-col-graphics", "Quality: High", 1),
        slider("ui-opt-distance", "ui-col-graphics", "View distance", 2, 2.0, 0.0, 2.0, 1.0),
        toggle("ui-opt-shadows", "ui-col-graphics", "Shadows", 3, True),
        toggle("ui-opt-clouds", "ui-col-graphics", "Clouds", 4, True),
        toggle("ui-opt-bloom", "ui-col-graphics", "Bloom", 5, True),
        toggle("ui-opt-vsync", "ui-col-graphics", "VSync", 6, True),
        button("ui-opt-window", "ui-col-graphics", "Window: Windowed", 7),
        toggle("ui-opt-fps", "ui-col-graphics", "Show FPS", 8, False),
    ]


# --------------------------------------------------------------------------
# Scene
# --------------------------------------------------------------------------
def tf(translation, rotation=(0, 0, 0), scale=(1, 1, 1)):
    return {"translation": [round(v, 4) for v in translation],
            "rotation_degrees": [round(v, 4) for v in rotation],
            "scale": [round(v, 4) for v in scale]}


def drawable(mesh, extra=None, texture="white", color=(1, 1, 1)):
    value = {"layer": "3d", "mesh": mesh, "texture": texture, "color": list(color), "uv_scale": [1, 1]}
    value.update(extra or {})
    return value


def number(v):
    return {"scalar": {"number": float(v)}}


def flag(v):
    return {"scalar": {"bool": bool(v)}}


def text(v):
    return {"scalar": {"text": v}}


def build():
    files, assets, objects = {}, {}, []

    def emit_json(path, value):
        files[path] = (json.dumps(value, indent=1) + "\n").encode()

    def emit_gltf(name, gltf, external=False):
        binary = bytes(gltf.buffer)
        if external:
            files[f"scenes/assets/{name}.bin"] = binary
            emit_json(f"scenes/assets/{name}.gltf", gltf.document(f"{name}.bin"))
        else:
            uri = "data:application/octet-stream;base64," + base64.b64encode(binary).decode()
            emit_json(f"scenes/assets/{name}.gltf", gltf.document(uri))
        assets[name] = {"kind": "mesh", "path": f"assets/{name}.gltf"}

    stats = {}
    island = Gltf("island")
    mesh = island_mesh()
    island.add_mesh(mesh)
    emit_gltf("island", island, external=True)
    stats["island_quads"] = mesh.quads()

    trunk, crown, origin = palm_voxels()
    base_y = round(ground(*PALM) / VOXEL) * VOXEL - VOXEL
    trunk_mesh = mesh_voxels(trunk, VOXEL, offset=(-0.5 * VOXEL, 0.0, -0.5 * VOXEL))
    palm_trunk = Gltf("palm-trunk")
    palm_trunk.add_mesh(trunk_mesh)
    emit_gltf("palm-trunk", palm_trunk)
    crown_mesh = mesh_voxels(crown, VOXEL, offset=(-0.5 * VOXEL, 0.0, -0.5 * VOXEL))
    palm_crown = Gltf("palm-crown")
    palm_crown.add_mesh(crown_mesh)
    emit_gltf("palm-crown", palm_crown)
    stats["palm_voxels"] = len(trunk) + len(crown)

    ship, lights = ship_voxels()
    ship_mesh = mesh_voxels(ship, 1.0)
    ship_gltf = Gltf("ship")
    ship_gltf.add_mesh(ship_mesh)
    emit_gltf("ship", ship_gltf, external=True)
    lights_gltf = Gltf("ship-lights")
    lights_gltf.add_mesh(mesh_voxels(lights, 1.0, inflate=0.03), alpha=0.999,
                         emissive={LAMP_WHITE: 60.0, LAMP_RED: 60.0, LAMP_GREEN: 60.0, WINDOW_LIT: 12.0})
    emit_gltf("ship-lights", lights_gltf)
    stats["ship_voxels"] = len(ship)

    sun = Gltf("sun")
    sun.add_mesh(mesh_voxels(disc(8, [0xfff6dc], rim=0xffd27a), 1.0), alpha=0.999,
                 emissive={0xfff6dc: 9.0, 0xffd27a: 6.0})
    emit_gltf("sun", sun)
    sun_low = Gltf("sun-low")
    sun_low.add_mesh(mesh_voxels(disc(8, [0xffb25e], rim=0xff7a3a), 1.0), alpha=0.999,
                     emissive={0xffb25e: 2.4, 0xff7a3a: 1.7})
    emit_gltf("sun-low", sun_low)
    moon = Gltf("moon")
    moon.add_mesh(mesh_voxels(disc(6, [0xe8ecf4, 0xb9c0cc], craters=((3, 3, 1.4), (7, 6, 1.2), (5, 9, 1.0))), 1.0),
                  alpha=0.999, emissive={0xe8ecf4: 2.2, 0xb9c0cc: 1.5})
    emit_gltf("moon", moon)
    for i in range(CLOUDS):
        cloud = Gltf(f"cloud-{i}")
        cloud.add_mesh(mesh_voxels(cloud_voxels(0xC10D + i * 97), 1.0), alpha=0.86)
        emit_gltf(f"cloud-{i}", cloud)
    for i in range(3):
        puff = Gltf(f"smoke-puff-{i}")
        puff.add_mesh(mesh_voxels(smoke_voxels(0x5300 + i * 31), 1.0), alpha=0.72)
        emit_gltf(f"smoke-puff-{i}", puff)

    files["scenes/assets/ocean-alpha.png"] = png_rgba(4, 4, lambda x, y: (255, 255, 255, 170))
    assets["ocean-alpha"] = {"kind": "image", "path": "assets/ocean-alpha.png"}
    files["scenes/assets/icons.png"] = icons_png()
    assets["icons"] = {"kind": "image", "path": "assets/icons.png"}

    # Game props.
    for i in range(3):
        log = Gltf(f"driftwood-{i}")
        log.add_mesh(mesh_voxels(driftwood_voxels(0xD21F + i * 7), VOXEL))
        emit_gltf(f"driftwood-{i}", log)
    nut = Gltf("coconut")
    nut.add_mesh(mesh_voxels(coconut_voxels(), 0.125, offset=(-0.1875, 0.0, -0.1875)))
    emit_gltf("coconut", nut)
    site = Gltf("raft-site")
    site.add_mesh(mesh_voxels(stake_voxels(), VOXEL))
    emit_gltf("raft-site", site)
    stages = raft_stages()
    for index, voxels in enumerate(stages, start=1):
        stage = Gltf(f"raft-stage-{index}")
        stage.add_mesh(mesh_voxels(voxels, VOXEL))
        emit_gltf(f"raft-stage-{index}", stage)
        ghost = Gltf(f"raft-ghost-{index}")
        ghost.add_mesh(mesh_voxels({p: GHOST for p in voxels}, VOXEL), alpha=0.3, emissive={GHOST: 0.35})
        emit_gltf(f"raft-ghost-{index}", ghost)
    solid, glass, windows, base = lighthouse_voxels()
    islet = heightfield_mesh(ISLET_RADIUS, LH_VOXEL, islet_height, islet_color)
    mesh_voxels(solid, LH_VOXEL, mesh=islet)
    lighthouse = Gltf("lighthouse")
    lighthouse.add_mesh(islet)
    emit_gltf("lighthouse", lighthouse, external=True)
    centred = (-0.5 * LH_VOXEL, -0.5 * LH_VOXEL, -0.5 * LH_VOXEL)
    for name, mesh, alpha, emissive in (
            ("lighthouse-glass", mesh_voxels(glass, LH_VOXEL), 0.35, None),
            ("lighthouse-windows", mesh_voxels(windows, LH_VOXEL, inflate=0.02), 0.999, {0xffcf7a: 8.0}),
            ("lighthouse-lamp", mesh_voxels(lamp_voxels(), LH_VOXEL, offset=centred), 0.999, {LAMP: 40.0}),
            ("lighthouse-beam", mesh_voxels(beam_voxels(), 1.2, offset=(0.0, -0.6, -0.6)), 0.22, {BEAM: 4.0})):
        gltf = Gltf(name)
        gltf.add_mesh(mesh, alpha=alpha, emissive=emissive)
        emit_gltf(name, gltf)
    beacon = (LIGHTHOUSE[0], round((base + 43.5) * LH_VOXEL, 3), LIGHTHOUSE[1])
    stats["lighthouse_voxels"] = len(solid)

    files["scenes/scripts/island.rhai"] = island_library(beacon).encode()
    assets["island-lib"] = {"kind": "script", "path": "scripts/island.rhai"}
    for name in ("player", "world", "game", "hud", "ship", "smoke", "cloud", "palm"):
        assets[f"{name}-script"] = {"kind": "script", "path": f"scripts/{name}.rhai"}

    # Objects; IDs sort into render order. Inactive pickups and unbuilt raft stages are
    # parked far below the island until the game places them.
    parked = (0.0, -500.0, 0.0)
    spawn_y = ground(*SPAWN)
    look = math.degrees(math.atan2(-SHIP_BEARING[0], -SHIP_BEARING[1]))
    objects.append({"id": "camera", "name": "Camera / the castaway's eyes",
                    "transform": tf((SPAWN[0], spawn_y + 1.62, SPAWN[1]), (2, look, 0)),
                    "camera": {"projection": "perspective", "vertical_fov_degrees": 75, "near": 0.15,
                               "far": 12000}})
    objects.append({"id": "game", "name": "Game / gathering, camp, raft, lighthouse",
                    "transform": tf((0, 0, 0)),
                    "blackboard": {
                        "wood_on": {"list": {"element": "bool", "capacity": WOOD,
                                             "values": [{"bool": i < len(START_WOOD)} for i in range(WOOD)]}},
                        "nut_on": {"list": {"element": "bool", "capacity": NUTS,
                                            "values": [{"bool": i < len(START_NUTS)} for i in range(NUTS)]}},
                        "palm_stock": number(10), "palm_next": number(0), "wood_next": number(45),
                        "tutorial": number(0), "tutorial_time": number(0), "look_time": number(0),
                        "cursor": flag(False), "warned": flag(False), "fade_applied": flag(False),
                        "gathered": number(0)},
                    "script_manager": {"scripts": [{"enabled": True, "script": "game-script"}]}})
    objects.append({"id": "player", "name": "Player / WASD, mouse, Shift, Space",
                    "transform": tf((SPAWN[0], spawn_y, SPAWN[1])),
                    "blackboard": {
                        "px": number(SPAWN[0]), "py": number(spawn_y), "pz": number(SPAWN[1]),
                        "vx": number(0), "vy": number(0), "vz": number(0),
                        "yaw": number(round(look, 3)), "pitch": number(2), "grounded": flag(True),
                        "dip": number(0), "bob": number(0), "bob_amount": number(0), "fov_now": number(75),
                        "aboard": flag(False), "raft_x": number(SITE[0]), "raft_z": number(SITE[2]),
                        "raft_yaw": number(SITE[3]), "raft_speed": number(0)},
                    "script_manager": {"scripts": [{"enabled": True, "script": "player-script"}]}})
    objects.append({"id": "world", "name": "World / day and night, 1 s = 8 game s",
                    "transform": tf((0, 0, 0)), "blackboard": {"low_sun": flag(False)},
                    "script_manager": {"scripts": [{"enabled": True, "script": "world-script"}]}})
    objects.append({"id": "a-island", "name": "Island / sand and seafloor",
                    "transform": tf((0, 0, 0)), "drawable": drawable({"asset": "island"})})
    objects.append({"id": "a-palm", "name": "Palm tree",
                    "transform": tf((PALM[0], base_y + VOXEL, PALM[1]))})
    objects.append({"id": "a-palm-trunk", "name": "Palm trunk", "parent": "a-palm", "transform": tf((0, 0, 0)),
                    "drawable": drawable({"asset": "palm-trunk"})})
    objects.append({"id": "a-palm-crown", "name": "Palm crown / sways, shakes", "parent": "a-palm",
                    "transform": tf((origin[0] * VOXEL, origin[1] * VOXEL, origin[2] * VOXEL)),
                    "drawable": drawable({"asset": "palm-crown"}, {"gi_static": False}),
                    "script_manager": {"scripts": [{"enabled": True, "script": "palm-script"}]}})
    site_pose = ((SITE[0], SITE[1], SITE[2]), (SITE[4], SITE[3], 0))
    objects.append({"id": "a-raft-site", "name": "Raft site / stakes on the south-west beach",
                    "transform": tf(*site_pose), "drawable": drawable({"asset": "raft-site"})})
    objects.append({"id": "raft", "name": "Raft / built in five stages", "transform": tf(*site_pose)})
    for index in range(1, len(STAGES) + 1):
        objects.append({"id": f"raft-stage-{index}", "name": f"Raft stage {index}", "parent": "raft",
                        "transform": tf(parked), "drawable": drawable({"asset": f"raft-stage-{index}"},
                                                                      {"gi_static": False})})
        objects.append({"id": f"raft-ghost-{index}", "name": f"Raft stage {index} preview", "parent": "raft",
                        "transform": tf((0, 0, 0) if index == 1 else parked),
                        "drawable": drawable({"asset": f"raft-ghost-{index}"}, {"gi_static": False})})
    rng = Lcg(0x3E5)
    for i in range(WOOD):
        if i < len(START_WOOD):
            x, z = START_WOOD[i]
            place = (x, round(ground(x, z) / VOXEL) * VOXEL, z)
        else:
            place = parked
        objects.append({"id": f"wood-{i}", "name": f"Driftwood {i + 1}",
                        "transform": tf(place, (0, rng.rnd(0, 360), 0)),
                        "drawable": drawable({"asset": f"driftwood-{i % 3}"}, {"gi_static": False})})
    for i in range(NUTS):
        if i < len(START_NUTS):
            x, z = START_NUTS[i]
            place = (x, round(ground(x, z) / VOXEL) * VOXEL, z)
        else:
            place = parked
        objects.append({"id": f"nut-{i}", "name": f"Coconut {i + 1}",
                        "transform": tf(place, (0, rng.rnd(0, 360), 0)),
                        "drawable": drawable({"asset": "coconut"}, {"gi_static": False})})
    start = (SHIP_BEARING[0] * SHIP_START, 0, SHIP_BEARING[1] * SHIP_START)
    objects.append({"id": "b-ship", "name": "Ship / steaming away", "transform": tf(start, (0, SHIP_YAW, 0)),
                    "drawable": drawable({"asset": "ship"}, {"gi_static": False}),
                    "script_manager": {"scripts": [{"enabled": True, "script": "ship-script"}]}})
    objects.append({"id": "ship-lights", "name": "Ship lights / night only", "parent": "b-ship",
                    "transform": tf((0, 0, 0)), "drawable": drawable({"asset": "ship-lights"}, {"gi_static": False})})
    lighthouse_at = (LIGHTHOUSE[0], 0, LIGHTHOUSE[1])
    objects.append({"id": "b-lighthouse", "name": "Lighthouse islet / the way home",
                    "transform": tf(lighthouse_at), "drawable": drawable({"asset": "lighthouse"})})
    objects.append({"id": "lighthouse-windows", "name": "Lighthouse cottage windows / night only",
                    "transform": tf(lighthouse_at),
                    "drawable": drawable({"asset": "lighthouse-windows"}, {"gi_static": False})})
    objects.append({"id": "c-ocean", "name": "Ocean / shader graph waves",
                    "transform": tf((0, -0.05, 0), scale=(20000, 0.1, 20000)),
                    "drawable": drawable("cube", {"gi_static": False, "roughness": 0.05, "metallic": 0.0},
                                         texture={"asset": "ocean-alpha"}),
                    "shader_graph": ocean_graph()})
    objects.append({"id": "lighthouse-glass", "name": "Lighthouse lantern glass", "transform": tf(lighthouse_at),
                    "drawable": drawable({"asset": "lighthouse-glass"}, {"gi_static": False})})
    objects.append({"id": "lighthouse-lamp", "name": "Lighthouse lamp / night only", "transform": tf(beacon),
                    "drawable": drawable({"asset": "lighthouse-lamp"}, {"gi_static": False})})
    objects.append({"id": "lighthouse-beam", "name": "Lighthouse beam / turns at night", "transform": tf(beacon),
                    "drawable": drawable({"asset": "lighthouse-beam"}, {"gi_static": False})})
    rng = Lcg(0xC10D5)
    for i in range(CLOUDS):
        x, y, z = rng.rnd(-1500, 1500), rng.rnd(170, 260), rng.rnd(-1300, 1300)
        objects.append({"id": f"cloud-{i}", "name": f"Cloud {i + 1}",
                        "transform": tf((x, y, z), (0, rng.rnd(0, 360), 0), (9, 7, 9)),
                        "drawable": drawable({"asset": f"cloud-{i}"}, {"gi_static": False}),
                        "blackboard": {"speed": number(round(rng.rnd(2.5, 4.5), 3))},
                        "script_manager": {"scripts": [{"enabled": True, "script": "cloud-script"}]}})
    objects.append({"id": "moon", "name": "Moon", "transform": tf((0, -600, 0), scale=(2, 2, 2)),
                    "drawable": drawable({"asset": "moon"}, {"gi_static": False})})
    objects.append({"id": "smoke", "name": "Ship smoke / voxel puffs", "transform": tf((0, 0, 0)),
                    "script_manager": {"scripts": [{"enabled": True, "script": "smoke-script"}]}})
    for i in range(SMOKE):
        objects.append({"id": f"smoke-{i:02d}", "name": f"Smoke puff {i + 1}",
                        "transform": tf((start[0], 20, start[2]), scale=(0.001, 0.001, 0.001)),
                        "drawable": drawable({"asset": f"smoke-puff-{i % 3}"}, {"gi_static": False},
                                             color=(0.2, 0.2, 0.2))})
    objects.append({"id": "sun", "name": "Sun", "transform": tf((0, 600, 0), scale=(2, 2, 2)),
                    "drawable": drawable({"asset": "sun"}, {"gi_static": False})})
    objects += ui_objects()

    theta = (START_HOUR - 6) / 12 * math.pi
    sun_direction = [math.cos(theta), math.sin(theta) * math.cos(0.42), math.sin(theta) * math.sin(0.42)]
    inventory = {item: number(0) for item in ITEMS}
    scene = {
        "version": 1,
        "name": "Sand Island",
        "views": {"3d": "camera"},
        "assets": dict(sorted(assets.items())),
        "blackboard": {
            "mode": text("title"), "control": flag(False), "camp_open": flag(False), "menu_open": flag(False),
            **inventory, "energy": number(85), "raft_stage": number(0), "lighthouse_seen": flag(False),
            "prompt": text(""), "pickup": text(""), "pickup_time": number(-100),
            "toast": text(""), "toast_time": number(-100), "fade_kind": text(""), "fade_start": number(-100),
            "launch_start": number(-100), "launch_armed": number(-100), "start_time": number(0),
            "shake_time": number(-100),
            "time_of_day": number(START_HOUR), "clock": number(START_HOUR * 3600), "clock_fraction": number(0),
            "day": number(1), "world_time": number(0), "daylight": number(1),
            "sensitivity": number(1.0), "invert_y": flag(False), "bobbing": flag(True), "fov": number(75),
            "view_distance": number(2), "shadows": flag(True), "clouds": flag(True), "bloom": flag(True),
        },
        "lighting": {"shadows": True, "shadow_resolution": 2048, "shadow_bias": 0.004,
                     "shadow_normal_bias": 0.02, "shadow_distance": 32,
                     "sun_direction": [round(v, 5) for v in sun_direction],
                     "sun_color": [1.0, 0.95, 0.86], "sun_intensity": SUN_INTENSITY,
                     "ambient_color": [0.86, 0.93, 1.0], "ambient_intensity": 0.08},
        "environment": {**SKY_DAY, "intensity": SKY_INTENSITY, "star_intensity": 0, "background": True},
        "fog": {"enabled": True, "color": [round(c * SKY_INTENSITY, 5) for c in SKY_DAY["horizon"]],
                "distance_density": 0.0006, "start_distance": 180, "height_density": 0, "base_height": 0,
                "height_falloff": 1},
        "display": {"exposure_ev": 0, "tone_mapping": True, "tone_mapper": TONE_MAPPER,
                    "bloom": {"enabled": True, "intensity": BLOOM, "threshold": 1.0, "scatter": 0.7,
                              "anamorphic": 0},
                    "ambient_occlusion": {"enabled": True, "intensity": 0.8, "radius": 0.6, "bias": 0.025},
                    "temporal_aa": {"enabled": True}},
        "objects": objects,
    }
    emit_json("scenes/island.json", scene)
    emit_json("bozzard.project.json", {"version": 1, "name": "Sand Island",
                                       "start_scene": "scenes/island.json", "view": "3d"})
    stats["objects"] = len(objects)
    stats["bytes"] = sum(len(v) for v in files.values())
    return files, stats


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--check", action="store_true", help="verify files instead of writing")
    args = parser.parse_args(argv)
    files, stats = build()
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
    print(json.dumps(stats))
    if stale:
        print("stale: " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
