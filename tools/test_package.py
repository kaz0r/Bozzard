"""Check development archives without requiring native binaries for every OS."""

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package


class PackageTests(unittest.TestCase):
    def test_steam_library_is_adjacent_to_every_executable(self):
        for system, machine, library in [
            ("Darwin", "arm64", "libsteam_api.dylib"),
            ("Linux", "x86_64", "libsteam_api.so"),
            ("Windows", "AMD64", "steam_api64.dll"),
        ]:
            for steam in (True, False):
                with self.subTest(system=system, steam=steam):
                    self.check_archive(system, machine, library, steam)

    def check_archive(self, system, machine, library, steam):
        with tempfile.TemporaryDirectory(prefix="bozzard-package-test-") as temporary:
            root = Path(temporary)
            build = root / "target/release"
            build.mkdir(parents=True)
            suffix = ".exe" if system == "Windows" else ""
            names = ("bozzard-player", "bozzard-server", "bozzard-editor")
            for name in names:
                (build / f"{name}{suffix}").write_bytes(name.encode())
            # A leftover SDK must not leak into a non-Steam package.
            payload = b"Steam SDK redistributable fixture"
            (build / library).write_bytes(payload)
            scenes = root / "examples/demo/scenes"
            scenes.mkdir(parents=True)
            for scene in (package.ROOT / "examples/demo/scenes").glob("*.json"):
                (scenes / scene.name).write_text("{}", encoding="utf-8")
            (scenes / "assets").mkdir()
            (scenes / "assets/fixture.txt").write_text("asset", encoding="utf-8")
            runtime = {"steam_library": {"name": library} if steam else None}
            with (
                patch.object(package, "ROOT", root),
                patch.object(package.platform, "system", return_value=system),
                patch.object(package.platform, "machine", return_value=machine),
                patch.object(package.subprocess, "check_output", return_value=json.dumps(runtime).encode()),
                patch("sys.argv", ["package.py"]),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                package.main()

            folder = f"bozzard-{system.lower()}-{machine.lower()}"
            with zipfile.ZipFile(root / "dist" / f"{folder}.zip") as archive:
                extracted = root / "extracted"
                archive.extractall(extracted)
            bundle = extracted / folder
            executables = [bundle / f"{name}{suffix}" for name in names]
            if system == "Darwin":
                executables[0] = bundle / "Bozzard.app/Contents/MacOS/bozzard-player"
                executables[2] = bundle / "Bozzard Editor.app/Contents/MacOS/bozzard-editor"
            for executable in executables:
                self.assertTrue(executable.is_file(), str(executable))
                adjacent = executable.parent / library
                if steam:
                    self.assertTrue(adjacent.is_file(), str(adjacent))
                    self.assertEqual(adjacent.read_bytes(), payload)
                else:
                    self.assertFalse(adjacent.exists(), str(adjacent))


if __name__ == "__main__":
    unittest.main()
