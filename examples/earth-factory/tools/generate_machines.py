"""Model the sketch-based Mk1 machines in Blender.

blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_machines.py
Exports meter-scale, Y-up GLBs with their output facing +X. The editable Blender
source and studio preview live in assets/factory-machines. Runtime status lamps
and furnace emission are separate prefabs, controlled by the power graph.
"""
from pathlib import Path
import math
import sys

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
from blender_helpers import create_principled_material

OUT = ROOT / "examples/earth-factory/scenes/assets/models"
SOURCE = ROOT / "assets/factory-machines"
OUT.mkdir(parents=True, exist_ok=True)
SOURCE.mkdir(parents=True, exist_ok=True)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

paint = create_principled_material(bpy, "Graphite enamel", (.105, .125, .14), .55, .42)
steel = create_principled_material(bpy, "Machined steel", (.30, .34, .36), .72, .32)
dark = create_principled_material(bpy, "Recesses and belt", (.025, .032, .038), .15, .7)
inlet = create_principled_material(bpy, "Cyan inlet markings", (.24, .78, .84), .2, .45)
outlet = create_principled_material(bpy, "Gold outlet markings", (.95, .73, .25), .2, .45)


def emission(name, color, strength):
    material = create_principled_material(bpy, name, color, .0, .45)
    shader = material.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Emission Color"].default_value = (*color, 1)
    shader.inputs["Emission Strength"].default_value = strength
    return material


green = emission("Powered indicator (preview)", (.24, .9, .29), 1.3)
hot = emission("Furnace heat (preview)", (1., .23, .045), 3)
parts = []


def finish(obj, name, material, bevel=0):
    obj.name = name
    obj.data.materials.append(material)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = obj.modifiers.new("Edge highlights", "BEVEL")
        mod.width = bevel
        mod.segments = 1
        bpy.ops.object.modifier_apply(modifier=mod.name)
    parts.append(obj)
    return obj


def box(name, pos, size, material=paint, bevel=.006):
    # Author in the engine's X/Y(up)/Z coordinates, then map to Blender Z-up.
    x, y, z = pos
    sx, sy, sz = size
    bpy.ops.mesh.primitive_cube_add(size=1, location=(x, -z, y))
    obj = bpy.context.object
    obj.dimensions = (sx, sz, sy)
    return finish(obj, name, material, bevel)


def cylinder(name, pos, radius, depth, material=steel, axis="vertical", tip=None):
    x, y, z = pos
    bpy.ops.mesh.primitive_cone_add(vertices=12, radius1=radius if tip is None else tip,
                                    radius2=radius, depth=depth, location=(x, -z, y))
    obj = bpy.context.object
    if axis == "roller":
        obj.rotation_euler[0] = math.pi / 2
    elif axis == "output":
        obj.rotation_euler[1] = math.pi / 2
    return finish(obj, name, material)


def belt(prefix, x, length, width):
    box(prefix + " belt", (x, .305, 0), (length, .055, width), dark)
    for side in [-1, 1]:
        box(prefix + " side rail", (x, .325, side * (width / 2 + .025)),
            (length, .07, .04), steel, .004)
    for i in range(7):
        box(prefix + " tread", (x - length * .42 + length * .14 * i, .34, 0),
            (.017, .014, width - .025), steel, .001)


def drill():
    cylinder("Drill gearbox", (.08, .65, 0), .18, .15)
    cylinder("Auger core", (.08, .36, 0), .085, .46, steel)
    cylinder("Drill tip", (.08, .12, 0), .09, .14, steel, tip=.015)
    # A coarse helical flight makes the drill readable from the game camera.
    verts, faces = [], []
    steps = 48
    for i in range(steps + 1):
        a = i / steps * math.tau * 2.5
        height = .17 + i / steps * .38
        for radius, offset in [(.082, -.012), (.145, -.012), (.145, .012), (.082, .012)]:
            verts.append((.08 + radius * math.cos(a), radius * math.sin(a), height + offset))
    for i in range(steps):
        for j in range(4):
            faces.append((i*4+j, i*4+(j+1)%4, (i+1)*4+(j+1)%4, (i+1)*4+j))
    faces += [(3, 2, 1, 0), tuple(steps*4+j for j in range(4))]
    mesh = bpy.data.meshes.new("Auger flight")
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("Helical drill flight", mesh)
    bpy.context.collection.objects.link(obj)
    finish(obj, "Helical drill flight", steel)


def terminal(height):
    cylinder("Centered cable socket", (0, height - .025, 0), .07, .06, dark)
    cylinder("Cable terminal", (0, height + .03, 0), .032, .05, steel)


