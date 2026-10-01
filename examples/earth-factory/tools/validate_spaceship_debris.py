"""Validate the actual exported wreck meshes, pivots, materials and prefabs."""
import json
import math
from pathlib import Path

from validate_machine_expansion import read_glb, values

ROOT = Path(__file__).resolve().parents[3]
ASSETS = ROOT / "examples/earth-factory/scenes/assets"
SOURCE = ROOT / "assets/spaceship-debris"


def validate():
    manifest = json.loads((SOURCE / "manifest.json").read_text())
    entries = manifest["models"]
    assert len(entries) == 4
    assert {e["id"] for e in entries} == {"debris-"+s for s in ["cockpit", "hull", "wing", "engine"]}
    assert manifest["spawn"]["minimum_per_planet"] == 2
    assert manifest["spawn"]["maximum_per_planet"] == 6
    clouds = []
    shared = {"Survey ship ivory armor", "Exposed survey ship frame",
              "Scorched graphite interiors", "Survey ship gold identification",
              "Survey ship cyan service lines"}
    for entry in entries:
        slug = entry["id"]
        doc, binary = read_glb(ASSETS / entry["mesh"])
        assert not any(k in doc for k in ["animations", "skins", "cameras", "images"]), slug
        assert all("uri" not in b for b in doc["buffers"]), slug
        assert shared <= {m["name"] for m in doc["materials"]}, slug
        points, triangles = [], 0
        for mesh in doc["meshes"]:
            for primitive in mesh["primitives"]:
                assert primitive.get("mode", 4) == 4, slug
                vertices = values(doc, binary, primitive["attributes"]["POSITION"], 3)
                normals = values(doc, binary, primitive["attributes"]["NORMAL"], 3)
                indices = [v[0] for v in values(doc, binary, primitive["indices"], 1)]
                assert len(indices) % 3 == 0 and max(indices) < len(vertices), slug
                assert all(math.isfinite(v) for p in vertices for v in p), slug
                assert all(abs(sum(v*v for v in n)-1) < .001 for n in normals), slug
                for a, b, c in zip(indices[::3], indices[1::3], indices[2::3]):
                    ab = [vertices[b][i]-vertices[a][i] for i in range(3)]
                    ac = [vertices[c][i]-vertices[a][i] for i in range(3)]
                    cross = [ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0]]
                    assert sum(v*v for v in cross) > 1e-14, (slug, "degenerate triangle")
                triangles += len(indices)//3
                points.extend(vertices)
        assert triangles == entry["triangles"] < 5000, slug
        assert len(doc["meshes"]) == entry["surfaces"] <= 6, slug
        for node in doc["nodes"]:
            assert node.get("translation", [0, 0, 0]) == [0, 0, 0], slug
            assert node.get("scale", [1, 1, 1]) == [1, 1, 1], slug
            assert node.get("rotation", [0, 0, 0, 1]) == [0, 0, 0, 1], slug
        low = [min(p[i] for p in points) for i in range(3)]
        high = [max(p[i] for p in points) for i in range(3)]
        assert abs(low[1]) < .001, (slug, "not grounded")
        assert min(low[0], low[2]) >= -1.45 and max(high[0], high[2]) <= 1.45, slug
        assert all(abs(a-d) < .00001 for bound, declared in zip([low, high], entry["bounds"])
                   for a, d in zip(bound, declared)), slug
        prefab = json.loads((ASSETS / entry["prefab"]).read_text())
        assert prefab["objects"][0]["transform"] == {
            "translation": [0, 0, 0], "rotation_degrees": [0, 0, 0], "scale": [1, 1, 1]}, slug
        assert all((ASSETS / a["path"]).is_file() for a in prefab["assets"].values()), slug
        cloud = {tuple(round(v, 4) for v in p) for p in points}
        assert all(cloud != other for other in clouds), (slug, "duplicate model")
        clouds.append(cloud)
        print(f"{slug}: {triangles} triangles; geometry, grounded pivot, palette and prefab OK")
    scene = json.loads((ASSETS.parent / "earth.json").read_text())
    assert all(e["id"] in scene["assets"] for e in entries)
    assert "factory-debris" in scene["assets"]
    print("Validated all four distinct spaceship debris prototypes")


if __name__ == "__main__":
    validate()
