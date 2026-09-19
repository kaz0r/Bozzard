#!/usr/bin/env python3
"""Author the midnight Sponza scene and a local gilded material variant.

Run download_sponza.py first. The licensed source and derived glTF stay in work/;
original geometry, textures, normal maps and upstream files remain untouched.
"""
import json
import base64
import math
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "work/sponza/glTF/Sponza.gltf"
SCENE = ROOT / "examples/sponza/gilded-night.json"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def build():
    if not SOURCE.exists():
        raise SystemExit("Run python3 tools/download_sponza.py first.")
    model = json.loads(SOURCE.read_text())
    # Material slots belong to the pinned Khronos revision in download_sponza.py.
    # Gild the carved friezes, capitals, shields, chains and lion reliefs.
    for index in [1, 4, 8, 10, 12, 13, 20, 21, 22, 23]:
        pbr = model["materials"][index]["pbrMetallicRoughness"]
        pbr["baseColorFactor"] = [0.92, 0.61, 0.22, 1]
        pbr["metallicFactor"] = 0.76
        pbr["roughnessFactor"] = 0.32 if index in [4, 20, 23] else 0.42
        pbr.pop("metallicRoughnessTexture", None)
    # Stone remains matte enough to catch pools of lantern light.
    for index in [5, 6, 7, 9, 11, 24]:
        pbr = model["materials"][index]["pbrMetallicRoughness"]
        pbr["baseColorFactor"] = [0.46, 0.48, 0.53, 1]
        pbr["metallicFactor"] = 0
        pbr["roughnessFactor"] = 0.35 if index == 6 else 0.72
        pbr.pop("metallicRoughnessTexture", None)
    for index in range(14, 20):
        pbr = model["materials"][index]["pbrMetallicRoughness"]
        pbr["metallicFactor"] = 0.12
        pbr["roughnessFactor"] = 0.76
        pbr.pop("metallicRoughnessTexture", None)
    model["asset"]["generator"] = "Bozzard Gilded Midnight material study; derived from pinned Khronos Sponza"
    write_json(SOURCE.with_name("Sponza-gilded.gltf"), model)

    objects = []

    def obj(id, name, pos, scale=(1, 1, 1), rotation=(0, 0, 0), **components):
        value = {"id": id, "name": name, "transform": {
            "translation": list(pos), "rotation_degrees": list(rotation), "scale": list(scale)
        }, **components}
        objects.append(value)
        return value

    def mesh(id, name, pos, scale, color, metallic=0.0, roughness=0.5, asset=None, rotation=(0,0,0)):
        return obj(id, name, pos, scale, rotation, drawable={
            "layer": "3d", "mesh": {"asset": asset} if asset else "cube",
            "texture": "white", "color": color, "uv_scale": [1, 1],
            "metallic": metallic, "roughness": roughness,
        })

    def light(id, name, pos, color, intensity, radius, shadow=False, spot=False, rotation=(0,0,0)):
        settings = {"kind": "spot" if spot else "point", "color": color,
                    "intensity": intensity, "range": radius, "shadows": shadow,
                    "shadow_bias": 0.003, "shadow_normal_bias": 0.015}
        if spot:
            settings.update(inner_angle_degrees=24, outer_angle_degrees=52)
        return obj(id, name, pos, rotation=rotation, light=settings)

    obj("camera", "Midnight courtyard — hero view", [7.4, 1.8, 0.0], rotation=[12, 90, 0],
        camera={"projection": "perspective", "vertical_fov_degrees": 68, "near": 0.05, "far": 100})
    obj("sponza", "Sponza — gold leaf, cool limestone and silk", [0,0,0],
        drawable={"layer": "3d", "mesh": {"asset": "sponza-gilded"}, "texture": "white",
                  "color": [1,1,1], "uv_scale": [1,1]})

    gold = [0.83, 0.53, 0.16]
    bronze = [0.12, 0.065, 0.022]
    warm = [1, 0.52, 0.17]
    # Repeated freestanding lanterns lead the eye into the courtyard.
    for row, x in enumerate([4.8, -0.8, -6.4]):
        for side, z in enumerate([-1.6, 1.6]):
            stem = f"lantern-{row}-{side}"
            y = 1.65
            mesh(stem+"-foot", "Lantern / bronze plinth", [x,.16,z], [.5,.28,.5], bronze, .85, .32)
            mesh(stem+"-base", "Lantern / gold stepped base", [x,.32,z], [.38,.08,.38], gold, .9, .25)
            mesh(stem+"-stem", "Lantern / slender gold standard", [x,.86,z], [.09,1.0,.09], gold, .9, .27)
            mesh(stem+"-tray", "Lantern / lower cornice", [x,y-.34,z], [.4,.1,.4], gold, .9, .23)
            mesh(stem+"-glow", "Lantern / amber luminous core", [x,y,z], [.17,.5,.17], [1,1,1], asset="lantern-glow")
            mesh(stem+"-cap", "Lantern / crown", [x,y+.34,z], [.42,.1,.42], gold, .9, .23)
            mesh(stem+"-finial", "Lantern / diamond finial", [x,y+.49,z], [.15,.19,.15], gold, .9, .2, rotation=[0,45,45])
            for corner, (dx,dz) in enumerate([(-.15,-.15),(-.15,.15),(.15,-.15),(.15,.15)]):
                mesh(stem+f"-bar-{corner}", "Lantern / gold cage", [x+dx,y,z+dz], [.026,.62,.026], gold, .9, .24)
            light(stem+"-light", "Lantern / warm shadowed light" if row != 1 else "Lantern / warm fill",
                  [x,y+.7,z], warm, 22, 7.5, shadow=row != 1)

    # Eight restrained uplights reveal the gold carving and upper arcade.
    for row, x in enumerate([7.7, 2.7, -2.7, -7.7]):
        for side, z in enumerate([-2.65, 2.65]):
            light(f"uplight-{row}-{side}", "Arcade / gold architectural uplight",
                  [x,.38,z], [1,.66,.3], 38, 10, shadow=True, spot=True, rotation=[90,0,0])
    for side, z in enumerate([-1.7,1.7]):
        light(f"gallery-{side}", "Upper gallery / soft amber bounce", [-4,6.6,z], [1,.6,.23], 22, 8)
    light("blue-distance", "Far arch / cool midnight separation", [-10,3.2,0], [.18,.33,1], 16, 6)
    mesh("crescent-moon", "Sky / silver crescent", [-25,24,-.1], [1,1,1],
         [1,1,1], asset="moon", rotation=[0,90,-18])

    scene = {
        "version": 1, "name": "Gilded Sponza — Midnight", "views": {"3d": "camera"},
        "assets": {
            "sponza-gilded": {"kind": "mesh", "path": "../../work/sponza/glTF/Sponza-gilded.gltf"},
            "lantern-glow": {"kind": "mesh", "path": "assets/gilded-night/lantern-glow.gltf"},
            "moon": {"kind": "mesh", "path": "assets/gilded-night/moon.gltf"},
        },
        "objects": objects,
        "lighting": {"sun_direction": [-.32,.86,.38], "sun_color": [.24,.4,1],
                     "sun_intensity": .32, "ambient_color": [.25,.38,.72], "ambient_intensity": .012,
                     "shadows": True, "shadow_resolution": 4096, "shadow_bias": .003, "shadow_normal_bias": .015},
        "environment": {"zenith": [.002,.004,.014], "horizon": [.008,.013,.035],
                        "ground": [.003,.002,.002], "intensity": .55, "background": True},
        "display": {
            "exposure_ev": -1.0, "tone_mapping": True, "tone_mapper": "filmic",
            "bloom": {"enabled": True, "intensity": .2, "threshold": 1.1, "scatter": .72, "anamorphic": .15},
            "ambient_occlusion": {"enabled": True, "intensity": .7, "radius": .45, "bias": .025},
            "color_grading": {"temperature": .025, "tint": 0, "saturation": .96, "contrast": 1.06,
                              "lift": [0,0,.001], "gamma": [1,1,1], "gain": [1,1,1]},
            "vignette": {"intensity": .22, "roundness": .8, "feather": .75},
            "volumetric_fog": {"enabled": True, "density": .008, "albedo": [.65,.75,1],
                               "anisotropy": .2, "base_height": 0, "height_falloff": .3,
                               "start_distance": .5, "max_distance": 40, "noise_amount": .2,
                               "noise_scale": .24, "wind": [.02,0,.015], "light_intensity": .35,
                               "ambient": .03, "steps": 48},
            "reflections": {"enabled": True, "strength": .6, "max_distance": 20, "thickness": .2,
                            "roughness_cutoff": .65, "steps": 64},
        },
    }
    glow = json.loads((ROOT/"examples/demo/scenes/assets/bonfire/gold-cube.gltf").read_text())
    glow["materials"][0]["emissiveFactor"] = [1, .55, .12]
    glow["materials"][0]["alphaMode"] = "BLEND"
    write_json(SCENE.parent/"assets/gilded-night/lantern-glow.gltf", glow)
    # A geometric crescent, authored here; no additional downloaded imagery.
    vertices, indices = [], []
    for row in range(65):
        y = -1 + row / 32
        outer = math.sqrt(max(0, 1-y*y))
        inner = min(outer, .43-math.sqrt(max(0, 1.04**2-y*y)))
        inner = max(-outer, inner)
        vertices.extend([(-outer,y,0), (inner,y,0)])
        if row:
            a = (row-1)*2
            indices.extend([a,a+1,a+2,a+1,a+3,a+2])
    positions = b''.join(struct.pack('<3f', *v) for v in vertices)
    normals = struct.pack('<3f',0,0,1)*len(vertices)
    triangles = struct.pack('<'+'H'*len(indices), *indices)
    data = positions+normals+triangles
    moon = {
        "asset": {"version": "2.0", "generator": "Bozzard gilded-night original crescent"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION":0,"NORMAL":1}, "indices":2, "material":0}]}],
        "materials": [{"doubleSided": True, "pbrMetallicRoughness": {"baseColorFactor":[0,0,0,1],
                        "metallicFactor":0,"roughnessFactor":1}, "emissiveFactor":[.7,.82,1]}],
        "buffers": [{"byteLength":len(data), "uri":"data:application/octet-stream;base64,"+base64.b64encode(data).decode()}],
        "bufferViews": [{"buffer":0,"byteOffset":0,"byteLength":len(positions)},
                        {"buffer":0,"byteOffset":len(positions),"byteLength":len(normals)},
                        {"buffer":0,"byteOffset":len(positions)+len(normals),"byteLength":len(triangles)}],
        "accessors": [{"bufferView":0,"componentType":5126,"count":len(vertices),"type":"VEC3",
                       "min":[-1,-1,0],"max":[1,1,0]},
                      {"bufferView":1,"componentType":5126,"count":len(vertices),"type":"VEC3"},
                      {"bufferView":2,"componentType":5123,"count":len(indices),"type":"SCALAR"}],
    }
    write_json(SCENE.parent/"assets/gilded-night/moon.gltf", moon)
    write_json(SCENE, scene)
    print(f"Authored {len(objects)} objects, 17 lights: {SCENE}")


if __name__ == "__main__":
    build()
