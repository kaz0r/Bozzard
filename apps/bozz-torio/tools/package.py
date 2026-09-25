#!/usr/bin/env python3
"""Build a self-contained native Steam depot folder for Bozz-torio."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


ASSETS = (
    "sprites.png", "conveyor_preview.png", "conveyor_animation.png",
    "furnace_animation.png", "generator_animation.png", "progress_pixel.png",
    "sprite_manifest.json",
)


def validate_inventory(directory: Path, binary_name: str, library: str) -> None:
    """Check the actual scene catalog after relocation, not just a hardcoded copy list."""
    for relative in (binary_name, library, "steam_appid.txt", "scene/bozz-torio.json"):
        if not (directory / relative).is_file():
            raise FileNotFoundError(f"Packaged game is missing {relative}")
    scene_path = directory / "scene/bozz-torio.json"
    scene = json.loads(scene_path.read_text(encoding="utf-8"))
    for asset_id, asset in scene["assets"].items():
        relative = Path(asset["path"])
        resolved = (scene_path.parent / relative).resolve()
        if not resolved.is_relative_to(directory.resolve()):
            raise ValueError(f"Scene asset {asset_id} escapes the package: {relative}")
        if not resolved.is_file():
            raise FileNotFoundError(f"Scene asset {asset_id} is missing: {relative}")
    for name in ASSETS:
        if not (directory / "assets" / name).is_file():
            raise FileNotFoundError(f"Packaged game is missing assets/{name}")


def verify_relocated(directory: Path, binary_name: str, library: str) -> None:
    with tempfile.TemporaryDirectory(prefix="bozz-torio-package-") as temporary:
        root = Path(temporary)
        relocated = root / "relocated"
        shutil.copytree(directory, relocated)
        validate_inventory(relocated, binary_name, library)
        elsewhere = root / "unrelated-working-directory"
        elsewhere.mkdir()
        save_dir = root / "isolated-save"
        scene = relocated / "scene/bozz-torio.json"
        binary = relocated / binary_name
        route = [str(binary), "--offline", "--scene", str(scene),
                 "--save-dir", str(save_dir), "--verify-factory-route"]
        result = subprocess.run(route, cwd=elsewhere, capture_output=True, text=True, timeout=90)
        if result.returncode or "factory_route_ok" not in result.stdout:
            raise RuntimeError(f"Relocated factory route failed: {result.stdout}\n{result.stderr}")
        screenshot = root / "factory.png"
        smoke = [str(binary), "--offline", "--play", "--scene", str(scene),
                 "--save-dir", str(save_dir), "--screenshot", str(screenshot)]
        result = subprocess.run(smoke, cwd=elsewhere, capture_output=True, text=True, timeout=90)
        if result.returncode:
            raise RuntimeError(f"Relocated window smoke failed: {result.stdout}\n{result.stderr}")
        if not screenshot.is_file() or not screenshot.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"):
            raise RuntimeError("Relocated game did not produce a PNG screenshot")
        print("relocated_factory_ok")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app-id", type=int, help="The real Steamworks App ID")
    parser.add_argument("--out", type=Path, required=True, help="Destination depot directory")
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--offline", action="store_true", help="Use Cargo's offline cache")
    parser.add_argument("--verify", action="store_true", help="Run the relocated offline route and screenshot smoke")
    parser.add_argument("--verify-only", action="store_true", help="Verify an existing factory export")
    args = parser.parse_args()
    if not args.verify_only and args.app_id is None:
        parser.error("--app-id is required when creating a package")
    if args.app_id is not None and args.app_id <= 0:
        parser.error("--app-id must be positive")

    system = platform.system()
    binary_name = "bozz-torio.exe" if system == "Windows" else "bozz-torio"
    library = {"Windows": "steam_api64.dll", "Linux": "libsteam_api.so", "Darwin": "libsteam_api.dylib"}.get(system)
    if library is None:
        parser.error(f"Unsupported host: {system}")
    output = args.out.resolve()
    if args.verify_only:
        verify_relocated(output, binary_name, library)
        return

    workspace = Path(__file__).resolve().parents[3]
    target = Path(os.environ.get("CARGO_TARGET_DIR", workspace / "target")).resolve()
    command = ["cargo", "build", "-p", "bozz-torio", "--locked"]
    if args.profile == "release":
        command.append("--release")
    if args.offline:
        command.append("--offline")
    subprocess.run(command, cwd=workspace, check=True)

    built = target / args.profile
    output.mkdir(parents=True, exist_ok=True)
    for name in (binary_name, library):
        source = built / name
        if not source.is_file():
            raise FileNotFoundError(f"Missing build artifact: {source}")
        shutil.copy2(source, output / name)
    (output / "scene").mkdir(exist_ok=True)
    (output / "assets").mkdir(exist_ok=True)
    shutil.copy2(workspace / "apps/bozz-torio/scene/bozz-torio.json", output / "scene/bozz-torio.json")
    shutil.copy2(workspace / "apps/bozz-torio/assets/sprites.png", output / "assets/sprites.png")
    shutil.copy2(workspace / "apps/bozz-torio/assets/conveyor_preview.png", output / "assets/conveyor_preview.png")
    for name in ("conveyor_animation.png", "furnace_animation.png", "generator_animation.png", "progress_pixel.png"):
        shutil.copy2(workspace / "apps/bozz-torio/assets" / name, output / "assets" / name)
    shutil.copy2(workspace / "apps/bozz-torio/assets/sprite_manifest.json", output / "assets/sprite_manifest.json")
    (output / "steam_appid.txt").write_text(f"{args.app_id}\n", encoding="ascii")
    validate_inventory(output, binary_name, library)
    if args.verify:
        verify_relocated(output, binary_name, library)
    print(f"Packaged {binary_name}, {library}, editable scene, sprites, and app {args.app_id} into {output}")


if __name__ == "__main__":
    main()
