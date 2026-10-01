"""Four grounded wreck sections of one survey ship, with GLBs and a showroom.

blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_spaceship_debris.py
Add ``-- --no-render`` to skip the contact sheet. No external textures are used.
Game coordinates: X/Y(up)/Z, one unit per tile, pivot at ground center.
"""
from pathlib import Path
import math
import sys

import bpy
import bmesh
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from blender_helpers import create_principled_material
from generate_scene import cube, transform, write_json

ASSETS = ROOT / "examples/earth-factory/scenes/assets"
OUT = ASSETS / "models/debris"
SOURCE = ROOT / "assets/spaceship-debris"
parts, groups, catalog = [], [], []


def point(p):
    return Vector((p[0], -p[2], p[1]))


def finish(obj, name, material):
    obj.name = name
    obj.data.materials.append(material)
    parts.append(obj)
    return obj


def mesh_part(name, vertices, faces, material):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([point(p) for p in vertices], [], faces)
    mesh.update()
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, material)


def box(name, pos, size, material, bevel=.012):
    bpy.ops.mesh.primitive_cube_add(size=1, location=point(pos))
    obj = bpy.context.object
    obj.dimensions = (size[0], size[2], size[1])
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        # A bevel reaching half a thin panel's thickness collapses its faces.
        bevel = min(bevel, min(size) * .20)
        mod = obj.modifiers.new("Chipped edges", "BEVEL")
        mod.width, mod.segments = bevel, 1
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return finish(obj, name, material)


def beam(name, a, b, width=.055, material=None):
    a, b = point(a), point(b)
    obj = box(name, (0, 0, 0), (width, (b-a).length, width), material or steel, 0)
    obj.location = (a+b)/2
    obj.rotation_euler = (b-a).to_track_quat("Z", "Y").to_euler()
    return obj


def plate(name, outline, height, thickness, material):
    n = len(outline)
    verts = [(x, height, z) for x, z in outline]
    verts += [(x, height+thickness, z) for x, z in outline]
    faces = [tuple(reversed(range(n))), tuple(range(n, 2*n))]
    faces += [(i, (i+1)%n, (i+1)%n+n, i+n) for i in range(n)]
    return mesh_part(name, verts, faces, material)


def shell(name, stations, material, missing=(), canopy=False):
    """Octagonal armor with real wall thickness and uneven fracture edges.

    Stations: longitudinal Z, X radius, Y radius, Y center, per-corner Z tears.
    """
    count = 8
    vertices, faces, slots = [], [], []
    for scale in [1.0, .87]:
        for z, rx, ry, center, tears in stations:
            for i in range(count):
                a = math.tau*i/count
                vertices.append((rx*scale*math.cos(a), center+ry*scale*math.sin(a), z+tears[i]))
    inside = len(stations)*count
    for s in range(len(stations)-1):
        for i in range(count):
            if i in missing:
                continue
            a, b = s*count+i, s*count+(i+1)%count
            faces.extend([(a, b, b+count, a+count),
                          (inside+a+count, inside+b+count, inside+b, inside+a)])
            slots.extend([3 if canopy and s == 1 and i in [1, 2] else 0, 1])
    for s in [0, len(stations)-1]:
        for i in range(count):
            if i in missing:
                continue
            a, b = s*count+i, s*count+(i+1)%count
            faces.append((a, inside+a, inside+b, b))
            slots.append(2)
    # Close the wall on both sides of each missing panel, leaving an open cavity.
    for edge in [i for i in range(count) if (i in missing) != ((i-1)%count in missing)]:
        for s in range(len(stations)-1):
            a, b = s*count+edge, (s+1)*count+edge
            faces.append((a, b, inside+b, inside+a))
            slots.append(2)
    obj = mesh_part(name, vertices, faces, material)
    for mat in [char, steel, glass]:
        obj.data.materials.append(mat)
    for face, slot in zip(obj.data.polygons, slots):
        face.material_index = slot
    return obj


def rib(z, radius=.65, center=.62, missing=(1, 2)):
    for i in range(8):
        if i in missing:
            continue
        a, b = math.tau*i/8, math.tau*(i+1)/8
        beam("Exposed octagonal frame rib", (radius*math.cos(a), center+radius*.76*math.sin(a), z),
             (radius*math.cos(b), center+radius*.76*math.sin(b), z), .045)


