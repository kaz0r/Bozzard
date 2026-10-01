"""Check logistics GLBs, one-tile bounds and matching pipe/belt connections."""
import json
import math
from pathlib import Path

from validate_machine_expansion import read_glb, values

ROOT = Path(__file__).resolve().parents[3]
ASSETS = ROOT / "examples/earth-factory/scenes/assets"
EXPECTED = {"pipe-straight", "pipe-elbow", "belt-turn-left", "belt-turn-right"}


def validate():
    entries = json.loads((ROOT / "assets/factory-machines/logistics-manifest.json").read_text())["models"]
    assert len(entries) == 4 and {e["id"] for e in entries} == EXPECTED
    clouds, lookup = {}, {e["id"]: e for e in entries}
    for entry in entries:
        slug = entry["id"]
        doc, binary = read_glb(ASSETS / entry["mesh"])
        assert not any(k in doc for k in ["animations", "skins", "cameras", "images"]), slug
        assert all("uri" not in b for b in doc["buffers"]), slug
        assert all("emissiveFactor" not in m for m in doc["materials"]), slug
        points, triangles = [], 0
        for mesh in doc["meshes"]:
            for primitive in mesh["primitives"]:
                assert primitive.get("mode", 4) == 4, slug
                vertices = values(doc, binary, primitive["attributes"]["POSITION"], 3)
                normals = values(doc, binary, primitive["attributes"]["NORMAL"], 3)
                indices = [v[0] for v in values(doc, binary, primitive["indices"], 1)]
                assert len(indices) % 3 == 0 and max(indices) < len(vertices), slug
                assert all(math.isfinite(v) for p in vertices for v in p), slug
                assert all(abs(sum(v*v for v in normal) - 1) < .001 for normal in normals), slug
                triangles += len(indices) // 3
                points.extend(vertices)
        assert triangles == entry["triangles"] < 4000, slug
        assert len(doc["meshes"]) == entry["surfaces"] <= 4, slug
        for node in doc["nodes"]:
            assert node.get("translation", [0, 0, 0]) == [0, 0, 0], slug
            assert node.get("scale", [1, 1, 1]) == [1, 1, 1], slug
            assert node.get("rotation", [0, 0, 0, 1]) == [0, 0, 0, 1], slug
        low = [min(p[i] for p in points) for i in range(3)]
        high = [max(p[i] for p in points) for i in range(3)]
        assert abs(low[1]) < .001, (slug, "not grounded", low)
        assert min(low[0], low[2]) >= -.501 and max(high[0], high[2]) <= .501, slug
        assert all(abs(actual - declared) < .00001
                   for bounds, declared_bounds in zip([low, high], entry["bounds"])
                   for actual, declared in zip(bounds, declared_bounds)), slug
        assert not entry["requires_power"] and len(entry["ports"]) == 2, slug
        for port in entry["ports"]:
            assert port["position"][1] == .34, slug
            assert abs(port["position"][0]) + abs(port["position"][2]) == .5, slug
        prefab = json.loads((ASSETS / entry["prefab"]).read_text())
        assert prefab["objects"][0]["transform"]["scale"] == [1, 1, 1], slug
        assert all((ASSETS / asset["path"]).is_file() for asset in prefab["assets"].values()), slug
        clouds[slug] = {tuple(round(v, 4) for v in point) for point in points}
        print(f"{slug}: {triangles} triangles; GLB, bounds and ports OK")
    left, right = lookup["belt-turn-left"], lookup["belt-turn-right"]
    assert left["triangles"] == right["triangles"]
    assert clouds["belt-turn-left"] == {(x, y, -z) for x, y, z in clouds["belt-turn-right"]}, "belt handedness mismatch"
    assert left["ports"][0]["position"] == [0, .34, -.5]
    assert right["ports"][0]["position"] == [0, .34, .5]
    assert left["ports"][1]["position"] == right["ports"][1]["position"] == [.5, .34, 0]
    for slug in ["pipe-straight", "pipe-elbow"]:
        assert all(port["role"] == "bidirectional" for port in lookup[slug]["ports"])
    print("Validated 4 models; mirrored belt turns and tile-edge connections match")


if __name__ == "__main__":
    validate()
