"""Generate an editor-native, voxel-styled Earth factory scene and reusable prefabs."""

from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCENES = ROOT / "scenes"
ASSETS = SCENES / "assets"


def bake_ui_mask():
    """Small antialiased white rounded rectangle, stretched with the engine's nine-slice UI."""
    size, radius, samples = 64, 16, 4
    pixels = bytearray()
    for y in range(size):
        pixels.append(0)  # PNG row filter
        for x in range(size):
            covered = 0
            for sy in range(samples):
                for sx in range(samples):
                    px, py = x + (sx + 0.5) / samples, y + (sy + 0.5) / samples
                    dx = max(radius - px, px - (size - radius), 0)
                    dy = max(radius - py, py - (size - radius), 0)
                    covered += dx * dx + dy * dy <= radius * radius
            pixels.extend((255, 255, 255, round(255 * covered / (samples * samples))))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(pixels)))
    png += chunk(b"IEND", b"")
    (ASSETS / "ui-rounded.png").write_bytes(png)


def factory_ui():
    cream = [0.94, 0.97, 0.92, 1]
    muted = [0.62, 0.74, 0.68, 1]
    panel = [0.055, 0.09, 0.078, 0.88]
    accent = [0.74, 0.28, 0.11, 1]
    objects = [{"id": "factory-ui", "name": "Factory HUD", "transform": transform(),
                "ui_canvas": {"layer": "3d", "reference": [1080, 600], "scaling": "fit", "order": 20}}]

    def widget(name, parent, pos, size, text="", font=14, color=None,
               background=None, anchor=(0, 0), pivot=(0, 0), padding=(0, 0, 0, 0), order=0):
        content = {
            "kind": "label" if text else "panel", "text": text, "font_size": font,
            "anchors": {"min": anchor, "max": anchor, "pivot": pivot, "offset": pos, "size": size},
            "text_color": color or cream, "background": background or [0, 0, 0, 0],
            "padding": padding, "auto_text_height": False, "clip_children": False, "order": order,
        }
        if background:
            content.update(image="ui-rounded", border=[16, 16, 16, 16])
        objects.append({"id": name, "name": name.replace("-", " ").title(), "parent": parent,
                        "transform": transform(), "ui_widget": content})

    widget("objective-shadow", "factory-ui", (24, 29), (400, 180), background=[0, 0, 0, 0.18])
    widget("objective-panel", "factory-ui", (24, 24), (400, 180), background=panel, order=1)
    widget("objective-title", "objective-panel", (22, 18), (240, 32), "Objective", 27)
    widget("objective-tier", "objective-panel", (298, 20), (80, 28), "Tier 1", 16,
           background=[0.16, 0.23, 0.19, 0.9], padding=(18, 5, 4, 0))
    widget("objective-text", "objective-panel", (22, 62), (356, 28), "Press Play to start your factory", 18)
    widget("objective-next", "objective-panel", (22, 101), (270, 20), "ASSEMBLY MILESTONE", 14, muted)
    widget("objective-count", "objective-panel", (320, 97), (64, 24), "0 / 8", 17)
    widget("objective-track", "objective-panel", (22, 132), (356, 12), background=[0.018, 0.03, 0.025, 0.95])
    widget("objective-fill", "objective-panel", (22, 132), (0, 12), background=accent, order=2)
    widget("objective-note", "objective-panel", (22, 153), (356, 20), "Production continues while you build.", 14, muted)

    widget("world-panel", "factory-ui", (-24, 24), (206, 142), background=panel, anchor=(1, 0), pivot=(1, 0))
    widget("world-status", "world-panel", (18, 16), (172, 24), "EARTH  /  DAY", 16)
    widget("stored-iron", "world-panel", (18, 51), (172, 22), "Iron       0", 16, muted)
    widget("stored-copper", "world-panel", (18, 74), (172, 22), "Copper     0", 16, muted)
    widget("stored-parts", "world-panel", (18, 97), (172, 22), "Parts      0", 16)
    widget("power-status", "factory-ui", (-24, 175), (206, 34), "POWER  9 / 18", 14, muted,
           panel, anchor=(1, 0), pivot=(1, 0), padding=(18, 9, 8, 0))

    widget("build-panel", "factory-ui", (0, -22), (760, 132), background=panel, anchor=(0.5, 1), pivot=(0.5, 1))
    names = ["Miner", "Belt", "Smelter", "Storage", "Assembler", "Generator", "Splitter", "Merger"]
    for i, name in enumerate(names):
        key = str(i + 1)
        widget("slot-" + key, "build-panel", (18 + i * 91, 12), (87, 58),
               background=[0.13, 0.19, 0.16, 0.80])
        widget("slot-key-" + key, "slot-" + key, (10, 6), (60, 18), key, 14, muted)
        widget("slot-name-" + key, "slot-" + key, (8, 29), (78, 22), name, 15)
    widget("build-status", "build-panel", (22, 83), (250, 22), "MINER  /  Facing East", 14)
    widget("controls-hint", "build-panel", (280, 83), (466, 22), "WASD Move   Space Build   R Rotate machine   X Remove", 14, muted)
    widget("camera-hint", "build-panel", (22, 108), (720, 20), "E  Open nearby storage      Ctrl + R  Turn camera      N  New world", 14, muted)
    widget("build-message", "factory-ui", (0, -187), (720, 26), "Start Play to bring this factory to life.", 16,
           anchor=(0.5, 1), pivot=(0.5, 0))

    widget("nearby-tooltip", "factory-ui", (0, -12), (160, 36), "Miner: Quartz", 16,
           background=[0.024, 0.034, 0.029, 0.96], pivot=(0.5, 1), padding=(14, 8, 10, 0), order=100)
    objects[-1]["ui_widget"]["visible"] = False

    # A script-driven modal. Slot buttons supply hit targets; labels inherit their input.
    widget("storage-overlay", "factory-ui", (0, 0), (0, 0), background=[0.01, 0.02, 0.015, 0.62], order=200)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    objects[-1]["ui_widget"]["visible"] = False
    widget("storage-panel", "storage-overlay", (0, 24), (640, 500), background=[0.045, 0.075, 0.06, 0.99],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("storage-title", "storage-panel", (26, 20), (460, 34), "Storage", 27)
    widget("storage-subtitle", "storage-panel", (26, 63), (565, 23), "16 slots  /  100 items per stack", 15, muted)
    widget("storage-close", "storage-panel", (536, 24), (78, 32), "Close  E", 14, cream,
           [0.16, 0.23, 0.19, 1], padding=(12, 7, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["E"])
    for slot in range(16):
        name = "inventory-slot-" + str(slot)
        widget(name, "storage-panel", (26 + (slot % 4) * 150, 104 + (slot // 4) * 88), (138, 78),
               background=[0.10, 0.15, 0.12, 1])
        objects[-1]["ui_widget"].update(kind="button", accessible_name="Storage slot " + str(slot + 1))
        widget(name + "-icon", name, (12, 11), (14, 14), background=[0.24, 0.31, 0.26, 1])
        widget(name + "-name", name, (12, 32), (119, 36), "Empty", 14, muted)
        widget(name + "-count", name, (79, 8), (48, 22), "", 17)
    widget("storage-help", "storage-panel", (26, 465), (590, 23),
           "Drag to move or merge  •  Right-click for stack actions", 14, muted)
    widget("stack-menu", "storage-overlay", (0, 0), (192, 137), background=[0.025, 0.045, 0.033, 1], order=30)
    objects[-1]["ui_widget"]["visible"] = False
    widget("stack-menu-title", "stack-menu", (12, 12), (168, 26), "Stack", 14, muted)
    for name, label, y, color in [("stack-split", "Split", 44, cream), ("stack-delete", "Delete all", 86, [1, 0.6, 0.48, 1])]:
        widget(name, "stack-menu", (8, y), (176, 36), label, 16, color,
               [0.12, 0.18, 0.14, 1], padding=(12, 8, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("stack-drag", "storage-overlay", (12, 12), (152, 68), "", 15, cream,
           [0.25, 0.35, 0.27, 0.95], padding=(12, 12, 8, 0), order=40)
    objects[-1]["ui_widget"].update(kind="label", visible=False)
    return objects


def transform(x=0, y=0, z=0, sx=1, sy=1, sz=1):
    return {
        "translation": [x, y, z],
        "rotation_degrees": [0, 0, 0],
        "scale": [sx, sy, sz],
    }


def cube(object_id, name, pos, size, color, parent=None):
    obj = {
        "id": object_id,
        "name": name,
        "transform": transform(*pos, *size),
        "drawable": {
            "layer": "3d",
            "mesh": "cube",
            "texture": "white",
            "color": color,
            "uv_scale": [1, 1],
        },
    }
    if parent:
        obj["parent"] = parent
    return obj


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def bake_ground_mesh():
    """Bake the fixed board into one imported mesh, leaving nodes and machines to Rhai."""
    grass = (
        (0.28, 0.48, 0.25),
        (0.25, 0.44, 0.23),
        (0.31, 0.50, 0.26),
        (0.29, 0.46, 0.24),
    )
    materials = {f"grass-{i}": [] for i in range(len(grass))}
    materials["earth-cliff"] = []

    def quad(material, corners):
        materials[material].append(corners)

    def block(material, x, z, top, bottom, exposed_sides):
        left, right = x - 0.495, x + 0.495
        near, far = z - 0.495, z + 0.495
        quad(material, ((left, top, near), (left, top, far),
                        (right, top, far), (right, top, near)))
        if "north" in exposed_sides:
            quad(material, ((left, bottom, near), (left, top, near),
                            (right, top, near), (right, bottom, near)))
        if "south" in exposed_sides:
            quad(material, ((right, bottom, far), (right, top, far),
                            (left, top, far), (left, bottom, far)))
        if "west" in exposed_sides:
            quad(material, ((left, bottom, far), (left, top, far),
                            (left, top, near), (left, bottom, near)))
        if "east" in exposed_sides:
            quad(material, ((right, bottom, near), (right, top, near),
                            (right, top, far), (right, bottom, far)))

    for z in range(-7, 8):
        for x in range(-7, 8):
            shade = (x * 13 + z * 7 + x * z) % len(grass)
            sides = []
            if z == -7:
                sides.append("north")
            if z == 7:
                sides.append("south")
            if x == -7:
                sides.append("west")
            if x == 7:
                sides.append("east")
            block(f"grass-{shade}", x, z, 0.08, -0.24, sides)

    # The 0.01-unit tile seams reveal this darker soil instead of the sky beneath the island.
    quad("earth-cliff", ((-7.495, -0.23, -7.495), (-7.495, -0.23, 7.495),
                         (7.495, -0.23, 7.495), (7.495, -0.23, -7.495)))

    # The irregular lower rim is static too. Its gaps and trees remain as authored.
    for z in range(-8, 9):
        for x in range(-8, 9):
            if abs(x) != 8 and abs(z) != 8:
                continue
            if (x * 7 + z * 11) % 9 == 0:
                continue
            block("earth-cliff", x, z, -0.145, -0.695,
                  ("north", "south", "west", "east"))

    mtl = ["# Generated by tools/generate_scene.py"]
    for name, color in [(f"grass-{i}", shade) for i, shade in enumerate(grass)] + [
        ("earth-cliff", (0.33, 0.30, 0.22))
    ]:
        mtl.extend((f"newmtl {name}",
                    "Kd " + " ".join(f"{channel:.3f}" for channel in color), ""))
    (ASSETS / "earth-ground.mtl").write_text("\n".join(mtl).rstrip() + "\n")

    obj = ["# Generated by tools/generate_scene.py", "mtllib earth-ground.mtl"]
    vertex = 1
    for name, quads in materials.items():
        obj.extend((f"o {name}", f"usemtl {name}"))
        for corners in quads:
            obj.extend("v " + " ".join(f"{coordinate:.3f}" for coordinate in corner)
                       for corner in corners)
            obj.append(f"f {vertex} {vertex + 1} {vertex + 2}")
            obj.append(f"f {vertex} {vertex + 2} {vertex + 3}")
            vertex += 4
    (ASSETS / "earth-ground.obj").write_text("\n".join(obj) + "\n")


def prefab(name, base_color, details):
    # The root is an unscaled pivot. Parenting details to a flattened base cube used to
    # multiply every child height by 0.22, making the whole factory look like flat tiles.
    objects = [{"id": "root", "name": name, "transform": transform()}]
    objects.append(cube("base", f"{name} base", (0, 0.11, 0),
                        (0.88, 0.22, 0.88), base_color, "root"))
    for i, (pos, size, color) in enumerate(details):
        objects.append(cube(f"detail-{i}", f"{name} detail {i+1}", pos, size, color, "root"))
    write_json(
        ASSETS / f"{name}.prefab.json",
        {"version": 1, "name": name.replace("-", " ").title(), "root": "root", "objects": objects},
    )


def node_prefabs():
    specs = {
        "iron": ([0.20, 0.31, 0.39], [0.49, 0.67, 0.74]),
        "copper": ([0.40, 0.22, 0.13], [0.88, 0.45, 0.18]),
        "limestone": ([0.51, 0.49, 0.39], [0.88, 0.84, 0.65]),
        "coal": ([0.10, 0.13, 0.17], [0.26, 0.30, 0.35]),
        "quartz": ([0.43, 0.45, 0.58], [0.78, 0.72, 0.98]),
        "oil": ([0.17, 0.15, 0.22], [0.53, 0.30, 0.60]),
        "water": ([0.13, 0.35, 0.51], [0.28, 0.70, 0.87]),
    }
    for name, (base, crystal) in specs.items():
        details = [
            ((-0.22, 0.37, -0.12), (0.32, 0.52, 0.30), crystal),
            ((0.18, 0.29, 0.10), (0.37, 0.37, 0.34), crystal),
            ((0.04, 0.46, -0.22), (0.21, 0.70, 0.19), crystal),
        ]
        if name in ("oil", "water"):
            details = [
                ((0, 0.25, 0), (0.74, 0.25, 0.74), crystal),
                ((-0.25, 0.47, -0.25), (0.13, 0.35, 0.13), base),
                ((0.25, 0.47, 0.25), (0.13, 0.35, 0.13), base),
            ]
        prefab(f"node-{name}", base, details)


def machine_prefabs():
    prefab(
        "machine-miner", [0.16, 0.23, 0.27],
        [
            ((0, 0.43, 0), (0.62, 0.43, 0.62), [0.32, 0.43, 0.48]),
            ((0.24, 0.71, 0), (0.17, 0.16, 0.45), [0.83, 0.72, 0.29]),
            ((0.37, 0.30, 0), (0.22, 0.17, 0.28), [0.08, 0.12, 0.15]),
        ],
    )
    prefab(
        "machine-belt", [0.13, 0.17, 0.20],
        [
            ((0, 0.24, 0), (0.78, 0.07, 0.60), [0.43, 0.33, 0.20]),
            ((0.24, 0.29, 0), (0.17, 0.05, 0.30), [0.89, 0.72, 0.29]),
            ((-0.26, 0.29, 0), (0.12, 0.05, 0.30), [0.89, 0.72, 0.29]),
        ],
    )
    prefab(
        "machine-smelter", [0.23, 0.24, 0.28],
        [
            ((0, 0.50, 0), (0.68, 0.63, 0.67), [0.34, 0.35, 0.39]),
            ((0.22, 0.53, -0.35), (0.25, 0.25, 0.06), [0.95, 0.39, 0.10]),
            ((-0.23, 0.91, 0.20), (0.16, 0.30, 0.17), [0.15, 0.17, 0.20]),
        ],
    )
    prefab(
        "machine-storage", [0.22, 0.27, 0.26],
        [
            ((0, 0.44, 0), (0.68, 0.47, 0.68), [0.38, 0.55, 0.39]),
            ((0, 0.70, 0), (0.78, 0.12, 0.78), [0.17, 0.23, 0.21]),
        ],
    )
    prefab(
        "machine-assembler", [0.23, 0.19, 0.34],
        [
            ((0, 0.43, 0), (0.67, 0.46, 0.67), [0.48, 0.36, 0.67]),
            ((-0.23, 0.72, 0), (0.17, 0.24, 0.46), [0.76, 0.72, 0.91]),
            ((0.23, 0.72, 0), (0.17, 0.24, 0.46), [0.76, 0.72, 0.91]),
        ],
    )
    prefab(
        "machine-generator", [0.21, 0.18, 0.13],
        [
            ((0, 0.47, 0), (0.70, 0.50, 0.64), [0.67, 0.51, 0.18]),
            ((-0.20, 0.78, 0), (0.15, 0.25, 0.46), [0.12, 0.16, 0.16]),
            ((0.20, 0.78, 0), (0.15, 0.25, 0.46), [0.12, 0.16, 0.16]),
        ],
    )
    for name, color in (("splitter", [0.24, 0.52, 0.59]), ("merger", [0.54, 0.36, 0.24])):
        prefab(
            f"machine-{name}", [0.13, 0.17, 0.20],
            [
                ((0, 0.30, 0), (0.65, 0.15, 0.65), color),
                ((0.25, 0.44, 0), (0.15, 0.16, 0.32), [0.88, 0.80, 0.45]),
            ],
        )
    write_json(
        ASSETS / "item.prefab.json",
        {
            "version": 1, "name": "Moving item", "root": "root",
            "objects": [cube("root", "Moving item", (0, 0, 0), (0.23, 0.23, 0.23), [0.85, 0.78, 0.54])],
        },
    )


def scene():
    objects = [
        {"id": "camera-rig", "name": "Camera orbit pivot (Rhai)", "transform": transform()},
        {
            "id": "camera", "name": "Isometric camera", "parent": "camera-rig",
            "transform": {
                "translation": [20, 20, 20],
                "rotation_degrees": [-35.264, 45, 0],
                "scale": [1, 1, 1],
            },
            "camera": {"projection": "orthographic", "vertical_size": 19, "near": 0.1, "far": 100},
        },
        {
            "id": "controller", "name": "Earth factory controller (Rhai)",
            "transform": transform(),
            "script_manager": {"scripts": [{"enabled": True, "script": "earth-factory"}]},
        },
        {
            "id": "ground", "name": "Baked Earth ground and cliff",
            "transform": transform(),
            "drawable": {
                "layer": "3d", "mesh": {"asset": "earth-ground"},
                "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1],
            },
        },
        cube("cursor", "Build cursor", (0, 0.115, 0), (0.94, 0.055, 0.94), [0.16, 0.95, 0.70]),
    ]
    objects.extend(factory_ui())
    # Only the trees remain separate scene objects; ground and cliff cubes are one mesh.
    for z in range(-8, 9):
        for x in range(-8, 9):
            if abs(x) != 8 and abs(z) != 8:
                continue
            if (x * 7 + z * 11) % 9 == 0:
                continue
            if (x * 3 - z * 5) % 7 == 0:
                objects.append(cube(f"tree-trunk-{x+8}-{z+8}", "Tree trunk", (x, 0.24, z), (0.23, 0.80, 0.23), [0.31, 0.22, 0.13]))
                objects.append(cube(f"tree-leaf-{x+8}-{z+8}", "Tree canopy", (x, 0.87, z), (0.88, 0.85, 0.88), [0.16, 0.36, 0.17]))
    for i, (dx, dy, dz, size, color) in enumerate([
        (0, 0, 0, (1.25, 0.28, 1.25), [0.17, 0.23, 0.29]),
        (0, 0.52, 0, (0.83, 0.88, 0.83), [0.77, 0.79, 0.71]),
        (0, 1.11, 0, (0.43, 0.30, 0.43), [0.25, 0.54, 0.69]),
    ]):
        objects.append(cube(f"pod-{i}", "Landing pod", (8 + dx, dy, 6 + dz), size, color))

    asset_names = [
        "node-iron", "node-copper", "node-limestone", "node-coal", "node-quartz", "node-oil", "node-water",
        "machine-miner", "machine-belt", "machine-smelter", "machine-storage", "machine-assembler",
        "machine-generator", "machine-splitter", "machine-merger", "item",
    ]
    assets = {name: {"kind": "prefab", "path": f"assets/{name}.prefab.json"} for name in asset_names}
    assets["earth-ground"] = {"kind": "mesh", "path": "assets/earth-ground.obj"}
    assets["ui-rounded"] = {"kind": "image", "path": "assets/ui-rounded.png"}
    assets["earth-factory"] = {"kind": "script", "path": "scripts/earth_factory.rs"}

    def scalar(kind, value):
        return {"scalar": {kind: value}}

    def list_var(kind, capacity):
        return {"list": {"element": kind, "capacity": capacity, "values": []}}

    blackboard = {
        "started": scalar("bool", False),
        "seed": scalar("number", 0),
        "cursor_x": scalar("number", 0),
        "cursor_z": scalar("number", 0),
        "selected": scalar("number", 1),
        "direction": scalar("number", 0),
        "camera_heading": scalar("number", 0),
        "camera_progress": scalar("number", 1),
        "camera_pending": scalar("number", 0),
        "clock": scalar("number", 0),
        "ticks": scalar("number", 0),
        "tooltip_cell": scalar("number", -1),
        "tooltip_alpha": scalar("number", 0),
        "objective_display": scalar("number", 0),
        "storage_open": scalar("bool", False),
        "storage_cell": scalar("number", -1),
        "storage_alpha": scalar("number", 0),
        "storage_drag": scalar("number", -1),
        "storage_menu": scalar("number", -1),
        "storage_menu_alpha": scalar("number", 0),
        "storage_revision": scalar("number", 0),
        "storage_render_revision": scalar("number", -1),
        "storage_render_cell": scalar("number", -1),
        "storage_render_drag": scalar("number", -1),
        "storage_render_menu": scalar("number", -1),
        "storage_used": scalar("number", 0),
        "storage_view": list_var("number", 32),
        "power_supply": scalar("number", 18),
        "power_demand": scalar("number", 9),
        "message": scalar("text", "Starting the demonstration factory"),
        "nodes": list_var("number", 225),
        "builds": list_var("number", 225),
        "machine_cells": list_var("number", 225),
        "facings": list_var("number", 225),
        "items": list_var("number", 225),
        "progress": list_var("number", 225),
        "assembler_iron": list_var("number", 225),
        "assembler_copper": list_var("number", 225),
        "split_state": list_var("number", 225),
        "node_visuals": list_var("text", 225),
        "build_visuals": list_var("text", 225),
        "item_visuals": list_var("text", 225),
        "counts": list_var("number", 32),
        "motion_visuals": list_var("text", 225),
        "motion_from": list_var("number", 225),
        "motion_to": list_var("number", 225),
        "motion_from_y": list_var("number", 225),
        "motion_to_y": list_var("number", 225),
        "retired_visuals": list_var("text", 225),
        "item_pool": list_var("text", 225),
    }
    for page in range(4):
        blackboard["storage_kinds_" + str(page)] = list_var("number", 900)
        blackboard["storage_amounts_" + str(page)] = list_var("number", 900)
    write_json(
        SCENES / "earth.json",
        {
            "version": 1, "name": "Earth Factory Prototype", "views": {"3d": "camera"},
            "environment": {
                "zenith": [0.14, 0.37, 0.68], "horizon": [0.65, 0.77, 0.86],
                "ground": [0.24, 0.30, 0.24], "intensity": 0.50, "background": True,
            },
            "lighting": {
                "shadows": True, "shadow_resolution": 2048,
                "shadow_bias": 0.005, "shadow_normal_bias": 0.01,
                "sun_direction": [0.46, 0.81, 0.35], "sun_color": [1.0, 0.94, 0.82],
                "sun_intensity": 2.6, "ambient_color": [0.86, 0.94, 1.0], "ambient_intensity": 0.18,
            },
            "blackboard": blackboard,
            "assets": assets,
            "objects": objects,
        },
    )


def main():
    node_prefabs()
    machine_prefabs()
    bake_ground_mesh()
    bake_ui_mask()
    scene()
    write_json(
        ROOT / "bozzard.project.json",
        {
            "version": 1, "name": "Earth Factory Prototype",
            "start_scene": "scenes/earth.json", "view": "3d", "cook": "universal",
        },
    )


if __name__ == "__main__":
    main()
