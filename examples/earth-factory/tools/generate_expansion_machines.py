"""Build the fourteen Mk1 expansion machines in the original factory style.

blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_expansion_machines.py
Add ``-- --no-render`` to export without rendering the studio sheet.

Game coordinates are X/Y(up)/Z, one tile wide, grounded at Y=0, output +X.
No production rules are introduced: these are meshes, reusable prefabs and an
art showroom. Lamps and furnace heat stay separate from the static GLBs.
"""
from pathlib import Path
import json
import math
import sys

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from blender_helpers import create_principled_material
from generate_scene import cube, glowing, transform, write_json

ASSETS = ROOT / "examples/earth-factory/scenes/assets"
OUT = ASSETS / "models"
SOURCE = ROOT / "assets/factory-machines"
for directory in (OUT, SOURCE):
    directory.mkdir(parents=True, exist_ok=True)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

# Identical linear colors, metalness and roughness to generate_machines.py.
paint = create_principled_material(bpy, "Graphite enamel", (.105, .125, .14), .55, .42)
steel = create_principled_material(bpy, "Machined steel", (.30, .34, .36), .72, .32)
dark = create_principled_material(bpy, "Recesses and belt", (.025, .032, .038), .15, .7)
inlet = create_principled_material(bpy, "Cyan inlet markings", (.24, .78, .84), .2, .45)
outlet = create_principled_material(bpy, "Gold outlet markings", (.95, .73, .25), .2, .45)
leaves = create_principled_material(bpy, "Muted hydroponic foliage", (.12, .25, .13), .0, .7)
glass = create_principled_material(bpy, "Smoky greenhouse glazing", (.24, .43, .46), .1, .25)
glass.node_tree.nodes.get("Principled BSDF").inputs["Alpha"].default_value = .22
glass.diffuse_color = (.24, .43, .46, .22)
glass.surface_render_method = "DITHERED"


def emission(name, color, strength):
    mat = create_principled_material(bpy, name, color, .0, .45)
    shader = mat.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Emission Color"].default_value = (*color, 1)
    shader.inputs["Emission Strength"].default_value = strength
    return mat


green = emission("Powered indicator (preview)", (.24, .9, .29), 1.3)
hot = emission("Furnace heat (preview)", (1., .23, .045), 3)
STATIC_MATERIALS = (paint, steel, dark, inlet, outlet, glass, leaves)
parts, effects, ports = [], [], []
catalog, groups = [], []
power_socket = None
lamp = None


def point(pos):
    x, y, z = pos
    return Vector((x, -z, y))


def finish(obj, name, material, bevel=0):
    obj.name = name
    obj.data.materials.append(material)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = obj.modifiers.new("Edge highlights", "BEVEL")
        mod.width, mod.segments = bevel, 1
        bpy.ops.object.modifier_apply(modifier=mod.name)
    parts.append(obj)
    return obj


def box(name, pos, size, material=paint, bevel=.006):
    bpy.ops.mesh.primitive_cube_add(size=1, location=point(pos))
    obj = bpy.context.object
    obj.dimensions = (size[0], size[2], size[1])
    return finish(obj, name, material, bevel)


def cylinder(name, pos, radius, depth, material=steel, axis="y", tip=None):
    bpy.ops.mesh.primitive_cone_add(vertices=12, radius1=radius if tip is None else tip,
                                  radius2=radius, depth=depth, location=point(pos))
    obj = bpy.context.object
    if axis == "x":
        obj.rotation_euler[1] = math.pi / 2
    elif axis == "z":
        obj.rotation_euler[0] = math.pi / 2
    return finish(obj, name, material)


def strut(name, start, end, width=.035, material=steel):
    a, b = point(start), point(end)
    bpy.ops.mesh.primitive_cube_add(size=1, location=(a + b) / 2)
    obj = bpy.context.object
    obj.dimensions = (width, width, (b - a).length)
    obj.rotation_euler = (b - a).to_track_quat("Z", "Y").to_euler()
    return finish(obj, name, material, .003)


def pipe(name, points, radius=.035, material=steel):
    # Straight, twelve-sided segments and solid faceted elbows; no baked textures.
    for a, b in zip(points, points[1:]):
        start, end = point(a), point(b)
        obj = cylinder(name + " tube", (0, 0, 0), radius, (end - start).length, material)
        obj.location = (start + end) / 2
        obj.rotation_euler = (end - start).to_track_quat("Z", "Y").to_euler()
    for pos in points[1:-1]:
        bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=6, radius=radius,
                                           location=point(pos))
        finish(bpy.context.object, name + " elbow", material)


def ring(name, pos, outer, inner, depth, material=steel, axis="y", count=12):
    verts, faces = [], []
    center = point(pos)
    for length, radius in [(-depth / 2, outer), (depth / 2, outer),
                           (-depth / 2, inner), (depth / 2, inner)]:
        for i in range(count):
            a = math.tau * i / count
            u, v = radius * math.cos(a), radius * math.sin(a)
            offset = (length, u, v) if axis == "x" else ((u, length, v) if axis == "z" else (u, v, length))
            verts.append(center + Vector(offset))
    for i in range(count):
        j = (i + 1) % count
        faces.extend([(i, j, count + j, count + i),
                      (2 * count + i, 3 * count + i, 3 * count + j, 2 * count + j),
                      (i, 2 * count + i, 2 * count + j, j),
                      (count + i, count + j, 3 * count + j, 3 * count + i)])
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, material)