def export(name):
    """Group static pieces by material, excluding the runtime emissive lenses."""
    originals = list(parts)
    merged = []
    for material in [paint, steel, dark, inlet, outlet]:
        copies = []
        for original in originals:
            if original.data.materials[0] != material:
                continue
            clone = original.copy()
            clone.data = original.data.copy()
            bpy.context.collection.objects.link(clone)
            copies.append(clone)
        if not copies:
            continue
        bpy.ops.object.select_all(action="DESELECT")
        for obj in copies:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = copies[0]
        bpy.ops.object.join()
        obj = copies[0]
        obj.name = name + " / " + material.name
        merged.append(obj)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT / (name + ".glb")), export_format="GLB",
                              use_selection=True, export_yup=True, export_apply=True,
                              export_animations=False, export_cameras=False, export_lights=False)
    triangles = 0
    for obj in merged:
        obj.data.calc_loop_triangles()
        triangles += len(obj.data.loop_triangles)
        bpy.data.objects.remove(obj, do_unlink=True)
    print(f"machine_export name={name} triangles={triangles} materials={len(merged)}")
    return originals


def arrow(name, x, z, direction, material, height=.365, size=.18):
    """Flat, slightly raised port arrow; direction is in the engine's X/Z plane."""
    outline = [(-.5, -.2), (.05, -.2), (.05, -.45), (.5, 0),
               (.05, .45), (.05, .2), (-.5, .2)]
    vertices = []
    for y in [height, height + .008]:
        for a, b in outline:
            dx = size * (a * math.cos(direction) - b * math.sin(direction))
            dz = size * (a * math.sin(direction) + b * math.cos(direction))
            vertices.append((x + dx, -(z + dz), y))
    n = len(outline)
    faces = [tuple(range(n)), tuple(reversed(range(n, n * 2)))]
    faces += [(i + n, (i + 1) % n + n, (i + 1) % n, i) for i in range(n)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, material)


def rotated_belt(prefix, x, z, direction, length=.30, width=.27):
    start = len(parts)
    belt(prefix, 0, length, width)
    for obj in parts[start:]:
        px, py, height = obj.location
        obj.location = (x + px * math.cos(direction) + py * math.sin(direction),
                        -z - px * math.sin(direction) + py * math.cos(direction), height)
        obj.rotation_euler.z -= direction


def processor_frame(name):
    for z in [-.34, .34]:
        box(name + " foot", (0, .055, z), (.90, .11, .18), steel)
        for x in [-.30, .30]:
            box(name + " frame post", (x, .50, z), (.10, .83, .11))
    box(name + " top gantry", (0, .915, 0), (.82, .11, .80))
    box(name + " motor cover", (0, .96, 0), (.34, .045, .35), steel)
    terminal(.97)
    box(name + " indicator bezel", (.422, .75, .32), (.04, .15, .085), dark, .003)


def processor_lens(name):
    box(name + " powered lens", (.447, .75, .32), (.012, .105, .045), green, .002)


# Smelter: clear through-tunnel, overhead element, belt and four-legged stand.
for z in [-.34, .34]:
    box("Smelter foot", (0, .045, z), (.93, .09, .16), steel)
    for x in [-.31, .31]:
        box("Smelter leg", (x, .165, z), (.11, .24, .10))
    box("Furnace insulated side", (0, .67, z), (.76, .69, .13))
    box("Side inset", (0, .69, z * 1.20), (.47, .36, .018), dark, .004)
    for x in [-.20, -.10, 0, .10, .20]:
        box("Cooling fin", (x, .70, z * 1.23), (.027, .28, .018), steel, .002)
box("Furnace roof", (0, 1.005, 0), (.82, .10, .82))
box("Heater backing", (0, .92, 0), (.69, .055, .54), dark)
for x in [-.385, .385]:
    for z in [-.32, .32]:
        box("Opening frame", (x, .64, z), (.055, .59, .065), steel, .003)
    box("Opening lintel", (x, .947, 0), (.055, .04, .68), steel, .003)
box("Indicator bezel", (.418, .56, .332), (.034, .145, .105), dark, .003)
belt("Smelter", 0, .98, .50)
terminal(1.04)
smelter = export("smelter-mk1")
smelter_lamp = box("Smelter powered lens", (.44, .56, .332), (.012, .10, .055), green, .002)
for x in [-.397, .397]:
    box("Glowing heater strip", (x, .883, 0), (.02, .038, .51), hot, .002)
smelter_preview = list(parts)
parts.clear()

# Miner: tall motor head, slotted collar, exposed auger between its legs, side chute.
for z in [-.30, .30]:
    box("Miner foot", (-.045, .038, z), (.80, .075, .20), steel)
    box("Miner upright", (-.04, .37, z), (.22, .66, .10))
