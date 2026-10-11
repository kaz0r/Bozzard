#!/usr/bin/env python3
"""Generate examples/sand-island: a first-person voxel castaway island for Bozzard.

The player is stranded on a small sand island with one palm tree and an endless
ocean. Far out to sea a steamship sails away, trailing voxel smoke. A day/night
cycle runs at eight game seconds per real second, and a HUD shows the clock and
heading and opens a settings menu (mouse sensitivity, field of view, graphics).

Everything you can see is voxels except the ocean, which is a shaded plane:

* the island and its seafloor -> island.gltf, a greedy-meshed heightfield of
  0.25 m voxels with one primitive per colour;
* the palm trunk and its swaying crown -> palm-trunk.gltf, palm-crown.gltf;
* the steamship and its night lights -> ship.gltf, ship-lights.gltf (1 m voxels);
* smoke puffs, clouds, the sun and the moon -> small translucent voxel glTFs
  that cast no shadows, so the sun's shadow map stays on the island.

The player walks on the same analytic height function the island was voxelized
from (scripts/island.rhai), so movement over voxel steps stays smooth.

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


def mesh_voxels(voxels, size, offset=(0.0, 0.0, 0.0), mesh=None):
    """Greedy-mesh exposed faces of {(x, y, z): colour}; a voxel spans `size` metres."""
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
                        corners.append(tuple(c[i] * size + offset[i] for i in range(3)))
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


def island_mesh():
    n = int(math.ceil(GRID_RADIUS / VOXEL))
    top, color = {}, {}
    for i in range(-n, n):
        for k in range(-n, n):
            x, z = (i + 0.5) * VOXEL, (k + 0.5) * VOXEL
            if math.hypot(x, z) > GRID_RADIUS:
                continue
            h = ground(x, z)
            t = round(h / VOXEL)
            top[(i, k)] = t
            color[(i, k)] = sand_color(x, z, t * VOXEL, i, k)
    mesh = Mesh()
    # Tops: merge equal height and colour.
    for u, v, du, dv, (t, c) in greedy_rectangles({p: (top[p], color[p]) for p in top}):
        y = t * VOXEL
        corners = [(u * VOXEL, y, v * VOXEL), ((u + du) * VOXEL, y, v * VOXEL),
                   ((u + du) * VOXEL, y, (v + dv) * VOXEL), (u * VOXEL, y, (v + dv) * VOXEL)]
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
            runs.setdefault(line, {})[(along, 0)] = (t, nb, color[(i, k)])
        for line, cells in runs.items():
            for along, _, length, _, (t, nb, c) in greedy_rectangles(cells):
                edge = (line + (1 if (di + dk) > 0 else 0)) * VOXEL
                a0, a1 = along * VOXEL, (along + length) * VOXEL
                y0, y1 = nb * VOXEL, t * VOXEL
                if di:
                    corners = [(edge, y0, a0), (edge, y0, a1), (edge, y1, a1), (edge, y1, a0)]
                else:
                    corners = [(a0, y0, edge), (a1, y0, edge), (a1, y1, edge), (a0, y1, edge)]
                mesh.add(c, corners, (di, 0, dk))
    return mesh


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


def fallen_coconuts():
    """Two coconuts on the sand next to the palm, world voxel coordinates."""
    voxels = {}
    for (ox, oz, color) in ((PALM[0] + 1.6, PALM[1] - 1.1, COCONUT[1]), (PALM[0] - 1.3, PALM[1] + 0.9, COCONUT[2])):
        i, k = math.floor(ox / VOXEL), math.floor(oz / VOXEL)
        base = round(ground(ox, oz) / VOXEL)
        for x in range(2):
            for y in range(2):
                for z in range(2):
                    voxels[(i + x, base + y, k + z)] = color
    return voxels


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
def island_library():
    return f"""// Generated by tools/gen_island.py: the island's shape and the ship's course,
// shared by every script. tools/gen_island.py voxelizes the same functions.

const VOXEL = {VOXEL};
const SWIM_DEPTH = {SWIM_DEPTH};
const BOUNDARY = {BOUNDARY};
const PALM = [{PALM[0]}, {PALM[1]}];

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

fn palm() {{ global::PALM }}
fn swim_depth() {{ global::SWIM_DEPTH }}
fn boundary() {{ global::BOUNDARY }}

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