def skids(title):
    for z in [-.33, .33]:
        box(title + " skid", (0, .045, z), (.90, .09, .16), steel)
    box(title + " lower chassis", (0, .145, 0), (.70, .12, .67))


def vent(name, x, y, z, width=.42, height=.25, side="x"):
    size = (.018, height, width) if side == "x" else (width, height, .018)
    box(name + " recessed grille", (x, y, z), size, dark, .003)
    for i in range(5):
        offset = (i - 2) * width / 5
        pos = (x + .014, y, z + offset) if side == "x" else (x + offset, y, z + .014)
        rib = (.024, height * .80, .022) if side == "x" else (.022, height * .80, .024)
        box(name + " cooling fin", pos, rib, steel, .002)


def terminal(pos):
    global power_socket
    x, y, z = pos
    cylinder("Centered cable socket", (x, y - .025, z), .07, .06, dark)
    cylinder("Cable terminal", (x, y + .03, z), .032, .05, steel)
    power_socket = [x, round(y + .03, 5), z]


def status(pos):
    global lamp
    x, y, z = pos
    box("Power indicator bezel", pos, (.035, .145, .08), dark, .003)
    lamp = {"position": [x + .022, y, z], "size": [.012, .105, .045]}


def arrow(name, pos, direction, material, size=.13):
    x, y, z = pos
    outline = [(-.5, -.2), (.05, -.2), (.05, -.45), (.5, 0),
               (.05, .45), (.05, .2), (-.5, .2)]
    vertices = []
    for height in [y, y + .008]:
        for a, b in outline:
            dx = size * (a * math.cos(direction) - b * math.sin(direction))
            dz = size * (a * math.sin(direction) + b * math.cos(direction))
            vertices.append((x + dx, -(z + dz), height))
    n = len(outline)
    faces = [tuple(reversed(range(n))), tuple(range(n, n * 2))]
    faces += [(i, (i + 1) % n, (i + 1) % n + n, i + n) for i in range(n)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, material)


def belt(name, x, z=0, direction=0, length=.30, width=.27, output=False):
    start = len(parts)
    box(name + " belt", (0, .305, 0), (length, .055, width), dark)
    for side in [-1, 1]:
        box(name + " rail", (0, .325, side * (width / 2 + .025)),
            (length, .07, .04), steel, .004)
    for i in range(5):
        box(name + " tread", ((i - 2) * length * .18, .34, 0),
            (.017, .014, width - .025), steel, .001)
    arrow(name + " flow", (0, .356, 0), 0, outlet if output else inlet)
    for obj in parts[start:]:
        px, py, height = obj.location
        obj.location = (x + px * math.cos(direction) + py * math.sin(direction),
                        -z - px * math.sin(direction) + py * math.cos(direction), height)
        obj.rotation_euler.z -= direction
    # Inlet direction points into the machine; output points away.
    distance = length / 2 if output else -length / 2
    ports.append({"kind": "solid", "role": "output" if output else "input",
                  "position": [round(x + distance * math.cos(direction), 4), .34,
                               round(z + distance * math.sin(direction), 4)],
                  "direction": [round(math.cos(direction)), 0, round(math.sin(direction))]})


def fluid_port(name, pos, axis="x", output=False):
    color = outlet if output else inlet
    cylinder(name + " flange", pos, .08, .035, steel, axis=axis)
    cylinder(name + " flow collar", pos, .064, .043, color, axis=axis)
    cylinder(name + " dark bore", pos, .043, .046, dark, axis=axis)
    ports.append({"kind": "fluid", "role": "output" if output else "input",
                  "position": list(pos), "axis": axis})


def vessel(name, pos, radius, height):
    x, y, z = pos
    cylinder(name + " shell", pos, radius, height, paint)
    for level in [y - height * .38, y + height * .38]:
        cylinder(name + " steel band", (x, level, z), radius + .014, .033)
    cylinder(name + " lower taper", (x, y - height / 2 - .045, z), radius, .09, steel, tip=radius * .65)
    cylinder(name + " lid", (x, y + height / 2 + .018, z), radius + .015, .038)


def hopper(name, center, bottom_y=.58, top_y=.98, top=.65, bottom=.30):
    x, _, z = center
    verts = []
    for y, width in [(bottom_y, bottom), (top_y, top)]:
        for a, b in [(-1, -1), (1, -1), (1, 1), (-1, 1)]:
            verts.append(point((x + a * width / 2, y, z + b * width / 2)))
    faces = [(i + 4, (i + 1) % 4 + 4, (i + 1) % 4, i) for i in range(4)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    finish(obj, name + " open funnel", paint)
    mod = obj.modifiers.new("Hopper wall thickness", "SOLIDIFY")
    mod.thickness = .025
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)
    for side in [-1, 1]:
        box(name + " lip", (x, top_y, z + side * top / 2), (top + .04, .04, .04), steel)
        box(name + " lip", (x + side * top / 2, top_y, z), (.04, .04, top + .04), steel)