def cable(points, material=None):
    for a, b in zip(points, points[1:]):
        beam("Severed service cable", a, b, .027, material or cyan)


def insignia(x, y, z, width=.55):
    # Shared double gold bars and cyan service stripe identify the same vessel.
    for offset in [-.055, .055]:
        box("Survey ship double gold marking", (x, y, z+offset), (width, .012, .04), gold, 0)
    box("Survey ship cyan service marking", (x, y+.005, z+.16), (width*.6, .014, .025), cyan, 0)


def shards(side=1):
    for x, z, size in [(1.04*side, .82, .28), (-.93*side, -.85, .23), (1.08*side, -.65, .18)]:
        plate("Loose torn armor fragment", [(x-size/2, z-size/2), (x+size/2, z),
              (x+size*.3, z+size*.5), (x-size*.4, z+size*.3)], .0, .04, armor)
        beam("Loose snapped frame fragment", (x-.06, .055, z-.08), (x+.10, .08, z+.10), .035)


def cockpit():
    shell("Cockpit hull with ripped rear bulkhead", [
        (-1.04, .13, .13, .30, [0]*8),
        (-.63, .57, .36, .47, [0]*8),
        (.20, .69, .48, .55, [0]*8),
        (.76, .64, .43, .53, [.14, -.12, .17, -.19, .08, -.08, .15, -.14]),
    ], armor, canopy=True)
    # Broken canopy mullions cross the two sloped blue windshield panes.
    beam("Windshield center mullion", (0, .83, -.63), (0, 1.035, .20), .045, char)
    for side in [-1, 1]:
        beam("Canopy side rail", (side*.40, .72, -.63), (side*.49, .89, .20), .045, char)
        beam("Fractured rear spar", (side*.45, .20, .48), (side*.48, .34, 1.05), .065)
        box("Hull identification stripe", (side*.651, .53, .39), (.014, .12, .39), gold, 0)
    rib(.54, .57, .53, missing=(1,))
    box("Charred cockpit floor", (0, .18, .45), (.82, .09, .65), char)
    box("Exposed pilot seat frame", (0, .36, .53), (.24, .26, .21), steel)
    cable([(.32, .58, .73), (.27, .43, .93), (.53, .16, 1.02)])
    plate("Collapsed side armor", [(-.68, .45), (-1.04, .64), (-.94, 1.07), (-.53, .81)], .02, .06, armor)
    insignia(0, .435, -1.0, .17)
    shards()


def hull():
    shell("Split cargo fuselage with missing roof", [
        (-.83, .65, .48, .52, [.10, -.18, .16, -.12, .20, -.05, .09, -.14]),
        (-.18, .72, .53, .56, [0]*8),
        (.52, .70, .51, .54, [0]*8),
        (.90, .62, .45, .51, [-.10, .12, -.18, .13, -.08, .14, -.12, .07]),
    ], armor, missing=(1, 2))
    for z in [-.43, .35]:
        rib(z, .63, .56)
    box("Exposed blackened cargo deck", (0, .14, .08), (.80, .09, 1.28), char)
    for x in [-.27, .27]:
        beam("Snapped longitudinal skeleton", (x, .21, -.95), (x, .20, 1.04), .06)
    for x, z in [(-.28, -.28), (.26, .25)]:
        box("Ruptured cargo cassette", (x, .33, z), (.25, .25, .35), steel)
        box("Cargo cassette cyan latch", (x, .464, z), (.15, .015, .06), cyan, 0)
    for side in [-1, 1]:
        box("Matching fuselage gold band", (side*.71, .56, -.02), (.015, .24, .18), gold, 0)
    flap = plate("Peeled roof panel", [(-.37, -.5), (.39, -.43), (.26, .31), (-.28, .42)], .01, .065, armor)
    flap.rotation_euler.x = -.30
    flap.location += point((-.73, .48, -.17))
    cable([(.41, .82, .73), (.57, .55, .86), (.82, .12, .99)], gold)
    shards(-1)


