"""Build a stylized, game-ready AR-15 prop in Blender.

Run: blender --background --python tools/generate_ar15.py
Outputs: assets/ar15/ar15.blend, ar15.glb, and ar15_preview.png.
All dimensions are approximate visual proportions in meters; this is artwork,
not a parts drawing. The barrel points along +X and the top is +Z in Blender.
"""

import math
from pathlib import Path

import bpy
from mathutils import Vector


ROOT = Path(__file__).resolve().parents[1] / "assets" / "ar15"
ROOT.mkdir(parents=True, exist_ok=True)
bpy.context.preferences.filepaths.save_version = 0

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def material(name, color, metallic=0.0, roughness=0.5):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*color, 1)
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = roughness
    return mat


anodized = material("01 Receiver - graphite anodized", (0.095, 0.103, 0.108), 0.72, 0.38)
polymer = material("02 Polymer - charcoal", (0.047, 0.050, 0.051), 0.05, 0.77)
steel = material("03 Steel - parkerized", (0.16, 0.17, 0.17), 0.78, 0.42)
edge = material("04 Edge accents", (0.27, 0.28, 0.27), 0.74, 0.47)
recess = material("05 Recesses", (0.015, 0.018, 0.020), 0.10, 0.85)
optic_glass = material("06 Optic glass", (0.065, 0.16, 0.17), 0.38, 0.15)

parts = []


def finish(obj, name, mat, bevel=0.0):
    obj.name = name
    obj.data.materials.append(mat)
    if bevel:
        mod = obj.modifiers.new("Soft machined edges", "BEVEL")
        mod.width = bevel
        mod.segments = 2
        mod.affect = "EDGES"
        mod.loop_slide = True
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=mod.name)
    for poly in obj.data.polygons:
        poly.use_smooth = False
    parts.append(obj)
    return obj


def box(name, loc, size, mat, bevel=0.0, tilt=0.0):
    bpy.ops.mesh.primitive_cube_add(size=1, location=loc)
    obj = bpy.context.object
    obj.dimensions = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    obj.rotation_euler[1] = tilt
    return finish(obj, name, mat, bevel)


def profile(name, outline, width, mat, bevel=0.0, y=0.0):
    """Extrude an X/Z silhouette equally to either side of the centerline."""
    n = len(outline)
    verts = [(x, y - width / 2, z) for x, z in outline]
    verts += [(x, y + width / 2, z) for x, z in outline]
    faces = [tuple(range(n)), tuple(reversed(range(n, 2 * n)))]
    faces += [(i + n, (i + 1) % n + n, (i + 1) % n, i) for i in range(n)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, mat, bevel)


def cylinder(name, a, b, radius, mat, vertices=16):
    a, b = Vector(a), Vector(b)
    mid = (a + b) / 2
    delta = b - a
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=delta.length, location=mid)
    obj = bpy.context.object
    obj.rotation_euler = delta.to_track_quat("Z", "Y").to_euler()
    return finish(obj, name, mat)


def rod(name, points, radius, mat, vertices=8):
    for i in range(len(points) - 1):
        cylinder(f"{name} {i+1}", points[i], points[i + 1], radius, mat, vertices)


# Upper and lower receiver: a readable silhouette from either side.
profile("Upper receiver", [(-.155, .055), (.125, .055), (.135, .090), (.120, .150),
                            (.089, .165), (-.117, .165), (-.155, .134)], .064, anodized, .005)
profile("Lower receiver", [(-.145, .052), (.103, .052), (.085, -.009),
                            (.045, -.031), (-.058, -.030), (-.076, -.009),
                            (-.139, -.004)], .065, anodized, .004)
profile("Magazine well", [(.013, -.012), (.079, -.012), (.063, -.105),
                           (-.008, -.098)], .067, anodized, .003)
box("Upper and lower seam", (-.012, 0, .049), (.255, .067, .003), recess)
box("Ejection port", (.025, -.035, .108), (.082, .002, .028), recess, .002)
box("Ejection cover lip", (.025, -.038, .090), (.09, .004, .005), steel, .001)
box("Bolt detail", (.023, -.038, .109), (.057, .001, .012), edge, .001)
cylinder("Forward assist", (-.101, -.036, .103), (-.101, -.056, .103), .012, steel, 12)
cylinder("Selector", (-.079, -.036, .012), (-.079, -.046, .012), .009, steel, 12)
box("Selector lever", (-.069, -.047, .013), (.025, .006, .005), edge, .001, -.28)
for x, z in [(-.119, .026), (.068, .025)]:
    cylinder("Receiver pin", (x, -.036, z), (x, -.040, z), .005, steel, 12)

# Trigger and open guard; the narrow rods are visual only.
rod("Trigger guard", [(-.065, -.005, -.025), (-.050, -.005, -.054),
                       (.003, -.005, -.054), (.020, -.005, -.028)], .0034, steel)
rod("Trigger", [(-.009, -.005, -.018), (-.004, -.005, -.039)], .003, steel)