def heat_strip(name, pos, size):
    effects.append({"name": name, "position": list(pos), "size": list(size),
                    "color": [1., .23, .045]})


def water_pump():
    skids("Pump")
    box("Pump motor cradle", (-.10, .28, 0), (.46, .19, .48))
    cylinder("Centrifugal impeller housing", (.13, .47, 0), .22, .18, paint, "x")
    cylinder("Impeller cover", (.23, .47, 0), .19, .035, steel, "x")
    cylinder("Impeller hub", (.257, .47, 0), .075, .04, dark, "x")
    cylinder("Electric drive", (-.17, .48, 0), .13, .35, paint, "x")
    for x in [-.30, -.23, -.16, -.09]:
        cylinder("Motor cooling rib", (x, .48, 0), .144, .022, steel, "x")
    pipe("Water outlet", [(.13, .63, 0), (.13, .76, 0), (.34, .76, 0), (.34, .34, 0), (.46, .34, 0)])
    pipe("Intake riser", [(-.12, .18, -.31), (-.12, .45, -.31), (.12, .45, -.31), (.12, .45, 0)])
    fluid_port("Water output", (.46, .34, 0), output=True)
    box("Pump control box", (-.12, .64, .28), (.27, .22, .12))
    status((.024, .65, .29))
    terminal((-.12, .77, .28))


def oil_extractor():
    skids("Oil extractor")
    box("Wellhead plinth", (.26, .20, 0), (.24, .20, .31))
    cylinder("Wellhead collar", (.27, .34, 0), .12, .09)
    for z in [-.24, .24]:
        strut("Pumpjack rear trestle", (-.30, .20, z), (-.08, .86, z), .07, paint)
        strut("Pumpjack front trestle", (.16, .20, z), (-.08, .86, z), .07, paint)
    cylinder("Walking beam pivot", (-.08, .86, 0), .095, .59, steel, "z")
    beam = box("Walking beam", (.02, .94, 0), (.76, .10, .15))
    beam.rotation_euler.y = -.10
    box("Horsehead", (.36, .91, 0), (.13, .27, .17), steel)
    strut("Polished lift rod", (.36, .85, 0), (.27, .36, 0), .028)
    cylinder("Crank gear", (-.26, .37, .18), .14, .08, paint, "z")
    cylinder("Crank cover", (-.26, .37, .229), .11, .025, steel, "z")
    strut("Connecting rod", (-.33, .40, .23), (-.29, .93, .23), .03)
    box("Counterweight", (-.33, .45, .24), (.15, .13, .08), steel)
    box("Drive cabinet", (-.23, .37, -.24), (.32, .30, .21))
    pipe("Crude output", [(.27, .26, 0), (.27, .34, 0), (.46, .34, 0)])
    fluid_port("Crude output", (.46, .34, 0), output=True)
    status((-.059, .40, -.25))
    terminal((-.23, .55, -.24))


def crusher():
    skids("Crusher")
    for z in [-.29, .29]:
        box("Crusher bearing wall", (-.06, .45, z), (.58, .45, .13))
        cylinder("Roller bearing cap", (-.06, .49, z * 1.23), .105, .04, steel, "z")
    for x in [-.19, .08]:
        cylinder("Crushing roller", (x, .54, 0), .13, .46, steel, "z")
        for z in [-.16, -.08, 0, .08, .16]:
            cylinder("Crusher toothed wheel", (x, .54, z), .155, .026, dark, "z")
    hopper("Crusher", (-.055, 0, 0), .62, .99, .66, .35)
    belt("Crusher rear input", -.34, length=.28)
    belt("Crusher output", .34, length=.28, output=True)
    vent("Crusher motor", .247, .56, .28, width=.13, height=.20)
    status((.295, .68, .29))
    terminal((0, 1.03, -.30))


def ore_washer():
    skids("Ore washer")
    for x in [-.25, .25]:
        box("Drum bearing pedestal", (x, .39, 0), (.11, .40, .54))
    cylinder("Octagonal wash drum", (0, .66, 0), .25, .56, paint, "x")
    for x in [-.26, -.11, .11, .26]:
        ring("Wash drum hoop", (x, .66, 0), .269, .244, .035, steel, "x")
    for x in [-.292, .292]:
        ring("Open drum collar", (x, .66, 0), .237, .145, .03, steel, "x")
    cylinder("Drum throat shadow", (0, .66, 0), .146, .57, dark, "x")
    box("Drain tray", (0, .25, 0), (.64, .045, .54), dark)
    pipe("Spray header", [(-.29, .35, -.29), (-.29, .98, -.29), (.27, .98, -.29)], .028)
    for x in [-.18, 0, .18]:
        cylinder("Spray nozzle", (x, .935, -.29), .035, .06, inlet)
    fluid_port("Wash water", (0, .34, .46), axis="z")
    pipe("Water supply", [(0, .34, .46), (0, .34, -.29), (-.29, .34, -.29)], .035)
    belt("Washer feed", -.34, length=.28)
    belt("Washer output", .34, length=.28, output=True)
    box("Wash controller", (.12, .46, .30), (.28, .30, .11))
    status((.276, .47, .30))
    terminal((.12, .64, .30))


