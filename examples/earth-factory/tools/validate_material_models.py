"""Validate every exported item mesh and its centered conveyor-sized pivot."""
import json
import math
import re
from pathlib import Path

from validate_machine_expansion import read_glb, values

ROOT = Path(__file__).resolve().parents[3]
ASSETS = ROOT / "examples/earth-factory/scenes/assets"


def validate():
    entries = json.loads((ROOT / "assets/factory-materials/manifest.json").read_text())["items"]
    expected = set(range(1, 51)) - {19}
    assert len(entries) == 49 and {e["kind"] for e in entries} == expected
    scene = json.loads((ROOT / "examples/earth-factory/scenes/earth.json").read_text())
    table_source = (ROOT / "examples/earth-factory/scenes/scripts/factory/material_models.rhai").read_text()
    heights = json.loads(re.search(r"\n    (\[.*\])\[index\]", table_source).group(1))
    assert len(heights) == 51
    total = 0
    for entry in entries:
        name = entry["id"]
        doc, binary = read_glb(ASSETS / entry["mesh"])
        assert not any(k in doc for k in ["animations", "skins", "cameras", "images"]), name
        assert all("uri" not in b for b in doc["buffers"]), name
        points, triangles = [], 0
        for mesh in doc["meshes"]:
            for primitive in mesh["primitives"]:
                vertices = values(doc, binary, primitive["attributes"]["POSITION"], 3)
                normals = values(doc, binary, primitive["attributes"]["NORMAL"], 3)
                indices = [v[0] for v in values(doc, binary, primitive["indices"], 1)]
                assert len(indices) % 3 == 0 and max(indices) < len(vertices), name
                assert all(math.isfinite(v) for p in vertices for v in p), name
                assert all(abs(sum(v*v for v in n)-1) < .001 for n in normals), name
                triangles += len(indices) // 3
                points.extend(vertices)
        assert triangles == entry["triangles"] <= 1400, name
        assert len(doc["meshes"]) == entry["surfaces"] <= 7, name
        for node in doc["nodes"]:
            assert node.get("translation", [0,0,0]) == [0,0,0], name
            assert node.get("scale", [1,1,1]) == [1,1,1], name
            assert node.get("rotation", [0,0,0,1]) == [0,0,0,1], name
        low = [min(p[a] for p in points) for a in range(3)]
        high = [max(p[a] for p in points) for a in range(3)]
        assert all(abs(a+b) < .00001 for a,b in zip(low, high)), (name, "off-center pivot", low, high)
        assert high[0]-low[0] <= .40 and high[2]-low[2] <= .40, (name, "too wide for belt")
        assert high[1]-low[1] <= .36, name
        assert all(abs(a-b) < .00001 for bounds, declared in zip([low,high],entry["bounds"])
                   for a,b in zip(bounds,declared)), (name, "manifest bounds")
        assert abs(heights[entry["kind"]]-high[1]) < .00001, (name, "gameplay height")
        prefab = json.loads((ASSETS / entry["prefab"]).read_text())
        assert len(prefab["objects"]) == 1, name
        root = prefab["objects"][0]
        assert root["transform"]["scale"] == [1,1,1] and root["drawable"]["color"] == [1,1,1], name
        assert root["drawable"]["mesh"] == {"asset":entry["mesh_asset"]}, name
        assert scene["assets"][entry["mesh_asset"]]["path"] == "assets/"+entry["mesh"], name
        total += triangles
        print(f"{name}: {triangles} triangles; centered GLB and prefab OK")
    print(f"Validated {len(entries)} item models / {total} triangles")


if __name__ == "__main__":
    validate()
