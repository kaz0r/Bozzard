#!/usr/bin/env python3
"""Build a self-contained native Steam depot folder for Bozz-torio."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app-id", type=int, required=True, help="The real Steamworks App ID")
    parser.add_argument("--out", type=Path, required=True, help="Destination depot directory")
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--offline", action="store_true", help="Use Cargo's offline cache")
    args = parser.parse_args()
    if args.app_id <= 0:
        parser.error("--app-id must be positive")

    workspace = Path(__file__).resolve().parents[3]
    target = Path(os.environ.get("CARGO_TARGET_DIR", workspace / "target")).resolve()
    command = ["cargo", "build", "-p", "bozz-torio", "--locked"]
    if args.profile == "release":
        command.append("--release")
    if args.offline:
        command.append("--offline")
    subprocess.run(command, cwd=workspace, check=True)

    system = platform.system()
    binary_name = "bozz-torio.exe" if system == "Windows" else "bozz-torio"
    library = {"Windows": "steam_api64.dll", "Linux": "libsteam_api.so", "Darwin": "libsteam_api.dylib"}.get(system)
    if library is None:
        parser.error(f"Unsupported host: {system}")
    built = target / args.profile
    output = args.out.resolve()
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
    shutil.copy2(workspace / "apps/bozz-torio/assets/sprite_manifest.json", output / "assets/sprite_manifest.json")
    (output / "steam_appid.txt").write_text(f"{args.app_id}\n", encoding="ascii")
    print(f"Packaged {binary_name}, {library}, editable scene, sprites, and app {args.app_id} into {output}")


if __name__ == "__main__":
    main()