box("Rear spine", (-.29, .44, 0), (.17, .77, .59))
box("Motor head", (-.03, .99, 0), (.67, .43, .68))
box("Motor face inset", (.313, 1.015, 0), (.024, .27, .53), dark, .004)
for z in [-.19, -.095, 0, .095, .19]:
    box("Head cooling slot", (.33, 1.015, z), (.02, .20, .021), paint, .002)
for y in [.747, .798]:
    box("Motor collar rib", (-.03, y, 0), (.70, .027, .71), steel, .003)
drill()
box("Side outlet housing", (.32, .22, 0), (.29, .15, .38))
belt("Miner output", .35, .29, .30)
box("Miner indicator bezel", (.322, 1.07, .287), (.035, .15, .08), dark, .002)
terminal(1.24)
miner = export("miner-mk1")
box("Miner powered lens", (.345, 1.07, .287), (.015, .105, .045), green, .002)
miner_preview = list(parts)
parts.clear()

# Constructor: one straight feed and a single exposed pressing head.
processor_frame("Constructor")
belt("Constructor through-feed", 0, .98, .48)
for z in [-.265, .265]:
    cylinder("Press guide rod", (0, .65, z), .034, .48)
box("Single press carriage", (0, .79, 0), (.32, .10, .60), steel)
cylinder("Press ram", (0, .70, 0), .072, .16)
box("Press die", (0, .635, 0), (.28, .055, .36), steel)
box("Rear hydraulic cabinet", (-.13, .56, -.36), (.30, .41, .13))
for y in [.48, .54, .60, .66]:
    box("Hydraulic cooling grille", (-.13, y, -.432), (.23, .023, .012), dark, .001)
arrow("Constructor input", -.39, 0, 0, inlet)
arrow("Constructor output", .39, 0, 0, outlet)
export("constructor-mk1")
processor_lens("Constructor")
constructor_preview = list(parts)
parts.clear()

# Assembler: two incoming feed decks and two tooling heads over a shared bed.
processor_frame("Assembler")
box("Assembly bed", (0, .27, 0), (.68, .11, .68), paint)
box("Assembly work surface", (0, .337, 0), (.55, .023, .55), dark)
rotated_belt("Assembler rear input", -.35, 0, 0)
rotated_belt("Assembler side input", 0, .35, -math.pi / 2)
rotated_belt("Assembler output", .35, 0, 0)
for x, z in [(-.12, -.13), (.12, .13)]:
    cylinder("Assembly spindle motor", (x, .79, z), .098, .14, paint)
    cylinder("Assembly spindle collar", (x, .70, z), .072, .065, steel)
    cylinder("Assembly tool", (x, .63, z), .035, .08, steel, tip=.018)
box("Assembler rear cabinet", (0, .61, -.36), (.51, .43, .13))
for x in [-.17, -.085, 0, .085, .17]:
    box("Assembler cooling grille", (x, .62, -.432), (.025, .26, .012), dark, .001)
arrow("Assembler rear input", -.40, 0, 0, inlet)
arrow("Assembler side input", 0, .40, -math.pi / 2, inlet)
arrow("Assembler output", .40, 0, 0, outlet)
export("assembler-mk1")
processor_lens("Assembler")
assembler_preview = list(parts)
parts.clear()

# Junctions: four low belt mouths, with arrows matching the routing rules.
# Their buffered items remain visible on top of the low central turntable.
junction_previews = {}
for name in ["splitter", "merger"]:
    title = name.title()
    box(title + " plinth", (0, .085, 0), (.80, .17, .80), paint)
    box(title + " housing", (0, .205, 0), (.65, .10, .65), steel)
    cylinder(title + " turntable surround", (0, .29, 0), .265, .07, dark)
    cylinder(title + " central deck", (0, .335, 0), .225, .02, paint)
    for direction in [0, math.pi / 2, math.pi, -math.pi / 2]:
        x, z = .355 * math.cos(direction), .355 * math.sin(direction)
        is_output = direction == 0 or (name == "splitter" and direction != math.pi)
        material = outlet if is_output else inlet
        port = title + (" outlet" if is_output else " inlet")
        rotated_belt(port, x, z, direction, length=.27, width=.25)
        arrow(port + " arrow", x, z, direction if is_output else direction + math.pi,
              material, size=.19)
    # Raised corner guards leave all four mouths open.
    for x in [-.29, .29]:
        for z in [-.29, .29]:
            box(title + " corner guard", (x, .34, z), (.13, .20, .13), paint)
            box(title + " guard cap", (x, .445, z), (.14, .025, .14), steel, .003)
    # A small central branching glyph reads even from the top-down camera.
    for direction in [0, math.pi / 2, -math.pi / 2]:
        arrow(title + " routing mark", .11 * math.cos(direction), .11 * math.sin(direction),
              direction if name == "splitter" else direction + math.pi,
              outlet if name == "splitter" else inlet, height=.352, size=.095)
    export(name + "-mk1")
    junction_previews[name] = list(parts)
    parts.clear()

