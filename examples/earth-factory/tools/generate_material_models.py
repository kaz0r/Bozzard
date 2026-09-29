"""Model the complete Stellar-IX item catalog as small, reusable GLB assets.

blender --background -noaudio --threads 8 --python examples/earth-factory/tools/generate_material_models.py
Add -- --no-render to skip the labeled studio sheet. Coordinates are tile-sized,
Y-up in game and centered vertically for the existing conveyor animation pivot.
"""
from pathlib import Path
import json
import math
import random
import sys

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from blender_helpers import create_principled_material
from generate_scene import transform, write_json

ASSETS = ROOT / "examples/earth-factory/scenes/assets"
OUT = ASSETS / "models/materials"
SOURCE = ROOT / "assets/factory-materials"
OUT.mkdir(parents=True, exist_ok=True)
SOURCE.mkdir(parents=True, exist_ok=True)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def mat(name, color, metallic=0, roughness=.55):
    return create_principled_material(bpy, name, color, metallic, roughness)


paint = mat("Graphite enamel", (.105, .125, .14), .55, .42)
steel = mat("Machined steel", (.30, .34, .36), .72, .32)
dark = mat("Rubber and recesses", (.025, .032, .038), .05, .72)
iron = mat("Iron", (.40, .52, .57), .70, .38)
copper = mat("Copper", (.68, .29, .095), .72, .32)
gold = mat("Gold markings and contacts", (.95, .73, .25), .55, .38)
cyan = mat("Cyan markings", (.24, .78, .84), .2, .45)
stone = mat("Gray stone", (.30, .32, .29), 0, .85)
chalk = mat("Limestone and lime", (.73, .73, .61), 0, .85)
sand = mat("Sand and paper", (.66, .51, .29), 0, .85)
clay = mat("Clay and fired brick", (.55, .20, .10), 0, .8)
silver = mat("Silver", (.70, .74, .76), .8, .25)
plastic = mat("Ivory polymer", (.74, .77, .70), 0, .52)
green = mat("Industrial green", (.12, .31, .19), .08, .68)
leaves = mat("Biomass foliage", (.25, .43, .13), 0, .8)
blue = mat("Oxygen blue", (.09, .28, .52), .45, .4)
red = mat("Fuel red", (.52, .075, .055), .35, .42)
alloy = mat("Conductive alloy", (.28, .57, .39), .7, .32)
quartz = mat("Quartz faces", (.66, .79, .79), .08, .30)
glass = mat("Glass and lenses", (.22, .52, .57), .15, .19)
silicon = mat("Silicon wafer", (.14, .23, .37), .70, .24)
amortium = mat("Amorium", (.64, .48, .27), .32, .65)
moon = mat("Moondust", (.63, .66, .68), 0, .85)
tech = mat("Techtorium", (.95, .30, .04), .4, .4)
parts, groups, catalog = [], [], []


def point(p):
    return Vector((p[0], -p[2], p[1]))


def pose(position=(0,0,0), scale=(1,1,1), rotation=(0,0,0)):
    return {"translation":list(position),"rotation_degrees":list(rotation),"scale":list(scale)}


def finish(obj, name, material, bevel=0):
    obj.name = name
    obj.data.materials.append(material)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = obj.modifiers.new("Small edge highlights", "BEVEL")
        mod.width, mod.segments = bevel, 1
        bpy.ops.object.modifier_apply(modifier=mod.name)
    parts.append(obj)
    return obj


def box(name, p, size, material=steel, bevel=.002):
    bpy.ops.mesh.primitive_cube_add(size=1, location=point(p))
    obj = bpy.context.object
    obj.dimensions = (size[0], size[2], size[1])
    return finish(obj, name, material, bevel)


def cylinder(name, p, radius, depth, material=steel, axis="y", vertices=12, tip=None):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius if tip is None else tip,
                                  radius2=radius, depth=depth, location=point(p))
    obj = bpy.context.object
    if axis == "x": obj.rotation_euler.y = math.pi / 2
    elif axis == "z": obj.rotation_euler.x = math.pi / 2
    return finish(obj, name, material)