def foundry():
    skids("Foundry")
    for z in [-.31, .31]:
        box("Crucible support", (-.05, .38, z), (.53, .38, .10))
    ring("Open alloy crucible", (-.05, .65, 0), .29, .22, .37, paint)
    ring("Crucible rolled rim", (-.05, .85, 0), .305, .215, .04)
    cylinder("Crucible bottom", (-.05, .465, 0), .24, .04, dark)
    box("Casting spout", (.255, .52, 0), (.18, .11, .18), steel)
    box("Casting spout shadow", (.26, .582, 0), (.16, .02, .09), dark)
    for x in [-.28, .28]:
        box("Foundry canopy post", (x, .64, -.33), (.07, .88, .075))
    box("Foundry extraction canopy", (0, 1.10, -.04), (.74, .075, .69), steel)
    cylinder("Extraction flue", (-.12, 1.23, -.20), .083, .20, paint)
    ring("Flue rim", (-.12, 1.345, -.20), .095, .058, .028)
    belt("First alloy feed", -.34, length=.28)
    belt("Second alloy feed", 0, .34, -math.pi / 2, length=.28)
    belt("Cast alloy output", .34, length=.28, output=True)
    status((.324, .72, .30))
    terminal((0, 1.15, 0))
    heat_strip("Molten alloy surface", (-.05, .79, 0), (.31, .02, .31))


def refinery():
    skids("Refinery")
    vessel("Distillation tower", (-.14, .77, -.13), .175, 1.08)
    for y in [.40, .66, .91, 1.16]:
        cylinder("Distillation tray band", (-.14, y, -.13), .193, .025)
    vessel("Separator vessel", (.20, .53, .19), .145, .49)
    box("Refinery pump cabinet", (-.22, .39, .23), (.30, .35, .22))
    pipe("Overhead transfer", [(-.14, 1.35, -.13), (-.14, 1.44, -.13),
                              (.20, 1.44, -.13), (.20, .84, -.13), (.20, .84, .19)], .027)
    pipe("Refinery input", [(-.46, .34, 0), (-.14, .34, 0), (-.14, .44, -.13)])
    pipe("Fuel output", [(.20, .53, .19), (.33, .53, 0), (.33, .34, 0), (.46, .34, 0)], .025)
    fluid_port("Fuel output", (.46, .34, 0), output=True)
    pipe("Heavy oil output", [(.20, .53, .19), (0, .53, .19), (0, .34, .19), (0, .34, .46)], .025)
    fluid_port("Heavy oil output", (0, .34, .46), axis="z", output=True)
    fluid_port("Crude inlet", (-.46, .34, 0))
    for y in [.48, .64, .80, .96, 1.12]:
        box("Tower ladder rung", (-.14, y, -.345), (.19, .018, .025), steel, .001)
    for x in [-.24, -.04]:
        box("Tower ladder rail", (x, .80, -.345), (.023, .79, .028), steel, .002)
    status((-.055, .44, .25))
    terminal((-.14, 1.37, -.13))


def chemical_plant():
    skids("Chemical plant")
    vessel("Main reaction vessel", (-.08, .62, -.09), .23, .65)
    cylinder("Agitator gearbox", (-.08, 1.05, -.09), .10, .14, paint)
    box("Agitator motor cap", (-.08, 1.135, -.09), (.24, .035, .23), steel)
    vessel("Reagent tank", (.25, .46, .24), .11, .35)
    box("Reaction control cabinet", (-.24, .47, .28), (.28, .41, .16))
    vent("Reaction cabinet", -.24, .47, .372, width=.18, side="z")
    pipe("Reagent transfer", [(.25, .67, .24), (.25, .82, .24), (-.08, .82, .24), (-.08, .82, -.09)], .028)
    pipe("Reaction input", [(-.46, .34, 0), (-.08, .34, 0), (-.08, .43, -.09)])
    fluid_port("Chemical input", (-.46, .34, 0))
    belt("Solid reagent feed", 0, .34, -math.pi / 2, length=.28, width=.23)
    belt("Chemical product output", .34, length=.28, width=.23, output=True)
    status((-.084, .58, .29))
    terminal((-.08, 1.18, -.09))