# Angled polymer grip, finger ledge and shallow ribs.
profile("Pistol grip", [(-.114, -.015), (-.058, -.022), (-.094, -.201),
                        (-.129, -.197), (-.148, -.157)], .055, polymer, .006)
box("Grip heel", (-.112, 0, -.202), (.045, .059, .013), recess, .003, -.13)
for i in range(5):
    z = -.087 - i * .021
    x = -.112 - i * .004
    box(f"Grip texture {i+1}", (x, -.029, z), (.039, .002, .004), recess, .001, -.12)

# Curved-looking removable magazine with a distinct floor plate.
profile("Magazine shell", [(0.000, -.096), (.073, -.101), (.061, -.233),
                           (.025, -.280), (-.050, -.266), (-.041, -.216)],
        .049, steel, .004)
profile("Magazine front face", [(.072, -.116), (.061, -.229), (.025, -.275),
                                (.020, -.261), (.044, -.221), (.050, -.121)],
        .052, edge, .001)
for i in range(4):
    z = -.142 - i * .025
    box(f"Magazine rib {i+1}", (.022 - i * .004, -.026, z), (.064, .003, .004), recess, .001)
box("Magazine floor plate", (-.013, 0, -.276), (.085, .055, .011), polymer, .003, -.08)

# Buffer assembly and adjustable stock; negative X is the rear of the prop.
cylinder("Buffer tube", (-.367, 0, .085), (-.145, 0, .085), .025, steel, 16)
cylinder("Buffer tube collar", (-.166, 0, .085), (-.145, 0, .085), .032, edge, 16)
profile("Stock cheek rest", [(-.474, .112), (-.282, .115), (-.271, .091),
                             (-.300, .069), (-.455, .066)], .073, polymer, .009)
profile("Stock frame", [(-.465, .068), (-.406, .068), (-.369, .025),
                        (-.289, .040), (-.267, .030), (-.359, -.009),
                        (-.435, -.017)], .037, polymer, .006)
rod("Stock lower strut", [(-.430, -.017, -.012), (-.370, -.017, .032),
                          (-.301, -.017, .050)], .006, edge)
box("Stock butt pad", (-.480, 0, .050), (.020, .082, .149), polymer, .006, -.09)
box("Stock release", (-.329, -.028, .015), (.038, .010, .012), edge, .002)

# Octagonal handguard with thin axial facets. It leaves the exposed barrel visible.
hand_start, hand_end = .132, .446
for i in range(8):
    a = i * math.tau / 8
    b = (i + 1) * math.tau / 8
    r = .046
    # A thin quad wall running along X; all facets share the same bore axis.
    verts = [(hand_start, r*math.cos(a), .105+r*math.sin(a)),
             (hand_end, r*math.cos(a), .105+r*math.sin(a)),
             (hand_end, r*math.cos(b), .105+r*math.sin(b)),
             (hand_start, r*math.cos(b), .105+r*math.sin(b))]
    mesh = bpy.data.meshes.new(f"Handguard facet {i}")
    mesh.from_pydata(verts, [], [(3, 2, 1, 0)])
    mesh.update()
    obj = bpy.data.objects.new(f"Handguard facet {i+1}", mesh)
    bpy.context.collection.objects.link(obj)
    finish(obj, obj.name, anodized)
for x in (hand_start, hand_end):
    cylinder("Handguard end ring", (x-.006, 0, .105), (x+.006, 0, .105), .048, steel, 12)
for side in (-1, 1):
    for i in range(5):
        x = .169 + i*.054
        box(f"M-LOK slot {side} {i+1}", (x, side*.046, .104),
            (.033, .002, .008), recess, .003)
    box(f"Handguard lower groove {side}", (.288, side*.027, .064),
        (.275, .003, .004), recess, .001)

# Barrel and muzzle furniture are visual geometry, without internal mechanisms.
cylinder("Exposed barrel", (.438, 0, .105), (.659, 0, .105), .014, steel, 20)
cylinder("Gas block silhouette", (.446, 0, .105), (.478, 0, .105), .020, recess, 12)
cylinder("Muzzle shoulder", (.650, 0, .105), (.672, 0, .105), .019, edge, 12)
cylinder("Muzzle device", (.671, 0, .105), (.710, 0, .105), .022, steel, 16)
for x in (.681, .697):
    box("Muzzle port upper", (x, -.021, .114), (.007, .003, .009), recess, .001)
    box("Muzzle port lower", (x, -.021, .097), (.007, .003, .007), recess, .001)
box("Muzzle dark opening", (.710, 0, .105), (.001, .017, .017), recess, .001)

# Full top Picatinny silhouette, broken into small rail teeth.
box("Receiver rail spine", (-.007, 0, .166), (.245, .063, .010), steel, .002)
box("Handguard rail spine", (.288, 0, .153), (.315, .058, .010), steel, .002)
for i in range(29):
    x = -.115 + i*.0185
    z = .177 if x < .126 else .164
    box(f"Rail tooth {i+1:02d}", (x, 0, z), (.010, .062, .006), edge, .001)

