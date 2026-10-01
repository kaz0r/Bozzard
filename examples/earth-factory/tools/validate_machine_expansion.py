"""Check the exported expansion GLBs, geometry budgets and reusable prefabs.

Run with ordinary Python; no Blender or third-party packages are needed.
"""
import json
import math
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[3]
ASSETS = ROOT / "examples/earth-factory/scenes/assets"
MANIFEST = ROOT / "assets/factory-machines/expansion-manifest.json"
EXPECTED = {
    "water-pump", "oil-extractor", "crusher", "ore-washer", "foundry", "refinery",
    "chemical-plant", "electrolyzer", "kiln", "glassworks", "greenhouse",
    "electronics-fabricator", "manufacturer", "recycler",
}


def read_glb(path):
    raw = path.read_bytes()
    assert struct.unpack_from("<4sII", raw) == (b"glTF", 2, len(raw)), path
    size, kind = struct.unpack_from("<II", raw, 12)
    assert kind == 0x4E4F534A, path
    document = json.loads(raw[20:20 + size])
    offset = 20 + size
    size, kind = struct.unpack_from("<II", raw, offset)
    assert kind == 0x004E4942, path
    binary = raw[offset + 8:offset + 8 + size]
    assert document["buffers"][0]["byteLength"] <= len(binary), path
    return document, binary


def values(doc, binary, index, count):
    accessor = doc["accessors"][index]
    view = doc["bufferViews"][accessor["bufferView"]]
    code = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}[accessor["componentType"]]
    fmt = "<" + code * count
    stride = view.get("byteStride", struct.calcsize(fmt))
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    return [struct.unpack_from(fmt, binary, start + i * stride) for i in range(accessor["count"])]


def validate():
    manifest = json.loads(MANIFEST.read_text())
    machines = manifest["machines"]
    assert len(machines) == 14 and {m["id"] for m in machines} == EXPECTED
    total = 0
    for machine in machines:
        slug = machine["id"]
        doc, binary = read_glb(ASSETS / machine["mesh"])
        assert not any(key in doc for key in ["animations", "skins", "cameras", "images"]), slug
        assert all("uri" not in buf for buf in doc["buffers"]), slug
        assert all("emissiveFactor" not in mat for mat in doc["materials"]), slug
        triangles = 0
        positions = []
        for mesh in doc["meshes"]:
            for primitive in mesh["primitives"]:
                assert primitive.get("mode", 4) == 4, slug
                vertices = values(doc, binary, primitive["attributes"]["POSITION"], 3)
                normals = values(doc, binary, primitive["attributes"]["NORMAL"], 3)
                indices = [v[0] for v in values(doc, binary, primitive["indices"], 1)]
                assert len(indices) % 3 == 0 and max(indices) < len(vertices), slug
                assert all(math.isfinite(v) for p in vertices for v in p), slug
                assert all(abs(sum(v * v for v in n) - 1) < .001 for n in normals), slug
                assert all(len(set(indices[i:i + 3])) == 3 for i in range(0, len(indices), 3)), slug
                triangles += len(indices) // 3
                positions.extend(vertices)
        assert triangles == machine["triangles"] and triangles <= 4000, slug
        assert len(doc["meshes"]) == machine["surfaces"] <= 7, slug
        for node in doc["nodes"]:
            assert all(abs(v) < .00001 for v in node.get("translation", [0, 0, 0])), slug
            assert node.get("scale", [1, 1, 1]) == [1, 1, 1], slug
        # Blender's export stores its Y-up conversion on the surface nodes.
        # Manifest bounds are measured after that conversion in game coordinates.
        low, high = machine["bounds"]
        assert abs(low[1]) < .001 and high[1] > .7, slug
        assert min(low[0], low[2]) >= -.501 and max(high[0], high[2]) <= .501, slug
        prefab = json.loads((ASSETS / machine["prefab"]).read_text())
        assert prefab["objects"][0]["transform"]["scale"] == [1, 1, 1], slug
        for asset in prefab["assets"].values():
            assert (ASSETS / asset["path"]).is_file(), slug
        for state in ["on", "off"]:
            lamp = json.loads((ASSETS / f"{slug}-power-{state}.prefab.json").read_text())
            assert lamp["objects"][1]["transform"]["translation"] == machine["indicator"]["position"], slug
        assert machine["requires_power"] and machine["power_socket"] and machine["ports"], slug
        assert all(port["position"][1] == .34 for port in machine["ports"]), slug
        total += triangles
        print(f"{slug}: {triangles} triangles; self-contained GLB and prefabs OK")
    print(f"Validated {len(machines)} machines / {total} triangles")


if __name__ == "__main__":
    validate()
