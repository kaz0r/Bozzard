"""Checks for tools/gen_island.py: deterministic output, a consistent scene (every object a
script names exists), Bozzard's asset limits, and the island's playable shape.

Committed bytes are not compared: libm differences between operating systems can move a
voxel across a rounding boundary. `gen_island.py --check` is the Linux reference check for
the committed example; crates/bozzard-runtime/tests/sand_island.rs plays the scene.
"""
import importlib.util
import json
import math
from pathlib import Path
import re
import struct
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("gen_island", TOOLS / "gen_island.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


class GeneratorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.files, cls.stats = gen.build()
        cls.scene = json.loads(cls.files["scenes/island.json"])
        cls.objects = {o["id"]: o for o in cls.scene["objects"]}

    def test_generation_is_deterministic(self):
        again, _ = gen.build()
        self.assertEqual(sorted(again), sorted(self.files))
        for path, data in self.files.items():
            self.assertEqual(again[path], data, path)

    def test_scene_structure_and_references(self):
        objects = self.scene["objects"]
        self.assertEqual(len(objects), len(self.objects), "object IDs are unique")
        assets = self.scene["assets"]
        for asset in assets.values():
            self.assertIn("scenes/" + asset["path"], self.files, asset)
        for o in objects:
            if "parent" in o:
                self.assertIn(o["parent"], self.objects, o["id"])
            mesh = o.get("drawable", {}).get("mesh")
            if isinstance(mesh, dict):
                self.assertEqual(assets[mesh["asset"]]["kind"], "mesh", o["id"])
            for attachment in o.get("script_manager", {}).get("scripts", []):
                self.assertEqual(assets[attachment["script"]]["kind"], "script", o["id"])
        self.assertEqual(self.scene["views"], {"3d": "camera"})
        self.assertGreater(self.scene["lighting"]["shadow_distance"], 0)

    def test_every_object_a_script_names_exists(self):
        names = set()
        for path, data in self.files.items():
            if path.endswith(".rhai"):
                names |= set(re.findall(r'"((?:ui|smoke|cloud|ship)-[a-z0-9-]+|sun|moon|camera|player)"',
                                        data.decode()))
        names -= {"sun-low"}  # a mesh asset, swapped in with set_mesh
        self.assertIn("ui-opt-sensitivity", names)
        for name in sorted(names):
            self.assertIn(name, self.objects, name)
        for i in range(gen.SMOKE):
            self.assertIn(f"smoke-{i:02d}", self.objects)
        for i in range(gen.CLOUDS):
            self.assertIn(f"cloud-{i}", self.objects)
        self.assertIn("sun-low", self.scene["assets"])

    def test_scene_blackboard_declares_every_variable_scripts_use(self):
        declared = set(self.scene["blackboard"])
        used = set()
        for path, data in self.files.items():
            if path.endswith(".rhai"):
                used |= set(re.findall(r'(?:get|set)_scene_variable\("([a-z_]+)"', data.decode()))
        self.assertEqual(sorted(used - declared), [])
        player = set(self.objects["player"]["blackboard"])
        source = self.files["scenes/scripts/player.rhai"].decode()
        used = set(re.findall(r'(?:get|set)_object_variable\("([a-z_]+)"', source))
        self.assertEqual(sorted(used - player), [])

    def test_meshes_stay_within_import_limits(self):
        for path, data in self.files.items():
            if not path.endswith(".gltf"):
                continue
            doc = json.loads(data)
            self.assertLess(len(doc["meshes"][0]["primitives"]), 4096, path)
            vertices = sum(doc["accessors"][p["attributes"]["POSITION"]]["count"]
                           for p in doc["meshes"][0]["primitives"])
            self.assertLessEqual(vertices, 1_000_000, path)
            self.assertLessEqual(doc["buffers"][0]["byteLength"], 32 * 1024 * 1024, path)
        self.assertLess(self.stats["bytes"], 8 * 1024 * 1024)

    def test_island_tops_cover_every_column_once(self):
        doc = json.loads(self.files["scenes/assets/island.gltf"])
        binary = self.files["scenes/assets/island.bin"]
        area = 0.0
        for primitive in doc["meshes"][0]["primitives"]:
            positions = doc["accessors"][primitive["attributes"]["POSITION"]]
            normals = doc["accessors"][primitive["attributes"]["NORMAL"]]
            pv, nv = doc["bufferViews"][positions["bufferView"]], doc["bufferViews"][normals["bufferView"]]
            p = struct.unpack_from(f"<{positions['count'] * 3}f", binary, pv["byteOffset"])
            n = struct.unpack_from(f"<{normals['count'] * 3}f", binary, nv["byteOffset"])
            for quad in range(0, positions["count"], 4):
                if n[quad * 3 + 1] != 1.0:
                    continue
                xs = [p[(quad + i) * 3] for i in range(4)]
                zs = [p[(quad + i) * 3 + 2] for i in range(4)]
                area += (max(xs) - min(xs)) * (max(zs) - min(zs))
        n = int(math.ceil(gen.GRID_RADIUS / gen.VOXEL))
        columns = sum(1 for i in range(-n, n) for k in range(-n, n)
                      if math.hypot((i + 0.5) * gen.VOXEL, (k + 0.5) * gen.VOXEL) <= gen.GRID_RADIUS)
        # The fallen coconuts add their own small tops.
        self.assertAlmostEqual(area / gen.VOXEL ** 2, columns + 8, delta=0.5)

    def test_the_island_is_playable(self):
        ground = gen.ground
        self.assertGreater(ground(0, 0), 2.0)
        spawn, palm = gen.SPAWN, gen.PALM
        self.assertGreater(ground(*spawn), 0.5, "the castaway starts on dry sand")
        self.assertGreater(ground(*palm), 1.0, "the palm stands inland")
        self.assertGreater(math.dist(spawn, palm), 3)
        # Every direction reaches swimming depth before the current turns the swimmer back,
        # and the voxel grid reaches past that turning point.
        for degrees in range(0, 360, 15):
            a = math.radians(degrees)
            r = gen.BOUNDARY - 1
            self.assertLess(ground(math.cos(a) * r, math.sin(a) * r), -gen.SWIM_DEPTH, degrees)
        self.assertGreater(gen.GRID_RADIUS, gen.BOUNDARY)
        library = self.files["scenes/scripts/island.rhai"].decode()
        for value in (gen.COAST, gen.PEAK, gen.SWIM_DEPTH, gen.BOUNDARY):
            self.assertIn(f"{value}", library)

    def test_day_runs_at_eight_game_seconds_per_real_second(self):
        self.assertEqual(gen.TIME_SCALE, 8.0)
        world = self.files["scenes/scripts/world.rhai"].decode()
        self.assertIn("{ 8.0 }", world)
        self.assertEqual(self.scene["blackboard"]["clock"]["scalar"]["number"], gen.START_HOUR * 3600)

    def test_ocean_graph_is_within_shader_graph_limits(self):
        graph = self.objects["c-ocean"]["shader_graph"]
        self.assertLessEqual(len(graph["nodes"]), 128)
        self.assertLessEqual(len(graph["wires"]), 512)
        master = next(n["id"] for n in graph["nodes"] if n["kind"] == "master")
        wired = {w["to"]["port"] for w in graph["wires"] if w["to"]["node"] == master}
        self.assertEqual(wired, {0, 4, 5}, "base colour, alpha and normal come from the graph")

    def test_check_mode_reports_stale_files(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(gen.main(["--out", directory]), 0)
            self.assertEqual(gen.main(["--out", directory, "--check"]), 0)
            (Path(directory) / "scenes/island.json").write_text("{}")
            self.assertEqual(gen.main(["--out", directory, "--check"]), 1)


if __name__ == "__main__":
    unittest.main()