PLAYER_SCRIPT = """// Generated by tools/gen_island.py: first-person movement on the island.
//
// The body walks on island::ground, the analytic surface the voxels were cut
// from, so stepping over 0.25 m voxel terraces is a smooth slope rather than a
// stair. Speed eases in and out, the eye dips on landing and bobs while walking,
// the sea slows wading and floats a swimmer, and the current keeps the castaway
// near the island. Settings come from the scene blackboard the menu writes.
import "island-lib" as island;

const EYE = 1.62;
const WALK = 3.4;
const SPRINT = 6.0;
const SWIM = 1.9;
const GRAVITY = 20.0;
const JUMP = 6.2;
const RADIUS = 0.32;
const TRUNK = 0.34;

fn approach(current, target, rate, dt) {
    current + (target - current) * (1.0 - exp(-rate * dt))
}

fn on_start(me) {
    lock_cursor();
    set_camera_fov("camera", get_scene_variable("fov"));
}

fn on_update(me, dt) {
    let menu = get_scene_variable("menu_open");
    let p = [get_object_variable("px"), get_object_variable("py"), get_object_variable("pz")];
    let v = [get_object_variable("vx"), get_object_variable("vy"), get_object_variable("vz")];
    let yaw = get_object_variable("yaw");
    let pitch = get_object_variable("pitch");
    let grounded = get_object_variable("grounded");

    // Look.
    if !menu {
        let turn = get_scene_variable("sensitivity") * 0.1;
        let invert = if get_scene_variable("invert_y") { -1.0 } else { 1.0 };
        yaw -= mouse_x() * turn;
        pitch = clamp(pitch - mouse_y() * turn * invert, -88.0, 88.0);
    }

    // Wanted horizontal velocity along the view's flattened axes.
    let mx = 0.0;
    let my = 0.0;
    if !menu {
        mx = move_x();
        my = move_y();
    }
    let amount = sqrt(mx * mx + my * my);
    if amount > 1.0 {
        mx /= amount;
        my /= amount;
    }
    let a = to_radians(yaw);
    let forward = [-sin(a), -cos(a)];
    let right = [cos(a), -sin(a)];
    let floor_y = island::ground(p[0], p[2]);
    let depth = -floor_y;
    let swimming = depth > island::swim_depth();
    let sprinting = !menu && input_held("Shift") && my > 0.1;
    let speed = if swimming {
        if sprinting { SWIM * 1.45 } else { SWIM }
    } else {
        let wade = 1.0 - 0.45 * clamp(depth / 1.1, 0.0, 1.0);
        (if sprinting { SPRINT } else { WALK }) * wade
    };
    let want = [(forward[0] * my + right[0] * mx) * speed, (forward[1] * my + right[1] * mx) * speed];
    let grip = if swimming { 3.5 } else if grounded { 11.0 } else { 2.2 };
    v[0] = approach(v[0], want[0], grip, dt);
    v[2] = approach(v[2], want[1], grip, dt);
    p[0] += v[0] * dt;
    p[2] += v[2] * dt;

    // The palm trunk is a post to walk around.
    let palm = island::palm();
    let tx = p[0] - palm[0];
    let tz = p[2] - palm[1];
    let reach = RADIUS + TRUNK;
    let d = sqrt(tx * tx + tz * tz);
    if d < reach && d > 0.0001 {
        let nx = tx / d;
        let nz = tz / d;
        p[0] = palm[0] + nx * reach;
        p[2] = palm[1] + nz * reach;
        let into = v[0] * nx + v[2] * nz;
        if into < 0.0 {
            v[0] -= into * nx;
            v[2] -= into * nz;
        }
    }

    // The current turns a swimmer back before the open sea.
    let r = sqrt(p[0] * p[0] + p[2] * p[2]);
    let limit = island::boundary();
    if r > limit {
        p[0] *= limit / r;
        p[2] *= limit / r;
        let out = (v[0] * p[0] + v[2] * p[2]) / limit;
        if out > 0.0 {
            v[0] -= out * p[0] / limit;
            v[2] -= out * p[2] / limit;
        }
        if elapsed_time() - get_scene_variable("toast_time") > 6.0 {
            set_scene_variable("toast", "The current is too strong. You can't swim any further.");
            set_scene_variable("toast_time", elapsed_time());
        }
    }

    // Vertical: feet on the sand, a jump, or floating at the surface.
    floor_y = island::ground(p[0], p[2]);
    depth = -floor_y;
    swimming = depth > island::swim_depth();
    let dip = get_object_variable("dip");
    if swimming {
        let float_y = -island::swim_depth() + 0.08 * sin(elapsed_time() * 1.7);
        p[1] = approach(p[1], max(float_y, floor_y), 4.0, dt);
        v[1] = 0.0;
        grounded = false;
    } else {
        let was_grounded = grounded;
        v[1] -= GRAVITY * dt;
        if grounded && !menu && input_pressed("jump") {
            v[1] = JUMP;
        }
        p[1] += v[1] * dt;
        // Follow the sand down gentle slopes instead of skipping off them.
        if was_grounded && v[1] <= 0.0 && p[1] - floor_y < 0.3 {
            p[1] = floor_y;
        }
        if p[1] <= floor_y {
            if !was_grounded && v[1] < -3.0 {
                dip = max(-0.16, v[1] * 0.018);
            }
            p[1] = floor_y;
            v[1] = 0.0;
            grounded = true;
        } else {
            grounded = false;
        }
    }
    dip = approach(dip, 0.0, 7.0, dt);

    // The eye: head bob while walking, a bigger field of view while sprinting.
    let horizontal = sqrt(v[0] * v[0] + v[2] * v[2]);
    let phase = get_object_variable("bob");
    let bob_on = get_scene_variable("bobbing");
    let bob_amount = approach(get_object_variable("bob_amount"),
        if bob_on && grounded && horizontal > 0.4 { min(horizontal / WALK, 1.4) } else { 0.0 }, 8.0, dt);
    phase += dt * horizontal * 1.9;
    let bob_y = sin(phase * 2.0) * 0.032 * bob_amount;
    let bob_x = cos(phase) * 0.022 * bob_amount;
    let eye = if swimming { 0.3 + island::swim_depth() } else { EYE };
    let fov = get_scene_variable("fov");
    let sprint_fov = approach(get_object_variable("fov_now"),
        fov + if sprinting && horizontal > WALK { 7.0 } else { 0.0 }, 6.0, dt);
    if abs(sprint_fov - get_object_variable("fov_now")) > 0.01 {
        set_camera_fov("camera", sprint_fov);
    }

    set_object_variable("px", p[0]);
    set_object_variable("py", p[1]);
    set_object_variable("pz", p[2]);
    set_object_variable("vx", v[0]);
    set_object_variable("vy", v[1]);
    set_object_variable("vz", v[2]);
    set_object_variable("yaw", yaw);
    set_object_variable("pitch", pitch);
    set_object_variable("grounded", grounded);
    set_object_variable("dip", dip);
    set_object_variable("bob", phase);
    set_object_variable("bob_amount", bob_amount);
    set_object_variable("fov_now", sprint_fov);
    set_position(me, p);
    set_position("camera", [p[0] + right[0] * bob_x, p[1] + eye + bob_y + dip, p[2] + right[1] * bob_x]);
    set_rotation("camera", [pitch, yaw, 0.0]);
}
"""


