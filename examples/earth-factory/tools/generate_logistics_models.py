"""Straight pipes, 90-degree elbow pipes, and left/right Mk1 corner belts.

blender --background -noaudio --threads 12 --python examples/earth-factory/tools/generate_logistics_models.py
Add ``-- --no-render`` to skip the studio render. Assets only: no fluid simulation
or new build-bar entries are introduced. Reuses the expansion modeling palette.
"""
from pathlib import Path
import math
import sys

import bpy
import bmesh
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
import generate_expansion_machines as kit
from generate_scene import cube, transform, write_json

parts = kit.parts
groups, catalog = [], []


def mesh_part(name, vertices, faces, material):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([kit.point(p) for p in vertices], [], faces)
    mesh.update()
    # Resolve the cavity and mirrored curve winding before export.
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return kit.finish(obj, name, material)


def hollow_tube(name, stations, outer=.064, inner=.045, material=None):
    """Sweep a closed tube wall; each station gives center and horizontal normal."""
    material = kit.paint if material is None else material
    vertices, faces = [], []
    count = 12
    for radius in [outer, inner]:
        for (x, y, z), (nx, nz) in stations:
            for i in range(count):
                a = math.tau * i / count
                vertices.append((x + radius * nx * math.cos(a),
                                 y + radius * math.sin(a), z + radius * nz * math.cos(a)))
    loops = len(stations)
    inside = loops * count
    for s in range(loops - 1):
        for i in range(count):
            j = (i + 1) % count
            a, b = s * count + i, s * count + j
            faces.append((a, b, b + count, a + count))
            faces.append((inside + a + count, inside + b + count, inside + b, inside + a))
    for s in [0, loops - 1]:
        for i in range(count):
            a, b = s * count + i, s * count + (i + 1) % count
            faces.append((a, inside + a, inside + b, b))
    return mesh_part(name, vertices, faces, material)


def corner(a, side=1, radius=.5):
    return (.5 + radius * math.cos(a), side * (.5 + radius * math.sin(a)))


def support(name, x, z, direction, saddle=False):
    start = len(parts)
    kit.box(name + " foot", (0, .035, 0), (.15, .07, .29), kit.paint)
    kit.box(name + " pedestal", (0, .175, 0), (.09, .21, .11), kit.steel)
    if saddle:
        hollow_tube(name + " saddle", [((-.03, .34, 0), (0, 1)), ((.03, .34, 0), (0, 1))],
                    .076, .064, kit.steel)
    for obj in parts[start:]:
        px, py, height = obj.location
        obj.location = (x + px * math.cos(direction) + py * math.sin(direction),
                        -z - px * math.sin(direction) + py * math.cos(direction), height)
        obj.rotation_euler.z -= direction


def pipe_straight():
    hollow_tube("Straight pressure pipe", [((-.5, .34, 0), (0, 1)), ((.5, .34, 0), (0, 1))])
    for x in [-.5, .46]:
        hollow_tube("Steel end coupling", [((x, .34, 0), (0, 1)), ((x + .04, .34, 0), (0, 1))],
                    .082, .045, kit.steel)
    hollow_tube("Cyan fluid identification band", [((-.12, .34, 0), (0, 1)), ((-.10, .34, 0), (0, 1))],
                .066, .064, kit.inlet)
    for x in [-.25, .25]:
        support("Pipe mount", x, 0, 0, saddle=True)
    return [{"kind": "fluid", "role": "bidirectional", "position": [x, .34, 0],
             "outward": [1 if x > 0 else -1, 0, 0]} for x in [-.5, .5]]


