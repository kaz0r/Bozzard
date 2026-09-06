#!/usr/bin/env python3
"""Bundle the native demo binaries and optionally verify a clean extraction.

This is a foundation for exporting; it does not yet cook assets or build user games.
Only Python's standard library is required. Run Cargo's native build first.
"""

import argparse
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def run(binary, *args, cwd):
    subprocess.run([str(binary), *args], cwd=cwd, check=True, timeout=90)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--window", action="store_true", help="Also present 3 frames; needs a desktop session")
    parser.add_argument("--editor-window", action="store_true", help="Verify the native editor workflow and capture its UI")
    parser.add_argument("--backend", choices=["metal", "vulkan", "dx12"])
    adapter = parser.add_mutually_exclusive_group()
    adapter.add_argument("--software", action="store_true")
    adapter.add_argument("--hardware", action="store_true")
    args = parser.parse_args()
    if (args.window or args.editor_window) and not args.verify:
        parser.error("window checks require --verify")

    system = platform.system().lower()
    machine = platform.machine().lower()
    suffix = ".exe" if system == "windows" else ""
    folder = f"bozzard-{system}-{machine}"
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    archive_path = dist / f"{folder}.zip"
    with tempfile.TemporaryDirectory(prefix="bozzard-package-") as temporary:
        stage = Path(temporary) / folder
        stage.mkdir()
        player_relative = Path(f"bozzard-player{suffix}")
        editor_relative = Path(f"bozzard-editor{suffix}")
        if system == "darwin":
            player_relative = Path("Bozzard.app/Contents/MacOS/bozzard-player")
            editor_relative = Path("Bozzard Editor.app/Contents/MacOS/bozzard-editor")
            for app_name, executable, identifier in [("Bozzard", "bozzard-player", "dev.bozzard.player"), ("Bozzard Editor", "bozzard-editor", "dev.bozzard.editor")]:
                info = stage / f"{app_name}.app/Contents/Info.plist"
                info.parent.mkdir(parents=True)
                with info.open("wb") as file:
                    plistlib.dump({
                        "CFBundleExecutable": executable,
                        "CFBundleIdentifier": identifier,
                        "CFBundleName": app_name,
                        "CFBundlePackageType": "APPL",
                        "CFBundleShortVersionString": "0.1.0",
                        "CFBundleVersion": "1",
                        "NSHighResolutionCapable": True,
                    }, file)
        server_relative = Path(f"bozzard-server{suffix}")
        for name, relative in [("bozzard-player", player_relative), ("bozzard-server", server_relative), ("bozzard-editor", editor_relative)]:
            destination = stage / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / "target" / args.profile / f"{name}{suffix}", destination)
        for scene in ["scene-lab.json", "asset-lab.json"]:
            shutil.copy2(ROOT / "examples/demo/scenes" / scene, stage / scene)
        shutil.copytree(ROOT / "examples/demo/scenes/assets", stage / "assets")
        (stage / "README.txt").write_text(
            "Bozzard engine foundation demo\n\n"
            "Launch Bozzard Editor to create/edit objects, import assets, save, and Play/Stop.\n"
            "Launch the player for a native WebGPU scene; Escape closes it.\n"
            "1/2: 2D/3D. Space: pause. Arrows: pan. F5: save. R: reload.\n"
            "The server runs 120 simulation ticks and exits (no networking yet).\n"
            "The default scene, shaders and procedural textures are embedded.\n"
            "Use --scene scene-lab.json to load the included editable copy.\n"
            "Use --scene asset-lab.json for imported PNG textures and OBJ meshes.\n"
            "File edits reload automatically; failed imports retain the last good asset.\n"
            "Requires the host OS graphics drivers and system runtime libraries.\n"
            "This development bundle is not notarized or distribution-ready.\n",
            encoding="utf-8",
        )
        with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for file in sorted(stage.rglob("*")):
                if file.is_file():
                    archive.write(file, file.relative_to(stage.parent))

    print(f"package={archive_path}", flush=True)
    if args.verify:
        with tempfile.TemporaryDirectory(prefix="bozzard-verify-") as temporary:
            extracted = Path(temporary)
            with zipfile.ZipFile(archive_path) as archive:
                archive.extractall(extracted)
            package = extracted / folder
            player, server, editor = package / player_relative, package / server_relative, package / editor_relative
            # Python's zip extractor does not restore executable bits.
            if system != "windows":
                player.chmod(0o755)
                server.chmod(0o755)
                editor.chmod(0o755)
            cwd = extracted / "empty-working-directory"
            cwd.mkdir()
            graphics = (["--backend", args.backend] if args.backend else [])
            if args.software:
                graphics.append("--software")
            if args.hardware:
                graphics.append("--hardware")
            run(server, "--ticks", "120", cwd=cwd)
            run(player, "--help", cwd=cwd)
            run(editor, "--help", cwd=cwd)
            saved = cwd / "saved-scene.json"
            run(server, "--scene", str(package / "asset-lab.json"), "--ticks", "120", "--save-scene", str(saved), cwd=cwd)
            run(player, "--scene", str(saved), "--smoke", "--output", str(ROOT / "work/package-smoke"), *graphics, cwd=cwd)
            if args.window:
                run(player, "--scene", str(package / "asset-lab.json"), "--frames", "3", *graphics, cwd=cwd)
                run(player, "--scene", str(package / "asset-lab.json"), "--view", "2d", "--frames", "3", *graphics, cwd=cwd)
            if args.editor_window:
                # Keep the editor snapshot on the same filesystem root as its source assets.
                editor_output = cwd / "editor-smoke"
                try:
                    run(editor, "--scene", str(package / "asset-lab.json"), "--smoke", str(editor_output), *graphics, cwd=cwd)
                finally:
                    diagnostics = ROOT / "work/editor-package-smoke"
                    diagnostics.mkdir(parents=True, exist_ok=True)
                    for capture in editor_output.glob("*.ppm"):
                        shutil.copy2(capture, diagnostics / capture.name)
        print("package_ok: extracted executables ran from an empty working directory", flush=True)


if __name__ == "__main__":
    main()
