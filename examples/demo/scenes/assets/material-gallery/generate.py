#!/usr/bin/env python3
"""Rebuild this original, CC0 gallery using Python's standard library only.

Run from any directory. --check verifies committed assets without writing them.
Coatings use core glTF low-roughness dielectrics, not unsupported clearcoat.
"""
import argparse
import json
import math
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent
OUTPUT = {}


def emit(path, data):
    OUTPUT[path] = data if isinstance(data, bytes) else (json.dumps(data, indent=2) + "\n").encode()


def png(name, pixels, size=128):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + bytes(pixels[y * size * 3:(y + 1) * size * 3]) for y in range(size))
    emit(ROOT / name, b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", size, size, 8, 2, 0, 0, 0))
         + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


# Periodic functions keep the UV seam continuous; packed MR uses glTF G/B.
for kind in ("walnut", "marble"):
    colors, normals, mr = [], [], []
    def height(u, v):
        if kind == "walnut":
            return math.sin(math.tau * (22 * u + .65 * math.sin(math.tau * v)
                                       + .2 * math.sin(math.tau * (2 * v + u))))
        return math.sin(math.tau * (3 * u + 2 * v) + 2.5 * math.sin(math.tau * (u - v))
                        + .6 * math.sin(math.tau * (7 * u + 5 * v))
                        + .2 * math.sin(math.tau * (17 * u - 11 * v)))
    for y in range(128):
        for x in range(128):
            u, v = x / 128, y / 128
            h = height(u, v)
            if kind == "walnut":
                grain = .5 + .25 * h + .25 * math.sin(math.tau * (37 * u + .4 * math.sin(math.tau * v)))
                fine = math.sin(math.tau * (49 * u + 2 * v)) * 4
                colors.extend(round(c + a * grain + fine) for c, a in [(62, 60), (30, 38), (15, 19)])
                roughness = 105 + round(45 * grain)
            else:
                vein = (.5 + .5 * h) ** 18
                colors.extend(round(c - a * vein) for c, a in [(215, 110), (219, 113), (210, 101)])
                roughness = 72 + round(52 * vein)
            dx = (height(u + 1 / 128, v) - height(u - 1 / 128, v)) * .12
            dy = (height(u, v + 1 / 128) - height(u, v - 1 / 128)) * .12
            length = math.sqrt(dx * dx + dy * dy + 1)
            normals.extend(round(127.5 * (c / length + 1)) for c in (-dx, -dy, 1))
            mr.extend((255, roughness, 0))
    png(kind + "-color.png", colors)
    png(kind + "-normal.png", normals)
    png(kind + "-mr.png", mr)

buffer = bytearray()
views, accessors = [], []


def accessor(values, width, component=5126, bounds=False):
    while len(buffer) % 4:
        buffer.append(0)
    start = len(buffer)
    flat = [c for row in values for c in row]
    buffer.extend(struct.pack("<" + ("f" if component == 5126 else "H") * len(flat), *flat))
    views.append({"buffer": 0, "byteOffset": start, "byteLength": len(buffer) - start})
    result = {"bufferView": len(views) - 1, "componentType": component,
              "count": len(values), "type": {1: "SCALAR", 2: "VEC2", 3: "VEC3"}[width]}
    if bounds:
        result.update(min=[min(row[i] for row in values) for i in range(width)],
                      max=[max(row[i] for row in values) for i in range(width)])
    accessors.append(result)
    return len(accessors) - 1


def geometry(positions, normals, uv, indices):
    return {"attributes": {"POSITION": accessor(positions, 3, bounds=True),
                           "NORMAL": accessor(normals, 3), "TEXCOORD_0": accessor(uv, 2)},
            "indices": accessor([(i,) for i in indices], 1, 5123), "material": 0}


# One smooth UV sphere and one rounded box, shared by every glTF wrapper.
pos, norm, uv, indices = [], [], [], []
for j in range(25):
    theta = math.pi * j / 24
    for i in range(49):
        phi = math.tau * i / 48
        n = (math.sin(theta) * math.cos(phi), math.cos(theta), math.sin(theta) * math.sin(phi))
        pos.append(tuple(c * .5 for c in n))
        norm.append(n)
        uv.append((i / 48, j / 24))
for j in range(24):
    for i in range(48):
        a = j * 49 + i
        if j > 0:
            indices.extend((a, a + 1, a + 49))
        if j < 23:
            indices.extend((a + 1, a + 50, a + 49))
sphere = geometry(pos, norm, uv, indices)

pos, norm, uv, indices = [], [], [], []
# Face axes ordered so U cross V is the outward normal.
for axis, u_axis, v_axis in [(0, 1, 2), (1, 2, 0), (2, 0, 1)]:
    for sign in (-1, 1):
        start = len(pos)
        steps = [-.5, -.475, -.425, -.4, 0, .4, .425, .475, .5]
        for j, v in enumerate(steps):
            for i, u in enumerate(steps):
                p = [0., 0., 0.]
                p[axis], p[u_axis], p[v_axis] = sign * .5, u, v * sign
                core = [max(-.4, min(.4, c)) for c in p]
                delta = [c - d for c, d in zip(p, core)]
                length = math.sqrt(sum(c * c for c in delta))
                n = [c / length for c in delta]
                pos.append([c + .1 * d for c, d in zip(core, n)])
                norm.append(n)
                uv.append((i / 8, j / 8))
        for j in range(8):
            for i in range(8):
                a = start + j * 9 + i
                indices.extend((a, a + 1, a + 9, a + 1, a + 10, a + 9))
box = geometry(pos, norm, uv, indices)
emit(ROOT / "geometry.bin", bytes(buffer))


def material(name, color, metallic, roughness, shape, texture=None):
    pbr = {"baseColorFactor": [*color, 1], "metallicFactor": metallic, "roughnessFactor": roughness}
    mat = {"name": name, "pbrMetallicRoughness": pbr}
    doc = {"asset": {"version": "2.0", "generator": "Bozzard material-gallery/generate.py (CC0)"},
           "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"name": name, "mesh": 0}],
           "meshes": [{"name": name, "primitives": [shape]}], "materials": [mat],
           "buffers": [{"uri": "geometry.bin", "byteLength": len(buffer)}],
           "bufferViews": views, "accessors": accessors}
    if texture:
        doc["images"] = [{"uri": f"{texture}-{slot}.png"} for slot in ("color", "normal", "mr")]
        doc["samplers"] = [{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]
        doc["textures"] = [{"source": i, "sampler": 0} for i in range(3)]
        pbr.update(baseColorTexture={"index": 0}, metallicRoughnessTexture={"index": 2})
        mat["normalTexture"] = {"index": 1, "scale": .65}
    emit(ROOT / (name + ".gltf"), doc)


samples = [
    ("gold-polished", "01 / Gold — polished / roughness 0.08", [.83, .57, .19], 1, .08, sphere, None),
    ("gold-satin", "02 / Gold — satin / roughness 0.32", [.83, .57, .19], 1, .32, sphere, None),
    ("gold-matte", "03 / Gold — matte / roughness 0.72", [.83, .57, .19], 1, .72, sphere, None),
    ("copper", "04 / Copper — burnished / roughness 0.20", [.95, .49, .30], 1, .20, sphere, None),
    ("walnut", "05 / Walnut — oiled grain / textured PBR", [1, 1, 1], 0, 1, box, "walnut"),
    ("marble", "06 / Marble — honed veins / textured PBR", [1, 1, 1], 0, 1, sphere, "marble"),
    ("lacquer", "07 / Vermilion — lacquered polymer / roughness 0.12", [.55, .025, .013], 0, .12, box, None),
    ("polymer", "08 / Petrol — soft-touch plastic / roughness 0.58", [.015, .19, .22], 0, .58, sphere, None),
]
for key, name, color, metal, rough, shape, texture in samples:
    material(key, color, metal, rough, shape, texture)
material("plinth", [.59, .55, .46], 0, .55, box)
material("graphite", [.035, .045, .052], 0, .65, box)
material("brass", [.65, .40, .12], 1, .27, box)

objects = []
assets = {}


def obj(key, name, position, scale=(1, 1, 1), rotation=(0, 0, 0), **components):
    value = {"id": key, "name": name, "transform": {"translation": position,
             "rotation_degrees": rotation, "scale": scale}, **components}
    objects.append(value)
    return value


def draw(key, name, asset, position, scale, rotation=(0, 0, 0)):
    assets[asset] = {"kind": "mesh", "path": f"assets/material-gallery/{asset}.gltf"}
    return obj(key, name, position, scale, rotation, drawable={"layer": "3d", "mesh": {"asset": asset},
               "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]})


obj("camera", "Gallery / overview", [7.8, 8, 14.8], rotation=[-24, 28, 0],
    camera={"projection": "perspective", "vertical_fov_degrees": 43, "near": .1, "far": 80})
obj("camera-2d", "Plan / 2D", [0, 0, 10],
    camera={"projection": "orthographic", "vertical_size": 12, "near": .1, "far": 80})
draw("stage", "Architecture / floating graphite stage", "graphite", [0, -.22, 0], [11, .44, 8])
draw("backdrop", "Architecture / charcoal backdrop", "graphite", [0, 2.1, -3.7], [10.8, 4.2, .24])
draw("horizon-inlay", "Architecture / brass horizon", "brass", [0, 3.55, -3.54], [9.4, .035, .03])
for i, (key, name, *_rest) in enumerate(samples):
    x, z = (i % 4 - 1.5) * 2.4, -1.5 if i < 4 else 1.5
    height = 1.25 if i < 4 else .65
    draw(key + "-plinth", f"Display {i + 1:02} / limestone plinth", "plinth", [x, height / 2, z], [1.9, height, 1.9])
    draw(key + "-trim", f"Display {i + 1:02} / brass foot", "brass", [x, .06, z], [1.92, .07, 1.92])
    draw(key, name, key, [x, height + .75, z], [1.5, 1.5, 1.5], [0, -15 if i >= 4 else 0, 0])
    # Small numbered brass ticks make the roughness progression legible spatially.
    for tick in range(i % 4 + 1):
        draw(f"{key}-index-{tick}", f"Display {i + 1:02} / index tick {tick + 1}", "brass",
             [x + (tick - (i % 4) / 2) * .12, height * .5, z + .952], [.05, .13, .015])
obj("warm-key", "Lighting / warm softbox", [-4, 5, 4],
    light={"kind": "point", "color": [1, .84, .66], "intensity": 170, "range": 16})
obj("cool-rim", "Lighting / cool rim", [4, 4, -2],
    light={"kind": "point", "color": [.60, .78, 1], "intensity": 120, "range": 13})
scene = {"version": 1, "name": "Material Gallery / Form & Finish", "views": {"3d": "camera", "2d": "camera-2d"},
         "lighting": {"sun_direction": [-.45, .85, .6], "sun_color": [1, .94, .83], "sun_intensity": 2.5,
                      "ambient_intensity": .025, "shadows": True, "shadow_resolution": 2048},
         "environment": {"zenith": [.25, .38, .55], "horizon": [.85, .82, .73], "ground": [.14, .12, .10],
                         "intensity": .65, "background": True},
         "fog": {"enabled": True, "color": [.28, .34, .42], "distance_density": .008,
                 "start_distance": 4, "height_density": .035, "base_height": .3, "height_falloff": 1.5},
         "display": {"exposure_ev": 0, "tone_mapping": True}, "objects": objects, "assets": assets}
emit(ROOT.parent.parent / "material-gallery.json", scene)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    for path, data in OUTPUT.items():
        if args.check:
            assert path.read_bytes() == data, f"Regenerate {path}"
        else:
            path.write_bytes(data)
    print(f"{'Verified' if args.check else 'Generated'} {len(OUTPUT)} files, {sum(map(len, OUTPUT.values())):,} bytes")