def pipe_elbow():
    stations = []
    for i in range(17):
        a = math.pi + i / 16 * math.pi / 2
        x, z = corner(a)
        stations.append(((x, .34, z), (math.cos(a), math.sin(a))))
    hollow_tube("Faceted quarter-circle elbow", stations)
    hollow_tube("East coupling", [((.46, .34, 0), (0, 1)), ((.5, .34, 0), (0, 1))],
                .082, .045, kit.steel)
    hollow_tube("South coupling", [((0, .34, .46), (1, 0)), ((0, .34, .5), (1, 0))],
                .082, .045, kit.steel)
    for fraction in [.25, .75]:
        a = math.pi + fraction * math.pi / 2
        x, z = corner(a)
        direction = math.atan2(math.cos(a), -math.sin(a))
        support("Elbow mount", x, z, direction, saddle=True)
    a = math.pi * 1.25
    band = []
    for angle in [a - .018, a + .018]:
        x, z = corner(angle)
        band.append(((x, .34, z), (math.cos(angle), math.sin(angle))))
    hollow_tube("Elbow fluid identification band", band, .067, .064, kit.inlet)
    return [{"kind": "fluid", "role": "bidirectional", "position": [.5, .34, 0], "outward": [1, 0, 0]},
            {"kind": "fluid", "role": "bidirectional", "position": [0, .34, .5], "outward": [0, 0, 1]}]


def arc_band(name, inner, outer, bottom, top, side, material):
    vertices, faces = [], []
    steps = 16
    for y, radius in [(bottom, inner), (bottom, outer), (top, inner), (top, outer)]:
        for i in range(steps + 1):
            a = math.pi + i / steps * math.pi / 2
            x, z = corner(a, side, radius)
            vertices.append((x, y, z))
    n = steps + 1
    for i in range(steps):
        j = i + 1
        faces.extend([(i, j, n + j, n + i), (2*n + i, 3*n + i, 3*n + j, 2*n + j),
                      (i, 2*n + i, 2*n + j, j), (n + i, n + j, 3*n + j, 3*n + i)])
    for i in [0, steps]:
        faces.append((i, n + i, 3*n + i, 2*n + i))
    return mesh_part(name, vertices, faces, material)


def belt_corner(side):
    # Same .46-wide bed, .305 deck center and .34 tread height as belt-mk1.
    arc_band("Curved belt deck", .27, .73, .2775, .3325, side, kit.dark)
    for radius in [.245, .755]:
        arc_band("Continuous curved guard rail", radius - .020, radius + .020,
                 .290, .36, side, kit.steel)
    for i in range(13):
        a = math.pi + (i + .35) / 13.7 * math.pi / 2
        x, z = corner(a, side)
        obj = kit.box("Radial belt tread", (x, .34, z), (.435, .014, .017), kit.steel, .001)
        obj.rotation_euler.z = -math.atan2(side * math.sin(a), math.cos(a))
    for fraction in [.28, .72]:
        a = math.pi + fraction * math.pi / 2
        x, z = corner(a, side)
        direction = math.atan2(side * math.cos(a), -math.sin(a))
        kit.arrow("Curved belt travel arrow", (x, .355, z), direction, kit.outlet, .12)
    for fraction in [.27, .73]:
        a = math.pi + fraction * math.pi / 2
        x, z = corner(a, side)
        direction = math.atan2(side * math.cos(a), -math.sin(a))
        start = len(parts)
        kit.box("Corner belt cross foot", (0, .05, 0), (.14, .10, .66))
        for edge in [-.245, .245]:
            kit.box("Corner belt support", (0, .19, edge), (.09, .23, .08), kit.steel)
        for obj in parts[start:]:
            px, py, height = obj.location
            obj.location = (x + px * math.cos(direction) + py * math.sin(direction),
                            -z - px * math.sin(direction) + py * math.cos(direction), height)
            obj.rotation_euler.z -= direction
    # Visible drums terminate both mouths without leaving a gap between tiles.
    kit.cylinder("Output end roller", (.43, .28, 0), .063, .47, kit.steel, "z")
    kit.cylinder("Input end roller", (0, .28, side * .43), .063, .47, kit.steel, "x")
    return [{"kind": "solid", "role": "input", "position": [0, .34, side * .5], "outward": [0, 0, side]},
            {"kind": "solid", "role": "output", "position": [.5, .34, 0], "outward": [1, 0, 0]}]


