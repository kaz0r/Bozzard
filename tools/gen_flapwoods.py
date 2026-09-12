#!/usr/bin/env python3
"""Generates examples/demo/scenes/flap-woods.json — a blueprint-driven flappy clone."""
import json, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCENE_PATH = os.path.join(ROOT, "examples", "demo", "scenes", "flap-woods.json")


def tf(t=(0, 0, 0), r=(0, 0, 0), s=(1, 1, 1)):
    return {"translation": list(t), "rotation_degrees": list(r), "scale": list(s)}


def drawable(color, mesh="cube"):
    return {"layer": "3d", "mesh": mesh, "texture": "white", "color": list(color), "uv_scale": [1, 1]}


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


SELF = {"object": "self_object"}


def bird_graph():
    g = graph(
        "Bird / flap physics, death, respawn",
        [
            node(1, "update", (0, 0)),
            # mirror transform onto the solid proxy so sensors can see the bird
            node(2, "position", (0, 640), [{"object": "self_object"}]),
            node(3, "set_position", (250, 640), ["exec", {"vector": [0, 0, 0]}, {"object": {"id": "bird-solid"}}]),
            # alive branch: integrate flap physics
            node(4, "get_variable", (-250, 150), variable="alive"),
            node(5, "greater", (0, 150), [{"number": 0}, {"number": 0.5}]),
            node(6, "branch", (250, -100), ["exec", {"bool": False}]),
            node(7, "get_variable", (-250, 400), variable="vy"),
            node(8, "delta_time", (-250, 550)),
            node(9, "multiply", (0, 550), [{"number": 0}, {"number": -22}]),
            node(10, "add", (250, 400), [{"number": 0}, {"number": 0}]),
            node(11, "set_variable", (500, 400), ["exec", {"number": 0}], variable="vy"),
            node(12, "multiply", (750, 550), [{"number": 0}, {"number": 0}]),
            node(13, "make_vector", (1000, 550), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(14, "translate", (1000, 400), ["exec", {"vector": [0, 0, 0]}, SELF]),
            node(15, "multiply", (1250, 550), [{"number": 0}, {"number": 4}]),
            node(16, "make_vector", (1500, 550), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(17, "set_rotation", (1500, 400), ["exec", {"vector": [0, 0, 0]}, SELF]),
            # dead chain: sink, then respawn after one second
            node(18, "get_variable", (-250, -300), variable="alive"),
            node(19, "less", (0, -300), [{"number": 0}, {"number": 0.5}]),
            node(20, "branch", (250, -450), ["exec", {"bool": False}]),
            node(21, "get_variable", (0, -700), variable="dead_t"),
            node(22, "add", (250, -700), [{"number": 0}, {"number": 0}]),
            node(23, "set_variable", (500, -700), ["exec", {"number": 0}], variable="dead_t"),
            node(24, "multiply", (500, -850), [{"number": 0}, {"number": -20}]),
            node(25, "make_vector", (750, -850), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(26, "translate", (750, -850), ["exec", {"vector": [0, 0, 0]}, SELF]),
            node(27, "greater", (1000, -700), [{"number": 0}, {"number": 1}]),
            node(28, "branch", (1250, -850), ["exec", {"bool": False}]),
            node(29, "set_variable", (1500, -1150), ["exec", {"number": 1}], variable="alive"),
            node(30, "set_variable", (1750, -1150), ["exec", {"number": 0}], variable="vy"),
            node(31, "set_variable", (2000, -1150), ["exec", {"number": 0}], variable="dead_t"),
            node(32, "set_color", (1500, -950), ["exec", {"vector": [1, 0.8, 0.25]}, SELF]),
            node(33, "set_position", (1750, -950), ["exec", {"vector": [-5, 0.65, 0]}, SELF]),
            node(34, "set_rotation", (2000, -950), ["exec", {"vector": [0, 0, 0]}, SELF]),
            node(35, "set_position", (2250, -950), ["exec", {"vector": [-5, 0.65, 0]}, {"object": {"id": "bird-solid"}}]),
            # death by touch: pipes or the floor enter the bird's trigger
            # (the invisible solid proxy also overlaps it every tick — filter it out)
            node(36, "body_enter", (0, -1300)),
            node(44, "object_equal", (0, -1480), [{"object": "none"}, {"object": {"id": "bird-solid"}}]),
            node(45, "not", (250, -1480), [{"bool": False}]),
            node(46, "branch", (0, -1150), ["exec", {"bool": False}]),
            node(37, "set_variable", (250, -1300), ["exec", {"number": 0}], variable="alive"),
            node(38, "set_color", (500, -1300), ["exec", {"vector": [0.45, 0.05, 0.03]}, SELF]),
            # flap: Space
            node(40, "input_pressed", (0, -1600), key="jump"),
            node(41, "branch", (250, -1600), ["exec", {"bool": False}]),
            node(42, "set_variable", (500, -1600), ["exec", {"number": 6.5}], variable="vy"),
        ],
        [
            wire(1, 0, 6, 0), wire(1, 0, 20, 0), wire(1, 0, 3, 0),
            wire(2, 0, 3, 1),
            wire(18, 0, 19, 0), wire(19, 0, 20, 1),
            wire(4, 0, 5, 0), wire(5, 0, 6, 1),
            wire(7, 0, 10, 0), wire(8, 0, 9, 0), wire(9, 0, 10, 1), wire(10, 0, 11, 1),
            wire(6, 0, 11, 0),
            wire(7, 0, 12, 0), wire(8, 0, 12, 1), wire(12, 0, 13, 1), wire(11, 0, 14, 0),
            wire(13, 0, 14, 1),
            wire(7, 0, 15, 0), wire(15, 0, 16, 1), wire(14, 0, 17, 0), wire(16, 0, 17, 1),
            wire(21, 0, 22, 0), wire(8, 0, 22, 1), wire(22, 0, 23, 1),
            wire(20, 0, 23, 0),
            wire(8, 0, 24, 0), wire(24, 0, 25, 1), wire(23, 0, 26, 0), wire(25, 0, 26, 1),
            wire(26, 0, 28, 0),
            wire(21, 0, 27, 0), wire(27, 0, 28, 1),
            wire(28, 0, 29, 0), wire(29, 0, 30, 0), wire(30, 0, 31, 0),
            wire(31, 0, 32, 0), wire(32, 0, 33, 0), wire(33, 0, 34, 0), wire(34, 0, 35, 0),
            wire(36, 1, 44, 0), wire(44, 0, 45, 0), wire(45, 0, 46, 1),
            wire(36, 0, 46, 0), wire(46, 0, 37, 0), wire(37, 0, 38, 0),
            wire(40, 0, 41, 0), wire(5, 0, 41, 1), wire(41, 0, 42, 0),
        ],
        {"vy": 0, "alive": 1, "dead_t": 0},
    )
    # flap branch condition reuses node 5's alive check
    return [{"enabled": True, "graph": g}]


def scoreline_graph():
    g = graph(
        "Score line / count gates, reset on death",
        [
            node(1, "body_enter", (0, 0)),
            node(2, "object_equal", (0, 180), [{"object": "none"}, {"object": {"id": "bird-solid"}}]),
            node(3, "not", (250, 180), [{"bool": False}]),
            node(4, "branch", (500, 0), ["exec", {"bool": False}]),
            node(5, "get_variable", (500, 220), variable="score"),
            node(6, "add", (750, 220), [{"number": 0}, {"number": 1}]),
            node(7, "set_variable", (1000, 0), ["exec", {"number": 0}], variable="score"),
            node(8, "divide", (1250, 0), [{"number": 0}, {"number": 2}]),
            node(9, "multiply", (1500, 0), [{"number": 0}, {"number": 3}]),
            node(10, "set_light_intensity", (1750, 0), ["exec", {"number": 0}, {"object": {"id": "beacon"}}]),
            node(11, "add", (1250, 220), [{"number": 0}, {"number": -4.6}]),
            node(12, "make_vector", (1500, 220), [{"number": 0}, {"number": 5.0}, {"number": 0}]),
            node(13, "set_position", (2000, 220), ["exec", {"vector": [0, 0, 0]}, {"object": {"id": "score-cube"}}]),
            node(14, "body_exit", (0, 450)),
            node(15, "object_equal", (0, 630), [{"object": "none"}, {"object": {"id": "bird-solid"}}]),
            node(16, "branch", (500, 450), ["exec", {"bool": False}]),
            node(17, "set_variable", (750, 450), ["exec", {"number": 0}], variable="score"),
            node(18, "set_light_intensity", (1000, 450), ["exec", {"number": 0}, {"object": {"id": "beacon"}}]),
            node(19, "set_position", (1250, 450), ["exec", {"vector": [-4.6, 5.0, 0]}, {"object": {"id": "score-cube"}}]),
        ],
        [
            wire(1, 0, 4, 0), wire(1, 1, 2, 0), wire(2, 0, 3, 0), wire(3, 0, 4, 1),
            wire(5, 0, 6, 0), wire(6, 0, 7, 1), wire(4, 0, 7, 0),
            wire(5, 0, 8, 0), wire(8, 0, 9, 0), wire(9, 0, 10, 1), wire(4, 0, 10, 0),
            wire(10, 0, 13, 0),
            wire(8, 0, 11, 0), wire(11, 0, 12, 0), wire(12, 0, 13, 1),
            wire(14, 0, 16, 0), wire(14, 1, 15, 0), wire(15, 0, 16, 1),
            wire(16, 0, 17, 0), wire(17, 0, 18, 0), wire(18, 0, 19, 0),
        ],
        {"score": 0},
    )
    return [{"enabled": True, "graph": g}]


def pipe_graph(start_x, init_idx, gap0, pid):
    g = graph(
        "Pipe / drift left, recycle, shift gap",
        [
            node(1, "update", (0, 0)),
            node(2, "get_variable", (-250, 150), variable="x"),
            node(3, "delta_time", (-250, 300)),
            node(4, "multiply", (0, 300), [{"number": 0}, {"number": 2.2}]),
            node(5, "subtract", (250, 150), [{"number": 0}, {"number": 0}]),
            node(6, "set_variable", (500, 0), ["exec", {"number": 0}], variable="x"),
            node(7, "make_vector", (750, 150), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(8, "set_position", (1000, 0), ["exec", {"vector": [0, 0, 0]}, SELF]),
            node(9, "less", (1250, 150), [{"number": 0}, {"number": -14}]),
            node(10, "branch", (1500, 0), ["exec", {"bool": False}]),
            node(11, "add", (1750, 150), [{"number": 0}, {"number": 27}]),
            node(12, "set_variable", (2000, 0), ["exec", {"number": 0}], variable="x"),
            node(13, "get_variable", (1750, 300), variable="idx"),
            node(14, "add", (2000, 300), [{"number": 0}, {"number": 1}]),
            node(15, "set_variable", (2250, 0), ["exec", {"number": 0}], variable="idx"),
            node(16, "multiply", (2000, 450), [{"number": 0}, {"number": 2.1}]),
            node(17, "sine", (2250, 450), [{"number": 0}]),
            node(18, "multiply", (2500, 450), [{"number": 0}, {"number": 2.2}]),
            node(19, "set_variable", (2750, 0), ["exec", {"number": 0}], variable="gap"),
            node(20, "get_variable", (2500, 600), variable="gap"),
            node(21, "subtract", (2750, 600), [{"number": 0}, {"number": 7.8}]),
            node(22, "make_vector", (3000, 600), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(23, "set_position", (3000, 0), ["exec", {"vector": [0, 0, 0]}, {"object": {"id": f"{pid}-bottom"}}]),
            node(24, "add", (2750, 750), [{"number": 0}, {"number": 7.8}]),
            node(25, "make_vector", (3000, -200), [{"number": 0}, {"number": 0}, {"number": 0}]),
            node(26, "set_position", (3250, -200), ["exec", {"vector": [0, 0, 0]}, {"object": {"id": f"{pid}-top"}}]),
        ],
        [
            wire(1, 0, 6, 0),
            wire(2, 0, 5, 0), wire(3, 0, 4, 0), wire(4, 0, 5, 1), wire(5, 0, 6, 1),
            wire(2, 0, 7, 0), wire(7, 0, 8, 1), wire(6, 0, 8, 0),
            wire(2, 0, 9, 0), wire(9, 0, 10, 1),
            wire(8, 0, 10, 0), wire(10, 0, 12, 0), wire(2, 0, 11, 0), wire(11, 0, 12, 1),
            wire(12, 0, 15, 0),
            wire(13, 0, 14, 0), wire(14, 0, 15, 1), wire(15, 0, 19, 0),
            wire(13, 0, 16, 0), wire(16, 0, 17, 0), wire(17, 0, 18, 0), wire(18, 0, 19, 1),
            wire(20, 0, 21, 0), wire(21, 0, 22, 1), wire(19, 0, 23, 0), wire(22, 0, 23, 1),
            wire(20, 0, 24, 0), wire(24, 0, 25, 1), wire(23, 0, 26, 0), wire(25, 0, 26, 1),
        ],
        {"x": start_x, "idx": init_idx, "gap": gap0},
    )
    return [{"enabled": True, "graph": g}]


objects = []


def add(**kw):
    objects.append(kw)


# camera & bird
add(id="camera", name="Camera", transform=tf((0, 0, 10)),
    camera={"projection": "orthographic", "vertical_size": 11, "near": 0.1, "far": 50})
add(id="bird", name="Bird — Space to flap", transform=tf((-5, 0.65, 0), s=(0.8, 0.8, 0.8)),
    drawable=drawable([1.0, 0.8, 0.25]),
    trigger={"volume": {"center": [0, 0, 0], "size": [0.9, 0.9, 2], "enabled": True},
             "action": {"kind": "sensor"}},
    blueprints=bird_graph())
add(id="bird-beak", name="Beak", parent="bird",
    transform=tf((0.55, -0.1, 0), s=(0.5, 0.3, 0.3)), drawable=drawable([1.0, 0.45, 0.1]))
add(id="bird-solid", name="Bird overlap proxy",
    transform=tf((-5, 0.65, 0), s=(0.8, 0.8, 0.8)),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})

# pipes: three recycling pairs
for i, (x0, idx0, gap0) in enumerate([(2, 0, 0), (11, 1, 1.9), (20, 2, -1.9)], start=1):
    pid = f"pipe-{i}"
    add(id=pid, name=f"Pipe pair {i}", transform=tf((x0, 0, 0)),
        blueprints=pipe_graph(x0, idx0, gap0, pid))
    add(id=f"{pid}-bottom", name=f"Pipe {i} bottom", parent=pid,
        transform=tf((0, gap0 - 7.8, 0), s=(1.2, 13, 1.2)),
        drawable=drawable([0.07, 0.32, 0.14]),
        collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})
    add(id=f"{pid}-top", name=f"Pipe {i} top", parent=pid,
        transform=tf((0, gap0 + 7.8, 0), s=(1.2, 13, 1.2)),
        drawable=drawable([0.07, 0.32, 0.14]),
        collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})

# invisible solid floor that kills the bird via its trigger
add(id="floor", name="Bramble floor (solid)", transform=tf((0, -7.5, 0), s=(40, 1, 2)),
    collider={"center": [0, 0, 0], "size": [1, 1, 1], "enabled": True})
add(id="ground-visual", name="Bramble ground", transform=tf((0, -7.4, -1.2), s=(40, 1.2, 1)),
    drawable=drawable([0.06, 0.11, 0.05]))

# score line sensor, beacon light, score cube
add(id="score-line", name="Score line", transform=tf((-5, 0, 0)),
    trigger={"volume": {"center": [0, 0, 0], "size": [1.2, 16, 2], "enabled": True},
             "action": {"kind": "sensor"}},
    blueprints=scoreline_graph())
add(id="beacon", name="Score beacon", transform=tf((0, 4.6, 0.8)),
    light={"kind": "point", "color": [1, 0.75, 0.3], "intensity": 0,
           "range": 10, "inner_angle_degrees": 16, "outer_angle_degrees": 30, "shadows": False})
add(id="score-cube", name="Score marker", transform=tf((-4.6, 5.0, 0), s=(0.4, 0.4, 0.4)),
    drawable=drawable([1, 0.8, 0.25]))

# backdrop trees behind the pipes
for i, (x, h) in enumerate([(-9, 3), (-6.5, 4.5), (-3, 2.5), (0.5, 5), (3.5, 3.5), (7, 4.5), (10, 3)]):
    add(id=f"bg-tree-{i}", name="Backdrop tree", transform=tf((x, h / 2 - 7, -3), (0, i * 40, 0), (0.7, h, 0.7)),
        drawable=drawable([0.04, 0.07, 0.035]))

add(id="title", name="Title", transform=tf((0, 5.25, -0.5)),
    text_rendering={"text": "FLAPWOODS", "font_size": 0.55, "layer": "3d",
                    "color": [0.95, 0.85, 0.6, 1]})
add(id="hint", name="Hint", transform=tf((0, -5.1, -0.5)),
    text_rendering={"text": "Space to flap — slip between the thorn pipes",
                    "font_size": 0.35, "layer": "3d", "color": [0.9, 0.9, 0.9, 1]})

scene = {
    "version": 1,
    "name": "Flapwoods",
    "views": {"3d": "camera"},
    "environment": {"zenith": [0.1, 0.12, 0.28], "horizon": [0.62, 0.36, 0.2],
                    "ground": [0.05, 0.06, 0.05], "intensity": 0.5, "background": True},
    "display": {
        "bloom": {"enabled": True, "intensity": 0.45, "threshold": 0.5, "scatter": 0.8,
                  "anamorphic": 0.0},
        "exposure_ev": 0.2,
        "tone_mapping": True,
        "tone_mapper": "filmic",
        "color_grading": {"temperature": 0.05, "tint": 0.0, "saturation": 1.0,
                          "contrast": 1.05, "lift": [0, 0, 0], "gamma": [1, 1, 1],
                          "gain": [1, 1, 1]},
        "ambient_occlusion": {"enabled": True, "intensity": 0.5, "radius": 0.5, "bias": 0.025},
        "heat_distortion": {"enabled": False, "strength": 4, "threshold": 0.6, "speed": 1, "rise": 0.08},
        "grain": {"intensity": 0.02, "size": 1},
        "vignette": {"intensity": 0.3, "roundness": 0.7, "feather": 0.6},
        "volumetric_fog": {"enabled": False, "density": 0.03, "albedo": [0.9, 0.9, 1],
                           "anisotropy": 0.25, "base_height": 0, "height_falloff": 0.3,
                           "start_distance": 0.25, "max_distance": 30, "noise_amount": 0.8,
                           "noise_scale": 0.45, "wind": [0.1, 0.02, 0.05],
                           "light_intensity": 0.6, "ambient": 0.15, "steps": 48},
        "depth_of_field": {"enabled": False, "focus_distance": 4.3, "focal_length_mm": 85,
                           "aperture": 1.8, "max_blur_radius": 20},
        "auto_exposure": {"enabled": False, "min_ev": -1, "max_ev": 1, "target_gray": 0.15,
                          "speed_up": 1, "speed_down": 3, "center_weight": 0.7},
        "temporal_aa": {"enabled": False, "history_weight": 0.9},
        "motion_blur": {"enabled": False, "shutter_angle": 180, "max_radius": 32, "samples": 12},
        "reflections": {"enabled": False, "strength": 0.5, "max_distance": 25,
                        "thickness": 0.2, "roughness_cutoff": 0.6, "steps": 64},
    },
    "lighting": {"shadows": False, "shadow_resolution": 2048, "shadow_bias": 0.005,
                 "shadow_normal_bias": 0.01, "sun_direction": [-0.5, 0.65, -0.6],
                 "sun_color": [1, 0.72, 0.45], "sun_intensity": 2.5,
                 "ambient_color": [0.3, 0.3, 0.42], "ambient_intensity": 0.45},
    "objects": objects,
}

with open(SCENE_PATH, "w") as f:
    json.dump(scene, f, indent=1)
    f.write("\n")
print("wrote", SCENE_PATH)