def ring(name, p, outer, inner, depth, material=steel, axis="y", count=12):
    vertices, faces = [], []
    for height, radius in [(-depth / 2, outer), (-depth / 2, inner),
                           (depth / 2, outer), (depth / 2, inner)]:
        for i in range(count):
            a = math.tau * i / count
            x, y, z = radius * math.cos(a), height, radius * math.sin(a)
            if axis == "x": x, y = y, x
            elif axis == "z": y, z = z, y
            vertices.append(tuple(point((p[0]+x, p[1]+y, p[2]+z))))
    for i in range(count):
        j = (i + 1) % count
        faces.extend([(i, j, count+j, count+i), (2*count+i, 3*count+i, 3*count+j, 2*count+j),
                      (i, 2*count+i, 2*count+j, j), (count+i, count+j, 3*count+j, 3*count+i)])
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish(obj, name, material)


def rock(name, p, size, material, seed=0):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=1, location=point(p))
    obj = bpy.context.object
    rng = random.Random(seed)
    for v in obj.data.vertices: v.co *= rng.uniform(.86, 1.12)
    obj.dimensions = (size[0], size[2], size[1])
    obj.rotation_euler.z = rng.uniform(-.4, .4)
    return finish(obj, name, material)


def nuggets(material, count=5, clean=False):
    for i in range(count):
        a = i * 2.39996
        radius = .065 if count > 3 else .055
        rock("Clean mineral shard" if clean else "Ore fragment",
             (radius*math.cos(a), .047+(i%2)*.04, radius*math.sin(a)),
             (.10 if clean else .12, .09 if clean else .10, .085), material, i+7)


def raw_ore(material, seed, lunar=False):
    rock("Faceted host stone", (0,.08,0), (.29,.18,.24), stone if not lunar else moon, seed)
    for i, p in enumerate([(-.07,.15,-.02),(.06,.15,.035),(.005,.12,-.095)]):
        rock("Exposed mineral vein", p, (.085,.065,.065), material, seed+i)


def tray(material, name="Granular material"):
    box("Low transport tray", (0,.018,0), (.28,.035,.24), paint)
    for i in range(7):
        a = i * 2.39996
        rock(name, (.07*math.cos(a),.053,.065*math.sin(a)), (.11,.07,.09), material, i)


def ingots(material, stripe=None):
    for i in range(2):
        # Tapered rectangular cast bars, broader below and narrow at the top.
        vertices = [(x,-z,y) for y,s in [(i*.065,1),(i*.065+.06,.82)]
                    for x,z in [(-.15*s,-.075*s),(.15*s,-.075*s),(.15*s,.075*s),(-.15*s,.075*s)]]
        mesh = bpy.data.meshes.new("Cast ingot")
        mesh.from_pydata(vertices,[],[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)])
        mesh.update()
        obj=bpy.data.objects.new("Tapered cast ingot",mesh);bpy.context.collection.objects.link(obj)
        finish(obj,"Tapered cast ingot",material,.003)
    box("Raised foundry stamp",(.075,.129,0),(.035,.006,.04),stripe or material,0)
    if stripe: box("Alloy identity band",(-.07,.129,0),(.025,.006,.12),stripe,0)


def fluid_container(kind):
    if kind in [6,34]:
        cylinder("Sealed oil drum",(0,.11,0),.105,.22,paint)
        for y in [.025,.105,.195]: ring("Steel drum hoop",(0,y,0),.11,.101,.016,steel)
        cylinder("Oil bung",(.038,.228,0),.023,.016,gold,vertices=8)
        box("Oil grade band",(0,.115,.104),(.105,.045,.005),gold if kind==6 else sand,0)
    elif kind==7:
        cylinder("Water sample bottle",(0,.105,0),.077,.16,glass,vertices=8)
        cylinder("Water shoulder",(0,.195,0),.043,.035,glass,vertices=8,tip=.077)
        cylinder("Sealed cap",(0,.225,0),.047,.025,plastic,vertices=8)
        ring("Water blue band",(0,.10,0),.079,.073,.032,cyan, count=8)
    elif kind==33:
        box("Fuel can",(0,.105,0),(.19,.20,.13),red,.008)
        box("Handle left",(-.045,.231,0),(.025,.06,.055),paint)
        box("Handle right",(.045,.231,0),(.025,.06,.055),paint)
        box("Handle bridge",(0,.26,0),(.10,.022,.055),paint)
        cylinder("Fuel cap",(.057,.215,.033),.027,.024,gold,vertices=8)
        box("Fuel stripe",(0,.11,.068),(.13,.027,.006),gold,0)
    else:
        cylinder("Pressure cylinder",(0,.125,0),.072,.22,plastic if kind==38 else blue)
        cylinder("Cylinder shoulder",(0,.249,0),.028,.03,steel,tip=.065)
        cylinder("Brass valve",(0,.275,0),.022,.028,gold,vertices=8)
        box("Valve wheel",(0,.295,0),(.072,.014,.02),paint,0)
        ring("Gas identification band",(0,.16,0),.074,.066,.028,cyan if kind==38 else plastic)