# Storage: an armored bin above a rear-to-front feed tunnel. Both ports share
# the neighboring belts' deck height; the arrows match the simulation facing.
for z in [-.33, .33]:
    box("Storage skid", (0, .045, z), (.90, .09, .16), steel)
    box("Storage side wall", (0, .51, z), (.64, .86, .12))
    box("Storage recessed panel", (0, .64, z * 1.2), (.49, .35, .015), dark, .003)
    for x in [-.19, 0, .19]:
        box("Storage panel rib", (x, .64, z * 1.22), (.025, .34, .022), steel, .002)
for x in [-.32, .32]:
    box("Storage end bulkhead", (x, .79, 0), (.09, .29, .60))
    box("Storage loading hatch", (x * 1.16, .775, 0), (.024, .16, .40), steel)
    box("Storage latch", (x * 1.21, .79, 0), (.027, .055, .13), dark, .002)
box("Storage bin floor", (0, .64, 0), (.60, .055, .54), dark)
box("Storage roof", (0, .955, 0), (.77, .055, .82), steel)
box("Storage roof inset", (0, .991, 0), (.51, .024, .56))
belt("Storage through-feed", 0, .98, .45)
arrow("Storage input", -.415, 0, 0, inlet, size=.13)
arrow("Storage output", .415, 0, 0, outlet, size=.13)
for x, material in [(-.383, inlet), (.383, outlet)]:
    box("Storage port stripe", (x, .605, 0), (.020, .034, .48), material, .002)
export("storage-mk1")
storage_preview = list(parts)
parts.clear()

# Coal generator: the miner's exposed auger and braced stance, topped with a
# combustion housing, alternator and twin stacks instead of an ore outlet.
for z in [-.32, .32]:
    box("Generator skid", (-.02, .04, z), (.88, .08, .19), steel)
    box("Generator drill support", (-.06, .365, z), (.20, .65, .10))
box("Coal riser housing", (-.29, .49, 0), (.21, .89, .56))
drill()
box("Combustion chamber", (-.03, .93, 0), (.68, .40, .65))
box("Combustion collar", (-.03, .72, 0), (.73, .04, .70), steel)
box("Generator cooling recess", (.319, .96, 0), (.025, .23, .49), dark)
for z in [-.17, -.085, 0, .085, .17]:
    box("Generator radiator fin", (.341, .96, z), (.033, .18, .024), steel, .002)
# A side-mounted alternator makes the power source distinct from the miner.
cylinder("Alternator case", (-.045, .81, .365), .115, .40, paint, axis="output")
cylinder("Alternator end cap", (.168, .81, .365), .099, .025, steel, axis="output")
for x in [-.15, -.07, .01]:
    cylinder("Alternator cooling band", (x, .81, .365), .124, .024, steel, axis="output")
for z in [-.22, .22]:
    cylinder("Exhaust stack", (-.20, 1.24, z), .062, .32, paint)
    cylinder("Exhaust rim", (-.20, 1.405, z), .077, .028, steel)
    cylinder("Exhaust dark opening", (-.20, 1.421, z), .052, .005, dark)
    box("Generator warning stripe", (.20, 1.135, z), (.14, .018, .035), outlet, .001)
box("Generator indicator bezel", (.322, 1.055, .285), (.035, .13, .065), dark, .002)
terminal(1.24)
export("generator-mk1")
box("Generator powered lens", (.345, 1.055, .285), (.015, .09, .04), green, .002)
generator_preview = list(parts)
parts.clear()

# Conveyor: a low roller bed, open ends and directional markings. Side rails
# remain low enough for the existing side-entry/corner transport behavior.
for x in [-.32, .32]:
    box("Conveyor cross foot", (x, .05, 0), (.14, .10, .66))
    for z in [-.245, .245]:
        box("Conveyor support", (x, .19, z), (.09, .23, .08), steel)
for x in [-.405, .405]:
    cylinder("Conveyor end roller", (x, .28, 0), .063, .47, steel, axis="roller")
belt("Conveyor", 0, .99, .46)
for x in [-.17, .17]:
    arrow("Conveyor travel arrow", x, 0, 0, outlet, height=.355, size=.12)