def electrolyzer():
    skids("Electrolyzer")
    for x in [-.26, .21]:
        box("Cell stack end plate", (x, .61, 0), (.075, .57, .46))
    for i in range(8):
        x = -.21 + i * .053
        box("Electrode plate", (x, .61, 0), (.025, .48, .38), steel)
        box("Electrode gasket", (x + .019, .61, 0), (.012, .43, .34), dark, .001)
    for y in [.43, .79]:
        for z in [-.20, .20]:
            pipe("Cell stack tie rod", [(-.30, y, z), (.26, y, z)], .018)
    for x, color in [(-.24, inlet), (.22, outlet)]:
        vessel("Gas reservoir", (x, .80, -.28), .092, .63)
        cylinder("Gas identification collar", (x, 1.05, -.28), .105, .038, color)
    box("Electrolyzer power cabinet", (0, .95, .02), (.38, .15, .30))
    fluid_port("Water inlet", (-.46, .34, 0))
    pipe("Water feed", [(-.46, .34, 0), (-.32, .34, 0), (-.32, .53, 0)])
    fluid_port("Hydrogen outlet", (.46, .34, 0), output=True)
    pipe("Hydrogen line", [(.26, .77, -.19), (.33, .77, -.19), (.33, .34, 0), (.46, .34, 0)], .022)
    fluid_port("Oxygen outlet", (0, .34, .46), axis="z", output=True)
    pipe("Oxygen line", [(.26, .77, .19), (0, .77, .19), (0, .34, .19), (0, .34, .46)], .022)
    status((.231, .95, .13))
    terminal((0, 1.055, .02))


def kiln():
    skids("Kiln")
    for x in [-.23, .23]:
        box("Kiln cradle", (x, .32, 0), (.15, .27, .64))
    ring("Octagonal insulated kiln", (0, .65, 0), .335, .225, .63, paint, "x", count=8)
    for x in [-.31, .31]:
        ring("Kiln steel end band", (x, .65, 0), .348, .225, .045, steel, "x", count=8)
    ring("Dark refractory tunnel", (0, .65, 0), .227, .205, .62, dark, "x", count=8)
    box("Kiln rear service spine", (0, .68, -.33), (.50, .43, .10))
    cylinder("Kiln chimney", (-.13, 1.08, -.20), .08, .32, paint)
    ring("Kiln chimney rim", (-.13, 1.255, -.20), .095, .053, .025)
    belt("Kiln input", -.34, length=.28, width=.30)
    belt("Kiln output", .34, length=.28, width=.30, output=True)
    status((.34, .67, .27))
    terminal((0, 1.02, 0))
    for x in [-.327, .327]:
        heat_strip("Hot kiln lintel", (x, .80, 0), (.018, .033, .25))


def glassworks():
    skids("Glassworks")
    box("Glass melting furnace", (-.13, .65, 0), (.50, .66, .61))
    box("Melting furnace roof", (-.13, 1.005, 0), (.58, .06, .69), steel)
    vent("Glass furnace", -.13, .70, .315, side="z", width=.32, height=.30)
    box("Drawing aperture shadow", (.132, .54, 0), (.025, .23, .37), dark)
    box("Drawing aperture surround", (.149, .68, 0), (.034, .045, .43), steel)
    for x in [.20, .27, .34, .41]:
        cylinder("Glass forming roller", (x, .37, 0), .047, .42, steel, "z")
    for z in [-.235, .235]:
        box("Roller deck rail", (.32, .34, z), (.32, .075, .04), steel)
    box("Glass sheet sample", (.31, .424, 0), (.28, .014, .31), inlet, .001)
    arrow("Glassworks output", (.35, .442, 0), 0, outlet, .105)
    belt("Sand feed", -.34, length=.28, width=.26)
    ports.append({"kind": "solid", "role": "output", "position": [.48, .34, 0], "direction": [1, 0, 0]})
    cylinder("Annealing flue", (-.22, 1.18, -.19), .073, .29, paint)
    ring("Annealing flue cap", (-.22, 1.34, -.19), .09, .05, .025)
    status((.143, .82, .275))
    terminal((-.13, 1.06, 0))
    heat_strip("Glass furnace glow", (.153, .625, 0), (.02, .033, .34))


def greenhouse():
    skids("Greenhouse")
    box("Hydroponic enclosure sill", (0, .25, 0), (.77, .14, .72))
    for x in [-.34, .34]:
        for z in [-.31, .31]:
            box("Greenhouse corner post", (x, .61, z), (.045, .64, .045), steel, .003)
        strut("Greenhouse pitched roof", (x, .93, -.32), (x, 1.11, 0), .035)
        strut("Greenhouse pitched roof", (x, 1.11, 0), (x, .93, .32), .035)
    box("Roof ridge beam", (0, 1.11, 0), (.75, .04, .04), steel, .003)
    for z in [-.31, .31]:
        box("Greenhouse side sill", (0, .91, z), (.73, .035, .035), steel, .003)
        box("Smoky side window", (0, .63, z), (.65, .53, .009), glass, 0)
        for x in [-.115, .115]:
            box("Window mullion", (x, .63, z), (.022, .57, .018), steel, .001)
        panel = box("Smoky roof glazing", (0, 1.02, z / 2), (.64, .009, .346), glass, 0)
        panel.rotation_euler.x = math.copysign(-.512, z)
    for z in [-.15, .15]:
        box("Hydroponic grow tray", (0, .38, z), (.56, .075, .20), dark)
        for x in [-.18, 0, .18]:
            cylinder("Plant growing plug", (x, .43, z), .044, .025, leaves)
            strut("Seedling stem", (x, .44, z), (x, .61, z), .012, leaves)
            for side in [-1, 1]:
                leaf = box("Seedling leaf", (x + side * .043, .55, z), (.10, .018, .055), leaves, .004)
                leaf.rotation_euler.y = side * .40
    pipe("Irrigation header", [(-.24, .37, -.25), (-.24, .76, -.25), (.26, .76, -.25)], .018)
    fluid_port("Irrigation inlet", (0, .34, .46), axis="z")
    pipe("Irrigation feed", [(0, .34, .46), (0, .34, -.25), (-.24, .37, -.25)], .025)
    belt("Seed and fertilizer feed", -.34, length=.28, width=.23)
    belt("Harvest output", .34, length=.28, width=.23, output=True)
    box("Greenhouse climate cabinet", (.21, .64, .35), (.23, .27, .09))
    status((.342, .67, .35))
    terminal((.21, .82, .35))