def export(slug, title, ports):
    merged, triangles = [], 0
    low, high = [math.inf] * 3, [-math.inf] * 3
    for material in [kit.paint, kit.steel, kit.dark, kit.inlet, kit.outlet]:
        copies = []
        for original in parts:
            if original.data.materials[0] != material:
                continue
            obj = original.copy()
            obj.data = original.data.copy()
            bpy.context.collection.objects.link(obj)
            copies.append(obj)
        if not copies:
            continue
        bpy.ops.object.select_all(action="DESELECT")
        for obj in copies:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = copies[0]
        if len(copies) > 1:
            bpy.ops.object.join()
        obj = copies[0]
        obj.name = slug + " / " + material.name
        bpy.context.scene.cursor.location = (0, 0, 0)
        bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
        # Bake residual part rotations into geometry: unit-scale grounded pivots.
        bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
        obj.data.calc_loop_triangles()
        triangles += len(obj.data.loop_triangles)
        for vertex in obj.data.vertices:
            p = obj.matrix_world @ vertex.co
            p = (p.x, p.z, -p.y)
            for axis in range(3):
                low[axis], high[axis] = min(low[axis], p[axis]), max(high[axis], p[axis])
        merged.append(obj)
    assert min(low[0], low[2]) >= -.501 and max(high[0], high[2]) <= .501, (slug, low, high)
    assert abs(low[1]) < .001 and triangles < 4000, (slug, low, triangles)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(kit.OUT / f"{slug}-mk1.glb"), export_format="GLB",
                              use_selection=True, export_yup=True, export_apply=True,
                              export_animations=False, export_cameras=False, export_lights=False)
    entry = {"id": slug, "name": title, "mesh": f"models/{slug}-mk1.glb",
             "prefab": f"machine-{slug}.prefab.json", "requires_power": False,
             "triangles": triangles, "surfaces": len(merged), "ports": ports,
             "bounds": [[round(v, 6) for v in bound] for bound in [low, high]]}
    catalog.append(entry)
    write_json(kit.ASSETS / entry["prefab"], {
        "version": 1, "name": title + " Mk1", "root": "root",
        "assets": {slug + "-mk1": {"kind": "mesh", "path": entry["mesh"]}},
        "objects": [{"id": "root", "name": "machine-" + slug, "transform": transform(),
                     "drawable": {"layer": "3d", "mesh": {"asset": slug + "-mk1"},
                                  "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]}}],
    })
    for obj in merged:
        bpy.data.objects.remove(obj, do_unlink=True)
    groups.append((title, list(parts)))
    parts.clear()
    print(f"logistics_export name={slug}-mk1 triangles={triangles}", flush=True)


def showroom():
    assets, objects = {}, []
    objects.append({"id": "camera", "name": "Logistics showroom camera",
                    "transform": {**transform(8, 8, 8), "rotation_degrees": [-35.264, 45, 0]},
                    "camera": {"projection": "orthographic", "vertical_size": 4.5, "near": .1, "far": 100}})
    objects.append(cube("floor", "Studio floor", (0, -.06, 0), (20, .12, 20), [.18, .21, .23]))
    for i, entry in enumerate(catalog):
        slug = entry["id"]
        assets[slug + "-mk1"] = {"kind": "mesh", "path": "assets/" + entry["mesh"]}
        objects.append({"id": slug, "name": entry["name"],
                        "transform": transform((i % 2 - .5) * 2, 0, (i // 2 - .5) * 2.3),
                        "drawable": {"layer": "3d", "mesh": {"asset": slug + "-mk1"},
                                     "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]}})
    write_json(kit.ASSETS.parent / "logistics-showroom.json", {
        "version": 1, "name": "Stellar-IX — Mk1 pipes and corner belts",
        "views": {"3d": "camera"}, "assets": assets, "objects": objects,
        "environment": {"zenith": [.25, .29, .32], "horizon": [.25, .29, .32],
                        "ground": [.18, .21, .23], "intensity": .55, "background": True},
        "lighting": {"sun_direction": [.46, .81, .35], "sun_color": [1., .96, .90],
                     "sun_intensity": 2.6, "ambient_color": [.86, .94, 1.], "ambient_intensity": .24},
    })


def studio():
    right, forward = Vector((.78935, .61394, 0)), Vector((.61394, -.78935, 0))
    offsets = []
    for i, (title, group) in enumerate(groups):
        offset = right * ((i % 2 - .5) * 2.25) + forward * ((i // 2 - .5) * 3.0)
        offsets.append(offset)
        collection = bpy.data.collections.new(title + " Mk1")
        bpy.context.scene.collection.children.link(collection)
        for obj in group:
            obj.location += offset
            for old in list(obj.users_collection):
                old.objects.unlink(obj)
            collection.objects.link(obj)
    floor = kit.create_principled_material(bpy, "Studio floor", (.12, .15, .17), 0, .8)
    kit.box("Studio floor", (0, -.08, 0), (200, .1, 200), floor, 0)
    for name, loc, power, size in [("Key", (2, -3, 6), 950, 6), ("Rim", (-3, 2, 5), 1100, 5),
                                   ("Front fill", (4, 4, 4), 800, 5)]:
        data = bpy.data.lights.new(name, "AREA")
        data.energy, data.shape, data.size = power, "DISK", size
        obj = bpy.data.objects.new(name, data)
        bpy.context.collection.objects.link(obj)
        obj.location = loc
        obj.rotation_euler = (-obj.location).to_track_quat("-Z", "Y").to_euler()
    bpy.ops.object.camera_add(location=(7, -9, 8))
    camera = bpy.context.object
    camera.rotation_euler = (Vector((0, 0, .15)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type, camera.data.ortho_scale = "ORTHO", 5.3
    lettering = kit.create_principled_material(bpy, "Preview lettering", (.74, .82, .85), 0, .8)
    for (title, _), offset in zip(groups, offsets):
        bpy.ops.object.text_add(location=offset + forward * .82 + Vector((0, 0, .025)))
        label = bpy.context.object
        label.name = title + " preview label"
        label.rotation_euler = camera.rotation_euler
        label.data.body, label.data.align_x, label.data.size = title.upper(), "CENTER", .105
        label.data.materials.append(lettering)
    scene = bpy.context.scene
    scene.camera, scene.render.engine = camera, "CYCLES"
    scene.cycles.device, scene.cycles.samples, scene.cycles.use_denoising = "CPU", 32, True
    scene.render.resolution_x, scene.render.resolution_y = 2000, 1400
    scene.render.resolution_percentage = 100
    scene.world.color = (.20, .20, .20)
    scene.view_settings.view_transform = "AgX"
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(kit.SOURCE / "logistics-preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(kit.SOURCE / "mk1-logistics.blend"))
    if "--no-render" not in sys.argv:
        bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    for slug, title, build in [
        ("pipe-straight", "Straight pipe", pipe_straight),
        ("pipe-elbow", "90 degree pipe", pipe_elbow),
        ("belt-turn-left", "90 degree belt - left", lambda: belt_corner(-1)),
        ("belt-turn-right", "90 degree belt - right", lambda: belt_corner(1)),
    ]:
        export(slug, title, build())
    write_json(kit.SOURCE / "logistics-manifest.json", {
        "version": 1, "units": "tile", "up_axis": "Y", "connection_height": .34,
        "gameplay_status": "implemented: straight/elbow fluid transport and left/right corner belts, saves and co-op",
        "models": catalog,
    })
    showroom()
    studio()
