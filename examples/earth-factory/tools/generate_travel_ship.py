"""Upright survey rocket and launch pad, sharing the spaceship debris palette.

blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_travel_ship.py
Add ``-- --no-render`` to skip the construction-stage contact sheet.
Ship halves share one pivot; the station stays inside the existing 4x4 pad.
"""
from pathlib import Path
import math
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
import generate_spaceship_debris as kit
from generate_scene import cube, glowing, transform, write_json

ROOT = kit.ROOT
OUT = kit.ASSETS / "models/travel"
SOURCE = ROOT / "assets/travel-ship"
catalog, groups = [], {}
NAVIGATION = [[-.56, 1.94, 1.22], [.56, 1.94, 1.22]]


def cylinder(name, pos, radius, depth, material, axis="y", tip=None):
    bpy.ops.mesh.primitive_cone_add(vertices=12, radius1=radius if tip is None else tip,
        radius2=radius, depth=depth, location=kit.point(pos))
    obj = bpy.context.object
    if axis == "x":
        obj.rotation_euler.y = math.pi/2
    elif axis == "z":
        obj.rotation_euler.x = math.pi/2
    return kit.finish(obj, name, material)


def ring(name, pos, outer, inner, depth, material, axis="y", count=12):
    verts, faces = [], []
    for distance, radius in [(-depth/2, outer), (depth/2, outer),
                             (-depth/2, inner), (depth/2, inner)]:
        for i in range(count):
            a=math.tau*i/count
            u,v=radius*math.cos(a),radius*math.sin(a)
            p=(u, v, distance) if axis=="z" else (u, distance, v)
            verts.append(tuple(pos[j]+p[j] for j in range(3)))
    for i in range(count):
        j=(i+1)%count
        faces += [(i,j,count+j,count+i), (2*count+i,3*count+i,3*count+j,2*count+j),
                  (i,2*count+i,2*count+j,j), (count+i,count+j,3*count+j,3*count+i)]
    return kit.mesh_part(name,verts,faces,material)


def stack(name, stations, material, count=12):
    """Closed faceted rocket hull around the vertical Y axis."""
    vertices,faces=[],[]
    for y,radius in stations:
        for i in range(count):
            a=math.tau*i/count
            vertices.append((radius*math.cos(a),y,radius*math.sin(a)))
    for s in range(len(stations)-1):
        for i in range(count):
            j=(i+1)%count
            faces.append((s*count+i,s*count+j,(s+1)*count+j,(s+1)*count+i))
    faces += [tuple(reversed(range(count))),tuple(range((len(stations)-1)*count,len(stations)*count))]
    return kit.mesh_part(name,vertices,faces,material)


def bell(name, x, z, outer=.19):
    count=12
    vertices,faces=[],[]
    for y,radius in [(.14,outer),(.39,outer*.62),(.14,outer*.75),(.39,outer*.44)]:
        for i in range(count):
            a=math.tau*i/count
            vertices.append((x+radius*math.cos(a),y,z+radius*math.sin(a)))
    for i in range(count):
        j=(i+1)%count
        faces += [(i,j,count+j,count+i),(2*count+i,3*count+i,3*count+j,2*count+j),
                  (i,2*count+i,2*count+j,j),(count+i,count+j,3*count+j,3*count+i)]
    kit.mesh_part(name,vertices,faces,kit.steel)
    cylinder("Dark recessed thrust chamber",(x,.36,z),outer*.44,.045,kit.char)