def cable():
    for y in [.027,.064,.101]: ring("Coiled insulated cable",(0,y,0),.13,.08,.035,dark)
    box("Cable free end",(.08,.017,.097),(.16,.022,.026),dark)
    cylinder("Exposed copper end",(.158,.017,.097),.011,.035,copper,"x",vertices=8)
    box("Coil retaining band",(0,.12,0),(.035,.025,.27),gold)


def nuts_bolts():
    ring("Hexagonal nut",(-.065,.034,-.035),.069,.03,.06,steel,count=6)
    cylinder("Bolt shaft",(.042,.08,.035),.029,.16,steel,"x",vertices=8)
    cylinder("Hex bolt head",(-.04,.08,.035),.055,.032,iron,"x",vertices=6)
    for x in [.032,.063,.094]: ring("Thread ridge",(x,.08,.035),.032,.027,.009,silver,"x",8)


def machine_part():
    ring("Gear body",(0,.06,0),.10,.04,.055,steel)
    for i in range(10):
        a=i*math.tau/10
        tooth=box("Gear tooth",(.113*math.cos(a),.06,.113*math.sin(a)),(.036,.05,.035),iron,0)
        tooth.rotation_euler.z=-a
    cylinder("Bearing hub",(0,.093,0),.05,.045,paint)
    cylinder("Drive shaft",(0,.132,0),.022,.055,steel,vertices=8)


def portable(kind):
    if kind==23:
        for x in [-.04,.04]:
            cylinder("Bundled pole",(x,.13,0),.016,.26,steel,vertices=8)
            box("Pole cross arm",(x,.21,0),(.035,.025,.19),paint)
            for z in [-.065,.065]: cylinder("Small insulator",(x,.238,z),.017,.035,plastic,vertices=8)
        box("Bundle strap",(0,.105,0),(.13,.033,.05),gold)
    else:
        box("Equipment base",(0,.022,0),(.25,.045,.22),steel)
        box("Equipment casing",(0,.155,0),(.21,.18,.18),paint)
        for x in [-.06,0,.06]: box("Vent blade",(x,.173,.092),(.018,.10,.006),steel,0)
        if kind==21:
            cylinder("Portable drill",(0,.071,0),.035,.085,steel,tip=.008)
            cylinder("Motor cap",(0,.259,0),.035,.026,steel)
        else:
            for x in [-.065,.065]: cylinder("Twin generator exhaust",(x,.28,0),.018,.072,steel,vertices=8)
            cylinder("Alternator",(.12,.125,0),.042,.045,steel,"x",vertices=8)
        box("Equipment ID plate",(.07,.157,.096),(.025,.05,.008),cyan,0)


def bricks():
    for y,z in [(.035,0),(.108,-.014)]:
        box("Fired clay brick",(0,y,z),(.26,.065,.145),clay,.004)
        for x in [-.055,.055]: box("Brick recess",(x,y+.034,z),(.05,.004,.06),dark,0)


def sack(material, marking):
    box("Folded material sack",(0,.065,0),(.22,.13,.18),material,.014)
    box("Crimped top seam",(0,.14,0),(.15,.018,.035),material)
    box("Identity strip",(0,.065,.094),(.045,.095,.005),marking,0)


