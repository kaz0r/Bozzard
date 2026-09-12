#!/usr/bin/env python3
"""Generates examples/demo/scenes/hollow-walk.json and its apparition prefab."""
import json, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCENES = os.path.join(ROOT, "examples", "demo", "scenes")
SCENE_PATH = os.path.join(SCENES, "hollow-walk.json")
PREFAB_DIR = os.path.join(SCENES, "assets", "hollow-walk")
PREFAB_PATH = os.path.join(PREFAB_DIR, "apparition.prefab.json")


def tf(t=(0, 0, 0), r=(0, 0, 0), s=(1, 1, 1)):
    return {"translation": list(t), "rotation_degrees": list(r), "scale": list(s)}


def drawable(color, mesh="cube", scale_uv=(1, 1)):
    return {"layer": "3d", "mesh": mesh, "texture": "white", "color": list(color), "uv_scale": list(scale_uv)}


def node(nid, kind, pos, inputs=(), **extra):
    d = {"prefab": "", "id": nid, "position": list(pos), "kind": kind,
         "inputs": list(inputs), "variable": ""}
    d.update(extra)
    return d


def wire(a, ap, b, bp):
    return {"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}}


def graph(name, nodes, wires, variables=None):
    g = {"version": 1, "name": name, "nodes": nodes, "wires": wires}
    if variables:
        g["variables"] = variables
    return g


def flicker_graph(speed1, speed2, amp1, amp2, brightness):
    g = graph(
        "Lantern / flicker",
        [
            node(1, "update", (0, 0)),
            node(2, "elapsed_time", (0, 200)),
            node(3, "multiply", (300, 180), [{"number": 0}, {"number": speed1}]),
            node(4, "sine", (600, 180), [{"number": 0}]),
            node(5, "multiply", (900, 180), [{"number": 0}, {"number": amp1}]),
            node(6, "multiply", (300, 420), [{"number": 0}, {"number": speed2}]),
            node(7, "sine", (600, 420), [{"number": 0}]),
            node(8, "multiply", (900, 420), [{"number": 0}, {"number": amp2}]),
            node(9, "add", (1200, 180), [{"number": 0}, {"number": 0}]),
            node(10, "get_variable", (1200, 420), variable="brightness"),
            node(11, "add", (1500, 180), [{"number": 0}, {"number": 0}]),
            node(12, "set_light_intensity", (1800, 0),
                 ["exec", {"number": brightness}, {"object": "self_object"}]),
        ],
        [
            wire(1, 0, 12, 0),
            wire(2, 0, 3, 0), wire(3, 0, 4, 0), wire(4, 0, 5, 0),
            wire(2, 0, 6, 0), wire(6, 0, 7, 0), wire(7, 0, 8, 0),
            wire(5, 0, 9, 0), wire(8, 0, 9, 1),
            wire(9, 0, 11, 0), wire(10, 0, 11, 1),
            wire(11, 0, 12, 1),
        ],
        {"brightness": brightness},
    )
    return [{"enabled": True, "graph": g}]


def haunt_graph(spawn_pos):
    g = graph(
        "Haunt zone / dread spike + wraith",
        [
            node(1, "body_enter", (0, 0)),
            node(2, "is_valid_object", (0, 180), [{"object": "none"}]),
            node(3, "branch", (300, 0), ["exec", {"bool": False}]),
            node(4, "set_vignette_intensity", (600, 0), ["exec", {"number": 0.88}]),
            node(5, "set_grain_intensity", (900, 0), ["exec", {"number": 0.08}]),
            node(6, "spawn_prefab", (1200, 0), ["exec", {"vector": list(spawn_pos)}],
                 prefab="hollow-apparition"),
        ],
        [
            wire(1, 0, 3, 0), wire(1, 1, 2, 0), wire(2, 0, 3, 1),
            wire(3, 0, 4, 0), wire(4, 0, 5, 0), wire(5, 0, 6, 0),
        ],
    )
    return [{"enabled": True, "graph": g}]


def watcher_graph():
    g = graph(
        "Watcher / pace across the clearing",
        [
            node(1, "update", (0, 0)),
            node(2, "elapsed_time", (0, 200)),
            node(3, "multiply", (300, 200), [{"number": 0}, {"number": 0.22}]),
            node(4, "sine", (600, 200), [{"number": 0}]),
            node(5, "multiply", (900, 200), [{"number": 0}, {"number": 5.5}]),
            node(6, "make_vector", (1200, 200), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(7, "set_position", (1500, 0), ["exec", {"vector": [0, 0, 0]}, {"object": "self_object"}]),
        ],
        [
            wire(1, 0, 7, 0), wire(2, 0, 3, 0), wire(3, 0, 4, 0),
            wire(4, 0, 5, 0), wire(5, 0, 6, 0), wire(6, 0, 7, 1),
        ],
    )
    return [{"enabled": True, "graph": g}]


def apparition_graph():
    g = graph(
        "Wraith / rise, swell, fade after lifetime",
        [
            node(1, "update", (0, 0)),
            node(2, "set_variable", (300, 0), ["exec", {"number": 0}], variable="age"),
            node(3, "get_variable", (0, 300), variable="age"),
            node(4, "delta_time", (0, 460)),
            node(5, "add", (300, 300), [{"number": 0}, {"number": 0}]),
            node(6, "get_variable", (300, 640), variable="lifetime"),
            node(7, "greater", (600, 300), [{"number": 0}, {"number": 0}]),
            node(8, "branch", (700, 0), ["exec", {"bool": False}]),
            node(9, "destroy_prefab", (1000, -160), ["exec", {"object": "self_object"}]),
            node(10, "translate", (1000, 140), ["exec", {"vector": [0, 0.45, 0]}, {"object": "self_object"}]),
            node(11, "scale_vector", (700, 360), [{"vector": [0, 0.45, 0]}, {"number": 0}]),
            node(12, "divide", (0, 640), [{"number": 0}, {"number": 1}]),
            node(13, "multiply", (300, 760), [{"number": 0}, {"number": 0.8}]),
            node(14, "add", (600, 760), [{"number": 0}, {"number": 1}]),
            node(15, "scale_vector", (900, 640), [{"vector": [0.5, 2.4, 0.5]}, {"number": 0}]),
            node(16, "set_scale", (1300, 140), ["exec", {"vector": [0.5, 2.4, 0.5]}, {"object": "self_object"}]),
        ],
        [
            wire(1, 0, 2, 0),
            wire(3, 0, 5, 0), wire(4, 0, 5, 1), wire(5, 0, 2, 1),
            wire(3, 0, 7, 0), wire(6, 0, 7, 1), wire(7, 0, 8, 1),
            wire(2, 0, 8, 0),
            wire(8, 0, 9, 0),
            wire(8, 1, 10, 0), wire(11, 0, 10, 1), wire(4, 0, 11, 1),
            wire(10, 0, 16, 0), wire(15, 0, 16, 1),
            wire(3, 0, 12, 0), wire(6, 0, 12, 1),
            wire(12, 0, 13, 0), wire(13, 0, 14, 0), wire(14, 0, 15, 1),
        ],
        {"age": 0, "lifetime": 2.6},
    )
    return g


objects = []
assets = {"hollow-apparition": {"kind": "prefab", "path": "assets/hollow-walk/apparition.prefab.json"}}


def add(**kw):
    objects.append(kw)


# --- camera & player ---
add(id="camera", name="Follow camera", transform=tf((0, 3.5, 8), (-18, 0, 0)),
    camera={"projection": "perspective", "vertical_fov_degrees": 62, "near": 0.1, "far": 150})
add(id="player", name="Lost one — WASD / Space", transform=tf((0, 0.65, 2), s=(0.8, 1.2, 0.8)),
    drawable=drawable([0.32, 0.35, 0.42]),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True},
    gravity={"enabled": True, "max_speed": 50.0, "jump_speed": 5.0},
    player_controller={"camera": "camera", "move_speed": 3.6, "jump_speed": 6.0,
                       "camera_distance": 3.6, "camera_height": 1.0, "camera_radius": 0.3,
                       "orbit_sensitivity": 0.2, "fall_height": -8.0})

# --- world ---
add(id="ground", name="Dead forest earth", transform=tf((0, -0.2, -13), s=(44, 0.4, 74)),
    drawable=drawable([0.055, 0.058, 0.05]))
add(id="title", name="Title", transform=tf((0, 3.4, -2)),
    text_rendering={"text": "THE HOLLOW WALK", "font_size": 0.8, "layer": "3d",
                    "color": [0.7, 0.78, 0.92, 1]})
add(id="hint", name="Hint", transform=tf((0, 2.6, -2)),
    text_rendering={"text": "Gather the five souls. Reach the moon gate.",
                    "font_size": 0.4, "layer": "3d", "color": [0.45, 0.5, 0.62, 1]})

# dead trees (trunks + occasional branch)
trees = [(-3.6, -1, 6), (4.2, 0, 8), (-5.5, 0, 4), (3.4, 0, 1), (-4.4, 0, -5),
         (5.6, 0, -9), (-3.8, 0, -11), (4.0, 0, -16), (-5.8, 0, -17),
         (3.6, 0, -21), (-4.2, 0, -26), (5.2, 0, -27), (-3.4, 0, -31), (4.6, 0, -33),
         (-6.0, 0, -1), (6.2, 0, -14)]
for i, (x, _, z) in enumerate(trees):
    h = 3.6 + (i % 4) * 0.7
    tilt = (i % 3 - 1) * 4.0
    add(id=f"tree-{i}", name="Dead tree", parent="ground",
        transform=tf((x, h / 2 - 0.2, z), (0, (i * 37) % 360, tilt), (0.5, h, 0.5)),
        drawable=drawable([0.028, 0.024, 0.022]))
    if i % 3 == 0:  # a dead branch
        add(id=f"branch-{i}", name="Broken branch", parent=f"tree-{i}",
            transform=tf((0.3, h * 0.55, 0), (0, 0, -38), (0.14, 1.4, 0.14)),
            drawable=drawable([0.03, 0.026, 0.024]))

# fallen log with a soul on top
add(id="log", name="Fallen log", transform=tf((1.5, 0.35, -14), (0, 20, 0), (3, 0.7, 0.8)),
    drawable=drawable([0.09, 0.065, 0.045]),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})

