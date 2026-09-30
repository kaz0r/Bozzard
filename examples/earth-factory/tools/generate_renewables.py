"""Ground-mounted solar panel/array and compact wind turbine, in game tile units.

blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_renewables.py
"""
from pathlib import Path
import math, sys
import bpy
from mathutils import Vector
sys.path.insert(0, str(Path(__file__).resolve().parent))
import generate_spaceship_debris as kit
import generate_foundations as export_kit
from generate_scene import transform, write_json

SOURCE = kit.ROOT / 'assets/renewables'
OUT = kit.ASSETS / 'models/renewables'

def box(name, pos, size, mat, bevel=.004):
    return kit.box(name, pos, size, mat, bevel)

def panel(x):
    # Each rack is grounded and fits one tile; its photovoltaic face tilts 25°.
    for sx in [-.32, .32]:
        for z, height in [(-.27, .37), (.27, .12)]:
            box('Bolted ground shoe', (x+sx,.024,z), (.17,.048,.16), graphite)
            box('Rack leg', (x+sx,height/2+.035,z), (.055,height,.055), steel)
    start = len(kit.parts)
    box('Aluminium panel frame', (0,0,0), (.88,.055,.84), steel)
    box('Dark photovoltaic cells', (0,.035,0), (.825,.017,.782), cells, 0)
    for sx in [-.206, 0, .206]:
        box('Cell column busbar',(sx,.045,0),(.006,.003,.776),silver,0)
    for z in [-.26,-.13,0,.13,.26]:
        box('Cell row busbar',(0,.045,z),(.818,.003,.004),silver,0)
    # Transform all panel details as one rigid assembly, with no runtime children.
    tilt = math.radians(25)
    for obj in kit.parts[start:]:
        bpy.context.view_layer.update()
        obj.data.transform(obj.matrix_world); obj.matrix_world.identity()
        for v in obj.data.vertices:
            p = v.co.copy()
            v.co = Vector((p.x+x, math.cos(tilt)*p.y-math.sin(tilt)*p.z,
                           math.sin(tilt)*p.y+math.cos(tilt)*p.z+.29))
    box('Weatherproof inverter', (x,.105,.33), (.26,.17,.14), graphite)
    box('Inverter status strip', (x,.16,.405), (.14,.028,.009), cyan,0)

def solar_array():
    panel(0); panel(1)
    box('Shared rack spine',(.5,.06,0),(1.68,.055,.07),steel)
    box('Array service cabinet',(.5,.16,.31),(.12,.26,.18),graphite)
    box('Array caution marker',(.5,.265,.406),(.065,.025,.006),gold,0)