def electronics_fabricator():
    skids("Electronics fabricator")
    for z in [-.31, .31]:
        box("Clean enclosure side", (0, .64, z), (.65, .68, .105))
        box("Clean enclosure inset", (0, .68, z * 1.18), (.47, .39, .014), dark, .003)
        for x in [-.18, -.06, .06, .18]:
            box("Clean enclosure fin", (x, .68, z * 1.21), (.020, .32, .018), steel, .001)
    box("Filtered clean roof", (0, 1.0, 0), (.74, .075, .76), steel)
    cylinder("Roof filter casing", (-.09, 1.063, 0), .17, .052, paint)
    for z in [-.10, -.05, 0, .05, .10]:
        box("Filter grille", (-.09, 1.095, z), (.22, .016, .017), steel, .001)
    box("Precision tool gantry", (0, .87, 0), (.46, .065, .43))
    for z in [-.11, .11]:
        cylinder("Fine placement motor", (0, .78, z), .057, .12, steel)
        cylinder("Fine placement nozzle", (0, .68, z), .019, .085, dark)
    box("Circuit carrier", (0, .395, 0), (.35, .024, .29), inlet, .001)
    for z in [-.075, .075]:
        box("Circuit chip", (0, .422, z), (.12, .025, .08), dark, .001)
    belt("Electronic component input", -.34, length=.28)
    belt("Circuit output", .34, length=.28, output=True)
    box("Inspection lintel", (.338, .88, 0), (.034, .08, .55), paint)
    box("Inspection window trim", (.358, .82, 0), (.018, .026, .47), inlet, .002)
    status((.375, .71, .285))
    terminal((.17, 1.07, 0))


def manufacturer():
    skids("Manufacturer")
    for x in [-.30, .30]:
        for z in [-.31, .31]:
            box("Heavy cell upright", (x, .61, z), (.085, .85, .085))
    box("Heavy gantry roof", (0, 1.07, 0), (.78, .10, .79))
    box("Twin axis motor housing", (0, 1.14, 0), (.44, .045, .43), steel)
    box("Manufacturer tooling bed", (0, .40, 0), (.57, .075, .55), dark)
    cylinder("Rotary assembly fixture", (0, .46, 0), .16, .05)
    for x, z, sign in [(-.18, -.19, 1), (.18, .19, -1)]:
        cylinder("Robot shoulder", (x, .89, z), .082, .14, steel, "z")
        strut("Robot upper arm", (x, .89, z), (x + sign * .12, .73, z), .065, paint)
        cylinder("Robot elbow", (x + sign * .12, .73, z), .065, .11, steel, "z")
        strut("Robot forearm", (x + sign * .12, .73, z), (sign * .06, .57, z * .38), .045)
        for finger in [-1, 1]:
            box("Tool gripper finger", (sign * .06, .535, z * .38 + finger * .04), (.024, .075, .024), dark, .001)
    belt("Main component feed", -.34, length=.28, width=.25)
    belt("Side component feed", 0, .34, -math.pi / 2, length=.28, width=.25)
    belt("Rear component feed", 0, -.34, math.pi / 2, length=.28, width=.25)
    belt("Finished equipment output", .34, length=.28, width=.25, output=True)
    box("Manufacturer service cabinet", (-.19, .65, -.31), (.26, .39, .13))
    status((.369, .80, .31))
    terminal((0, 1.195, 0))


def recycler():
    skids("Recycler")
    for z in [-.29, .29]:
        box("Shredder armored cheek", (-.03, .45, z), (.63, .44, .115))
    for x in [-.17, .10]:
        cylinder("Shredder shaft", (x, .62, 0), .067, .50, dark, "z")
        for z in [-.16, -.08, 0, .08, .16]:
            cylinder("Shredder cutter", (x, .62, z), .14, .040, steel, "z")
    for z in [-.28, .28]:
        box("Shredder hopper rim", (-.03, .79, z), (.70, .045, .04), steel)
    for x in [-.36, .30]:
        box("Hopper end guard", (x, .73, 0), (.045, .16, .59))
    box("Magnetic separator cover", (.18, .44, 0), (.28, .14, .51))
    cylinder("Separation roller", (.26, .39, 0), .061, .41, steel, "z")
    belt("Scrap feed", -.34, length=.28)
    belt("Recovered material output", .34, length=.28, output=True)
    box("Recycler drive box", (-.10, .46, .37), (.36, .33, .11))
    vent("Recycler drive", -.10, .46, .431, side="z", width=.27, height=.21)
    # Three arrows form a small raised recycling glyph on the drive cover.
    for i in range(3):
        a = i * math.tau / 3
        arrow("Recycling glyph", (-.10 + .075 * math.cos(a), .637, .37 + .04 * math.sin(a)),
              a + math.pi / 2, inlet, .065)
    status((.097, .52, .37))
    terminal((-.10, .665, .37))


