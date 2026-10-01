"""Modular one-tile factory floors, roofs and edge walls, plus sliding doors."""
from pathlib import Path
import math, sys
import bpy
from mathutils import Vector
sys.path.insert(0,str(Path(__file__).resolve().parent))
import generate_spaceship_debris as kit
from generate_scene import transform, write_json
ROOT=kit.ROOT
OUT=kit.ASSETS/'models/foundations'
SOURCE=ROOT/'assets/foundations'
CATALOG=[]
GROUPS={}

def box(name,pos,size,mat,bevel=.008):
    return kit.box(name,pos,size,mat,bevel)

def floor(wood=False):
    box('Load bearing foundation',(0,-.052,0),(.998,.30,.998),timber if wood else concrete)
    if wood:
        for i in range(5):
            box('Timber floorboard',(0,.090,-.40+i*.20),(.99,.022,.188),wood_light if i%2 else timber,.003)
    else:
        box('Concrete finish',(0,.091,0),(.984,.022,.984),concrete_light,.003)
        for x in [-.38,.38]:
            for z in [-.38,.38]:box('Flush foundation socket',(x,.104,z),(.035,.006,.035),metal,0)

def wall(material):
    box('Structural wall',(0,1.515,0),(.16,2.88,1.0),material)
    box('Top coping',(0,2.96,0),(.18,.08,1.0),trim)
    box('Wall skirting',(0,.145,0),(.185,.14,1.0),trim)
    if material==brick:
        # Shallow staggered face bricks retain a closed structural backing.
        for side in [-1,1]:
            for row in range(12):
                for col in range(3):
                    z=-.5+(col+.5)*.34+(.17 if row%2 else 0)
                    lo,hi=max(-.5,z-.16),min(.5,z+.16)
                    if hi>lo:box('Fired brick',(side*.087,.27+row*.224,(lo+hi)/2),(.025,.207,hi-lo),brick_light if (row+col)%3 else brick,.003)
    elif material==timber:
        for side in [-1,1]:
            for i in range(6):box('Timber wall board',(side*.087,1.52,-.418+i*.167),(.025,2.72,.158),wood_light if i%2 else timber,.003)
    elif material==metal:
        for side in [-1,1]:
            for z in [-.38,0,.38]:box('Raised steel wall seam',(side*.091,1.52,z),(.026,2.75,.025),trim,.003)
        box('Cyan service marking',(.104,.43,0),(.008,.04,.6),cyan,0)
    else:
        for y in [.77,1.72,2.65]:box('Concrete panel joint',(.084,y,0),(.008,.015,1.0),trim,0)

def roof(wood=False):
    box('Roof slab',(0,3.035,0),(1.0,.16,1.0),timber if wood else concrete)
    if wood:
        for i in range(5):box('Weatherproof timber roof',(0,3.124,-.4+i*.2),(.996,.018,.188),wood_light if i%2 else timber,.002)
    else:
        box('Concrete roof finish',(0,3.122,0),(.984,.015,.984),concrete_light,.002)
    for z in [-.47,.47]:box('Roof support beam',(0,2.94,z),(.95,.07,.045),trim,.002)

def door_frame():
    for z in [-.45,.45]:box('Sliding door jamb',(0,1.44,z),(.22,2.73,.1),trim)
    box('Door overhead drive',(0,2.83,0),(.28,.26,1.0),metal)
    box('Door head concrete',(0,2.985,0),(.18,.07,1.0),concrete)
    box('Recessed threshold',(0,.09,0),(.3,.025,.98),metal,.002)
    box('Automatic entry sensor',(.155,2.83,0),(.014,.035,.10),cyan,.002)
    for z in [-.46,.46]:box('Door caution edge',(.119,1.43,z),(.006,2.63,.026),gold,0)

def door_leaf():
    box('Modern sliding leaf',(0,1.36,0),(.075,2.53,.385),metal)
    box('Door observation glass',(.041,1.76,0),(.012,.96,.29),glass,.002)
    box('Cyan door guide',(.05,.50,0),(.008,.032,.27),cyan,0)

def window():
    box('Concrete window sill',(0,.43,0),(.16,.71,1),concrete)
    box('Concrete window head',(0,2.78,0),(.16,.36,1),concrete)
    for z in [-.445,.445]:box('Window frame',(0,1.685,z),(.19,1.83,.11),trim)
    for y in [.80,2.58]:box('Window frame rail',(0,y,0),(.19,.08,.94),metal)
    # Clear opening: thin blue pane edges suggest glass without opaque occlusion.
    for z in [-.36,.36]:box('Glass reflection',(.023,1.69,z),(.008,1.68,.018),glass,0)
    box('Window sill cap',(0,.79,0),(.25,.045,1),metal,.002)
    box('Wall skirting',(0,.145,0),(.185,.14,1),trim)
    box('Top coping',(0,2.96,0),(.18,.08,1),trim)