# --- lanterns ---
lanterns = [
    ("lantern-1", -2.4, -6, 26, (8.3, 17.7, 5.0, 3.0)),
    ("lantern-2", 2.4, -12, 20, (6.1, 13.9, 4.0, 2.0)),
    ("lantern-3", -2.4, -22, 28, (9.7, 21.3, 5.5, 3.5)),
]
for lid, x, z, brightness, (s1, s2, a1, a2) in lanterns:
    add(id=lid, name="Lantern", transform=tf((x, 0, z)))
    add(id=f"{lid}-pole", name="Pole", parent=lid, transform=tf((0, 1.1, 0), s=(0.14, 2.2, 0.14)),
        drawable=drawable([0.03, 0.028, 0.026]))
    add(id=f"{lid}-head", name="Lantern head", parent=lid, transform=tf((0, 2.35, 0), s=(0.32, 0.34, 0.32)),
        drawable=drawable([1.0, 0.62, 0.22]))
    add(id=f"{lid}-light", name="Lantern light", parent=lid, transform=tf((0, 2.35, 0)),
        light={"kind": "point", "color": [1.0, 0.55, 0.18], "intensity": brightness,
               "range": 12.0, "inner_angle_degrees": 16, "outer_angle_degrees": 30,
               "shadows": False},
        blueprints=flicker_graph(s1, s2, a1, a2, brightness))

