#!/usr/bin/env python3
"""Fetch the pinned Khronos Sponza sample; verify/reuse existing files. Stdlib only."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import time
import urllib.request

REPOSITORY = "KhronosGroup/glTF-Sample-Assets"
REVISION = "90d7ede14c7e280af263824604b427a1ca02cb66"
ROOT = Path(__file__).resolve().parents[1] / "work/sponza"
PREFIX = "Models/Sponza/"


def fetch(url):
    for attempt in range(4):
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                return response.read()
        except OSError:
            if attempt == 3:
                raise
            time.sleep(2 ** attempt)


def download(entry):
    path = ROOT / entry["path"].removeprefix(PREFIX)
    if not path.resolve().is_relative_to(ROOT.resolve()):
        raise ValueError(f"Unsafe upstream path: {path}")

    def valid(data):
        header = b"blob " + str(len(data)).encode() + b"\0"
        return (len(data) == entry["size"]
                and hashlib.sha1(header + data).hexdigest() == entry["sha"])

    if path.is_file() and valid(path.read_bytes()):
        return
    data = fetch(f"https://raw.githubusercontent.com/{REPOSITORY}/{REVISION}/{entry['path']}")
    if not valid(data):
        raise ValueError(f"Upstream size/hash mismatch: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".part")
    temporary.write_bytes(data)
    temporary.replace(path)
    print(f"Downloaded {path.relative_to(ROOT)}", flush=True)


def main():
    tree = json.loads(fetch(f"https://api.github.com/repos/{REPOSITORY}/git/trees/{REVISION}?recursive=1"))
    if tree.get("truncated") or tree.get("sha") != REVISION:
        raise ValueError("Incomplete or unexpected upstream tree")
    files = [entry for entry in tree["tree"]
             if entry["type"] == "blob" and (
                 (entry["path"].startswith(PREFIX) and "/screenshot/" not in entry["path"])
                 or entry["path"] in ("LICENSES/CC-BY-4.0.txt", "LICENSES/LicenseRef-CRYENGINE-Agreement.txt"))]
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        list(pool.map(download, files))
    model = json.loads((ROOT / "glTF/Sponza.gltf").read_text())
    for item in model.get("buffers", []) + model.get("images", []):
        path = ROOT / "glTF" / item["uri"]
        if not path.is_file() or ("byteLength" in item and path.stat().st_size != item["byteLength"]):
            raise ValueError(f"Missing or invalid glTF resource: {path}")
    (ROOT / "SOURCE.json").write_text(json.dumps({
        "repository": REPOSITORY, "commit": REVISION, "files": files,
    }, indent=2) + "\n")
    print(f"Verified {len(files)} upstream files, {len(model['images'])} images, all glTF references")
    print(ROOT / "glTF/Sponza.gltf")


if __name__ == "__main__":
    main()