def lower():
    stack("Rocket engine fairing",[(.39,.37),(.60,.49),(.83,.52)],kit.char)
    stack("Lower fuel tank armor",[(.67,.49),(1.48,.52),(1.69,.51)],kit.armor)
    for x,z in [(-.21,.13),(.21,.13),(0,-.22)]:
        bell("Open rocket engine bell",x,z)
    ring("Engine bay steel collar",(0,.65,0),.514,.487,.075,kit.steel)
    for i in range(4):
        angle=math.tau*i/4
        outline=[(.44,.27),(.88,.10),(.88,.60),(.52,1.29),(.44,1.29)]
        vertices=[]
        for width in [-.045,.045]:
            for r,y in outline:
                vertices.append((r*math.cos(angle)-width*math.sin(angle),y,
                                 r*math.sin(angle)+width*math.cos(angle)))
        n=len(outline)
        faces=[tuple(reversed(range(n))),tuple(range(n,2*n))]
        faces += [(j,(j+1)%n,(j+1)%n+n,j+n) for j in range(n)]
        kit.mesh_part("Swept vertical rocket fin",vertices,faces,kit.armor)
        kit.box("Rocket landing shoe",(.78*math.cos(angle),.035,.78*math.sin(angle)),
                (.21,.07,.21),kit.char,.015)
        kit.beam("Fin cyan service stripe",(.58*math.cos(angle),1.03,.58*math.sin(angle)),
                 (.77*math.cos(angle),.66,.77*math.sin(angle)),.025,kit.cyan)
        kit.beam("Fin landing strut",(.75*math.cos(angle),.10,.75*math.sin(angle)),
                 (.48*math.cos(angle),.44,.48*math.sin(angle)),.055)
    for y in [1.43,1.54]:
        ring("Matching double gold identification band",(0,y,0),.527,.510,.042,kit.gold)
    ring("Exposed interstage frame",(0,1.73,0),.543,.462,.12,kit.steel)
    for i in range(8):
        a=math.tau*i/8
        kit.beam("Interstage structural rib",(.45*math.cos(a),1.66,.45*math.sin(a)),
                 (.45*math.cos(a),1.94,.45*math.sin(a)),.033)
    for x in [-.18,.18]:
        kit.beam("Engine service conduit",(x,.73,.502),(x,1.29,.515),.022,kit.cyan)
    kit.box("Lower engine access hatch",(0,.98,.525),(.22,.25,.022),kit.char,.012)
    kit.box("Hatch gold latch",(0,1.07,.542),(.09,.024,.012),kit.gold,0)


def upper():
    stack("Upper rocket fuselage",[(1.78,.515),(2.38,.51),(2.87,.48)],kit.armor)
    stack("Pointed space rocket nose",[(2.86,.48),(3.14,.39),(3.54,.19),(3.90,.004)],kit.armor)
    ring("Graphite interstage separator",(0,1.82,0),.543,.505,.08,kit.char)
    ring("Cyan fuselage service band",(0,2.02,0),.519,.501,.026,kit.cyan)
    ring("Nose gold collar",(0,2.84,0),.491,.474,.035,kit.gold)
    for side in [-1,1]:
        kit.box("Navigation lens mount",(side*.548,1.825,.22),(.085,.085,.075),kit.char,.01)
        kit.box("Upper hull segmented gold marking",(side*.513,2.16,.015),(.012,.23,.042),kit.gold,0)
        kit.box("Upper hull segmented gold marking",(side*.513,2.16,.105),(.012,.23,.042),kit.gold,0)
    for x,z,axis in [(0,.514,"z"),(.514,0,"x")]:
        size=(.34,.28,.025) if axis=="z" else (.025,.28,.34)
        kit.box("Cockpit port dark frame",(x,2.55,z),size,kit.char,.035)
        pos=(0,2.55,.534) if axis=="z" else (.534,2.55,0)
        size=(.25,.19,.014) if axis=="z" else (.014,.19,.25)
        kit.box("Smoky blue cockpit observation port",pos,size,kit.glass,.018)
    kit.box("Flight computer access panel",(0,2.19,.524),(.20,.23,.018),kit.steel,.015)
    kit.box("Flight computer cyan latch",(0,2.23,.538),(.115,.03,.012),kit.cyan,0)