def wing():
    outline = [(-1.10, -.42), (-.78, -.60), (-.93, -.79), (-.38, -.56),
               (1.18, -.95), (.93, -.12), (.41, .88), (-.15, .59), (-.40, .22), (-.95, .36)]
    plate("Torn swept survey wing", outline, .08, .13, armor)
    plate("Scorched exposed wing-root structure", [(-1.13, -.43), (-.68, -.61),
          (-.76, .27), (-1.06, .38), (-.88, .06)], .075, .14, char)
    for z in [-.38, -.06, .19]:
        beam("Broken wing-root spar", (-1.18, .16, z), (-.64, .18, z-.08), .065)
    plate("Wing graphite service panel", [(-.5, -.38), (.72, -.67), (.29, .38), (-.08, .38)], .215, .012, char)
    insignia(.13, .234, -.22, .61)
    for z in [-.48, -.33, -.18]:
        box("Wing exposed vent fin", (.57, .25, z), (.18, .035, .04), steel, 0)
    fin = plate("Folded wing-tip fin", [(.33, -.28), (.91, -.53), (.66, .38)], .02, .07, armor)
    fin.rotation_euler.x = .58
    fin.location += point((0, .24, .1))
    cable([(-.81, .26, .11), (-1.00, .25, .42), (-1.12, .05, .61)])
    shards(-1)


def engine():
    shell("Torn engine casing and hollow exhaust", [
        (-.80, .48, .43, .52, [.12, -.20, .08, -.10, .13, -.08, .19, -.14]),
        (-.19, .57, .49, .58, [0]*8),
        (.50, .62, .54, .60, [0]*8),
        (.94, .49, .43, .60, [0]*8),
    ], armor, missing=(2,))
    for z, radius in [(.50, .57), (.94, .47), (-.40, .48)]:
        rib(z, radius, .60, missing=() if z > 0 else (2,))
    for i in range(8):
        a = math.tau*i/8
        beam("Exhaust nozzle cooling petal", (.41*math.cos(a), .60+.38*math.sin(a), .54),
             (.35*math.cos(a), .60+.32*math.sin(a), .96), .06, char)
        # An inert turbine sits visibly recessed inside the exhaust opening.
        beam("Seized radial turbine blade", (.12*math.cos(a+.35), .60+.12*math.sin(a+.35), .40),
             (.38*math.cos(a), .60+.35*math.sin(a), .43), .08, steel)
    box("Inert turbine hub", (0, .60, .40), (.19, .19, .20), char)
    box("Matching engine gold band", (.575, .60, .07), (.017, .28, .17), gold, 0)
    beam("Broken engine pylon", (-.38, .26, -.14), (-.92, .15, -.48), .15, char)
    cable([(-.31, .85, -.78), (-.62, .65, -.94), (-.73, .24, -.98)])
    cable([(.21, .42, -.83), (.47, .27, -1.07), (.75, .04, -.97)], gold)
    shards()


