"""Checks for tools/gen_pagoda.py: deterministic output, the original's random
stream and placement, scene structure and Bozzard's asset limits.

Committed bytes are not compared: libm differences between operating systems
can move a voxel across a rounding boundary. `gen_pagoda.py --check` is the
Linux reference check for the committed example.
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("gen_pagoda", TOOLS / "gen_pagoda.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


class GeneratorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.files, cls.stats = gen.build(fixes=True)
        cls.scene = json.loads(cls.files["scenes/pagoda.json"])

    def test_random_stream_matches_the_original_lcg(self):
        rng = gen.Lcg(gen.SEED)
        self.assertEqual([rng.rand() * 4294967296 for _ in range(3)],
                         [3239374148, 2360088531, 1178942230])
        self.assertEqual(gen.js_round(-2.5), -2)
        self.assertEqual(gen.js_round(2.5), 3)

    def test_layout_matches_the_original_scene(self):
        stats = self.stats
        # 138,927 original voxels plus 3,292 filled valley columns; tolerate
        # a few cross-platform rounding differences.
        self.assertAlmostEqual(stats["world_voxels"], 142_219, delta=1500)
        self.assertEqual(stats["trees"], 47)
        self.assertEqual(len(stats["koi_colors"]), 11)
        self.assertEqual(stats["koi_colors"], [2, 2, 0, 1, 0, 1, 2, 2, 2, 1, 2])
        faithful = gen.build(fixes=False)[1]
        self.assertEqual(faithful["trees"], stats["trees"])
        self.assertEqual(faithful["koi_colors"], stats["koi_colors"])

    def test_generation_is_deterministic(self):
        again, _ = gen.build(fixes=True)
        self.assertEqual(sorted(again), sorted(self.files))
        for path, data in self.files.items():
            self.assertEqual(again[path], data, path)

    def test_scene_structure(self):
        scene = self.scene
        objects = scene["objects"]
        ids = [o["id"] for o in objects]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertLessEqual(len(objects), 3000)
        self.assertEqual(scene["views"]["3d"], "aa-camera")
        lights = [o for o in objects if "light" in o]
        self.assertEqual(len(lights), 5)
        self.assertLessEqual(len(lights), 32)
        for asset in scene["assets"].values():
            path = "scenes/" + asset["path"]
            self.assertIn(path, self.files, path)
            self.assertNotIn("\\", asset["path"])
            self.assertFalse(asset["path"].startswith("/"))
        for obj in objects:
            mesh = obj.get("drawable", {}).get("mesh")
            if isinstance(mesh, dict):
                self.assertIn(mesh["asset"], scene["assets"])
            for script in obj.get("script_manager", {}).get("scripts", []):
                self.assertEqual(scene["assets"][script["script"]]["kind"], "script")

    def test_drawables_are_grouped_by_mesh_in_id_order(self):
        drawables = sorted((o["id"], json.dumps(o["drawable"]["mesh"]))
                           for o in self.scene["objects"] if "drawable" in o)
        seen, previous = set(), None
        for _, mesh in drawables:
            if mesh != previous:
                self.assertNotIn(mesh, seen, f"{mesh} instances are not contiguous")
                seen.add(mesh)
            previous = mesh

    def test_assets_respect_import_limits(self):
        for path, data in self.files.items():
            if not path.endswith(".gltf"):
                continue
            doc = json.loads(data)
            self.assertLessEqual(len(data), gen.LIMITS["bytes"], path)
            primitives = doc["meshes"][0]["primitives"]
            self.assertLess(len(primitives), gen.LIMITS["parts"], path)
            vertices = sum(doc["accessors"][p["attributes"]["POSITION"]]["count"] for p in primitives)
            indices = sum(doc["accessors"][p["indices"]]["count"] for p in primitives)
            self.assertLessEqual(vertices, gen.LIMITS["vertices"], path)
            self.assertLessEqual(indices, gen.LIMITS["indices"], path)
            if path.startswith("scenes/assets/trees/"):
                self.assertEqual(len(primitives), 1, path)
            uri = doc["buffers"][0]["uri"]
            if not uri.startswith("data:"):
                self.assertIn(str(Path(path).parent / uri), self.files)

    def test_check_mode_round_trips_through_a_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(gen.main(["--out", directory]), 0)
            self.assertEqual(gen.main(["--out", directory, "--check"]), 0)
            target = Path(directory) / "scenes" / "scripts" / "koi.rhai"
            target.write_text("// edited\n")
            self.assertEqual(gen.main(["--out", directory, "--check"]), 1)


if __name__ == "__main__":
    unittest.main()