def turbine():
    box('Ground anchor',(0,.035,0),(.48,.07,.48),graphite)
    for x in [-.18,.18]:
        for z in [-.18,.18]:box('Anchor bolt',(x,.08,z),(.04,.02,.04),silver,0)
    kit.beam('Turbine mast',(0,.07,0),(0,1.68,0),.105,steel)
    box('Lower service sleeve',(0,.26,0),(.18,.34,.18),graphite)
    box('Cyan service indicator',(0,.28,-.095),(.07,.025,.008),cyan,0)
    box('Generator nacelle',(0,1.70,0),(.24,.21,.39),steel,.015)
    box('Nacelle vent',(0,1.70,.201),(.12,.09,.009),graphite,0)
    rotor_start=len(kit.parts)
    # Three swept blades in the X/Y plane, on the nose of the compact nacelle.
    blade=[(-.045,.075,-.24),(-.038,.36,-.23),(.025,.425,-.22),
           (.055,.15,-.245),(-.045,.075,-.26),(-.038,.36,-.25),
           (.025,.425,-.24),(.055,.15,-.265)]
    faces=[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
    for i in range(3):
        a=i*2*math.pi/3
        vertices=[(math.cos(a)*x-math.sin(a)*y,1.70+math.sin(a)*x+math.cos(a)*y,z) for x,y,z in blade]
        kit.mesh_part('Swept rotor blade',vertices,faces,silver)
    box('Rotor hub',(0,1.70,-.25),(.13,.13,.11),silver,.015)
    box('Gold rotor cap',(0,1.70,-.312),(.06,.06,.014),gold)
    global ROTOR
    ROTOR=list(kit.parts[rotor_start:])
    kit.mesh_part('Tail vane',[(0,1.73,.20),(0,1.85,.43),(0,1.60,.43),
        (.02,1.73,.20),(.02,1.85,.43),(.02,1.60,.43)],
        [(0,2,1),(3,4,5),(0,1,4,3),(1,2,5,4),(2,0,3,5)],cyan)
    # Face the rotor toward the game's default isometric camera (+Z).
    for obj in kit.parts:
        bpy.context.view_layer.update()
        obj.data.transform(obj.matrix_world);obj.matrix_world.identity()
        for v in obj.data.vertices:v.co.x=-v.co.x;v.co.y=-v.co.y

def main():
    global graphite,steel,cells,silver,cyan,gold
    OUT.mkdir(parents=True,exist_ok=True);SOURCE.mkdir(parents=True,exist_ok=True)
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    materials=[kit.create_principled_material(bpy,*args) for args in [
        ('Graphite housings',(.085,.12,.14),.55,.46),('Factory steel',(.36,.46,.49),.75,.38),
        ('Photovoltaic blue',(.025,.095,.25),.45,.24),('Silver busbars and blades',(.64,.73,.77),.6,.36),
        ('Cyan service paint',(.08,.62,.68),.25,.45),('Gold safety paint',(.91,.61,.15),.2,.48)]]
    graphite,steel,cells,silver,cyan,gold=materials
    export_kit.OUT=OUT;export_kit.SOURCE=SOURCE;export_kit.MATERIALS=materials
    export_kit.CATALOG.clear();export_kit.GROUPS.clear()
    for slug,kind,name,build in [('solar-panel',40,'Solar panel',lambda:panel(0)),
        ('solar-array',41,'Solar array',solar_array),('wind-turbine',42,'Small wind turbine',turbine)]:
        build()
        if kind==42:
            kit.parts[:]=[obj for obj in kit.parts if obj not in ROTOR]
        export_kit.export(slug,kind,name)
    # Export a rigid child around the nacelle axle. It remains an indexed stock
    # mesh, so animated turbines continue to use the regular instance batches.
    for obj in ROTOR:
        for vertex in obj.data.vertices: vertex.co-=kit.point((0,1.70,.25))
    kit.parts[:]=ROTOR
    export_kit.export('wind-turbine-rotor',0,'Wind turbine rotor')
    rotor_entry=export_kit.CATALOG.pop()
    (kit.ASSETS/rotor_entry['prefab']).unlink()
    catalog=export_kit.CATALOG
    # The reusable exporter supplies geometry merging; renewables are machine assets.
    for entry in catalog:
        old=entry['id'];new=old.replace('foundation-','machine-',1)
        prefab=kit.ASSETS/entry['prefab'];data=__import__('json').loads(prefab.read_text())
        data['assets'][new]=data['assets'].pop(old)
        data['assets'][new]['path']=f"models/renewables/{new.removeprefix('machine-')}.glb"
        data['objects'][0]['drawable']['mesh']['asset']=new
        if entry['kind']==42:
            data['assets']['wind-turbine-rotor']={'kind':'mesh','path':'models/renewables/wind-turbine-rotor.glb'}
            data['assets']['wind-rotor']={'kind':'script','path':'../scripts/wind_rotor.rhai'}
            data['objects'].append({'id':'rotor','name':'Wind turbine rotor','parent':'root',
                'transform':transform(0,1.70,.25),
                'drawable':{'layer':'3d','mesh':{'asset':'wind-turbine-rotor'},'texture':'white',
                    'color':[1,1,1],'uv_scale':[1,1],'gi_static':False},
                'script_manager':{'scripts':[{'enabled':True,'script':'wind-rotor'}]}})
            entry['rotor_mesh']='models/renewables/wind-turbine-rotor.glb'
            entry['rotor_triangles']=rotor_entry['triangles']
            entry['rotor_pivot']=[0,1.70,.25]
        prefab.unlink();entry['id']=new;entry['prefab']=new+'.prefab.json'
        entry['mesh']=data['assets'][new]['path']
        write_json(kit.ASSETS/entry['prefab'],data)
        entry['footprint']=[[0,0],[1,0]] if entry['kind']==41 else [[0,0]]
    write_json(SOURCE/'manifest.json',{'version':1,'units':'tile','up_axis':'Y',
        'array_pivot':'first tile; second tile along facing','models':catalog})
    # Arrange the editable source on a shared studio ground.
    for slug,offset in [('solar-panel',(-1.65,0,0)),('solar-array',(-.35,0,0)),('wind-turbine',(2.0,0,0))]:
        for obj in export_kit.GROUPS[slug]:obj.location+=kit.point(offset)
    for obj in ROTOR:obj.location+=kit.point((2.0,1.70,.25))
    ground=kit.create_principled_material(bpy,'Studio ground',(.07,.11,.12),0,.9)
    box('Studio',(0,-.06,0),(200,.10,200),ground,0)
    for name,pos,power,size in [('Key',(3,-5,7),1300,6),('Rim',(-4,3,6),1000,5)]:
        light=bpy.data.lights.new(name,'AREA');light.energy=power;light.size=size
        obj=bpy.data.objects.new(name,light);bpy.context.collection.objects.link(obj)
        obj.location=pos;obj.rotation_euler=(-obj.location).to_track_quat('-Z','Y').to_euler()
    bpy.ops.object.camera_add(location=(5,-8,6))
    camera=bpy.context.object;camera.rotation_euler=(Vector((.3,0,.65))-camera.location).to_track_quat('-Z','Y').to_euler()
    camera.data.type='ORTHO';camera.data.ortho_scale=5.9;scene=bpy.context.scene;scene.camera=camera
    scene.render.engine='CYCLES';scene.cycles.samples=24;scene.cycles.use_denoising=True
    scene.render.resolution_x,scene.render.resolution_y,scene.render.resolution_percentage=1500,950,100
    scene.world.color=(.2,.2,.2);scene.view_settings.view_transform='AgX'
    scene.render.image_settings.file_format='PNG';scene.render.filepath=str(SOURCE/'renewables-preview.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE/'renewable-power.blend'))
    if '--no-render' not in sys.argv:bpy.ops.render.render(write_still=True)

if __name__=='__main__':main()