def export(slug, title, description):
    # Bake each part into a grounded, centered group before joining by material.
    bpy.context.view_layer.update()
    for obj in parts:
        obj.data.transform(obj.matrix_world)
        obj.matrix_world.identity()
    vertices = [v.co for obj in parts for v in obj.data.vertices]
    low = Vector(tuple(min(v[i] for v in vertices) for i in range(3)))
    high = Vector(tuple(max(v[i] for v in vertices) for i in range(3)))
    offset = Vector((-(low.x+high.x)/2, -(low.y+high.y)/2, -low.z))
    for obj in parts:
        for vertex in obj.data.vertices:
            vertex.co += offset
    # Separate shell multi-material faces before merging: one surface per material.
    bpy.ops.object.select_all(action="DESELECT")
    for obj in list(parts):
        if len(obj.data.materials) > 1:
            bpy.context.view_layer.objects.active = obj
            obj.select_set(True)
            bpy.ops.object.mode_set(mode="EDIT")
            bpy.ops.mesh.select_all(action="SELECT")
            bpy.ops.mesh.separate(type="MATERIAL")
            bpy.ops.object.mode_set(mode="OBJECT")
            for split in list(bpy.context.selected_objects):
                if split not in parts:
                    parts.append(split)
            bpy.ops.object.select_all(action="DESELECT")
    merged = []
    for material in MATERIALS:
        copies = []
        for original in parts:
            if not original.data.polygons or original.data.materials[original.data.polygons[0].material_index] != material:
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
        obj.name = slug+" / "+material.name
        # Remove unused material slots left by material separation.
        obj.data.materials.clear()
        obj.data.materials.append(material)
        for face in obj.data.polygons:
            face.material_index = 0
        merged.append(obj)
    low, high = [math.inf]*3, [-math.inf]*3
    triangles = 0
    for obj in merged:
        obj.data.calc_loop_triangles()
        triangles += len(obj.data.loop_triangles)
        for vertex in obj.data.vertices:
            p = (vertex.co.x, vertex.co.z, -vertex.co.y)
            for i in range(3):
                low[i], high[i] = min(low[i], p[i]), max(high[i], p[i])
    assert abs(low[1]) < .001 and triangles < 5000, (slug, low, triangles)
    assert min(low[0], low[2]) >= -1.45 and max(high[0], high[2]) <= 1.45, (slug, low, high)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT / f"{slug}.glb"), export_format="GLB",
        use_selection=True, export_yup=True, export_apply=True, export_animations=False,
        export_cameras=False, export_lights=False)
    entry = {"id": slug, "name": title, "description": description,
        "mesh": f"models/debris/{slug}.glb", "prefab": f"{slug}.prefab.json",
        "triangles": triangles, "surfaces": len(merged),
        "bounds": [[round(v, 6) for v in bound] for bound in [low, high]]}
    catalog.append(entry)
    write_json(ASSETS / entry["prefab"], {"version": 1, "name": title,
        "root": "root", "assets": {slug: {"kind": "mesh", "path": entry["mesh"]}},
        "objects": [{"id": "root", "name": title, "transform": transform(),
            "drawable": {"layer": "3d", "mesh": {"asset": slug}, "texture": "white",
                         "color": [1, 1, 1], "uv_scale": [1, 1]}}]})
    for obj in merged:
        bpy.data.objects.remove(obj, do_unlink=True)
    groups.append((title, list(parts)))
    parts.clear()
    print(f"debris_export {slug}: {triangles} triangles", flush=True)


