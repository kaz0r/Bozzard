"""Checks for tools/gen_island.py and the Sand Island scripts: deterministic output, a
consistent scene (every object, variable and asset a script names exists), Bozzard's
asset limits, the island's playable shape and the game's recipe tables.

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
import zlib

TOOLS = Path(__file__).resolve().parent
SCRIPTS = TOOLS.parent / "examples" / "sand-island" / "scenes" / "scripts"
spec = importlib.util.spec_from_file_location("gen_island", TOOLS / "gen_island.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


class GeneratorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.files, cls.stats = gen.build()
        cls.scene = json.loads(cls.files["scenes/island.json"])
        cls.objects = {o["id"]: o for o in cls.scene["objects"]}
        cls.scripts = {path.stem: path.read_text() for path in SCRIPTS.glob("*.rhai") if path.stem != "island"}
        cls.scripts["island"] = cls.files["scenes/scripts/island.rhai"].decode()

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
            path = "scenes/" + asset["path"]
            generated = path in self.files
            authored = asset["kind"] == "script" and (SCRIPTS / Path(asset["path"]).name).exists()
            self.assertTrue(generated or authored, asset)
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
        pattern = (r'"((?:ui|smoke|cloud|ship|wood|nut|raft|lighthouse|a)-[a-z0-9-]+'
                   r'|sun|moon|camera|player|raft)"')
        for source in self.scripts.values():
            names |= set(re.findall(pattern, source))
        assets = {"sun-low", "island-lib"}
        self.assertIn("ui-opt-sensitivity", names)
        self.assertIn("ui-craft-rope", names)
        for name in sorted(names - assets):
            self.assertIn(name, self.objects, name)
        for prefix, count in (("smoke-", gen.SMOKE), ("cloud-", gen.CLOUDS), ("wood-", gen.WOOD),
                              ("nut-", gen.NUTS)):
            for i in range(count):
                self.assertIn(f"{prefix}{i:02d}" if prefix == "smoke-" else f"{prefix}{i}", self.objects)
        for i in range(1, len(gen.STAGES) + 1):
            self.assertIn(f"raft-stage-{i}", self.objects)
            self.assertIn(f"raft-ghost-{i}", self.objects)
        for item in gen.ITEMS:
            for kind in ("slot", "icon", "count"):
                self.assertIn(f"ui-{kind}-{item}", self.objects)
        self.assertIn("sun-low", self.scene["assets"])

    def test_every_variable_a_script_uses_is_declared(self):
        declared = set(self.scene["blackboard"])
        for name, source in self.scripts.items():
            used = set(re.findall(r'(?:get|set)_scene_variable\("([a-z_]+)"', source))
            used |= {item for item in gen.ITEMS if "get_scene_variable(item)" in source}
            self.assertEqual(sorted(used - declared), [], name)
        owners = {"player": "player", "game": "game", "hud": "ui", "world": "world"}
        for script, owner in owners.items():
            source = self.scripts[script]
            own = set(re.findall(r'get_object_variable\("([a-z_]+)"\)', source))
            own |= set(re.findall(r'set_object_variable\("([a-z_]+)",\s*(?!")', source))
            own |= set(re.findall(r'(?:get|set)_object_list\("([a-z_]+)"', source))
            own -= {"player"}
            if script == "hud":
                own |= {f"count_{item}" for item in gen.ITEMS}
            board = set(self.objects[owner].get("blackboard", {}))
            self.assertEqual(sorted(own - board), [], script)
            for target, variable in re.findall(r'(?:get|set)_object_variable\("([a-z_-]+)",\s*"([a-z_]+)"', source):
                self.assertIn(variable, self.objects[target]["blackboard"], (script, target, variable))

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
        self.assertAlmostEqual(area / gen.VOXEL ** 2, columns, delta=0.5)

    def test_the_island_is_playable(self):
        ground = gen.ground
        self.assertGreater(ground(0, 0), 2.0)
        spawn, palm = gen.SPAWN, gen.PALM
        self.assertGreater(ground(*spawn), 0.5, "the castaway starts on dry sand")
        self.assertGreater(ground(*palm), 1.0, "the palm stands inland")
        self.assertGreater(ground(*gen.SHADE), 1.0, "a collapsed castaway wakes on land")
        self.assertGreater(math.dist(spawn, palm), 3)
        # Every direction reaches swimming depth before the current turns the swimmer back,
        # and the voxel grid reaches past that turning point.
        for degrees in range(0, 360, 15):
            a = math.radians(degrees)
            r = gen.BOUNDARY - 1
            self.assertLess(ground(math.cos(a) * r, math.sin(a) * r), -gen.SWIM_DEPTH, degrees)
        self.assertGreater(gen.GRID_RADIUS, gen.BOUNDARY)
        library = self.scripts["island"]
        for value in (gen.COAST, gen.PEAK, gen.SWIM_DEPTH, gen.BOUNDARY):
            self.assertIn(f"{value}", library)

    def test_the_raft_can_be_built_and_launched(self):
        # The site is on dry sand facing the lighthouse; the raft floats where it launches.
        x, y, z, yaw, _ = gen.SITE
        self.assertGreater(gen.ground(x, z), 0.3)
        forward = (-math.sin(math.radians(yaw)), -math.cos(math.radians(yaw)))
        self.assertGreater(forward[0] * gen.TO_LIGHT[0] + forward[1] * gen.TO_LIGHT[1], 0.999)
        self.assertLess(gen.ground(*gen.LAUNCH), -1.0)
        self.assertLess(math.dist(gen.LAUNCH, (x, z)), 15)
        # The beach spawn spots are on the sand and away from the site.
        self.assertGreaterEqual(len(gen.BEACH), 12)
        for spot in gen.BEACH:
            self.assertGreater(gen.ground(*spot), 0.1, spot)
            self.assertGreater(math.dist(spot, (x, z)), 6.9, spot)
        # Everything the raft needs, raw: what the island must be able to supply.
        need = {item: 0.0 for item in gen.ITEMS}

        def spend(cost):
            for item, amount in cost.items():
                if item in gen.RECIPES:
                    for _ in range(int(amount)):
                        spend(gen.RECIPES[item])
                else:
                    need[item] += amount
        for stage in gen.STAGES:
            spend(stage["cost"])
        self.assertEqual({k: v for k, v in need.items() if v}, {"wood": 8.0, "fronds": 18.0, "coconuts": 2.0})
        self.assertEqual(len(gen.STAGES), 5)
        game = self.scripts["game"]
        self.assertIn("const PALM_STOCK = 10.0;", game, "ten pulls of two fronds cover the 18 needed")

    def test_the_lighthouse_is_far_out_to_the_south_west(self):
        x, z = gen.LIGHTHOUSE
        self.assertTrue(600 < math.hypot(x, z) < 800)
        self.assertLess(x, 0)
        self.assertGreater(z, 0)
        beacon = re.search(r"const BEACON = \[([-0-9., ]+)\];", self.scripts["island"]).group(1)
        self.assertGreater(float(beacon.split(", ")[1]), 20.0, "the lamp burns high on its tower")

    def test_icons_cover_every_item(self):
        png = self.files["scenes/assets/icons.png"]
        width, height = struct.unpack(">II", png[16:24])
        self.assertEqual((width, height), (32 * len(gen.ITEMS), 32))
        self.assertEqual(set(gen.ICONS), set(gen.ITEMS))
        raw = zlib.decompress(png[png.index(b"IDAT") + 4:png.index(b"IEND") - 8])
        stride = width * 4 + 1
        for index in range(len(gen.ITEMS)):
            opaque = sum(raw[y * stride + 1 + (index * 32 + x) * 4 + 3] > 0
                         for y in range(32) for x in range(32))
            self.assertGreater(opaque, 100, gen.ITEMS[index])

    def test_day_runs_at_eight_game_seconds_per_real_second(self):
        self.assertEqual(gen.TIME_SCALE, 8.0)
        self.assertIn("const TIME_SCALE = 8.0;", self.scripts["island"])
        self.assertIn("island::time_scale()", self.scripts["world"])
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