MACHINES = [
    ("water-pump", "Water pump", "Centrifugal impeller, finned motor and raised outlet pipe", water_pump),
    ("oil-extractor", "Oil extractor", "Walking beam, horsehead and counterweighted pumpjack", oil_extractor),
    ("crusher", "Crusher", "Tall open hopper feeding twin crushing rollers", crusher),
    ("ore-washer", "Ore washer", "Banded wash drum, spray header and drain tray", ore_washer),
    ("foundry", "Foundry", "Open alloy crucible, dual feeds and extraction canopy", foundry),
    ("refinery", "Refinery", "Tall distillation tower, tray bands and separator vessel", refinery),
    ("chemical-plant", "Chemical plant", "Agitated reaction vessel and separate reagent tank", chemical_plant),
    ("electrolyzer", "Electrolyzer", "Exposed electrode stack and twin gas reservoirs", electrolyzer),
    ("kiln", "Kiln", "Octagonal refractory tunnel with a short chimney", kiln),
    ("glassworks", "Glassworks", "Enclosed melting furnace and exposed forming rollers", glassworks),
    ("greenhouse", "Greenhouse", "Pitched glazed roof, growing trays and climate controls", greenhouse),
    ("electronics-fabricator", "Electronics fabricator", "Filtered enclosure and fine circuit placement heads", electronics_fabricator),
    ("manufacturer", "Manufacturer", "Heavy gantry, twin robot arms and three component inputs", manufacturer),
    ("recycler", "Recycler", "Low open shredder, paired cutting shafts and separation roller", recycler),
]


def export_machine(slug, title, description):
    global power_socket, lamp
    merged = []
    triangles = 0
    bounds = [Vector((math.inf,) * 3), Vector((-math.inf,) * 3)]
    for material in STATIC_MATERIALS:
        originals = [o for o in parts if o.data.materials[0] == material]
        copies = []
        for original in originals:
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
        if len(copies) > 1:
            bpy.ops.object.join()
        obj = copies[0]
        obj.name = slug + " / " + material.name
        # All surface nodes share the grounded machine pivot, simplifying imports.
        bpy.context.scene.cursor.location = (0, 0, 0)
        bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
        obj.data.calc_loop_triangles()
        triangles += len(obj.data.loop_triangles)
        for corner in obj.bound_box:
            p = obj.matrix_world @ Vector(corner)
            p = Vector((p.x, p.z, -p.y))
            for axis in range(3):
                bounds[0][axis] = min(bounds[0][axis], p[axis])
                bounds[1][axis] = max(bounds[1][axis], p[axis])
        merged.append(obj)
    assert triangles <= 4000, (slug, triangles)
    assert min(bounds[0].x, bounds[0].z) >= -.501, (slug, "outside tile", bounds)
    assert max(bounds[1].x, bounds[1].z) <= .501, (slug, "outside tile", bounds)
    assert abs(bounds[0].y) < .001, (slug, "floating base", bounds)
    assert power_socket is not None and lamp is not None, slug
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT / f"{slug}-mk1.glb"), export_format="GLB",
                              use_selection=True, export_yup=True, export_apply=True,
                              export_animations=False, export_cameras=False, export_lights=False)
    for obj in merged:
        bpy.data.objects.remove(obj, do_unlink=True)
    entry = {"id": slug, "name": title, "design": description, "requires_power": True,
             "mesh": f"models/{slug}-mk1.glb", "prefab": f"machine-{slug}.prefab.json",
             "triangles": triangles, "surfaces": len(merged),
             "bounds": [[round(v, 5) for v in bound] for bound in bounds],
             "power_socket": power_socket, "indicator": lamp, "ports": list(ports),
             "heat": list(effects)}
    catalog.append(entry)
    write_prefabs(entry)
    print(f"machine_export name={slug}-mk1 triangles={triangles} materials={len(merged)}", flush=True)
    box(title + " powered lens (preview)", lamp["position"], lamp["size"], green, .002)
    for effect in effects:
        box(effect["name"] + " (preview)", effect["position"], effect["size"], hot, .002)
    groups.append((title, list(parts)))
    parts.clear()
    effects.clear()
    ports.clear()
    power_socket, lamp = None, None