# --- soul collectibles ---
souls = [("soul-1", 1.6, 0.9, -4), ("soul-2", -1.9, 0.9, -9), ("soul-3", 1.4, 1.35, -14),
         ("soul-4", -2.2, 0.9, -19), ("soul-5", 1.2, 0.9, -27)]
for sid, x, y, z in souls:
    add(id=sid, name="Soul light", transform=tf((x, y, z), s=(0.4, 0.4, 0.4)),
        drawable=drawable([0.55, 0.92, 1.0]), spin=[0.0, 80.0, 0.0],
        trigger={"volume": {"center": [0, 0, 0], "size": [2.5, 3, 2.5], "enabled": True},
                 "action": {"kind": "collectible"}})
    add(id=f"{sid}-glow", name="Soul glow", parent=sid, transform=tf((0, 0, 0)),
        light={"kind": "point", "color": [0.5, 0.85, 1.0], "intensity": 7,
               "range": 6.0, "inner_angle_degrees": 16, "outer_angle_degrees": 30,
               "shadows": False})

# --- checkpoint ---
add(id="checkpoint", name="Pale shrine — respawn", transform=tf((0, 0.1, -16), s=(2, 0.2, 1.5)),
    drawable=drawable([0.12, 0.3, 0.42]),
    trigger={"volume": {"center": [0, 3, 0], "size": [1, 8, 1], "enabled": True},
             "action": {"kind": "checkpoint", "respawn": [0, 0.65, -16]}})