def station():
    outline=[(-1.5,-1.10),(-1.10,-1.5),(2.10,-1.5),(2.5,-1.10),
             (2.5,2.10),(2.10,2.5),(-1.10,2.5),(-1.5,2.10)]
    kit.plate("Armored station foundation",outline,-.85,.75,kit.steel)
    kit.plate("Graphite landing deck",outline,-.10,.195,kit.char)
    for i,a in enumerate(outline):
        b=outline[(i+1)%len(outline)]
        kit.beam("Steel perimeter rim",(a[0],.042,a[1]),(b[0],.042,b[1]),.09)
    # Low, inset maintenance channels leave existing machine/cursor heights intact.
    for x in [-1.30,2.30]:
        kit.box("Cyan pad edge guidance",(x,.101,.50),(.035,.008,2.78),kit.cyan,0)
    for z in [-1.30,2.30]:
        kit.box("Gold pad edge marking",(.5,.101,z),(2.78,.008,.035),kit.gold,0)
    for x in [-.5,.5,1.5]:
        kit.box("Deck expansion seam",(x,.099,.5),(.013,.007,3.50),deck,0)
    for z in [-.5,.5,1.5]:
        kit.box("Deck expansion seam",(.5,.099,z),(3.50,.007,.013),deck,0)
    ring("Octagonal landing target",(0,.105,1),.94,.905,.012,kit.gold,count=8)
    for x in [-.19,.19]:
        kit.box("Landing H marking",(x,.105,1),(.035,.014,.48),kit.gold,0)
    kit.box("Landing H crossbar",(0,.105,1),(.39,.014,.035),kit.gold,0)
    for x in [-1.11,2.10]:
        for z in [-1.10,2.10]:
            kit.box("Corner service cover",(x,.105,z),(.29,.025,.29),kit.steel,.006)
            for i in range(3):
                obj=kit.box("Corner caution stripe",(x+(i-1)*.078,.124,z),(.028,.012,.22),kit.gold,0)
                obj.rotation_euler.z=.30
    # Same gameplay source at (0,0), with its cable socket exactly at Y=1.26.
    kit.box("Station power cabinet",(0,.36,-.22),(.65,.52,.50),deck,.035)
    kit.box("Power terminal ivory housing",(0,.63,-.23),(.61,.14,.46),kit.armor,.025)
    kit.box("Power terminal display",(0,.53,.04),(.34,.17,.016),kit.glass,.005)
    for x in [-.18,.18]:
        kit.box("Power status cyan stripe",(x,.66,.01),(.09,.014,.06),kit.cyan,0)
    cylinder("Power cable mast",(0,.91,0),.034,.62,kit.steel)
    cylinder("Gold power terminal",(0,1.245,0),.064,.03,kit.gold)
    # Existing fuel-input tile stays at (2,1).
    kit.box("Fuel dock pedestal",(2,.20,1),(.73,.20,.81),kit.steel,.04)
    kit.box("Fuel input cabinet",(2,.47,1),(.65,.38,.69),deck,.03)
    kit.box("Fuel dock ivory hood",(2,.69,1),(.69,.08,.72),kit.armor,.025)
    kit.box("Fuel input display",(2,.50,1.355),(.32,.15,.012),kit.glass,.004)
    for x in [1.82,2.18]:
        kit.box("Fuel dock cyan latch",(x,.69,1.02),(.07,.014,.23),kit.cyan,0)
    ring("Gold fuel input coupling",(2.345,.44,1),.115,.071,.07,kit.gold,axis="y")
    kit.beam("Fuel service pipe",(1.67,.19,1),(1.37,.19,1),.065)
    kit.beam("Fuel service elbow",(1.37,.19,1),(1.22,.19,1.23),.065)
    kit.beam("Fuel umbilical connection",(1.22,.19,1.23),(.84,.19,1.23),.065)
    kit.plate("Fuel input direction marker",[(1.90,1.72),(2.10,1.72),(2.10,1.62),
        (2.18,1.62),(2,1.43),(1.82,1.62),(1.90,1.62)],.103,.009,kit.gold)


def exhaust():
    for x,z in [(-.21,.13),(.21,.13),(0,-.22)]:
        cylinder("Downward rocket exhaust plume",(x,-.33,z),.115,.90,kit.cyan,tip=.024)


