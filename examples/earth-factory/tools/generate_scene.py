"""Generate an editor-native, voxel-styled Earth factory scene and reusable prefabs."""

from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCENES = ROOT / "scenes"
ASSETS = SCENES / "assets"


def glowing(obj, color):
    obj["shader_graph"] = {"version": 1, "name": "Indicator glow", "nodes": [
        {"id": 1, "position": [0, 0], "kind": "master", "inputs": [
            {"vector": color}, {"float": 0}, {"float": 0.8}, {"vector": color}, {"float": 1}, {"vector": [0, 1, 0]}]},
        {"id": 2, "position": [-200, 0], "kind": "vector", "inputs": [{"vector": color}]},
    ], "wires": [{"from": {"node": 2, "port": 0}, "to": {"node": 1, "port": 3}}]}
    return obj


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
    # Keep HUD reference units (including script-sized bars and tooltips), but
    # render them at 80% of the modal UI scale. Anchors still follow screen edges.
    objects = [{"id": "factory-ui", "name": "Factory HUD", "transform": transform(),
                "ui_canvas": {"layer": "3d", "reference": [1350, 750], "scaling": "fit", "order": 20}}]

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

    def scroll_text(name, parent, pos, size, text="", font=15, color=None):
        # A bounded viewport, with natural-height text, keeps long Steam names
        # and all four chat messages readable without covering nearby controls.
        viewport = name + "-scroll"
        widget(viewport, parent, pos, size, padding=(0, 0, 12, 0))
        objects[-1]["ui_widget"].update(layout="column", scrollable=True, clip_children=True)
        widget(name, viewport, (0, 0), (size[0] - 12, 0), text, font, color)
        objects[-1]["ui_widget"].update(auto_text_height=True)

    widget("objective-shadow", "factory-ui", (24, 29), (400, 180), background=[0, 0, 0, 0.18])
    widget("objective-panel", "factory-ui", (24, 24), (400, 180), background=panel, order=1)
    widget("objective-title", "objective-panel", (22, 18), (240, 32), "Objective", 27)
    widget("objective-tier", "objective-panel", (-22, 20), (80, 28), "Tier 1", 16,
           background=[0.16, 0.23, 0.19, 0.9], anchor=(1, 0), pivot=(1, 0), padding=(8, 4.5, 8, 4.5))
    objects[-1]["ui_widget"].update(text_alignment="center", auto_text_width=True)
    widget("objective-text", "objective-panel", (22, 62), (356, 28), "Press Play to start your factory", 18)
    widget("objective-next", "objective-panel", (22, 101), (270, 20), "ASSEMBLY MILESTONE", 14, muted)
    widget("objective-count", "objective-panel", (320, 97), (64, 24), "0 / 8", 17)
    widget("objective-track", "objective-panel", (22, 132), (356, 12), background=[0.018, 0.03, 0.025, 0.95])
    widget("objective-fill", "objective-panel", (22, 132), (0, 12), background=accent, order=2)
    widget("objective-note", "objective-panel", (22, 153), (356, 20), "Production continues while you build.", 14, muted)

    widget("debug-panel", "factory-ui", (24, 214), (336, 158), background=panel, order=1)
    widget("debug-fps", "debug-panel", (18, 12), (204, 27), "-- FPS", 21)
    widget("debug-title", "debug-panel", (262, 17), (60, 18), "DEBUG", 12, muted)
    widget("debug-timing", "debug-panel", (18, 47), (304, 18), "Frame -- ms   CPU draw -- ms", 13, muted)
    widget("debug-chunks", "debug-panel", (18, 67), (304, 18), "Chunks -- loaded / -- explored", 13, muted)
    widget("debug-entities", "debug-panel", (18, 87), (304, 18), "Visible entities --   Draws --", 13, muted)
    widget("debug-triangles", "debug-panel", (18, 107), (304, 18), "Triangles --   Simulating --", 13, muted)
    widget("debug-simulation", "debug-panel", (18, 127), (304, 18), "Sim --   CPU -- ms   Wait -- ms", 13, muted)

    widget("world-panel", "factory-ui", (-24, 24), (206, 142), background=panel, anchor=(1, 0), pivot=(1, 0))
    widget("world-status", "world-panel", (18, 16), (172, 24), "STELLAR-BX / DAY", 14)
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
        objects[-1]["ui_widget"].update(kind="button", accessible_name=name)
        widget("slot-key-" + key, "slot-" + key, (10, 6), (60, 18), key, 11, muted)
        widget("slot-name-" + key, "slot-" + key, (8, 29), (78, 22), name, 11)
    widget("build-status", "build-panel", (22, 83), (250, 22), "MINER  /  Facing East", 14)
    widget("controls-hint", "build-panel", (280, 83), (466, 22), "WASD Move   Space Build   R Rotate machine   X Remove", 14, muted)
    widget("camera-hint", "build-panel", (22, 108), (720, 20), "Ctrl + 1/2/3  Bars    F Gather   J Journal   I Inventory   M Map   E Interact   Ctrl+R Camera", 13, muted)
    widget("bar-title", "factory-ui", (0, -167), (720, 22), "I  /  PRODUCTION", 14,
           anchor=(0.5, 1), pivot=(0.5, 0))
    widget("chunk-status", "factory-ui", (-24, 216), (206, 40), "Region 0, 0", 13, muted,
           panel, anchor=(1, 0), pivot=(1, 0), padding=(12, 10, 0, 0))
    widget("menu-open", "factory-ui", (-24, 266), (206, 34), "Menu   Esc", 14, cream,
           panel, anchor=(1, 0), pivot=(1, 0), padding=(18, 9, 8, 0))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["Escape"])
    widget("map-open", "factory-ui", (-24, 308), (206, 34), "Map   M", 14, cream,
           panel, anchor=(1, 0), pivot=(1, 0), padding=(18, 9, 8, 0))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["M"])
    widget("zoom-hint", "factory-ui", (-24, 350), (206, 22), "Mouse wheel  /  Zoom", 13, muted,
           anchor=(1, 0), pivot=(1, 0))
    widget("build-message", "factory-ui", (0, -187), (720, 26), "Start Play to bring this factory to life.", 16,
           anchor=(0.5, 1), pivot=(0.5, 0))

    # World labels sit below the fixed HUD panels and their text.
    widget("nearby-tooltip", "factory-ui", (0, -12), (0, 36), "Miner: Quartz", 16,
           background=[0.024, 0.034, 0.029, 0.96], pivot=(0.5, 1), padding=(14, 8, 14, 8), order=-1)
    objects[-1]["ui_widget"].update(visible=False, text_alignment="center", auto_text_width=True)

    for slot in range(4):
        widget(f"coop-name-{slot}", "factory-ui", (0, -8), (0, 30), "Player", 15,
               background=[0.024, 0.034, 0.029, 0.96], pivot=(0.5, 1), padding=(12, 6, 12, 6), order=-1)
        objects[-1]["ui_widget"].update(visible=False, text_alignment="center", auto_text_width=True)

    # Reading and interaction panels retain their larger scale above the HUD.
    objects.append({"id": "factory-panels", "name": "Factory Panels", "transform": transform(),
                    "ui_canvas": {"layer": "3d", "reference": [1080, 600], "scaling": "fit", "order": 21}})
    # A script-driven modal. Slot buttons supply hit targets; labels inherit their input.
    widget("storage-overlay", "factory-panels", (0, 0), (0, 0), background=[0.01, 0.02, 0.015, 0.62], order=200)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    objects[-1]["ui_widget"]["visible"] = False
    widget("storage-panel", "storage-overlay", (0, 24), (640, 500), background=[0.045, 0.075, 0.06, 0.99],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("storage-title", "storage-panel", (26, 20), (460, 34), "Storage", 27)
    widget("storage-subtitle", "storage-panel", (26, 63), (565, 23), "16 slots  /  100 items per stack", 15, muted)
    widget("storage-close", "storage-panel", (536, 24), (78, 32), "Close  E", 14, cream,
           [0.16, 0.23, 0.19, 1], padding=(8, 7.5, 8, 7.5))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["E"], text_alignment="center")
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
    widget("storage-take", "storage-panel", (380, 63), (235, 30), "Take items into backpack", 14,
           cream, [0.16, 0.23, 0.19, 1], padding=(10, 6, 0, 0))
    objects[-1]["ui_widget"].update(kind="button")
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

    widget("world-context", "game-panels", (0, 0), (170, 128), background=[0.025, 0.045, 0.06, 1], order=200)
    objects[-1]["ui_widget"].update(visible=False)
    for name, title, y in [("link", "Link", 8), ("unlink", "Unlink", 48), ("inspect", "Inspect", 8), ("cancel", "Cancel", 88)]:
        widget("world-"+name, "world-context", (8, y), (154, 32), title, 15,
               cream, [0.10, 0.16, 0.19, 1], padding=(10, 7, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("machine-inspect-overlay", "game-panels", (0, 0), (0, 0), background=[0.008, 0.015, 0.025, 0.78], order=270)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("machine-inspect-panel", "machine-inspect-overlay", (0, 0), (640, 330), background=[0.035, 0.065, 0.08, 1], anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("machine-inspect-title", "machine-inspect-panel", (28, 25), (480, 38), "Machine / Buffer", 25)
    widget("machine-inspect-close", "machine-inspect-panel", (504, 26), (108, 32), "Close  E", 14, cream,
           [0.14, 0.21, 0.24, 1], padding=(8, 7.5, 8, 7.5))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("machine-inspect-body", "machine-inspect-panel", (28, 88), (580, 85), "Buffer empty", 22)
    widget("machine-inspect-status", "machine-inspect-panel", (28, 187), (580, 45), "", 15, muted)
    widget("machine-inspect-take", "machine-inspect-panel", (28, 260), (250, 40), "Collect items", 16, cream,
           [0.18, 0.32, 0.32, 1], padding=(12, 10, 0, 0))
    objects[-1]["ui_widget"].update(kind="button")

    # A book built from ordinary editable widgets, sized to the canvas's fit scaling.
    ink, faded = [0.20, 0.13, 0.08, 1], [0.43, 0.34, 0.24, 1]
    widget("journal-overlay", "factory-panels", (0, 0), (0, 0), background=[0.025, 0.02, 0.015, 0.7], order=250)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    objects[-1]["ui_widget"]["visible"] = False
    widget("journal-book", "journal-overlay", (0, 0), (940, 540), background=[0.22, 0.12, 0.065, 1],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    for side, x in [("left", 14), ("right", 475)]:
        widget("journal-paper-" + side, "journal-book", (x, 14), (451, 512), background=[0.91, 0.85, 0.69, 1])
    widget("journal-spine", "journal-book", (462, 20), (15, 500), background=[0.47, 0.32, 0.18, 0.4])
    widget("journal-title", "journal-book", (38, 30), (380, 38), "FIELD JOURNAL", 26, ink)
    widget("journal-subtitle", "journal-book", (38, 76), (390, 26), "EARTH  /  THE FIRST FACTORY", 13, faded)
    for page, title in enumerate(["I  Unlocks", "II  Recipes", "III  Spaceship"], 1):
        widget(f"journal-tab-{page}", "journal-book", (38 + (page - 1) * 133, 115), (127, 36), title, 14, ink,
               [0.77, 0.67, 0.48, 1], padding=(9, 10, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", accessible_name=title)
    widget("journal-left-title", "journal-book", (38, 177), (390, 28), "Your discoveries", 21, ink)
    widget("journal-left-body", "journal-book", (38, 219), (390, 247), "", 16, ink)
    widget("journal-right-title", "journal-book", (502, 68), (380, 36), "Next delivery", 23, ink)
    widget("journal-right-body", "journal-book", (502, 117), (380, 212), "", 16, ink)
    widget("journal-backpack", "journal-book", (502, 341), (380, 58), "", 14, faded)
    for row, item in enumerate([11, 12, 14, 15, 16, 17, 13, 21, 18, 20, 23, 22]):
        widget(f"journal-recipe-{item}", "journal-book", (38, 216 + row * 20), (390, 19), "Recipe", 13, ink,
               [0.84, 0.77, 0.60, 1], padding=(9, 2, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", visible=False)
    for name, label, x, y, width in [
        ("journal-deliver", "Deliver materials", 502, 418, 380),
        ("journal-craft-one", "Craft once", 502, 427, 180),
        ("journal-craft-ten", "Craft up to 10", 702, 427, 180),
    ]:
        widget(name, "journal-book", (x, y), (width, 36), label, 15, ink,
               [0.75, 0.64, 0.42, 1], padding=(12, 9, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("journal-close", "journal-book", (807, 25), (90, 30), "Close  J", 14, ink,
           [0.77, 0.67, 0.48, 1], padding=(8, 6.5, 8, 6.5))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["J"], text_alignment="center")
    widget("journal-footer", "journal-book", (38, 482), (390, 25), "J  Close    Left / Right  Turn page", 13, faded)

    # A fixed north-up map. Hidden cells are ordinary lightweight UI widgets;
    # the script updates their colors only when discovery or residency changes.
    widget("map-overlay", "factory-panels", (0, 0), (0, 0), background=[0.008, 0.015, 0.012, 0.78], order=275)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("map-panel", "map-overlay", (0, 0), (880, 540), background=[0.035, 0.065, 0.053, 0.99],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("map-title", "map-panel", (28, 22), (600, 36), "STELLAR-BX / REGION MAP", 27)
    widget("map-close", "map-panel", (748, 24), (104, 32), "Close  M", 14, cream,
           [0.16, 0.23, 0.19, 1], padding=(8, 7.5, 8, 7.5))
    objects[-1]["ui_widget"].update(kind="button", shortcuts=["M"], text_alignment="center")
    widget("map-north", "map-panel", (211, 62), (100, 20), "NORTH", 12, muted)
    for coordinate in [-8, 0, 8]:
        offset = (coordinate + 8) * 23
        label = "+8" if coordinate == 8 else str(coordinate)
        widget(f"map-axis-x-{coordinate}", "map-panel", (48 + offset, 81), (28, 16), label, 11, muted)
        widget(f"map-axis-z-{coordinate}", "map-panel", (21, 104 + offset), (28, 16), label, 11, muted)
    for z in range(17):
        for x in range(17):
            region = z * 17 + x
            widget(f"map-cell-{region}", "map-panel", (50 + x * 23, 102 + z * 23), (20, 20),
                   "H" if region == 144 else "", 12, cream, [0.018, 0.032, 0.026, 1], padding=(5, 3, 0, 0))
            objects[-1]["ui_widget"].pop("image")
            objects[-1]["ui_widget"].pop("border")
    widget("map-south", "map-panel", (211, 496), (100, 20), "SOUTH", 12, muted)
    widget("map-region", "map-panel", (496, 101), (336, 32), "Region 0, 0", 25)
    widget("map-counts", "map-panel", (496, 145), (336, 48), "1 loaded\n1 explored / 289 regions", 16, muted)
    for row, (label, color) in enumerate([
        ("Your current region", [0.64, 0.29, 0.085, 1]),
        ("Loaded", [0.13, 0.38, 0.26, 1]),
        ("Explored, currently unloaded", [0.11, 0.17, 0.20, 1]),
        ("Unexplored", [0.018, 0.032, 0.026, 1]),
    ]):
        widget(f"map-legend-{row}", "map-panel", (496, 220 + row * 40), (20, 20), background=color)
        objects[-1]["ui_widget"].pop("image")
        objects[-1]["ui_widget"].pop("border")
        widget(f"map-legend-label-{row}", "map-panel", (530, 220 + row * 40), (300, 24), label, 15, muted)
    widget("map-home-key", "map-panel", (496, 388), (336, 24), "H  Landing site / Region 0, 0", 15)
    widget("map-help", "map-panel", (496, 434), (336, 44), "Explore to reveal neighboring regions.\nFactories continue while the map is open.", 14, muted)
    widget("map-footer", "map-panel", (50, 519), (800, 18), "North stays up as the camera turns.     M  Close map     Esc  Menu", 12, muted)

    widget("menu-overlay", "factory-panels", (0, 0), (0, 0), background=[0.01, 0.02, 0.015, 0.60], order=300)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("menu-panel", "menu-overlay", (0, 0), (380, 530), background=[0.045, 0.075, 0.06, 0.98],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("menu-title", "menu-panel", (28, 24), (324, 36), "Stellar-IX", 28)
    widget("menu-subtitle", "menu-panel", (28, 69), (324, 24), "Your factory keeps running.", 16, muted)
    for name, label, y in [("continue", "Continue", 110), ("save", "Save", 166),
                           ("load", "Load", 222), ("main-menu", "Main menu", 278), ("exit", "Exit", 334)]:
        widget("menu-" + name, "menu-panel", (28, y), (324, 44), label, 18,
               cream if name != "exit" else [1, 0.66, 0.53, 1],
               [0.16, 0.23, 0.19, 1], padding=(18, 11, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", focus_order=y)
        if name == "continue":
            objects[-1]["ui_widget"]["shortcuts"] = ["Escape"]
    widget("coop-open-menu", "menu-panel", (28, 390), (324, 44), "Steam co-op / Invite friends", 18, cream, [0.16, 0.23, 0.19, 1], padding=(0, 11, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("menu-save-note", "menu-panel", (28, 458), (324, 20), "Auto-save every 20 minutes. Host saves the world.", 13, muted)
    widget("menu-exit-note", "menu-panel", (28, 484), (324, 20), "Save your progress before leaving.", 13, muted)
    # Each canvas has a parent to hide its gameplay widgets and title shortcuts.
    for canvas, group in [("factory-ui", "game-hud"), ("factory-panels", "game-panels")]:
        for obj in objects:
            if obj.get("parent") == canvas:
                obj["parent"] = group
        widget(group, canvas, (0, 0), (0, 0))
        objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
        objects[-1]["ui_widget"].update(visible=False, enabled=False)

    widget("assembler-overlay", "game-panels", (0, 0), (0, 0), background=[0.008, 0.015, 0.025, 0.75], order=260)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("assembler-panel", "assembler-overlay", (0, 0), (740, 460), background=[0.04, 0.07, 0.09, 1],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("assembler-title", "assembler-panel", (30, 24), (500, 35), "ASSEMBLER / OUTPUT RECIPE", 25)
    widget("assembler-close", "assembler-panel", (590, 25), (120, 32), "Close  E", 14, cream,
           [0.14, 0.21, 0.24, 1], padding=(8, 7.5, 8, 7.5))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("assembler-status", "assembler-panel", (30, 74), (680, 55), "", 16, muted)
    for i, (name, label) in enumerate([("alloy", "Conductive alloy"), ("parts", "Machine parts"), ("concrete", "Concrete")]):
        widget("assembler-" + name, "assembler-panel", (30 + i * 230, 140), (220, 98),
               label + "\nChoose output", 14, cream, [0.12, 0.19, 0.23, 1], padding=(16, 16, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("assembler-buffer", "assembler-panel", (30, 255), (680, 66), "", 16, muted)
    for name, label, x in [("feed", "Load ingredients", 30), ("take", "Collect output", 380)]:
        widget("assembler-" + name, "assembler-panel", (x, 339), (330, 44), label, 17, cream,
               [0.17, 0.29, 0.29, 1], padding=(16, 12, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("assembler-help", "assembler-panel", (30, 404), (690, 38),
           "Collect output before changing recipe. Unused inputs return to inventory.\nConnect a power pole to run this machine. Production continues while open.", 13, muted)

    for name, title in [("inventory", "PLAYER INVENTORY"), ("rocket", "ROCKET / DESTINATIONS"), ("dock", "FUEL DOCK / INPUT")]:
        overlay = f"player-{name}-overlay"
        panel_id = f"player-{name}-panel"
        widget(overlay, "game-panels", (0, 0), (0, 0), background=[0.008, 0.015, 0.025, 0.78], order=270)
        objects[-1]["ui_widget"].update(visible=False, enabled=False)
        objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
        widget(panel_id, overlay, (0, 0), (820, 520), background=[0.035, 0.065, 0.08, 1],
               anchor=(0.5, 0.5), pivot=(0.5, 0.5))
        widget(f"player-{name}-title", panel_id, (28, 24), (620, 38), title, 26)
        widget(f"player-{name}-close", panel_id, (684, 26), (108, 32), "Close  " + ("I" if name == "inventory" else "E"), 14,
               cream, [0.14, 0.21, 0.24, 1], padding=(8, 7.5, 8, 7.5))
        objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("player-inventory-help", "player-inventory-panel", (28, 72), (760, 26),
           "Drag to move, swap or merge / Right-click to split or destroy", 14, muted)
    for slot in range(25):
        widget(f"player-slot-{slot}", "player-inventory-panel", (28 + slot % 5 * 155, 112 + slot // 5 * 73), (145, 64),
               "", 14, cream, [0.10, 0.16, 0.19, 1], padding=(12, 9, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", accessible_name=f"Inventory slot {slot+1}")
    widget("player-inventory-footer", "player-inventory-panel", (28, 487), (490, 20), "25 slots / 100 per stack", 13, muted)
    widget("backpack-destroy-all", "player-inventory-panel", (647, 482), (145, 30), "Destroy All", 14,
           [1, 0.65, 0.52, 1], [0.24, 0.075, 0.045, 1], padding=(8, 6, 8, 6))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("backpack-menu", "player-inventory-overlay", (0, 0), (170, 128), background=[0.025, 0.045, 0.06, 1], order=30)
    objects[-1]["ui_widget"]["visible"] = False
    for name, label, y in [("split", "Split", 8), ("destroy", "Destroy", 48), ("cancel", "Cancel", 88)]:
        widget("backpack-"+name, "backpack-menu", (8, y), (154, 32), label, 15,
               cream, [0.10, 0.16, 0.19, 1], padding=(10, 7, 0, 0))
        objects[-1]["ui_widget"].update(kind="button")
    widget("backpack-drag", "player-inventory-overlay", (12, 12), (165, 40), "", 14, cream,
           [0.18, 0.32, 0.32, 0.9], padding=(10, 10, 0, 0), order=40)
    objects[-1]["ui_widget"].update(kind="label", visible=False)
    widget("rocket-current", "player-rocket-panel", (28, 79), (750, 30), "CURRENT WORLD / STELLAR-BX", 19, muted)
    widget("rocket-moon", "player-rocket-panel", (28, 124), (370, 76), "STELLA-Z2\nMoon / Next destination", 19,
           cream, [0.15, 0.30, 0.34, 1], padding=(18, 12, 0, 0))
    objects[-1]["ui_widget"].update(kind="button")
    widget("rocket-launch", "player-rocket-panel", (418, 124), (370, 76), "LAUNCH\nNo fuel required yet", 16,
           muted, [0.09, 0.14, 0.18, 1], padding=(18, 15, 0, 0))
    objects[-1]["ui_widget"].update(kind="button")
    for i in range(8):
        widget(f"rocket-future-{i}", "player-rocket-panel", (28 + i % 4 * 194, 222 + i // 4 * 72), (182, 62),
               f"Planet {i+3}\nComing soon", 15, muted, [0.065, 0.11, 0.14, 1], padding=(14, 10, 0, 0))
    widget("rocket-detail", "player-rocket-panel", (28, 392), (760, 95),
           "STELLA-Z2 / MOON\nPermanent night / Sparse lunar resources\nReturn trips available. No fuel required yet.", 17, muted)
    widget("dock-description", "player-dock-panel", (34, 117), (750, 260),
           "ROCKET FUEL INPUT\n\nThis dock is connected to the landing site.\n\nFuel production and launch requirements are still to come.\nNo materials are accepted or consumed yet.", 20, muted)

    widget("title-overlay", "factory-panels", (0, 0), (0, 0), background=[0.008, 0.014, 0.029, 1], order=400)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    objects[-1]["ui_widget"].pop("image")
    objects[-1]["ui_widget"].pop("border")
    # Deterministic, lightweight stars in the title backdrop.
    for i in range(64):
        x, y = (i * 173 + 17) % 1080, (i * 97 + 23) % 600
        widget(f"title-star-{i}", "title-overlay", (x, y), (2 if i % 5 else 3, 2 if i % 5 else 3),
               background=[0.34, 0.48, 0.65, 0.65])
    widget("title-content", "title-overlay", (0, 0), (820, 510), anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("title-kicker", "title-content", (0, 0), (820, 24), "BUILD A FACTORY. FIND YOUR WAY TO THE STARS.", 14, [0.48, 0.73, 0.76, 1])
    widget("title-name", "title-content", (0, 36), (820, 90), "Stellar-IX", 76)
    widget("title-description", "title-content", (4, 143), (810, 50),
           "One landing pod. An unexplored world. Your first factory starts here.", 19, muted)
    widget("title-new", "title-content", (4, 206), (810, 26), "CREATE A NEW WORLD", 16, cream)
    for i, (mode, label, detail) in enumerate([
        ("survival", "Survival", "Gather materials. Deliver milestones.\nUnlock your factory one step at a time."),
        ("creative", "Creative", "Every machine and recipe unlocked.\nBuild freely. Design a working power grid."),
    ]):
        widget("title-" + mode, "title-content", (4 + i * 414, 248), (398, 122), label, 25, cream,
               [0.10, 0.20, 0.24, 1], padding=(20, 16, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", focus_order=i)
        widget("title-" + mode + "-detail", "title-" + mode, (0, 44), (355, 54), detail, 15, muted)
    widget("title-create", "title-content", (4, 398), (398, 54), "Create Survival world", 20, cream,
           [0.20, 0.41, 0.40, 1], padding=(20, 16, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", focus_order=2)
    widget("title-load", "title-content", (418, 398), (192, 54), "Load world", 18, cream,
           [0.10, 0.22, 0.28, 1], padding=(20, 17, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", focus_order=3)
    widget("title-exit", "title-content", (624, 398), (180, 54), "Exit", 18, muted,
           [0.08, 0.13, 0.19, 1], padding=(20, 17, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", focus_order=3)
    widget("coop-open-title", "title-content", (4, 466), (192, 34), "Steam co-op", 16, cream, [0.10, 0.22, 0.28, 1], padding=(0, 8, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("title-note", "title-content", (212, 472), (598, 28), "Auto-save every 20 minutes. Save anytime from the game menu.", 13, muted)
    widget("coop-overlay", "factory-panels", (0, 0), (0, 0), background=[0.008, 0.014, 0.029, 0.94], order=600)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("coop-panel", "coop-overlay", (0, 0), (900, 560), background=[0.045, 0.075, 0.10, 1], anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("coop-title", "coop-panel", (28, 24), (480, 40), "STEAM CO-OP", 28)
    widget("coop-steam-overlay", "coop-panel", (552, 24), (196, 34), "Open Steam overlay", 15, cream, [0.14, 0.22, 0.26, 1], padding=(0, 8, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("coop-close", "coop-panel", (760, 24), (112, 34), "Close", 16, cream, [0.14, 0.22, 0.26, 1], padding=(0, 7, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center", shortcuts=["Escape"])
    widget("coop-status", "coop-panel", (28, 80), (588, 66), "Create a lobby to invite friends.", 16, muted)
    scroll_text("coop-members", "coop-panel", (630, 78), (242, 88), "", 16)
    x = 28
    for action, label, width in [("create", "Create lobby", 162), ("invite", "Invite friends", 158), ("friends", "Friend picker", 154), ("chat", "Chat", 100), ("leave", "Leave lobby", 148)]:
        widget("coop-" + action, "coop-panel", (x, 174), (width, 42), label, 16, cream, [0.10, 0.22, 0.28, 1], padding=(0, 11, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
        x += width + 12
    widget("coop-hint", "coop-panel", (28, 234), (844, 46), "", 15, muted)
    widget("coop-friends-list", "coop-panel", (28, 300), (382, 172), padding=(0, 0, 12, 0))
    objects[-1]["ui_widget"].update(layout="column", gap=8, scrollable=True, clip_children=True)
    for i in range(4):
        widget(f"coop-friend-{i}", "coop-friends-list", (0, 0), (370, 34), "Invite friend", 15, cream, [0.10, 0.18, 0.23, 1], padding=(12, 8, 12, 8))
        objects[-1]["ui_widget"].update(kind="button", visible=False, auto_text_height=True)
    widget("coop-friends-next", "coop-panel", (28, 478), (150, 34), "More friends", 15, cream, [0.10, 0.18, 0.23, 1], padding=(0, 8, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", visible=False, text_alignment="center")
    scroll_text("coop-lobby-log", "coop-panel", (438, 296), (432, 134), "No messages yet.", 15, muted)
    scroll_text("coop-lobby-draft", "coop-panel", (438, 438), (432, 48), "Press Enter to chat", 16)
    widget("coop-send", "coop-panel", (724, 504), (148, 34), "Send / Enter", 15, cream, [0.10, 0.22, 0.28, 1], padding=(0, 8, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", text_alignment="center")
    widget("coop-overlay-status", "coop-panel", (28, 526), (684, 20), "Checking Steam overlay…", 13, muted)
    widget("coop-chat-overlay", "factory-panels", (20, -20), (560, 180), background=[0.025, 0.045, 0.06, 0.97], anchor=(0, 1), pivot=(0, 1), order=610)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    scroll_text("coop-chat-log", "coop-chat-overlay", (16, 16), (528, 100), "", 15, muted)
    scroll_text("coop-chat-draft", "coop-chat-overlay", (16, 124), (528, 44), "Press Enter to chat", 16)
    widget("saves-overlay", "factory-panels", (0, 0), (0, 0), background=[0.008, 0.014, 0.025, 0.90], order=500)
    objects[-1]["ui_widget"].update(visible=False, enabled=False)
    objects[-1]["ui_widget"]["anchors"]["max"] = (1, 1)
    widget("saves-panel", "saves-overlay", (0, 0), (680, 568), background=[0.045, 0.075, 0.10, 1],
           anchor=(0.5, 0.5), pivot=(0.5, 0.5))
    widget("saves-title", "saves-panel", (28, 22), (450, 38), "LOAD WORLD", 26)
    widget("saves-close", "saves-panel", (540, 22), (112, 34), "Close", 16, cream, [0.14, 0.22, 0.26, 1], padding=(0, 7, 0, 0))
    objects[-1]["ui_widget"].update(kind="button", focus_order=7, text_alignment="center")
    for slot in range(6):
        widget(f"save-slot-{slot}", "saves-panel", (28, 74 + slot * 64), (624, 56),
               "Auto-save" if slot == 0 else f"Save {slot}", 18, cream, [0.085, 0.145, 0.18, 1], padding=(16, 16, 0, 0))
        objects[-1]["ui_widget"].update(kind="button", focus_order=slot)
        widget(f"save-info-{slot}", f"save-slot-{slot}", (134, -6), (444, 44), "Reading saves…", 14, muted)
    widget("saves-status", "saves-panel", (28, 472), (624, 72), "", 14, muted)
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


def bake_ground_mesh(moon=False):
    """Bake the fixed board into one imported mesh, leaving nodes and machines to Rhai."""
    grass = (
        (0.28, 0.48, 0.25),
        (0.25, 0.44, 0.23),
        (0.31, 0.50, 0.26),
        (0.29, 0.46, 0.24),
    )
    if moon:
        grass = ((0.35, 0.36, 0.39), (0.30, 0.31, 0.34), (0.39, 0.40, 0.43), (0.33, 0.34, 0.37))
    ground_name = "moon-ground" if moon else "earth-ground"
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

    # The footprint stays inside 15 x 15 so neighboring chunks tile without overlaps.

    mtl = ["# Generated by tools/generate_scene.py"]
    for name, color in [(f"grass-{i}", shade) for i, shade in enumerate(grass)] + [
        ("earth-cliff", (0.19, 0.20, 0.23) if moon else (0.33, 0.30, 0.22))
    ]:
        mtl.extend((f"newmtl {name}",
                    "Kd " + " ".join(f"{channel:.3f}" for channel in color), ""))
    (ASSETS / f"{ground_name}.mtl").write_text("\n".join(mtl).rstrip() + "\n")

    obj = ["# Generated by tools/generate_scene.py", f"mtllib {ground_name}.mtl"]
    vertex = 1
    for name, quads in materials.items():
        obj.extend((f"o {name}", f"usemtl {name}"))
        for corners in quads:
            obj.extend("v " + " ".join(f"{coordinate:.3f}" for coordinate in corner)
                       for corner in corners)
            obj.append(f"f {vertex} {vertex + 1} {vertex + 2}")
            obj.append(f"f {vertex} {vertex + 2} {vertex + 3}")
            vertex += 4
    (ASSETS / f"{ground_name}.obj").write_text("\n".join(obj) + "\n")


def prefab(name, base_color, details):
    # The root is an unscaled pivot. Parenting details to a flattened base cube used to
    # multiply every child height by 0.22, making the whole factory look like flat tiles.
    objects = [{"id": "root", "name": name, "transform": transform()}]
    objects.append(cube("base", f"{name} base", (0, 0.11, 0),
                        (0.88, 0.22, 0.88), base_color, "root"))
    if name in ("machine-miner", "machine-smelter", "machine-assembler", "machine-generator", "machine-constructor"):
        # Centered terminal remains attached when a machine turns; world height 1.08.
        objects.append(cube("power-terminal", "Cable terminal", (0, 0.83, 0),
                            (0.085, 0.34, 0.085), [0.55, 0.38, 0.17], "root"))
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
        "stone": ([0.27, 0.28, 0.27], [0.52, 0.55, 0.51]),
        "sand": ([0.52, 0.41, 0.24], [0.94, 0.78, 0.48]),
        "silver": ([0.29, 0.35, 0.42], [0.81, 0.89, 0.94]),
        "amorium": ([0.43, 0.37, 0.25], [0.88, 0.76, 0.55]),
        "moondust": ([0.39, 0.41, 0.45], [0.87, 0.90, 0.94]),
        "techtorium": ([0.025, 0.030, 0.04], [1.0, 0.38, 0.045]),
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
        if name == "moondust":
            details = [((x, y * 0.6, z), (sx * 1.15, sy * 0.5, sz * 1.15), color)
                       for ((x, y, z), (sx, sy, sz), color) in details]
        if name == "techtorium":
            details[1] = (details[1][0], details[1][1], [0.94, 0.96, 0.99])
            details.append(((0.21, 0.52, 0.09), (0.15, 0.13, 0.22), base))
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
    prefab("machine-constructor", [0.17, 0.27, 0.30], [
        ((0, 0.44, 0), (0.65, 0.46, 0.65), [0.25, 0.55, 0.57]),
        ((0, 0.75, 0), (0.54, 0.16, 0.22), [0.79, 0.71, 0.40]),
        ((0.33, 0.46, 0), (0.12, 0.22, 0.31), [0.08, 0.15, 0.17]),
    ])
    prefab(
        "machine-generator", [0.21, 0.18, 0.13],
        [
            ((0, 0.47, 0), (0.70, 0.50, 0.64), [0.67, 0.51, 0.18]),
            ((-0.20, 0.78, 0), (0.15, 0.25, 0.46), [0.12, 0.16, 0.16]),
            ((0.20, 0.78, 0), (0.15, 0.25, 0.46), [0.12, 0.16, 0.16]),
        ],
    )
    # Cyan marks an inlet, gold marks an outlet. Facing zero is +X;
    # rotating the root rotates all four physical ports with the routing rules.
    inlet, outlet = [0.24, 0.78, 0.84], [0.95, 0.73, 0.25]
    for name, color in (("splitter", [0.24, 0.52, 0.59]), ("merger", [0.54, 0.36, 0.24])):
        side_color = outlet if name == "splitter" else inlet
        prefab(
            f"machine-{name}", [0.13, 0.17, 0.20],
            [
                ((0, 0.30, 0), (0.65, 0.15, 0.65), color),
                ((-0.34, 0.39, 0), (0.18, 0.06, 0.26), inlet),
                ((0.34, 0.39, 0), (0.18, 0.06, 0.26), outlet),
                ((0, 0.39, -0.34), (0.26, 0.06, 0.18), side_color),
                ((0, 0.39, 0.34), (0.26, 0.06, 0.18), side_color),
            ],
        )
    write_json(
        ASSETS / "item.prefab.json",
        {
            "version": 1, "name": "Moving item", "root": "root",
            "objects": [cube("root", "Moving item", (0, 0, 0), (0.23, 0.23, 0.23), [0.85, 0.78, 0.54])],
        },
    )


    prefab("machine-pole", [0.12, 0.18, 0.23], [
        ((0, 0.92, 0), (0.12, 1.64, 0.12), [0.23, 0.32, 0.38]),
        ((0, 1.39, 0), (0.55, 0.09, 0.14), [0.52, 0.34, 0.18]),
        ((0, 1.69, 0), (0.25, 0.14, 0.25), [0.08, 0.13, 0.16]),
    ])
    for name, size, color in [("power-wire", (1, 0.035, 0.035), [0.14, 0.21, 0.24]),
                               ("power-lamp", (0.21, 0.12, 0.21), [0.78, 1, 0.67])]:
        obj = cube("root", name, (0, 0, 0), size, color)
        if name == "power-lamp":
            obj["shader_graph"] = {"version": 1, "name": "Powered lamp", "nodes": [
                {"id": 1, "position": [0, 0], "kind": "master", "inputs": [
                    {"vector": color}, {"float": 0}, {"float": 0.8},
                    {"vector": [0.9, 1.5, 0.6]}, {"float": 1}, {"vector": [0, 1, 0]}]},
                {"id": 2, "position": [-200, 0], "kind": "vector", "inputs": [{"vector": [0.9, 1.5, 0.6]}]},
            ], "wires": [{"from": {"node": 2, "port": 0}, "to": {"node": 1, "port": 3}}]}
        write_json(ASSETS / f"{name}.prefab.json", {"version": 1, "name": name, "root": "root", "objects": [obj]})


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
            "steam_coop": {"app_id": 480, "max_players": 4},
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
    for slot, color in enumerate([[0.12, 0.44, 1.0], [0.95, 0.16, 0.18], [1.0, 0.49, 0.08], [0.18, 0.83, 0.34]]):
        marker = cube(f"coop-player-{slot}", "Remote player", (0, -1000, 0), (0.84, 0.10, 0.84), color)
        objects.append(marker)
    objects.append({"id": "hover-tile", "name": "Mouse placement outline", "transform": transform()})
    for i, (position, size) in enumerate([
        ((-0.48, 0, 0), (0.04, 0.025, 1.0)), ((0.48, 0, 0), (0.04, 0.025, 1.0)),
        ((0, 0, -0.48), (1.0, 0.025, 0.04)), ((0, 0, 0.48), (1.0, 0.025, 0.04)),
    ]):
        objects.append(glowing(cube(f"hover-edge-{i}", "Placement outline", position, size, [0.35, 1.0, 0.78], parent="hover-tile"), [0.35, 1.0, 0.78]))
    # Reuse unshadowed lights for nearby poles; the finished rocket borrows slot 31.
    for i in range(32):
        objects.append({"id": f"pole-light-{i}", "name": "Pooled pole illumination", "transform": transform(),
                        "light": {"kind": "point", "color": [0.72, 1.0, 0.59], "intensity": 0, "range": 3.5}})
    objects.extend(factory_ui())
    for i, (dx, dy, dz, size, color) in enumerate([
        (0, 0, 0, (1.25, 0.28, 1.25), [0.17, 0.23, 0.29]),
        (0, 0.52, 0, (0.83, 0.88, 0.83), [0.77, 0.79, 0.71]),
        (0, 1.11, 0, (0.43, 0.30, 0.43), [0.25, 0.54, 0.69]),
    ]):
        objects.append(cube(f"pod-{i}", "Landing pod", (dx, dy, dz), size, color))

    asphalt = [0.10, 0.13, 0.16]
    markings = [0.82, 0.67, 0.25]
    site = [
        ((0.5, -0.40, 0.5), (4, 1, 4), asphalt),
        ((0.5, 0.108, -1.3), (3.5, 0.015, 0.07), markings),
        ((0.5, 0.108, 2.3), (3.5, 0.015, 0.07), markings),
        ((-1.3, 0.108, 0.5), (0.07, 0.015, 3.5), markings),
        ((2.3, 0.108, 0.5), (0.07, 0.015, 3.5), markings),
        ((2, 0.27, 1), (0.72, 0.44, 0.78), [0.21, 0.33, 0.40]),
        ((2.40, 0.29, 1), (0.10, 0.25, 0.42), [0.84, 0.50, 0.16]),
        ((1.12, 0.12, 1), (1.25, 0.13, 0.16), [0.41, 0.47, 0.49]),
    ]
    lower = [
        ((0, 0.20, 1), (1.25, 0.25, 1.25), [0.16, 0.22, 0.28]),
        ((0, 0.51, 1), (0.67, 0.48, 0.67), [0.26, 0.30, 0.36]),
        ((0, 1.05, 1), (0.95, 0.78, 0.95), [0.71, 0.77, 0.77]),
        ((-0.58, 0.57, 1), (0.25, 0.65, 0.72), [0.29, 0.43, 0.50]),
        ((0.58, 0.57, 1), (0.25, 0.65, 0.72), [0.29, 0.43, 0.50]),
        ((-0.36, 1.58, 1.36), (0.10, 0.45, 0.10), [0.34, 0.43, 0.48]),
        ((0.36, 1.58, 0.64), (0.10, 0.45, 0.10), [0.34, 0.43, 0.48]),
        ((0, 1.15, 1), (0.98, 0.14, 0.98), [0.25, 0.49, 0.61]),
    ]
    upper = [
        ((0, 1.92, 1), (0.93, 1.02, 0.93), [0.80, 0.84, 0.79]),
        ((0, 2.52, 1), (0.76, 0.27, 0.76), [0.73, 0.78, 0.75]),
        ((0, 2.76, 1), (0.55, 0.23, 0.55), [0.62, 0.73, 0.75]),
        ((0, 2.97, 1), (0.28, 0.23, 0.28), [0.38, 0.56, 0.64]),
        ((0, 2.08, 1.48), (0.55, 0.26, 0.06), [0.14, 0.42, 0.57]),
        ((-0.44, 1.79, 1.48), (0.12, 0.66, 0.09), [0.32, 0.44, 0.51]),
        ((0.44, 1.79, 1.48), (0.12, 0.66, 0.09), [0.32, 0.44, 0.51]),
        ((0, 1.47, 1), (0.97, 0.13, 0.97), [0.25, 0.49, 0.61]),
    ]
    objects.append({"id": "rocket-rig", "name": "Rocket flight pivot", "transform": transform()})
    for prefix, parts in [("site", site), ("rocket-lower", lower), ("rocket-upper", upper)]:
        for i, (position, size, color) in enumerate(parts):
            objects.append(cube(f"{prefix}-{i}", prefix, position, size, color, parent="rocket-rig" if prefix.startswith("rocket") else None))
    objects.append(glowing(cube("rocket-exhaust", "Rocket thruster", (0, -0.12, 1), (0.28, 0.7, 0.28), [0.3, 0.8, 1.0], parent="rocket-rig"), [0.3, 0.8, 1.0]))
    for i, color in enumerate([[0.95, 0.20, 0.10], [0.20, 0.95, 0.58]]):
        lamp = cube(f"rocket-light-{i}", "Blinking navigation light", ((i*2-1)*0.58, 0.97, 1.38), (0.16, 0.12, 0.12), color, parent="rocket-rig")
        lamp["shader_graph"] = {"version": 1, "name": "Navigation light", "nodes": [
            {"id": 1, "position": [0, 0], "kind": "master", "inputs": [
                {"vector": color}, {"float": 0}, {"float": 0.8}, {"vector": color}, {"float": 1}, {"vector": [0, 1, 0]}]},
            {"id": 2, "position": [-200, 0], "kind": "vector", "inputs": [{"vector": [v*2 for v in color]}]},
        ], "wires": [{"from": {"node": 2, "port": 0}, "to": {"node": 1, "port": 3}}]}
        objects.append(lamp)

    asset_names = [
        "node-iron", "node-copper", "node-limestone", "node-coal", "node-quartz", "node-oil", "node-water", "node-stone", "node-sand", "node-silver", "machine-constructor",
        "machine-miner", "machine-belt", "machine-smelter", "machine-storage", "machine-assembler",
        "machine-generator", "machine-splitter", "machine-merger", "machine-pole", "power-wire", "power-lamp", "item", "earth-chunk", "moon-chunk", "node-amorium", "node-moondust", "node-techtorium",
    ]
    assets = {name: {"kind": "prefab", "path": f"assets/{name}.prefab.json"} for name in asset_names}
    assets["moon-ground"] = {"kind": "mesh", "path": "assets/moon-ground.obj"}
    assets["earth-ground"] = {"kind": "mesh", "path": "assets/earth-ground.obj"}
    assets["ui-rounded"] = {"kind": "image", "path": "assets/ui-rounded.png"}
    assets["earth-factory"] = {"kind": "script", "path": "scripts/earth_factory.rs"}
    for module in sorted((SCENES / "scripts" / "factory").glob("*.rhai")):
        assets["factory-" + module.stem] = {"kind": "script", "path": module.relative_to(SCENES).as_posix()}

    def scalar(kind, value):
        return {"scalar": {kind: value}}

    def list_var(kind, capacity):
        return {"list": {"element": kind, "capacity": capacity, "values": []}}

    transport_board = {"earth": list_var("text", 867), "moon": list_var("text", 867),
                       "beats": list_var("number", 2), "guest_beat": list_var("number", 1)}
    transport_board["beats"]["list"]["values"] = [{"number": -1}, {"number": -1}]
    transport_board["guest_beat"]["list"]["values"] = [{"number": -1}]
    objects.append({"id": "factory-transports", "name": "Transient factory presentation",
                    "transform": transform(), "blackboard": transport_board})

    blackboard = {
        "started": scalar("bool", False),
        "demo_mode": scalar("bool", False),
        "chunk_x": scalar("number", 0),
        "chunk_z": scalar("number", 0),
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
        "factory_visuals": list_var("text", 867),
        "storage_view": list_var("number", 32),
        "power_supply": scalar("number", 18),
        "power_demand": scalar("number", 9),
        "message": scalar("text", "Starting the demonstration factory"),
        "nodes": list_var("number", 225),
        "builds": list_var("number", 225),
        "machine_cells": list_var("number", 225),
        "facings": list_var("number", 225),
        "items": list_var("number", 225),
        "item_amounts": list_var("number", 225),
        "input_items": list_var("number", 225),
        "input_amounts": list_var("number", 225),
        "progress": list_var("number", 225),
        "assembler_iron": list_var("number", 225),
        "assembler_copper": list_var("number", 225),
        "split_state": list_var("number", 225),
        "node_visuals": list_var("text", 225),
        "build_visuals": list_var("text", 225),
        "item_visuals": list_var("text", 225),
        "counts": list_var("number", 32),
        # Three 75-cell pages per region; motion_visuals adds an active-page index.
        "motion_visuals": list_var("text", 868),
        "motion_from": list_var("text", 867),
        "motion_to": list_var("text", 867),
        "motion_from_y": list_var("text", 867),
        "motion_to_y": list_var("text", 867),
        "retired_visuals": list_var("text", 867),
        "item_pool": list_var("text", 1024),
    }
    for page in range(4):
        blackboard["storage_kinds_" + str(page)] = list_var("number", 900)
        blackboard["storage_amounts_" + str(page)] = list_var("number", 900)
    # Chunk archives and interaction state belong to the controller, keeping both
    # blackboards within the engine's 64-variable / 1024-element limits.
    controller = next(obj for obj in objects if obj["id"] == "controller")
    controller["blackboard"] = {
        "title_open": scalar("bool", True),
        "session": {"list": {"element": "number", "capacity": 128, "values": [{"number": n} for n in [0, 0, 0, 11, -1, -1, 0] + [0]*121]}},
        "creative": scalar("bool", False),
        "assembler_cell": scalar("number", -1),
        "recipes": list_var("number", 225),
        "cache_recipes": list_var("text", 289),
        "power_data": list_var("text", 867),
        "power_dirty": scalar("bool", True),
        "wire_start": scalar("number", -1),
        "power_other": list_var("text", 867),
        "power_live": list_var("number", 225),
        "bar": scalar("number", 1),
        "bar_slots": {"list": {"element": "number", "capacity": 3, "values": [{"number": 1}] * 3}},
        "phase": scalar("number", 0),
        "stock": list_var("number", 32),
        "gather_clock": scalar("number", 0),
        "debug_clock": scalar("number", 0.25),
        "ui_views": {"list": {"element": "text", "capacity": 6, "values": [{"text": ""}] * 6}},
        "menu_open": scalar("bool", False),
        "map_open": scalar("bool", False),
        "map_state": scalar("text", ""),
        "camera_zoom": scalar("number", 19),
        "camera_zoom_target": scalar("number", 19),
        "camera_pan_progress": scalar("number", 1),
        "camera_pan_from": list_var("number", 3),
        "journal_open": scalar("bool", False),
        "journal_page": scalar("number", 1),
        "journal_alpha": scalar("number", 0),
        "rotation_cells": list_var("number", 225),
        "rotation_time": list_var("number", 225),
        "rotation_from": list_var("number", 225),
        "rotation_turns": list_var("number", 225),
        "visited": list_var("number", 289),
        "resident": list_var("number", 289),
        "residency_view": scalar("text", ""),
        "grounds": list_var("text", 289),
        "chunk_nodes": list_var("text", 289),
        "chunk_node_visuals": list_var("text", 289),
    }
    for name in ["builds", "facings", "items", "item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"]:
        controller["blackboard"]["cache_" + name] = list_var("text", 289)
    for page in range(4):
        for name in ["storage_kinds_", "storage_amounts_"]:
            controller["blackboard"]["cache_" + name + str(page)] = list_var("text", 289)
    for page in range(3):
        controller["blackboard"]["cache_visuals_" + str(page)] = list_var("text", 289)
    for page in range(5):
        controller["blackboard"]["power_fx_" + str(page)] = list_var("text", 867)
    for name, value in controller["blackboard"].items():
        if name == "chunk_nodes" or name.startswith("cache_") and not name.startswith("cache_visuals_"):
            value["list"]["capacity"] = 578  # Earth and Moon archives; only one planet has live models.
    assert len(blackboard) <= 64 and len(controller["blackboard"]) <= 64
    write_json(
        SCENES / "earth.json",
        {
            "version": 1, "name": "Stellar-IX", "views": {"3d": "camera"},
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
    bake_ground_mesh(moon=True)
    bake_ui_mask()
    write_json(ASSETS / "earth-chunk.prefab.json", {
        "version": 1, "name": "Earth terrain chunk", "root": "root",
        "assets": {"earth-ground": {"kind": "mesh", "path": "earth-ground.obj"}},
        "objects": [{"id": "root", "name": "Terrain chunk", "transform": transform(),
                     "drawable": {"layer": "3d", "mesh": {"asset": "earth-ground"},
                                  "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]}}],
    })
    write_json(ASSETS / "moon-chunk.prefab.json", {
        "version": 1, "name": "Stella-Z2 terrain chunk", "root": "root",
        "assets": {"moon-ground": {"kind": "mesh", "path": "moon-ground.obj"}},
        "objects": [{"id": "root", "name": "Lunar regolith", "transform": transform(),
                     "drawable": {"layer": "3d", "mesh": {"asset": "moon-ground"},
                                  "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]}}],
    })
    scene()
    write_json(
        ROOT / "bozzard.project.json",
        {
            "version": 1, "name": "Stellar-IX",
            "start_scene": "scenes/earth.json", "view": "3d", "cook": "universal",
        },
    )


if __name__ == "__main__":
    main()