def showroom():
    assets, objects = {}, []
    objects.append({"id": "camera", "name": "Debris showroom camera",
        "transform": {**transform(10, 10, 10), "rotation_degrees": [-35.264, 45, 0]},
        "camera": {"projection": "orthographic", "vertical_size": 10.8, "near": .1, "far": 100}})
    objects.append(cube("floor", "Neutral planet surface", (0, -.06, 0), (30, .12, 30), [.14, .17, .18]))
    for i, entry in enumerate(catalog):
        slug = entry["id"]
        assets[slug] = {"kind": "mesh", "path": "assets/"+entry["mesh"]}
        objects.append({"id": slug, "name": entry["name"],
            "transform": transform((i%2-.5)*4.4, 0, (i//2-.5)*4.4),
            "drawable": {"layer": "3d", "mesh": {"asset": slug}, "texture": "white",
                         "color": [1, 1, 1], "uv_scale": [1, 1]}})
    write_json(ASSETS.parent / "debris-showroom.json", {"version": 1,
        "name": "Stellar-IX — survey ship wreckage prototypes", "views": {"3d": "camera"},
        "assets": assets, "objects": objects,
        "environment": {"zenith": [.20, .25, .29], "horizon": [.20, .25, .29],
                        "ground": [.14, .17, .18], "intensity": .55, "background": True},
        "lighting": {"shadows": True, "sun_direction": [.46, .81, .35],
            "sun_color": [1, .96, .90], "sun_intensity": 2.6,
            "ambient_color": [.86, .94, 1], "ambient_intensity": .24}})


def studio():
    right, forward = Vector((.7071, .7071, 0)), Vector((.7071, -.7071, 0))
    offsets = []
    for i, (title, group) in enumerate(groups):
        offset = right*((i%2-.5)*3.8)+forward*((i//2-.5)*4.0)
        offsets.append(offset)
        collection = bpy.data.collections.new(title)
        bpy.context.scene.collection.children.link(collection)
        for obj in group:
            obj.location += offset
            for old in list(obj.users_collection):
                old.objects.unlink(obj)
            collection.objects.link(obj)
    floor = create_principled_material(bpy, "Studio ground", (.045, .062, .075), 0, .85)
    box("Studio ground", (0, -.065, 0), (200, .10, 200), floor, 0)
    for name, location, energy, size in [
        ("Key", (2, -4, 7), 1250, 6), ("Rim", (-5, 2, 6), 1450, 5),
        ("Fill", (4, 5, 5), 950, 6)]:
        data = bpy.data.lights.new(name, "AREA")
        data.energy, data.shape, data.size = energy, "DISK", size
        obj = bpy.data.objects.new(name, data)
        bpy.context.collection.objects.link(obj)
        obj.location = location
        obj.rotation_euler = (-obj.location).to_track_quat("-Z", "Y").to_euler()
    bpy.ops.object.camera_add(location=(9, -9, 11))
    camera = bpy.context.object
    camera.rotation_euler = (Vector((0, 0, .12))-camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type, camera.data.ortho_scale = "ORTHO", 9.8
    lettering = create_principled_material(bpy, "Preview lettering", (.70, .79, .81), 0, .8)
    for (title, _), offset in zip(groups, offsets):
        bpy.ops.object.text_add(location=offset+forward*1.75+Vector((0, 0, .05)))
        label = bpy.context.object
        label.name = title+" preview label"
        label.rotation_euler = camera.rotation_euler
        label.data.body, label.data.align_x, label.data.size = title.upper(), "CENTER", .15
        label.data.materials.append(lettering)
    scene = bpy.context.scene
    scene.camera, scene.render.engine = camera, "CYCLES"
    scene.cycles.device, scene.cycles.samples, scene.cycles.use_denoising = "CPU", 24, True
    scene.render.resolution_x, scene.render.resolution_y = 1600, 1200
    scene.render.resolution_percentage = 100
    scene.world.color = (.18, .18, .18)
    scene.view_settings.view_transform = "AgX"
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(SOURCE / "debris-preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE / "survey-ship-debris.blend"))
    if "--no-render" not in sys.argv:
        bpy.ops.render.render(write_still=True)


def create_materials():
    """The intact travel ship and its wreckage share this material palette."""
    global armor, steel, char, gold, cyan, glass, MATERIALS
    armor = create_principled_material(bpy, "Survey ship ivory armor", (.48, .53, .52), .45, .65)
    steel = create_principled_material(bpy, "Exposed survey ship frame", (.22, .27, .29), .72, .43)
    char = create_principled_material(bpy, "Scorched graphite interiors", (.024, .032, .038), .15, .85)
    gold = create_principled_material(bpy, "Survey ship gold identification", (.95, .63, .17), .2, .55)
    cyan = create_principled_material(bpy, "Survey ship cyan service lines", (.12, .55, .63), .2, .55)
    glass = create_principled_material(bpy, "Opaque fractured cockpit glazing", (.035, .14, .19), .4, .22)
    MATERIALS = [armor, steel, char, gold, cyan, glass]
    return MATERIALS


def main():
    for directory in [OUT, SOURCE]:
        directory.mkdir(parents=True, exist_ok=True)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    create_materials()
    for slug, title, description, build in [
        ("debris-cockpit", "Broken cockpit", "Fractured canopy, severed bulkhead and exposed pilot frame.", cockpit),
        ("debris-hull", "Split cargo hull", "Missing roof, peeled armor, cargo cassettes and torn ribs.", hull),
        ("debris-wing", "Torn survey wing", "Swept wing, snapped root spars and folded tip fin.", wing),
        ("debris-engine", "Ruptured engine", "Open exhaust, inert turbine, torn casing and broken pylon.", engine),
    ]:
        build()
        export(slug, title, description)
    write_json(SOURCE / "manifest.json", {"version": 1, "ship": "Stellar-IX survey ship",
        "units": "tile", "up_axis": "Y", "pivot": "ground center", "footprint": [3, 3],
        "spawn": {"minimum_per_planet": 2, "maximum_per_planet": 6,
            "optional_slots": 4, "optional_slot_chance_percent": 12,
            "placement": "seeded distinct non-landing regions; resources kept clear"},
        "models": catalog})
    showroom()
    studio()


if __name__ == "__main__":
    main()
