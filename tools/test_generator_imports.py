"""Check Blender script helper imports with no repository cwd on sys.path."""

import builtins
from contextlib import chdir
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock


TOOLS = Path(__file__).resolve().parent
GENERATORS = (
    "generate_ar15.py",
    "generate_pistol_shotgun.py",
    "generate_character_assets.py",
)


class HelperImportReached(Exception):
    def __init__(self, module):
        self.module = module


class GeneratorImportTests(unittest.TestCase):
    def test_each_script_loads_sibling_helper_from_an_arbitrary_cwd(self):
        original_path = sys.path[:]
        original_cwd = Path.cwd()
        original_import = builtins.__import__
        original_bpy = sys.modules.get("bpy")
        original_mathutils = sys.modules.get("mathutils")
        try:
            # Keep standard library/site paths, but remove the repo and tools
            # paths so success depends on the script's own bootstrap.
            isolated_path = [
                entry for entry in original_path
                if Path(entry or original_cwd).resolve() not in (TOOLS, TOOLS.parent)
            ]
            # Restore cwd before cleanup: Windows cannot remove the active cwd.
            with tempfile.TemporaryDirectory() as temporary, chdir(temporary):
                sys.modules["bpy"] = Mock()
                sys.modules["mathutils"] = SimpleNamespace(Vector=Mock())

                for filename in GENERATORS:
                    with self.subTest(generator=filename):
                        sys.path[:] = isolated_path
                        sys.modules.pop("blender_helpers", None)

                        def stop_after_helper_import(name, *args, **kwargs):
                            module = original_import(name, *args, **kwargs)
                            if name == "blender_helpers":
                                raise HelperImportReached(module)
                            return module

                        builtins.__import__ = stop_after_helper_import
                        source_path = TOOLS / filename
                        namespace = {
                            "__file__": str(source_path),
                            "__name__": "__main__",
                        }
                        with self.assertRaises(HelperImportReached) as reached:
                            exec(compile(source_path.read_text(), str(source_path), "exec"), namespace)
                        self.assertTrue(callable(reached.exception.module.create_extruded_profile))
                        builtins.__import__ = original_import
        finally:
            builtins.__import__ = original_import
            sys.path[:] = original_path
            sys.modules.pop("blender_helpers", None)
            if original_bpy is None:
                sys.modules.pop("bpy", None)
            else:
                sys.modules["bpy"] = original_bpy
            if original_mathutils is None:
                sys.modules.pop("mathutils", None)
            else:
                sys.modules["mathutils"] = original_mathutils


if __name__ == "__main__":
    unittest.main()