SKY_DAY = {"zenith": [0.07, 0.32, 0.95], "horizon": [0.62, 0.84, 1.0], "ground": linear(0x34607a)}


def world_script():
    night = {"zenith": [0.0032, 0.0055, 0.018], "horizon": [0.011, 0.017, 0.042], "ground": [0.006, 0.01, 0.024]}
    dusk = {"zenith": [0.05, 0.08, 0.2], "horizon": [0.92, 0.42, 0.19], "ground": [0.16, 0.11, 0.13]}
    day = SKY_DAY

    def rgb(c):
        return "[" + ", ".join(f"{v:.5f}" for v in c) + "]"
    return f"""// Generated by tools/gen_island.py: the day/night cycle.
//
// One real second is {TIME_SCALE:g} game seconds (a day lasts three real hours);
// hold T to speed time up. The sun rises in the east (+X), crosses the southern
// sky and sets in the west; the moon follows on the opposite side. The scene's
// one sun light follows whichever of the two is up, and the sky, fog, ambient
// light and stars blend between night, dusk and day by the sun's height.
import "island-lib" as island;

const DISTANCE = 520.0;
const TILT = 0.42;

fn mix(a, b, t) {{
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}}

// Turn a disc (its face toward local +Z) to face the camera from `distance` along d.
fn face(id, eye, d, distance) {{
    set_position(id, [eye[0] + d[0] * distance, eye[1] + d[1] * distance, eye[2] + d[2] * distance]);
    set_rotation(id, [to_degrees(asin(clamp(d[1], -1.0, 1.0))), to_degrees(atan2(-d[0], -d[2])), 0.0]);
}}

fn on_update(me, dt) {{
    let menu = get_scene_variable("menu_open");
    let rate = if !menu && input_held("T") {{ 720.0 }} else {{ {TIME_SCALE:.1f} }};
    // Game seconds since midnight are kept whole plus a fraction: adding a sixtieth of
    // a second to a float that counts a whole day would round the clock off.
    let clock = get_scene_variable("clock");
    let fraction = get_scene_variable("clock_fraction");
    if !menu {{
        set_scene_variable("world_time", get_scene_variable("world_time") + dt);
        fraction += dt * rate;
        let whole = floor(fraction);
        fraction -= whole;
        clock += whole;
        if clock >= 86400.0 {{
            clock -= 86400.0;
            set_scene_variable("day", get_scene_variable("day") + 1.0);
        }}
        set_scene_variable("clock", clock);
        set_scene_variable("clock_fraction", fraction);
    }}
    let hours = (clock + fraction) / 3600.0;
    set_scene_variable("time_of_day", hours);

    let theta = (hours - 6.0) / 12.0 * 3.14159265;
    let sun = [cos(theta), sin(theta) * cos(TILT), sin(theta) * sin(TILT)];
    let moon = [-sun[0], -sun[1], sun[2] * 0.6 + 0.25];
    let e = sun[1];

    // Sky: night -> dusk -> day by the sun's elevation.
    let night_zenith = {rgb(night['zenith'])};
    let night_horizon = {rgb(night['horizon'])};
    let night_ground = {rgb(night['ground'])};
    let dusk_zenith = {rgb(dusk['zenith'])};
    let dusk_horizon = {rgb(dusk['horizon'])};
    let dusk_ground = {rgb(dusk['ground'])};
    let day_zenith = {rgb(day['zenith'])};
    let day_horizon = {rgb(day['horizon'])};
    let day_ground = {rgb(day['ground'])};
    let zenith = [];
    let horizon = [];
    let ground = [];
    if e < 0.0 {{
        let t = island::smooth(-0.24, 0.0, e);
        zenith = mix(night_zenith, dusk_zenith, t);
        horizon = mix(night_horizon, dusk_horizon, t);
        ground = mix(night_ground, dusk_ground, t);
    }} else {{
        let t = island::smooth(0.0, 0.32, e);
        zenith = mix(dusk_zenith, day_zenith, t);
        horizon = mix(dusk_horizon, day_horizon, t);
        ground = mix(dusk_ground, day_ground, t);
    }}
    set_environment(zenith, horizon, ground, {SKY_INTENSITY});
    let distance = get_scene_variable("view_distance");
    let density = if distance < 0.5 {{ 0.0026 }} else if distance < 1.5 {{ 0.0012 }} else {{ 0.0006 }};
    set_fog(mix([0.0, 0.0, 0.0], horizon, {SKY_INTENSITY}), density);
    set_star_intensity(0.8 * (1.0 - island::smooth(-0.2, -0.02, e)));

    // One directional light: the sun by day, the moon by night, dark in between.
    let daylight = island::smooth(-0.02, 0.3, e);
    set_scene_variable("daylight", daylight);
    if e > -0.03 {{
        set_sun_direction(sun);
        set_sun_light(mix([1.0, 0.42, 0.16], [1.0, 0.96, 0.88], island::smooth(0.0, 0.4, e)), {SUN_INTENSITY} * daylight);
    }} else {{
        set_sun_direction(moon);
        set_sun_light([0.55, 0.66, 1.0], 0.32 * island::smooth(-0.02, 0.3, moon[1]));
    }}
    set_ambient_light(mix([0.42, 0.52, 0.95], [0.86, 0.93, 1.0], daylight), 0.03 + 0.06 * daylight);

    // The voxel sun and moon sit far out along their directions, facing the eye.
    let eye = get_position("camera");
    face("sun", eye, sun, DISTANCE);
    face("moon", eye, moon, DISTANCE);
    set_visible("sun", e > -0.12);
    let low = e < 0.1;
    if low != get_object_variable("low_sun") {{
        set_object_variable("low_sun", low);
        set_mesh("sun", if low {{ "sun-low" }} else {{ "sun" }});
    }}
    set_visible("moon", moon[1] > -0.12);
    set_visible("ship-lights", e < 0.06);
}}
"""


