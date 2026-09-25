"""Cheap layout checks before native compilation in CI."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "apps/bozz-torio/tools/package.py"
spec = importlib.util.spec_from_file_location("bozz_torio_package", SCRIPT)
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackageInventoryTests(unittest.TestCase):
    def test_scene_catalog_is_checked_after_relocation(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for path in ["bozz-torio", "libsteam_api.so", "steam_appid.txt"]:
                (root / path).write_bytes(b"stub")
            (root / "scene").mkdir()
            (root / "assets").mkdir()
            for name in package.ASSETS:
                (root / "assets" / name).write_bytes(b"stub")
            scene = {"assets": {"sprites": {"kind": "image", "path": "../assets/sprites.png"}}}
            path = root / "scene/bozz-torio.json"
            path.write_text(json.dumps(scene), encoding="utf-8")
            package.validate_inventory(root, "bozz-torio", "libsteam_api.so")
            (root / "assets/sprites.png").unlink()
            with self.assertRaisesRegex(FileNotFoundError, "sprites"):
                package.validate_inventory(root, "bozz-torio", "libsteam_api.so")
            scene["assets"]["sprites"]["path"] = "../../outside.png"
            path.write_text(json.dumps(scene), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "escapes"):
                package.validate_inventory(root, "bozz-torio", "libsteam_api.so")


if __name__ == "__main__":
    unittest.main()