def circuits():
    box("Fiberglass circuit board",(0,.018,0),(.28,.032,.22),green)
    box("Central integrated circuit",(0,.05,0),(.09,.04,.07),dark)
    for x in [-.064,.064]:
        for z in [-.022,0,.022]: box("Chip pin",(x,.037,z),(.033,.009,.01),silver,0)
    for x,z in [(-.095,-.07),(.095,.06),(.08,-.07)]:
        box("Discrete component",(x,.04,z),(.047,.025,.031),plastic,0)
    for x,z,sx,sz in [(-.095,0,.008,.13),(.094,-.014,.008,.075),(-.035,-.083,.13,.006),(.03,.084,.16,.006)]:
        box("Gold PCB trace",(x,.036,z),(sx,.006,sz),gold,0)
    for x in [-.075,-.035,.005,.045,.085]: box("Edge contact",(x,.036,.106),(.018,.006,.018),gold,0)


def motor():
    box("Motor feet",(0,.018,0),(.22,.036,.18),paint)
    cylinder("Motor casing",(0,.11,0),.082,.20,steel,"x")
    for x in [-.075,-.037,0,.037,.075]: ring("Cooling rib",(x,.11,0),.088,.075,.009,paint,"x")
    cylinder("Drive shaft",(.135,.11,0),.023,.07,silver,"x",vertices=8)
    box("Terminal box",(0,.209,0),(.085,.045,.07),paint)
    box("Motor label",(0,.233,0),(.054,.003,.032),gold,0)