SHIP_SCRIPT = """// Generated by tools/gen_island.py: the ship steams away, rolling gently.
import "island-lib" as island;

fn on_update(me, dt) {
    let t = get_scene_variable("world_time");
    let p = island::ship_position(t);
    set_position(me, [p[0], p[1] + 0.25 * sin(t * 0.7), p[2]]);
    set_rotation(me, [0.6 * sin(t * 0.53), island::ship_yaw(), 1.3 * sin(t * 0.71)]);
}
"""


def smoke_script(count, life):
    return f"""// Generated by tools/gen_island.py: voxel smoke from the ship's funnel.
//
// Puff i was released at the last multiple of the release interval; its age
// alone decides where the wind has carried it, how far it has risen, how big and
// how pale it is, so the trail needs no stored state and stays behind the ship.
import "island-lib" as island;

const COUNT = {count};
const LIFE = {life:.1f};
const FUNNEL = [{FUNNEL_TOP[0]}, {FUNNEL_TOP[1]}, {FUNNEL_TOP[2]}];

fn on_update(me, dt) {{
    let t = get_scene_variable("world_time");
    let light = 0.25 + 0.75 * get_scene_variable("daylight");
    let interval = LIFE / COUNT.to_float();
    for i in 0..COUNT {{
        let offset = i.to_float() * interval;
        let cycle = floor((t - offset) / LIFE);
        let born = cycle * LIFE + offset;
        let age = t - born;
        let start = island::ship_point(born, FUNNEL);
        let rise = 9.0 * (1.0 - exp(-age * 0.45)) + age * 0.35;
        let jitter = sin(cycle * 12.9898 + i.to_float() * 78.233) * 2.0;
        let x = start[0] + age * 1.7 + jitter;
        let y = start[1] + rise;
        let z = start[2] + age * 0.8 - jitter * 0.5;
        let grow = island::smooth(0.0, 0.8, age) * (1.0 - island::smooth(LIFE * 0.45, LIFE, age));
        let size = (0.7 + age * 0.17) * grow + 0.001;
        let shade = (0.05 + 0.22 * island::smooth(0.0, LIFE * 0.7, age)) * light;
        let id = if i < 10 {{ `smoke-0${{i}}` }} else {{ `smoke-${{i}}` }};
        set_position(id, [x, y, z]);
        set_scale(id, [size, size, size]);
        set_rotation(id, [0.0, age * 9.0 + i.to_float() * 37.0, 0.0]);
        set_color(id, [shade, shade, shade * 1.03]);
    }}
}}
"""