def export(slug,title,role):
    bpy.context.view_layer.update()
    for obj in kit.parts:
        obj.data.transform(obj.matrix_world)
        obj.matrix_world.identity()
    merged=[]
    for mat in materials:
        copies=[]
        for original in kit.parts:
            if original.data.materials[0]!=mat:
                continue
            copy=original.copy();copy.data=original.data.copy()
            bpy.context.collection.objects.link(copy);copies.append(copy)
        if not copies:
            continue
        bpy.ops.object.select_all(action="DESELECT")
        for obj in copies:
            obj.select_set(True)
        bpy.context.view_layer.objects.active=copies[0]
        if len(copies)>1:
            bpy.ops.object.join()
        obj=copies[0];obj.name=title+" / "+mat.name
        merged.append(obj)
    low,high=[math.inf]*3,[-math.inf]*3
    triangles=0
    for obj in merged:
        obj.data.calc_loop_triangles();triangles+=len(obj.data.loop_triangles)
        for vertex in obj.data.vertices:
            p=(vertex.co.x,vertex.co.z,-vertex.co.y)
            for i in range(3):
                low[i]=min(low[i],p[i]);high[i]=max(high[i],p[i])
    assert triangles<6000,(slug,triangles)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in merged:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT / f"{slug}.glb"),export_format="GLB",
        use_selection=True,export_yup=True,export_apply=True,export_animations=False,
        export_cameras=False,export_lights=False)
    catalog.append({"id":slug,"name":title,"role":role,"mesh":f"models/travel/{slug}.glb",
        "triangles":triangles,"surfaces":len(merged),
        "bounds":[[round(v,6) for v in bound] for bound in [low,high]]})
    for obj in merged:
        bpy.data.objects.remove(obj,do_unlink=True)
    groups[slug]=list(kit.parts);kit.parts.clear()
    print(f"travel_export {slug}: {triangles} triangles",flush=True)


def showroom():
    assets={e["id"]:{"kind":"mesh","path":"assets/"+e["mesh"]} for e in catalog}
    objects=[{"id":"camera","name":"Ship showroom camera",
        "transform":{**transform(10,10,10),"rotation_degrees":[-35.264,45,0]},
        "camera":{"projection":"orthographic","vertical_size":7.4,"near":.1,"far":100}}]
    objects.append(cube("ground","Planet surface",(0,-.035,0),(30,.06,30),[.14,.18,.19]))
    for slug,pos in [("landing-station",(0,0,0)),("survey-ship-lower",(0,.10,1)),("survey-ship-upper",(0,.10,1))]:
        objects.append({"id":slug,"name":slug,"transform":transform(*pos),
            "drawable":{"layer":"3d","mesh":{"asset":slug},"texture":"white",
                        "color":[1,1,1],"uv_scale":[1,1]}})
    write_json(kit.ASSETS.parent / "travel-ship-showroom.json",{"version":1,
        "name":"Stellar-IX — space rocket and launch pad","views":{"3d":"camera"},
        "assets":assets,"objects":objects,
        "environment":{"zenith":[.20,.25,.29],"horizon":[.20,.25,.29],
            "ground":[.14,.18,.19],"intensity":.55,"background":True},
        "lighting":{"shadows":True,"sun_direction":[.46,.81,.35],"sun_color":[1,.96,.90],
            "sun_intensity":2.6,"ambient_color":[.86,.94,1],"ambient_intensity":.24}})