def write_prefabs(entry):
    slug, title = entry["id"], entry["name"]
    mesh = slug + "-mk1"
    write_json(ASSETS / entry["prefab"], {
        "version": 1, "name": title + " Mk1", "root": "root",
        "assets": {mesh: {"kind": "mesh", "path": entry["mesh"]}},
        "objects": [{"id": "root", "name": "machine-" + slug, "transform": transform(),
                     "drawable": {"layer": "3d", "mesh": {"asset": mesh},
                                  "texture": "white", "color": [1, 1, 1], "uv_scale": [1, 1]}}],
    })
    for state, color in [("on", [.24, .90, .29]), ("off", [.80, .045, .025])]:
        lens = entry["indicator"]
        obj = glowing(cube("lens", "Power " + state, lens["position"], lens["size"], color, "root"), color)
        write_json(ASSETS / f"{slug}-power-{state}.prefab.json", {
            "version": 1, "name": title + " power " + state, "root": "root",
            "objects": [{"id": "root", "name": title + " indicator", "transform": transform()}, obj],
        })
    if entry["heat"]:
        objects = [{"id": "root", "name": title + " heat", "transform": transform()}]
        for i, effect in enumerate(entry["heat"]):
            objects.append(glowing(cube("heat-" + str(i), effect["name"], effect["position"],
                                        effect["size"], effect["color"], "root"), effect["color"]))
        write_json(ASSETS / f"{slug}-heat.prefab.json", {
            "version": 1, "name": title + " heat", "root": "root", "objects": objects,
        })


def showroom():
    objects = [{"id": "camera", "name": "Expansion showroom camera",
                "transform": {**transform(13, 13, 13), "rotation_degrees": [-35.264, 45, 0]},
                "camera": {"projection": "orthographic", "vertical_size": 12, "near": .1, "far": 100}}]
    assets = {}
    objects.append(cube("floor", "Studio floor", (0, -.06, 0), (20, .12, 18), [.18, .21, .23]))
    for i, entry in enumerate(catalog):
        slug = entry["id"]
        x, z = (i % 5 - 2) * 2, (i // 5 - 1) * 3
        mesh = slug + "-mk1"
        assets[mesh] = {"kind": "mesh", "path": "assets/" + entry["mesh"]}
        objects.append({"id": slug, "name": entry["name"] + " Mk1", "transform": transform(x, 0, z),
                        "drawable": {"layer": "3d", "mesh": {"asset": mesh}, "texture": "white",
                                     "color": [1, 1, 1], "uv_scale": [1, 1]}})
        lens = entry["indicator"]
        objects.append(glowing(cube(slug + "-lens", entry["name"] + " preview lamp", lens["position"],
                                   lens["size"], [.24, .9, .29], slug), [.24, .9, .29]))
        for j, effect in enumerate(entry["heat"]):
            objects.append(glowing(cube(slug + "-heat-" + str(j), effect["name"], effect["position"],
                                       effect["size"], effect["color"], slug), effect["color"]))
    write_json(ASSETS.parent / "machine-expansion-showroom.json", {
        "version": 1, "name": "Stellar-IX — Mk1 expansion machine art showroom",
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
        row, col = divmod(i, 5)
        # Center the last four machines instead of leaving a conspicuous empty slot.
        column = col - 2 + (.5 if row == 2 else 0)
        offset = right * (column * 1.95) + forward * ((row - 1) * 3.75)
        offsets.append(offset)
        collection = bpy.data.collections.new(title + " Mk1")
        bpy.context.scene.collection.children.link(collection)
        for obj in group:
            obj.location += offset
            for old_collection in list(obj.users_collection):
                old_collection.objects.unlink(obj)
            collection.objects.link(obj)
    floor = create_principled_material(bpy, "Studio floor", (.12, .15, .17), 0, .8)
    box("Studio floor", (0, -.08, 0), (200, .1, 200), floor, 0)
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
    camera.rotation_euler = (Vector((0, 0, .45)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type, camera.data.ortho_scale = "ORTHO", 10.65
    lettering = create_principled_material(bpy, "Preview lettering", (.74, .82, .85), 0, .8)
    for (title, _), offset in zip(groups, offsets):
        bpy.ops.object.text_add(location=offset + forward * .88 + Vector((0, 0, .025)))
        label = bpy.context.object
        label.name = title + " preview label"
        label.rotation_euler = camera.rotation_euler
        label.data.body = title.upper()
        label.data.align_x, label.data.size, label.data.space_line = "CENTER", .108, 1.25
        label.data.materials.append(lettering)
    scene = bpy.context.scene
    scene.camera = camera
    scene.render.engine = "CYCLES"
    scene.cycles.device, scene.cycles.samples = "CPU", 40
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = 3000, 2200
    scene.render.resolution_percentage = 100
    scene.world.color = (.20, .20, .20)
    scene.view_settings.view_transform = "AgX"
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(SOURCE / "expansion-preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE / "mk1-expansion-machines.blend"))
    if "--no-render" not in sys.argv:
        bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    for slug, title, description, build in MACHINES:
        build()
        export_machine(slug, title, description)
    write_json(SOURCE / "expansion-manifest.json", {
        "version": 1, "units": "tile", "up_axis": "Y", "output_axis": "+X",
        "belt_height": .34, "gameplay_status": "implemented: powered production, recipes, buffers, solid/fluid transport, saves and co-op",
        "machines": catalog,
    })
    showroom()
    studio()