CLOUD_SCRIPT = """// Generated by tools/gen_island.py: clouds drift with the trade wind and wrap around.
fn on_update(me, dt) {
    if get_scene_variable("menu_open") {
        return;
    }
    let p = get_position(me);
    let x = p[0] + get_object_variable("speed") * dt;
    if x > 1800.0 {
        x = -1800.0;
    }
    set_position(me, [x, p[1], p[2] + get_object_variable("speed") * 0.3 * dt]);
}
"""


PALM_SCRIPT = """// Generated by tools/gen_island.py: the crown sways in the breeze.
fn on_update(me, dt) {
    let t = get_scene_variable("world_time");
    set_rotation(me, [1.5 * sin(t * 0.9) + 0.4 * sin(t * 2.3 + 1.0), 0.6 * sin(t * 0.37), 1.1 * cos(t * 0.7)]);
}
"""


HUD_SCRIPT = """// Generated by tools/gen_island.py: the HUD and the settings menu.
//
// Esc (or the Settings button) opens the menu; game time, the ship and the
// player pause while it is open. Sliders and toggles are read every tick and
// applied when they change; quality, VSync and window mode go through the
// engine's player settings, so they are also saved for the next run.
import "island-lib" as island;

const QUALITY = ["low", "medium", "high"];
const WINDOW = ["windowed", "borderless", "fullscreen"];
const DISTANCE = ["Near", "Medium", "Far"];
const HEADINGS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

fn title(text) {
    text.sub_string(0, 1).to_upper() + text.sub_string(1)
}

fn set_menu(open) {
    set_scene_variable("menu_open", open);
    set_ui_visible("ui-menu", open);
    set_ui_visible("ui-settings-button", !open);
    clear_ui_focus();
    if open { unlock_cursor(); } else { lock_cursor(); }
}

fn cycle(list, current) {
    let i = list.index_of(current);
    list[(i + 1) % list.len()]
}

fn on_start(me) {
    set_ui_value("ui-opt-vsync", if get_vsync_setting() { 1.0 } else { 0.0 });
    set_ui_text("ui-opt-quality", `Quality: ${title(get_quality_setting())}`);
    set_ui_text("ui-opt-window", `Window: ${title(get_window_mode_setting())}`);
    set_scene_variable("toast", "You wake up on a tiny island. Far out, a ship is steaming away...");
    set_scene_variable("toast_time", 0.5);
}

fn on_update(me, dt) {
    let open = get_scene_variable("menu_open");
    for event in ui_events() {
        if event.kind != "activate" {
            continue;
        }
        switch event.target {
            "ui-settings-button" => { set_menu(true); open = true; }
            "ui-resume" => { set_menu(false); open = false; }
            "ui-quit" => quit_game(),
            "ui-opt-quality" => {
                let next = cycle(QUALITY, get_quality_setting());
                set_quality_setting(next);
                apply_settings();
                save_settings();
                set_ui_text("ui-opt-quality", `Quality: ${title(next)}`);
            }
            "ui-opt-window" => {
                let next = cycle(WINDOW, get_window_mode_setting());
                set_window_mode_setting(next);
                apply_settings();
                save_settings();
                set_ui_text("ui-opt-window", `Window: ${title(next)}`);
            }
        }
    }

    // Settings: apply whatever changed since the last tick.
    let sensitivity = get_ui_value("ui-opt-sensitivity");
    if sensitivity != get_scene_variable("sensitivity") {
        set_scene_variable("sensitivity", sensitivity);
    }
    set_ui_text("ui-opt-sensitivity", `Mouse sensitivity   ${island::fixed(sensitivity, 2)}x`);
    let fov = round(get_ui_value("ui-opt-fov"));
    if fov != get_scene_variable("fov") {
        set_scene_variable("fov", fov);
    }
    set_ui_text("ui-opt-fov", `Field of view   ${fov.to_int()}°`);
    set_scene_variable("invert_y", get_ui_value("ui-opt-invert") > 0.5);
    set_scene_variable("bobbing", get_ui_value("ui-opt-bob") > 0.5);
    let distance = round(get_ui_value("ui-opt-distance"));
    set_scene_variable("view_distance", distance);
    set_ui_text("ui-opt-distance", `View distance   ${DISTANCE[distance.to_int()]}`);
    let shadows = get_ui_value("ui-opt-shadows") > 0.5;
    if shadows != get_scene_variable("shadows") {
        set_scene_variable("shadows", shadows);
        set_sun_shadows(shadows);
    }
    let clouds = get_ui_value("ui-opt-clouds") > 0.5;
    if clouds != get_scene_variable("clouds") {
        set_scene_variable("clouds", clouds);
        for i in 0..CLOUDS {
            set_visible(`cloud-${i}`, clouds);
        }
    }
    let bloom = get_ui_value("ui-opt-bloom") > 0.5;
    if bloom != get_scene_variable("bloom") {
        set_scene_variable("bloom", bloom);
        set_bloom_intensity(if bloom { BLOOM } else { 0.0 });
    }
    let vsync = get_ui_value("ui-opt-vsync") > 0.5;
    if vsync != get_vsync_setting() {
        set_vsync_setting(vsync);
        apply_settings();
        save_settings();
    }
    let show_fps = get_ui_value("ui-opt-fps") > 0.5;
    set_ui_visible("ui-fps", show_fps);
    if show_fps {
        let stats = render_stats();
        if stats.available && stats.rate_ready {
            set_ui_text("ui-fps", `${round(stats.fps).to_int()} FPS   ${island::fixed(stats.frame_ms, 1)} ms`);
        }
    }

    // Clock and compass.
    let hours = get_scene_variable("time_of_day");
    let day = get_scene_variable("day").to_int();
    let sun_up = hours >= 6.0 && hours < 18.0;
    set_ui_text("ui-clock", `Day ${day}   ${island::hours_text(hours)}   ${if sun_up { "day" } else { "night" }}`);
    let yaw = get_object_variable("player", "yaw");
    let heading = (360.0 - (yaw % 360.0) + 360.0) % 360.0;
    let index = (round(heading / 45.0).to_int()) % 8;
    set_ui_text("ui-compass", `${HEADINGS[index]}   ${round(heading).to_int() % 360}°`);

    // Toast messages fade out after a few seconds; the controls hint after the intro.
    let since = elapsed_time() - get_scene_variable("toast_time");
    let toast = get_scene_variable("toast");
    let visible = !open && toast != "" && since >= 0.0 && since < 6.0;
    set_ui_visible("ui-toast", visible);
    if visible {
        set_ui_text("ui-toast", toast);
        set_ui_opacity("ui-toast", min(1.0, min(since * 3.0, (6.0 - since) * 1.2)));
    }
    let hint = elapsed_time();
    set_ui_visible("ui-hint", hint < 20.0 && !open);
    set_ui_opacity("ui-hint", clamp((20.0 - hint) * 0.5, 0.0, 1.0));
}
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
    objs = [{"id": "ui", "name": "HUD / settings menu (Esc)", "transform": tf((0, 0, 0)),
             "ui_canvas": {"layer": "3d", "reference": [1280, 720], "scaling": "fit", "order": 10},
             "script_manager": {"scripts": [{"enabled": True, "script": "hud-script"}]}}]
    hud = [
        widget("ui-clock", "Clock", "ui", order=1, text="Day 1   08:00", font_size=22,
               anchors=anchors((0, 0), (0, 0), (0, 0), (24, 18), (420, 34))),
        widget("ui-compass", "Compass", "ui", order=2, text="N", font_size=20, text_alignment="center",
               anchors=anchors((0.5, 0), (0.5, 0), (0.5, 0), (0, 18), (300, 34))),
        widget("ui-crosshair", "Crosshair", "ui", kind="panel", order=3, background=[1.0, 1.0, 1.0, 0.8],
               anchors=anchors((0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0, 0), (4, 4))),
        widget("ui-settings-button", "Settings button", "ui", kind="button", order=4, text="Settings  (Esc)",
               font_size=16, text_alignment="center", background=[0.03, 0.05, 0.08, 0.55],
               padding=[8, 8, 8, 8], shortcuts=["Escape"], accessible_name="Open settings",
               anchors=anchors((1, 0), (1, 0), (1, 0), (-24, 16), (170, 38))),
        widget("ui-fps", "FPS", "ui", order=5, text="", font_size=16, text_alignment="right", visible=False,
               text_color=DIM, anchors=anchors((1, 0), (1, 0), (1, 0), (-24, 60), (240, 26))),
        widget("ui-hint", "Controls hint", "ui", order=6, font_size=17, text_alignment="center",
               text="WASD move   Shift sprint   Space jump   Hold T to speed up time   Esc settings",
               background=[0.0, 0.0, 0.0, 0.38], padding=[10, 7, 10, 7],
               anchors=anchors((0.5, 1), (0.5, 1), (0.5, 1), (0, -24), (860, 36))),
        widget("ui-toast", "Message", "ui", order=7, font_size=20, text_alignment="center", visible=False,
               background=[0.0, 0.0, 0.0, 0.45], padding=[12, 9, 12, 9],
               anchors=anchors((0.5, 0.7), (0.5, 0.7), (0.5, 0.5), (0, 0), (760, 42))),
    ]
    objs += hud
    menu = [
        widget("ui-menu", "Settings menu backdrop", "ui", kind="panel", order=20, visible=False,
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
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 494))),
        widget("ui-col-controls", "Controls column", "ui-menu-columns", kind="panel", order=0, layout="column",
               gap=10, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 494))),
        widget("ui-col-graphics", "Graphics column", "ui-menu-columns", kind="panel", order=1, layout="column",
               gap=10, anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (404, 494))),
        widget("ui-menu-buttons", "Buttons", "ui-menu-panel", kind="panel", order=3, layout="row", gap=16,
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (844, 46))),
        widget("ui-resume", "Resume", "ui-menu-buttons", kind="button", order=0, text="Resume  (Esc)",
               font_size=18, text_alignment="center", background=[0.13, 0.42, 0.33, 1.0], padding=[10, 10, 10, 10],
               shortcuts=["Escape"], anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (220, 46))),
        widget("ui-quit", "Quit", "ui-menu-buttons", kind="button", order=1, text="Quit game", font_size=18,
               text_alignment="center", background=[0.35, 0.12, 0.12, 1.0], padding=[10, 10, 10, 10],
               anchors=anchors((0, 0), (0, 0), (0, 0), (0, 0), (180, 46))),
    ]

    def header(object_id, parent, text, order):
        return widget(object_id, text, parent, order=order, text=text, font_size=15, text_color=DIM,
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
    menu += [
        header("ui-head-controls", "ui-col-controls", "MOUSE AND VIEW", 0),
        slider("ui-opt-sensitivity", "ui-col-controls", "Mouse sensitivity", 1, 1.0, 0.1, 3.0, 0.05),
        toggle("ui-opt-invert", "ui-col-controls", "Invert mouse Y", 2, False),
        slider("ui-opt-fov", "ui-col-controls", "Field of view", 3, 75.0, 60.0, 110.0, 1.0),
        toggle("ui-opt-bob", "ui-col-controls", "Head bobbing", 4, True),
        widget("ui-help", "Help", "ui-col-controls", order=5, font_size=15, text_color=DIM,
               text="WASD walk, Shift sprint, Space jump.\nHold T to speed up time.\n"
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
    return objs + menu


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


CLOUDS = 16
SMOKE = 40
SMOKE_LIFE = 30.0
BLOOM = 0.14


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
    mesh_voxels(fallen_coconuts(), VOXEL, mesh=mesh)
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
    lights_gltf.add_mesh(mesh_voxels(lights, 1.02, offset=(-0.01, -0.01, -0.01)), alpha=0.999,
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

    scripts = {
        "island": island_library(),
        "player": PLAYER_SCRIPT,
        "world": world_script(),
        "ship": SHIP_SCRIPT,
        "smoke": smoke_script(SMOKE, SMOKE_LIFE),
        "cloud": CLOUD_SCRIPT,
        "palm": PALM_SCRIPT,
        "hud": HUD_SCRIPT.replace("CLOUDS", str(CLOUDS)).replace("BLOOM", f"{BLOOM}"),
    }
    for name, source in scripts.items():
        files[f"scenes/scripts/{name}.rhai"] = source.encode()
        assets["island-lib" if name == "island" else f"{name}-script"] = {"kind": "script",
                                                                          "path": f"scripts/{name}.rhai"}

    # Objects; IDs sort into render order.
    spawn_y = ground(*SPAWN)
    look = math.degrees(math.atan2(-SHIP_BEARING[0], -SHIP_BEARING[1]))
    objects.append({"id": "camera", "name": "Camera / the castaway's eyes",
                    "transform": tf((SPAWN[0], spawn_y + 1.62, SPAWN[1]), (0, look, 0)),
                    "camera": {"projection": "perspective", "vertical_fov_degrees": 75, "near": 0.15,
                               "far": 12000}})
    objects.append({"id": "player", "name": "Player / WASD, mouse, Shift, Space",
                    "transform": tf((SPAWN[0], spawn_y, SPAWN[1])),
                    "blackboard": {
                        "px": number(SPAWN[0]), "py": number(spawn_y), "pz": number(SPAWN[1]),
                        "vx": number(0), "vy": number(0), "vz": number(0),
                        "yaw": number(round(look, 3)), "pitch": number(2), "grounded": flag(True),
                        "dip": number(0), "bob": number(0), "bob_amount": number(0), "fov_now": number(75)},
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
    objects.append({"id": "a-palm-crown", "name": "Palm crown / sways", "parent": "a-palm",
                    "transform": tf((origin[0] * VOXEL, origin[1] * VOXEL, origin[2] * VOXEL)),
                    "drawable": drawable({"asset": "palm-crown"}, {"gi_static": False}),
                    "script_manager": {"scripts": [{"enabled": True, "script": "palm-script"}]}})
    start = (SHIP_BEARING[0] * SHIP_START, 0, SHIP_BEARING[1] * SHIP_START)
    objects.append({"id": "b-ship", "name": "Ship / steaming away", "transform": tf(start, (0, SHIP_YAW, 0)),
                    "drawable": drawable({"asset": "ship"}, {"gi_static": False}),
                    "script_manager": {"scripts": [{"enabled": True, "script": "ship-script"}]}})
    objects.append({"id": "ship-lights", "name": "Ship lights / night only", "parent": "b-ship",
                    "transform": tf((0, 0, 0)), "drawable": drawable({"asset": "ship-lights"}, {"gi_static": False})})
    objects.append({"id": "c-ocean", "name": "Ocean / shader graph waves",
                    "transform": tf((0, -0.05, 0), scale=(20000, 0.1, 20000)),
                    "drawable": drawable("cube", {"gi_static": False, "roughness": 0.05, "metallic": 0.0},
                                         texture={"asset": "ocean-alpha"}),
                    "shader_graph": ocean_graph()})
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
    scene = {
        "version": 1,
        "name": "Sand Island",
        "views": {"3d": "camera"},
        "assets": dict(sorted(assets.items())),
        "blackboard": {
            "time_of_day": number(START_HOUR), "clock": number(START_HOUR * 3600), "clock_fraction": number(0),
            "day": number(1), "world_time": number(0),
            "menu_open": flag(False), "sensitivity": number(1.0), "invert_y": flag(False),
            "bobbing": flag(True), "fov": number(75), "view_distance": number(2), "shadows": flag(True),
            "clouds": flag(True), "bloom": flag(True), "toast": text(""), "toast_time": number(-100),
            "daylight": number(1),
        },
        "lighting": {"shadows": True, "shadow_resolution": 2048, "shadow_bias": 0.004,
                     "shadow_normal_bias": 0.02, "shadow_distance": 32,
                     "sun_direction": [round(v, 5) for v in sun_direction],
                     "sun_color": [1.0, 0.95, 0.86], "sun_intensity": SUN_INTENSITY,
                     "ambient_color": [0.86, 0.93, 1.0], "ambient_intensity": 0.08},
        "environment": {**SKY_DAY, "intensity": SKY_INTENSITY, "star_intensity": 0, "background": True},
        "fog": {"enabled": True, "color": [round(c * SKY_INTENSITY, 5) for c in SKY_DAY["horizon"]], "distance_density": 0.0006,
                "start_distance": 180, "height_density": 0, "base_height": 0, "height_falloff": 1},
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