def studio():
    right=Vector((.7071,.7071,0))
    for stage,label in [(0,"LAUNCH PAD"),(1,"ASSEMBLY / 50%"),(2,"SPACE ROCKET / 100%")]:
        offset=right*((stage-1)*5.5)
        collection=bpy.data.collections.new(label)
        bpy.context.scene.collection.children.link(collection)
        slugs=["landing-station"]+(["survey-ship-lower"] if stage>=1 else [])+(["survey-ship-upper"] if stage>=2 else [])
        for slug in slugs:
            for original in groups[slug]:
                copy=original.copy();copy.data=original.data.copy();collection.objects.link(copy)
                copy.location=offset+(kit.point((0,.10,1)) if slug.startswith("survey-ship") else Vector((0,0,0)))
        bpy.ops.object.text_add(location=offset+Vector((2.75,-2.75,.12)))
        label_obj=bpy.context.object
        label_obj.data.body,label_obj.data.align_x,label_obj.data.size=label,"CENTER",.18
        label_obj.data.materials.append(lettering)
    # Hide source meshes from the contact sheet; they remain editable at their
    # original pivots in named collections, with no studio offsets baked in.
    for slug,group in groups.items():
        collection=bpy.data.collections.new(slug+" / source")
        bpy.context.scene.collection.children.link(collection)
        collection.hide_render=True;collection.hide_viewport=True
        for obj in group:
            for old in list(obj.users_collection):
                old.objects.unlink(obj)
            collection.objects.link(obj)
    kit.box("Studio ground",(0,-.07,0),(200,.1,200),floor,0)
    for name,loc,power,size in [("Key",(0,-5,10),1900,9),("Rim",(-5,4,7),1700,7),("Fill",(7,4,6),1400,8)]:
        light=bpy.data.lights.new(name,"AREA");light.energy=power;light.shape="DISK";light.size=size
        obj=bpy.data.objects.new(name,light);bpy.context.collection.objects.link(obj)
        obj.location=loc;obj.rotation_euler=(-obj.location).to_track_quat("-Z","Y").to_euler()
    bpy.ops.object.camera_add(location=(10,-10,12))
    camera=bpy.context.object
    camera.rotation_euler=(Vector((.25,-.25,.10))-camera.location).to_track_quat("-Z","Y").to_euler()
    camera.data.type,camera.data.ortho_scale="ORTHO",17.5
    for obj in bpy.data.objects:
        if obj.type=="FONT":
            obj.rotation_euler=camera.rotation_euler
    scene=bpy.context.scene;scene.camera=camera;scene.render.engine="CYCLES"
    scene.cycles.device,scene.cycles.samples,scene.cycles.use_denoising="CPU",24,True
    scene.render.resolution_x,scene.render.resolution_y,scene.render.resolution_percentage=2100,1000,100
    scene.world.color=(.18,.18,.18);scene.view_settings.view_transform="AgX"
    scene.render.image_settings.file_format="PNG";scene.render.filepath=str(SOURCE / "travel-ship-preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE / "space-rocket-and-launch-pad.blend"))
    if "--no-render" not in sys.argv:
        bpy.ops.render.render(write_still=True)
        for name in ["LAUNCH PAD", "ASSEMBLY / 50%"]:
            bpy.data.collections[name].hide_render=True
        for obj in bpy.data.objects:
            if obj.type=="FONT":
                obj.hide_render=True
        target=right*5.5+Vector((.5,-.5,1.20))
        camera.location=target+Vector((7,-7,8))
        camera.rotation_euler=(target-camera.location).to_track_quat("-Z","Y").to_euler()
        camera.data.ortho_scale=7.5
        scene.render.resolution_x,scene.render.resolution_y=1300,1100
        scene.render.filepath=str(SOURCE / "travel-ship-hero.png")
        bpy.ops.render.render(write_still=True)


def main():
    global materials,deck,floor,lettering
    for directory in [OUT,SOURCE]:
        directory.mkdir(parents=True,exist_ok=True)
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.object.select_all(action="SELECT");bpy.ops.object.delete(use_global=False)
    materials=kit.create_materials()
    deck=kit.create_principled_material(bpy,"Station graphite enamel",(.105,.125,.14),.55,.42)
    materials.append(deck)
    floor=kit.create_principled_material(bpy,"Studio ground",(.045,.062,.075),0,.85)
    lettering=kit.create_principled_material(bpy,"Preview lettering",(.70,.79,.81),0,.8)
    for slug,title,role,build in [
        ("landing-station","Space rocket launch pad","phase 5 station",station),
        ("survey-ship-lower","Space rocket lower stage","phase 6 engines, fuel tank and fins",lower),
        ("survey-ship-upper","Space rocket upper stage","phase 7 upper hull and pointed nose",upper),
        ("survey-ship-exhaust","Rocket engine exhaust","flight-only effect",exhaust),
    ]:
        build();export(slug,title,role)
    write_json(SOURCE / "manifest.json",{"version":1,"units":"tile","up_axis":"Y",
        "ship":"Stellar-IX survey ship","station_footprint":[4,4],
        "station_bounds_xz":[[-1.55,-1.55],[2.55,2.55]],
        "ship_position":[0,.10,1],"ship_pivot":"shared assembly pivot",
        "power_socket":[0,1.26,0],"fuel_input_tile":[2,1],
        "navigation_lights":NAVIGATION,"models":catalog})
    showroom();studio()


if __name__=="__main__":
    main()