export("belt-mk1")
belt_preview = list(parts)
parts.clear()

# Power pole: anchored steel mast, braced crossarm and ribbed insulators. The
# centered socket and lamp surround preserve the existing cable/light heights.
box("Pole footing", (0, .055, 0), (.55, .11, .55), paint)
box("Pole base plate", (0, .122, 0), (.35, .035, .35), steel)
for x in [-.125, .125]:
    for z in [-.125, .125]:
        cylinder("Pole anchor bolt", (x, .15, z), .025, .025, dark)
box("Pole mast", (0, .685, 0), (.115, 1.12, .115), steel)
box("Pole service box", (0, .44, .096), (.22, .29, .13), paint)
box("Pole service latch", (.067, .44, .166), (.025, .075, .018), outlet, .002)
box("Pole crossarm", (0, 1.245, 0), (.76, .075, .12), paint)
for x in [-.285, .285]:
    brace = box("Pole diagonal brace", (x / 2, 1.08, 0), (.035, .41, .045), steel, .002)
    brace.rotation_euler[1] = math.copysign(.68, x)
    cylinder("Pole insulator core", (x, 1.355, 0), .034, .18, dark)
    for y in [1.30, 1.345, 1.39]:
        cylinder("Pole insulator ring", (x, y, 0), .066, .022, steel)
    cylinder("Pole conductor pin", (x, 1.43, 0), .018, .055, outlet)
terminal(1.36)
for x in [-.12, .12]:
    box("Pole upper lamp support", (x, 1.48, 0), (.032, .46, .045), steel)
box("Pole lamp pedestal", (0, 1.70, 0), (.29, .09, .20), paint)
box("Pole lamp unlit glass", (0, 1.82, 0), (.205, .115, .205), dark, .003)
box("Pole lamp roof", (0, 1.90, 0), (.29, .045, .29), steel)
for x in [-.12, .12]:
    for z in [-.12, .12]:
        box("Pole lamp guard", (x, 1.81, z), (.023, .18, .023), paint, .002)
export("pole-mk1")
box("Pole powered lamp (preview)", (0, 1.82, 0), (.21, .12, .21), green, .002)
pole_preview = list(parts)
parts.clear()

# Studio composition and editable source keep all individually named components.
groups = [("MINER", miner_preview), ("GENERATOR", generator_preview),
          ("SMELTER", smelter_preview), ("CONSTRUCTOR", constructor_preview),
          ("ASSEMBLER", assembler_preview), ("CONVEYOR", belt_preview),
          ("SPLITTER", junction_previews["splitter"]), ("MERGER", junction_previews["merger"]),
          ("STORAGE", storage_preview), ("POWER POLE", pole_preview)]
right, forward = Vector((.78935, .61394, 0)), Vector((.61394, -.78935, 0))
offsets = []
for i, (name, group) in enumerate(groups):
    offset = right * ((i % 5 - 2) * 1.90) + forward * ((i // 5 - .5) * 4.3)
    offsets.append(offset)
    collection = bpy.data.collections.new(name.title() + " Mk1")
    bpy.context.scene.collection.children.link(collection)
    for obj in group:
        obj.location += offset
        for old_collection in list(obj.users_collection):
            old_collection.objects.unlink(obj)
        collection.objects.link(obj)

floor_mat = create_principled_material(bpy, "Studio floor", (.12, .15, .17), 0, .8)
box("Studio floor", (0, -.08, 0), (200, .1, 200), floor_mat, 0)
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
camera.rotation_euler = (Vector((0, 0, .40)) - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.type = "ORTHO"
camera.data.ortho_scale = 11.0
label_material = create_principled_material(bpy, "Preview lettering", (.74, .82, .85), 0, .8)
for (name, _), offset in zip(groups, offsets):
    bpy.ops.object.text_add(location=offset + forward * .90 + Vector((0, 0, .025)))
    label = bpy.context.object
    label.name = name + " preview label"
    label.rotation_euler = camera.rotation_euler
    label.data.body, label.data.align_x, label.data.size = name, "CENTER", .105
    label.data.materials.append(label_material)
scene = bpy.context.scene
scene.camera = camera
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 32
scene.cycles.use_denoising = True
scene.render.resolution_x, scene.render.resolution_y = 2600, 1500
scene.render.resolution_percentage = 100
scene.world.color = (.20, .20, .20)
scene.view_settings.view_transform = "AgX"
scene.render.image_settings.file_format = "PNG"
scene.render.filepath = str(SOURCE / "preview.png")
bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE / "mk1-machines.blend"))
bpy.ops.render.render(write_still=True)