def export(slug,kind,title):
    bpy.context.view_layer.update()
    originals=list(kit.parts);kit.parts.clear()
    for obj in originals:
        obj.data.transform(obj.matrix_world);obj.matrix_world.identity()
    merged=[]
    for mat in MATERIALS:
        copies=[]
        for obj in originals:
            if obj.data.materials[0]!=mat:continue
            c=obj.copy();c.data=obj.data.copy();bpy.context.collection.objects.link(c);copies.append(c)
        if not copies:continue
        bpy.ops.object.select_all(action='DESELECT')
        for c in copies:c.select_set(True)
        bpy.context.view_layer.objects.active=copies[0]
        if len(copies)>1:bpy.ops.object.join()
        merged.append(copies[0])
    triangles=0
    for obj in merged:obj.data.calc_loop_triangles();triangles+=len(obj.data.loop_triangles)
    bpy.ops.object.select_all(action='DESELECT')
    for obj in merged:obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(OUT/f'{slug}.glb'),export_format='GLB',use_selection=True,
        export_yup=True,export_apply=True,export_animations=False,export_cameras=False,export_lights=False)
    for obj in merged:bpy.data.objects.remove(obj,do_unlink=True)
    mesh='foundation-'+slug
    write_json(kit.ASSETS/f'{mesh}.prefab.json',{'version':1,'name':title,'root':'root',
        'assets':{mesh:{'kind':'mesh','path':f'models/foundations/{slug}.glb'}},
        'objects':[{'id':'root','name':title,'transform':transform(),
        'drawable':{'layer':'3d','mesh':{'asset':mesh},'texture':'white','color':[1,1,1],'uv_scale':[1,1]}}]})
    CATALOG.append({'id':mesh,'kind':kind,'name':title,'triangles':triangles,'mesh':f'models/foundations/{slug}.glb',
        'prefab':f'{mesh}.prefab.json'})
    GROUPS[slug]=originals
    print(f'{slug}: {triangles} triangles',flush=True)

def studio():
    scene=bpy.context.scene
    for i,(slug,group) in enumerate(GROUPS.items()):
        offset=Vector(((i%6)*1.7-4.2,-(i//6)*2.7,0))
        for obj in group:obj.location+=offset
    ground=kit.create_principled_material(bpy,'Studio ground',(.045,.062,.075),0,.85)
    box('Studio',(0,-.26,0),(200,.1,200),ground,0)
    for name,pos,power,size in [('Key',(4,-8,12),2300,10),('Rim',(-7,6,8),1800,9),('Fill',(8,4,8),1400,8)]:
        light=bpy.data.lights.new(name,'AREA');light.energy=power;light.shape='DISK';light.size=size
        obj=bpy.data.objects.new(name,light);bpy.context.collection.objects.link(obj)
        obj.location=pos;obj.rotation_euler=(-obj.location).to_track_quat('-Z','Y').to_euler()
    bpy.ops.object.camera_add(location=(10,-14,11))
    camera=bpy.context.object;target=Vector((0,-1.5,1))
    camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
    camera.data.type='ORTHO';camera.data.ortho_scale=13.8;scene.camera=camera
    scene.render.engine='CYCLES';scene.cycles.samples=16;scene.cycles.use_denoising=True
    scene.render.resolution_x,scene.render.resolution_y,scene.render.resolution_percentage=1800,1100,100
    scene.world.color=(.18,.18,.18);scene.view_settings.view_transform='AgX'
    scene.render.image_settings.file_format='PNG';scene.render.filepath=str(SOURCE/'foundations-preview.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE/'factory-foundations.blend'))
    if '--no-render' not in sys.argv:bpy.ops.render.render(write_still=True)

def main():
    global concrete,concrete_light,brick,brick_light,timber,wood_light,metal,trim,glass,cyan,gold,MATERIALS
    OUT.mkdir(parents=True,exist_ok=True);SOURCE.mkdir(parents=True,exist_ok=True)
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    mats=[('Concrete',(.48,.51,.50),.05,.86),('Concrete finish',(.62,.65,.62),.05,.77),
        ('Fired brick',(.47,.20,.13),0,.88),('Brick faces',(.67,.32,.21),0,.84),
        ('Structural timber',(.28,.16,.075),0,.87),('Timber boards',(.52,.32,.15),0,.82),
        ('Brushed factory steel',(.42,.49,.51),.75,.39),('Graphite frames',(.11,.15,.17),.7,.44),
        ('Blue window reflection',(.18,.54,.65),.35,.20),('Cyan door sensor',(.15,.72,.79),.25,.35),
        ('Door warning paint',(.93,.65,.21),.1,.59)]
    MATERIALS=[kit.create_principled_material(bpy,*args) for args in mats]
    concrete,concrete_light,brick,brick_light,timber,wood_light,metal,trim,glass,cyan,gold=MATERIALS
    for slug,kind,title,build in [
        ('concrete-floor',30,'Concrete foundation',lambda:floor()),('wood-floor',31,'Timber foundation',lambda:floor(True)),
        ('concrete-wall',32,'Concrete wall',lambda:wall(concrete)),('brick-wall',33,'Brick wall',lambda:wall(brick)),
        ('metal-wall',34,'Metal wall',lambda:wall(metal)),('wood-wall',35,'Wood wall',lambda:wall(timber)),
        ('concrete-roof',36,'Concrete roof',lambda:roof()),('wood-roof',37,'Wood roof',lambda:roof(True)),
        ('sliding-door',38,'Modern sliding door',door_frame),('window',39,'Modern wall window',window),
        ('door-leaf',0,'Sliding door leaf',door_leaf)]:
        build();export(slug,kind,title)
    write_json(SOURCE/'manifest.json',{'version':1,'units':'tile','up_axis':'Y','floor_top':.107,
        'roof_height':3.0,'wall_alignment':'tile edge; default along Z','models':CATALOG})
    studio()
if __name__=='__main__':main()