# Low-profile rear sight and front post.
box("Rear sight base", (-.105, 0, .183), (.032, .050, .012), recess, .002)
for y in (-.020, .020):
    box("Rear sight ear", (-.105, y, .206), (.012, .009, .039), steel, .002)
box("Rear sight crossbar", (-.105, 0, .219), (.010, .034, .006), steel, .001)
box("Front sight base", (.428, 0, .167), (.025, .049, .011), recess, .001)
for y in (-.019, .019):
    box("Front sight ear", (.429, y, .192), (.010, .008, .039), steel, .001)
cylinder("Front sight post", (.430, 0, .173), (.430, 0, .212), .0025, edge, 8)

# Red-dot optic: its lens faces rearward and reads clearly at game distance.
box("Optic riser", (.016, 0, .194), (.064, .043, .025), steel, .003)
cylinder("Optic body", (-.013, 0, .221), (.046, 0, .221), .028, anodized, 20)
cylinder("Rear optic bezel", (-.020, 0, .221), (-.012, 0, .221), .030, steel, 20)
cylinder("Front optic bezel", (.045, 0, .221), (.053, 0, .221), .030, steel, 20)
cylinder("Front optic glass", (.053, 0, .221), (.054, 0, .221), .022, optic_glass, 20)
cylinder("Optic adjustment dial", (.013, 0, .251), (.013, 0, .264), .010, edge, 12)

# Named mount empties remain in the editable source for attachment placement.
for name, loc in [("MOUNT_muzzle_fx", (.714, 0, .105)),
                  ("MOUNT_grip", (-.090, 0, -.090)),
                  ("MOUNT_sight", (.430, 0, .213))]:
    empty = bpy.data.objects.new(name, None)
    empty.empty_display_size = .015
    empty.location = loc
    bpy.context.collection.objects.link(empty)

# Source is authored in Blender's Z-up system and is fully editable.
for obj in bpy.context.selected_objects:
    obj.select_set(False)
for obj in parts:
    obj.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
bpy.ops.wm.save_as_mainfile(filepath=str(ROOT / "ar15.blend"))

# Duplicate and join by material for fewer GLB draw calls while retaining the
# individually editable source pieces in the .blend file.
export_parts = []
for mat in (anodized, polymer, steel, edge, recess, optic_glass):
    originals = [o for o in parts if o.data.materials and o.data.materials[0] == mat]
    clones = []
    for original in originals:
        clone = original.copy()
        clone.data = original.data.copy()
        bpy.context.collection.objects.link(clone)
        clones.append(clone)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in clones:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = clones[0]
    if len(clones) > 1:
        bpy.ops.object.join()
    clones[0].name = "AR15 " + mat.name
    clones[0].data.name = clones[0].name
    export_parts.append(clones[0])

bpy.ops.object.select_all(action="DESELECT")
for obj in export_parts:
    obj.select_set(True)
bpy.context.view_layer.objects.active = export_parts[0]
bpy.ops.export_scene.gltf(filepath=str(ROOT / "ar15.glb"), export_format="GLB",
                          use_selection=True, export_yup=True,
                          export_apply=True, export_extras=False)

# Render a clean preview without putting studio objects into the game asset.
for obj in export_parts:
    bpy.data.objects.remove(obj, do_unlink=True)
bpy.ops.object.select_all(action="DESELECT")

world = bpy.context.scene.world
world.color = (.25, .25, .25)
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (.065, .085, .105, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = .6

def area(name, loc, energy, size):
    data = bpy.data.lights.new(name, "AREA")
    data.energy = energy
    data.shape = "DISK"
    data.size = size
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    obj.location = loc
    obj.rotation_euler = (Vector((0, 0, .02)) - obj.location).to_track_quat("-Z", "Y").to_euler()

area("Preview key", (.2, -1.3, 1.4), 125, 2.0)
area("Preview rim", (-.3, .8, 1.0), 150, 1.2)
camera_data = bpy.data.cameras.new("Preview camera")
camera = bpy.data.objects.new("Preview camera", camera_data)
bpy.context.collection.objects.link(camera)
camera.location = (1.12, -1.35, .70)
camera.rotation_euler = (Vector((.09, 0, .025)) - camera.location).to_track_quat("-Z", "Y").to_euler()
camera_data.type = "ORTHO"
camera_data.ortho_scale = 1.52
bpy.context.scene.camera = camera
bpy.context.scene.render.engine = "CYCLES"
bpy.context.scene.cycles.samples = 32
bpy.context.scene.render.resolution_x = 1600
bpy.context.scene.render.resolution_y = 900
bpy.context.scene.render.resolution_percentage = 100
bpy.context.scene.render.film_transparent = False
bpy.context.scene.render.image_settings.file_format = "PNG"
bpy.context.scene.render.filepath = str(ROOT / "ar15_preview.png")
bpy.ops.render.render(write_still=True)
print(f"Wrote {ROOT / 'ar15.blend'} and {ROOT / 'ar15.glb'}")