def build(kind):
    if kind in [1,2,10,24]: raw_ore({1:iron,2:copper,10:silver,24:amortium}[kind],kind,kind==24)
    elif kind==3:
        rock("Layered limestone",(0,.08,0),(.28,.18,.24),chalk,3)
        for y in [.05,.09,.13]: box("Limestone stratum",(0,y,.10),(.19,.01,.012),sand,0)
    elif kind==4: nuggets(dark,3)
    elif kind in [5,26]:
        if kind==26: rock("Lunar host rock",(0,.035,0),(.25,.07,.23),dark,26)
        for x,z,h in [(-.055,0,.19),(.045,.026,.24),(.035,-.06,.14)]:
            cylinder("Hexagonal crystal",(x,h/2,z),.039,h,quartz if kind==5 else tech,vertices=6)
            cylinder("Crystal termination",(x,h+.025,z),0,.05,quartz if kind==5 else tech,vertices=6,tip=.039)
    elif kind in [6,7,33,34,38,39]: fluid_container(kind)
    elif kind==8: rock("Rough stone",(0,.08,0),(.28,.19,.24),stone,8)
    elif kind in [9,25,42]: tray({9:sand,25:moon,42:chalk}[kind],"Fine mineral heap")
    elif kind in [11,12,13]: ingots({11:iron,12:copper,13:alloy}[kind],copper if kind==13 else None)
    elif kind==14:
        for y,x in [(.017,-.025),(.044,0),(.071,.025)]:
            box("Glass pane",(x,y,0),(.25,.022,.19),glass)
            box("Polished glass edge",(x,y+.012,-.09),(.24,.005,.008),quartz,0)
    elif kind==15:
        for y,x in [(.016,-.022),(.049,0),(.082,.022)]: box("Iron sheet",(x,y,0),(.29,.027,.22),iron)
    elif kind==16: nuts_bolts()
    elif kind==17: cable()
    elif kind==18:
        box("Concrete block",(0,.062,0),(.28,.124,.20),stone,.008)
        for x in [-.07,.07]: box("Block core recess",(x,.126,0),(.046,.004,.075),dark,0)
        for i in range(3): rock("Visible aggregate",(-.08+i*.07,.04,.104),(.025,.032,.009),chalk,i)
    elif kind==20: machine_part()
    elif kind in [21,22,23]: portable(kind)
    elif kind in [27,28,29,30,31]: nuggets({27:stone,28:iron,29:copper,30:silver,31:copper}[kind],4 if kind in [30,31] else 6,kind in [30,31])
    elif kind==32:
        for y in [.015,.10]: box("I beam flange",(0,y,0),(.30,.03,.14),steel)
        box("I beam web",(0,.057,0),(.30,.06,.034),steel)
    elif kind==35:
        for y in [.031,.094]: box("Molded polymer billet",(0,y,0),(.25,.059,.16),plastic,.005)
        box("Polymer band",(0,.126,0),(.026,.009,.16),cyan,0)
    elif kind==36:
        for y in [.035,.104]: ring("Rubber ring",(0,y,0),.13,.072,.065,dark)
        for i in range(8):
            a=i*math.tau/8
            b=box("Raised tire tread",(.125*math.cos(a),.104,.125*math.sin(a)),(.017,.045,.024),paint,0)
            b.rotation_euler.z=-a
    elif kind==37: sack(sand,green)
    elif kind==40:
        for i in range(3): rock("Pressed wet clay",(-.055+i*.055,.05+(i%2)*.065,0),(.14,.11,.16),clay,40+i)
    elif kind==41: bricks()
    elif kind==43:
        cylinder("Semiconductor wafer",(0,.016,0),.14,.026,silicon,vertices=16)
        for x in [-.075,0,.075]: box("Wafer die line",(x,.031,0),(.003,.003,.22),quartz,0)
        for z in [-.075,0,.075]: box("Wafer die line",(0,.031,z),(.22,.003,.003),quartz,0)
    elif kind==44:
        for x,z,y in [(-.035,-.02,.03),(.055,.044,.084)]:
            ring("Lens rim",(x,y,z),.091,.077,.027,steel)
            cylinder("Optical lens",(x,y+.005,z),.077,.025,glass,vertices=16,tip=.064)
    elif kind==45:
        sack(sand,dark)
        for i in range(4): rock("Visible seed",(-.07+i*.045,.03,.11),(.035,.019,.016),amortium,i)
    elif kind==46:
        box("Compressed biomass bale",(0,.073,0),(.26,.145,.20),green,.009)
        for x in [-.085,.085]: box("Bale strap",(x,.15,0),(.018,.017,.21),sand,0)
        for i in range(5):
            leaf=box("Folded leaf",(-.095+i*.047,.166,0),(.037,.015,.15),leaves,0)
            leaf.rotation_euler.z=(i%2-.5)*.24
    elif kind==47: circuits()
    elif kind==48: motor()
    elif kind==49:
        box("Machinery transport skid",(0,.019,0),(.31,.037,.26),steel)
        box("Finished machinery body",(-.025,.105,0),(.21,.15,.20),paint,.005)
        cylinder("Machinery actuator",(.104,.105,0),.056,.086,steel,"x")
        cylinder("Copper drive coupling",(.15,.105,0),.027,.037,copper,"x",vertices=8)
        box("Control panel",(-.04,.19,.01),(.12,.025,.085),steel)
        box("Display window",(-.055,.204,.01),(.064,.004,.054),cyan,0)
        for z in [-.08,-.026,.028,.082]: box("Case vent",(-.134,.117,z),(.007,.06,.016),steel,0)
    elif kind==50:
        for i,(x,z,a) in enumerate([(-.065,0,.38),(.025,.045,-.3),(.075,-.05,.6)]):
            b=box("Bent scrap plate",(x,.025+i*.03,z),(.12,.028,.085),steel if i<2 else copper,0)
            b.rotation_euler.y=a
        ring("Discarded bearing",(-.032,.12,-.025),.062,.032,.024,paint)