# --- haunt zones ---
add(id="haunt-1", name="Haunt zone I", transform=tf((0, 0, -7)),
    trigger={"volume": {"center": [0, 1.5, 0], "size": [7, 3, 3], "enabled": True},
             "action": {"kind": "sensor"}},
    blueprints=haunt_graph((2.6, 1.2, -8.5)))
add(id="haunt-2", name="Haunt zone II", transform=tf((0, 0, -25)),
    trigger={"volume": {"center": [0, 1.5, 0], "size": [7, 3, 3], "enabled": True},
             "action": {"kind": "sensor"}},
    blueprints=haunt_graph((-2.6, 1.2, -26.5)))

# --- watcher ---
add(id="watcher", name="The Watcher", transform=tf((0, 0, -30)), blueprints=watcher_graph())
add(id="watcher-body", name="Watcher body", parent="watcher",
    transform=tf((0, 1.5, 0), s=(0.7, 2.6, 0.45)), drawable=drawable([0.012, 0.012, 0.016]))
add(id="watcher-eye-l", name="Watcher eye L", parent="watcher",
    transform=tf((-0.14, 2.05, 0.24), s=(0.1, 0.1, 0.06)), drawable=drawable([1.0, 0.05, 0.02]))
add(id="watcher-eye-r", name="Watcher eye R", parent="watcher",
    transform=tf((0.14, 2.05, 0.24), s=(0.1, 0.1, 0.06)), drawable=drawable([1.0, 0.05, 0.02]))
add(id="watcher-light", name="Watcher aura", parent="watcher", transform=tf((0, 1.6, 0)),
    light={"kind": "point", "color": [0.9, 0.06, 0.03], "intensity": 5,
           "range": 4.5, "inner_angle_degrees": 16, "outer_angle_degrees": 30, "shadows": False})

# --- moon gate / goal ---
add(id="gate-post-l", name="Gate post L", transform=tf((-1.6, 2, -32), s=(0.5, 4, 0.6)),
    drawable=drawable([0.62, 0.68, 0.8]),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})
add(id="gate-post-r", name="Gate post R", transform=tf((1.6, 2, -32), s=(0.5, 4, 0.6)),
    drawable=drawable([0.62, 0.68, 0.8]),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})
add(id="gate-lintel", name="Gate lintel", transform=tf((0, 4.2, -32), s=(3.9, 0.5, 0.6)),
    drawable=drawable([0.62, 0.68, 0.8]))
add(id="goal", name="Moon gate — bring all five souls",
    transform=tf((0, 0.12, -32), s=(2.5, 0.24, 2.5)),
    drawable=drawable([0.75, 0.85, 1.0]),
    trigger={"volume": {"center": [0, 3, 0], "size": [1, 8, 1], "enabled": True},
             "action": {"kind": "goal"}})
add(id="gate-label", name="Gate label", transform=tf((0, 5.2, -32)),
    text_rendering={"text": "THE MOON GATE", "font_size": 0.5, "layer": "3d",
                    "color": [0.8, 0.88, 1.0, 1]})

