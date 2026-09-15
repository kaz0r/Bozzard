#!/usr/bin/env python3
"""Rebuild the original, deliberately small middleware test atlas and nine-slice panel."""
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "examples/demo/scenes/assets"


def png(path, width, height, pixel):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = b"".join(b"\0" + bytes(c for x in range(width) for c in pixel(x, y)) for y in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def atlas(x, y):
    frame, x = divmod(x, 32)
    if frame == 4:
        return (59, 151, 141, 255) if y < 5 else ((28, 56, 79, 255) if (x // 8 + y // 8) % 2 else (35, 69, 90, 255))
    # Four bobbing, blinking courier frames, with transparent gutters.
    y -= (0, -2, 0, 1)[frame]
    if 8 <= x < 24 and 9 <= y < 24:
        if y in (14, 15) and x in (11, 12, 19, 20):
            return (19, 42, 58, 255)
        if frame == 2 and y == 14 and 10 <= x < 22:
            return (19, 42, 58, 255)
        return (107, 239, 194, 255)
    if 10 <= x < 22 and 24 <= y < 27:
        return (239, 178, 87, 255)
    if x in (15, 16) and 5 <= y < 9:
        return (239, 178, 87, 255)
    return (0, 0, 0, 0)


def panel(x, y):
    distance = min(x, y, 31 - x, 31 - y)
    if distance < 2:
        return (75, 165, 165, 255)
    if distance < 4:
        return (32, 67, 87, 255)
    return (16, 32, 49, 246)


if __name__ == "__main__":
    png(ROOT / "middleware-atlas.png", 160, 32, atlas)
    png(ROOT / "middleware-panel.png", 32, 32, panel)