ITEMS = [(1,"iron-ore","Iron ore"),(2,"copper-ore","Copper ore"),(3,"limestone","Limestone"),
    (4,"coal","Coal"),(5,"quartz","Quartz"),(6,"crude-oil","Crude oil"),(7,"water","Water"),
    (8,"stone","Stone"),(9,"sand","Sand"),(10,"silver-ore","Silver ore"),(11,"iron-ingot","Iron ingot"),
    (12,"copper-ingot","Copper ingot"),(13,"conductive-alloy","Conductive alloy"),(14,"glass","Glass"),
    (15,"iron-sheet","Iron sheet"),(16,"nuts-bolts","Nuts and bolts"),(17,"cable","Cable"),(18,"concrete","Concrete"),
    (20,"machine-part","Machine part"),(21,"portable-miner","Portable miner"),(22,"generator-unit","Generator unit"),
    (23,"pole-bundle","Pole bundle"),(24,"amorium","Amorium"),(25,"moondust","Moondust"),(26,"techtorium","Techtorium"),
    (27,"gravel","Gravel"),(28,"crushed-iron-ore","Crushed iron ore"),(29,"crushed-copper-ore","Crushed copper ore"),
    (30,"purified-iron-ore","Purified iron ore"),(31,"purified-copper-ore","Purified copper ore"),(32,"steel","Steel"),
    (33,"fuel","Fuel"),(34,"heavy-oil","Heavy oil"),(35,"plastic","Plastic"),(36,"rubber","Rubber"),
    (37,"fertilizer","Fertilizer"),(38,"hydrogen","Hydrogen"),(39,"oxygen","Oxygen"),(40,"clay","Clay"),
    (41,"bricks","Bricks"),(42,"lime","Lime"),(43,"silicon","Silicon"),(44,"lenses","Lenses"),
    (45,"seeds","Seeds"),(46,"biomass","Biomass"),(47,"circuits","Circuits"),(48,"motor","Motor"),
    (49,"machinery","Machinery"),(50,"scrap","Scrap")]


def export_item(kind, slug, title):
    # Center the entire silhouette on its animation pivot, then bake every part.
    bpy.context.view_layer.update()
    low=Vector((math.inf,)*3);high=Vector((-math.inf,)*3)
    for obj in parts:
        for vertex in obj.data.vertices:
            p=obj.matrix_world@vertex.co
            for a in range(3): low[a]=min(low[a],p[a]);high[a]=max(high[a],p[a])
    center=(low+high)/2
    for obj in parts: obj.location-=center
    merged=[];triangles=0
    for material in dict.fromkeys(o.data.materials[0] for o in parts):
        copies=[]
        for original in parts:
            if original.data.materials[0]!=material: continue
            obj=original.copy();obj.data=original.data.copy();bpy.context.collection.objects.link(obj);copies.append(obj)
        bpy.ops.object.select_all(action="DESELECT")
        for obj in copies: obj.select_set(True)
        bpy.context.view_layer.objects.active=copies[0]
        if len(copies)>1: bpy.ops.object.join()
        obj=copies[0];obj.name=slug+" / "+material.name
        bpy.context.scene.cursor.location=(0,0,0);bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
        bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
        obj.data.calc_loop_triangles();triangles+=len(obj.data.loop_triangles);merged.append(obj)
    assert triangles<=1400,(slug,triangles)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged: obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT/f"{slug}.glb"),export_format="GLB",use_selection=True,
                              export_yup=True,export_apply=True,export_animations=False,export_cameras=False,export_lights=False)
    for obj in merged: bpy.data.objects.remove(obj,do_unlink=True)
    collection=bpy.data.collections.new(title);bpy.context.scene.collection.children.link(collection)
    for obj in parts:
        for old in list(obj.users_collection): old.objects.unlink(obj)
        collection.objects.link(obj)
    groups.append((title,list(parts)))
    size=high-low;bounds=[[-size.x/2,-size.z/2,-size.y/2],[size.x/2,size.z/2,size.y/2]]
    mesh=f"item-model-{kind}"
    write_json(ASSETS/f"item-{slug}.prefab.json",{
        "version":1,"name":title+" item","root":"root",
        "assets":{mesh:{"kind":"mesh","path":f"models/materials/{slug}.glb"}},
        "objects":[{"id":"root","name":"Moving item","transform":transform(),
                    "drawable":{"layer":"3d","mesh":{"asset":mesh},"texture":"white","color":[1,1,1],"uv_scale":[1,1],"gi_static":False}}]})
    catalog.append({"kind":kind,"id":slug,"name":title,"mesh_asset":mesh,"mesh":f"models/materials/{slug}.glb",
                    "prefab":f"item-{slug}.prefab.json","triangles":triangles,"surfaces":len({o.data.materials[0] for o in parts}),
                    "bounds":[[round(v,6) for v in p] for p in bounds],"fluid_sample":kind in [6,7,33,34,38,39]})
    print(f"material_export kind={kind} name={slug} triangles={triangles}")
    parts.clear()