scene = {
    "version": 1,
    "name": "The Hollow Walk",
    "views": {"3d": "camera"},
    "fog": {"enabled": True, "color": [0.004, 0.006, 0.012], "distance_density": 0.03,
            "start_distance": 8.0, "height_density": 0.014, "base_height": 0.0,
            "height_falloff": 0.6},
    "gi": {"enabled": False, "intensity": 1.0, "normal_bias": 0.05,
           "volume": {"min": [-10, -1, -30], "max": [10, 5, 4], "resolution": [8, 4, 8],
                      "samples": 128, "bounces": 2}},
    "environment": {"zenith": [0.003, 0.005, 0.012], "horizon": [0.007, 0.011, 0.022],
                    "ground": [0.002, 0.002, 0.004], "intensity": 0.18, "background": True},
    "display": {
        "bloom": {"enabled": True, "intensity": 0.5, "threshold": 0.4, "scatter": 0.8,
                  "anamorphic": 0.0},
        "exposure_ev": -0.4,
        "tone_mapping": True,
        "tone_mapper": "filmic",
        "color_grading": {"temperature": -0.28, "tint": 0.0, "saturation": 0.6,
                          "contrast": 1.15, "lift": [0.0, 0.0, 0.004], "gamma": [1, 1, 1],
                          "gain": [1, 1, 1]},
        "ambient_occlusion": {"enabled": True, "intensity": 0.8, "radius": 0.6, "bias": 0.025},
        "heat_distortion": {"enabled": False, "strength": 4.0, "threshold": 0.6,
                            "speed": 1.0, "rise": 0.08},
        "grain": {"intensity": 0.05, "size": 1.4},
        "vignette": {"intensity": 0.55, "roundness": 0.75, "feather": 0.5},
        "volumetric_fog": {"enabled": True, "density": 0.035, "albedo": [0.7, 0.78, 0.9],
                           "anisotropy": 0.25, "base_height": 0.0, "height_falloff": 0.3,
                           "start_distance": 0.25, "max_distance": 30.0, "noise_amount": 0.85,
                           "noise_scale": 0.45, "wind": [0.1, 0.02, 0.05],
                           "light_intensity": 0.6, "ambient": 0.12, "steps": 48},
        "depth_of_field": {"enabled": False, "focus_distance": 4.3, "focal_length_mm": 85.0,
                           "aperture": 1.8, "max_blur_radius": 20.0},
        "auto_exposure": {"enabled": True, "min_ev": -1.5, "max_ev": 0.4,
                          "target_gray": 0.02, "speed_up": 1.0, "speed_down": 3.0,
                          "center_weight": 0.7},
        "temporal_aa": {"enabled": True, "history_weight": 0.9},
        "motion_blur": {"enabled": False, "shutter_angle": 180.0, "max_radius": 32.0,
                        "samples": 12},
        "reflections": {"enabled": False, "strength": 0.5, "max_distance": 25.0,
                        "thickness": 0.2, "roughness_cutoff": 0.6, "steps": 64},
    },
    "lighting": {"shadows": False, "shadow_resolution": 2048, "shadow_bias": 0.005,
                 "shadow_normal_bias": 0.01, "sun_direction": [-0.4, 0.8, -0.2],
                 "sun_color": [0.1, 0.14, 0.25], "sun_intensity": 0.02,
                 "ambient_color": [0.07, 0.09, 0.13], "ambient_intensity": 0.012},
    "objects": objects,
    "assets": assets,
}

os.makedirs(PREFAB_DIR, exist_ok=True)
prefab = {
    "version": 1,
    "name": "Hollow Wraith (rises and fades)",
    "root": "apparition",
    "objects": [
        {"id": "apparition", "name": "Wraith", "transform": tf(s=(0.5, 2.4, 0.5)),
         "drawable": drawable([0.72, 0.82, 0.95]),
         "blueprints": [{"enabled": True, "graph": apparition_graph()}]},
        {"id": "wraith-light", "name": "Wraith chill", "parent": "apparition",
         "transform": tf((0, 0, 0)),
         "light": {"kind": "point", "color": [0.55, 0.75, 1.0], "intensity": 12,
                   "range": 7.0, "inner_angle_degrees": 16, "outer_angle_degrees": 30,
                   "shadows": False}},
    ],
}

with open(SCENE_PATH, "w") as f:
    json.dump(scene, f, indent=1)
    f.write("\n")
with open(PREFAB_PATH, "w") as f:
    json.dump(prefab, f, indent=1)
    f.write("\n")
print("wrote", SCENE_PATH)
print("wrote", PREFAB_PATH)