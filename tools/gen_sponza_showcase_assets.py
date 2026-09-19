#!/usr/bin/env python3
"""Generate original CC0 armillary meshes for The Gilded Hour. Stdlib only."""
import argparse
import json
import math
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1] / "examples/sponza/assets/gilded-hour"


def torus(radius, tube, rings=96, sides=12):
    positions, normals, uv, indices = [], [], [], []
    for i in range(rings + 1):
        a = math.tau * i / rings
        for j in range(sides + 1):
            b = math.tau * j / sides
            n = (math.cos(a) * math.cos(b), math.sin(a) * math.cos(b), math.sin(b))
            positions.extend((math.cos(a) * radius + tube * n[0], math.sin(a) * radius + tube * n[1], tube * n[2]))
            normals.extend(n)
            uv.extend((i / rings, j / sides))
    for i in range(rings):
        for j in range(sides):
            a = i * (sides + 1) + j
            b = a + sides + 1
            indices.extend((a, b, a + 1, a + 1, b, b + 1))
    return positions, normals, uv, indices


def asset(name, radius, tube):
    pos, normal, uv, indices = torus(radius, tube)
    chunks = [struct.pack(f"<{len(values)}f", *values) for values in (pos, normal, uv)]
    chunks.append(struct.pack(f"<{len(indices)}I", *indices))
    views, offset = [], 0
    for i, chunk in enumerate(chunks):
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(chunk), "target": 34963 if i == 3 else 34962})
        offset += len(chunk)
    accessors = [{"bufferView": i, "componentType": 5126, "count": len(values) // width, "type": kind}
                 for i, (values, width, kind) in enumerate(((pos, 3, "VEC3"), (normal, 3, "VEC3"), (uv, 2, "VEC2")))]
    accessors[0].update(min=[min(pos[i::3]) for i in range(3)], max=[max(pos[i::3]) for i in range(3)])
    accessors.append({"bufferView": 3, "componentType": 5125, "count": len(indices), "type": "SCALAR"})
    model = {
        "asset": {"version": "2.0", "generator": "Bozzard Gilded Hour / original CC0 geometry"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "name": name}],
        "buffers": [{"uri": f"{name}.bin", "byteLength": offset}], "bufferViews": views, "accessors": accessors,
        "materials": [{"name": "Satin bronze", "pbrMetallicRoughness": {"baseColorFactor": [0.72, 0.43, 0.16, 1], "metallicFactor": 0.92, "roughnessFactor": 0.24}}],
        "meshes": [{"name": name, "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3, "material": 0}]}],
    }
    return {f"{name}.bin": b"".join(chunks), f"{name}.gltf": (json.dumps(model, indent=2) + "\n").encode()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    files = asset("armillary-ring", 0.85, 0.035) | asset("armillary-filament", 0.81, 0.009)
    if args.check:
        for name, data in files.items():
            if not (ROOT / name).is_file() or (ROOT / name).read_bytes() != data:
                raise SystemExit(f"Out of date: {ROOT / name}")
    else:
        ROOT.mkdir(parents=True, exist_ok=True)
        for name, data in files.items():
            (ROOT / name).write_bytes(data)
    print(f"showcase_assets_ok files={len(files)} bytes={sum(map(len, files.values()))}")


if __name__ == "__main__":
    main()