def showroom():
    assets={};objects=[]
    for i,e in enumerate(catalog):
        assets[e["mesh_asset"]]={"kind":"mesh","path":"assets/"+e["mesh"]}
        x=(i%7-3)*1.25;z=(i//7-3)*1.25
        y=-.06+e["bounds"][1][1]*2.5+.004
        objects.append({"id":e["id"],"name":e["name"],"transform":pose((x,y,z),(2.5,2.5,2.5)),
                        "drawable":{"layer":"3d","mesh":{"asset":e["mesh_asset"]},"texture":"white","color":[1,1,1],"uv_scale":[1,1]}})
    objects.append({"id":"camera","name":"Material catalog camera","transform":pose((10,13,15),rotation=(-37,34,0)),
                    "camera":{"projection":"orthographic","vertical_size":13,"near":.1,"far":100}})
    objects.append({"id":"floor","name":"Studio floor","transform":pose((0,-.11,0),(12,.10,12)),
                    "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[.19,.22,.23],"uv_scale":[1,1]}})
    write_json(ROOT/"examples/earth-factory/scenes/materials-showroom.json",{"version":1,"name":"Stellar-IX material catalog",
               "assets":assets,"views":{"3d":"camera"},"objects":objects,
               "environment":{"zenith":[.25,.29,.32],"horizon":[.25,.29,.32],"ground":[.19,.22,.23],"intensity":.55,"background":True},
               "lighting":{"sun_direction":[.46,.81,.35],"sun_color":[1,.96,.9],"sun_intensity":2.6,
                           "ambient_color":[.86,.94,1],"ambient_intensity":.24}})


def studio():
    for i,(title,objects) in enumerate(groups):
        root=bpy.data.objects.new(title+" studio pivot",None);bpy.context.scene.collection.objects.link(root)
        root.location=((i%7-3)*1.65,(3-i//7)*1.65,catalog[i]["bounds"][1][1]*3+.004);root.scale=(3,3,3)
        for obj in objects: obj.parent=root
    floor=mat("Studio gray",(.34,.38,.40),0,.8)
    bpy.ops.mesh.primitive_plane_add(size=200);bpy.context.object.data.materials.append(floor)
    for p,power,size in [((2,-4,14),1900,10),((-9,-4,8),1300,8),((5,8,10),1600,8)]:
        bpy.ops.object.light_add(type="AREA",location=p);bpy.context.object.data.energy=power;bpy.context.object.data.shape="DISK";bpy.context.object.data.size=size
        bpy.context.object.rotation_euler=(-Vector(p)).to_track_quat("-Z","Y").to_euler()
    bpy.ops.object.camera_add(location=(9,-13,18));camera=bpy.context.object
    camera.rotation_euler=(-camera.location).to_track_quat("-Z","Y").to_euler();camera.data.type="ORTHO";camera.data.ortho_scale=17.8
    lettering=mat("Studio label",(.86,.88,.87),0,.7)
    for i,(title,_) in enumerate(groups):
        bpy.ops.object.text_add(location=((i%7-3)*1.65,(3-i//7)*1.65-.63,.12))
        label=bpy.context.object;label.name=title+" label";label.rotation_euler=camera.rotation_euler
        label.data.body=title.upper();label.data.align_x="CENTER";label.data.size=.12;label.data.materials.append(lettering)
    scene=bpy.context.scene;scene.camera=camera;scene.render.engine="CYCLES";scene.cycles.device="CPU"
    scene.cycles.samples=24;scene.cycles.use_denoising=True;scene.world.color=(.24,.24,.24)
    scene.render.resolution_x=2400;scene.render.resolution_y=2100;scene.render.resolution_percentage=100
    scene.view_settings.view_transform="AgX";scene.render.image_settings.file_format="PNG"
    scene.render.filepath=str(SOURCE/"materials-preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE/"mk1-materials.blend"))
    if "--no-render" not in sys.argv: bpy.ops.render.render(write_still=True)


if __name__=="__main__":
    for kind,slug,title in ITEMS: build(kind);export_item(kind,slug,title)
    write_json(SOURCE/"manifest.json",{"version":1,"units":"tile","up_axis":"Y","pivot":"bounds center",
               "belt_width":.46,"items":catalog})
    showroom();studio()
