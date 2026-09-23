"""Build a self-contained, editable Bozzard university dorm in Blender.
Run: blender -b --python assets/dorm_room/create_room.py
"""
import bpy, math, random, os
from mathutils import Vector
from math import sin, cos, pi
random.seed(23)
OUT = os.path.dirname(os.path.abspath(__file__))
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
for c in list(bpy.data.collections):
    if c.name != 'Collection': bpy.data.collections.remove(c)
ROOT = bpy.data.collections.get('Collection'); ROOT.name = 'Architecture'
COL = ROOT
def collection(name):
    global COL
    COL = bpy.data.collections.new(name); bpy.context.scene.collection.children.link(COL)
def move(o):
    for c in list(o.users_collection): c.objects.unlink(o)
    COL.objects.link(o)
    return o
def mat(name, color, rough=.55, metal=0, emission=0):
    m=bpy.data.materials.new(name); m.diffuse_color=(*color,1); m.use_nodes=True
    p=m.node_tree.nodes.get('Principled BSDF'); p.inputs['Base Color'].default_value=(*color,1)
    p.inputs['Roughness'].default_value=rough; p.inputs['Metallic'].default_value=metal
    if emission:
        p.inputs['Emission Color'].default_value=(*color,1); p.inputs['Emission Strength'].default_value=emission
    return m
def texture(m, scale, strength, distance=.025):
    n=m.node_tree.nodes; l=m.node_tree.links; p=n.get('Principled BSDF')
    noise=n.new('ShaderNodeTexNoise'); noise.inputs['Scale'].default_value=scale
    bump=n.new('ShaderNodeBump'); bump.inputs['Strength'].default_value=strength; bump.inputs['Distance'].default_value=distance
    l.new(noise.outputs['Fac'],bump.inputs['Height']); l.new(bump.outputs['Normal'],p.inputs['Normal'])
def box(name, loc, size, m, bevel=.02, rot=None):
    bpy.ops.mesh.primitive_cube_add(size=1, location=loc); o=move(bpy.context.object); o.name=name
    o.dimensions=size; bpy.ops.object.transform_apply(location=False,rotation=False,scale=True)
    if rot: o.rotation_euler=rot
    if m:o.data.materials.append(m)
    if bevel:
        mod=o.modifiers.new('Soft edges','BEVEL'); mod.width=bevel; mod.segments=3
        mod=o.modifiers.new('Weighted normals','WEIGHTED_NORMAL')
    return o
def uv(name, loc, scale, m):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=24,ring_count=12,location=loc)
    o=move(bpy.context.object);o.name=name;o.scale=scale;o.data.materials.append(m)
    for p in o.data.polygons:p.use_smooth=True
    return o
def cyl(name, loc, radius, depth, m, rot=None, vertices=32):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=radius,depth=depth,location=loc)
    o=move(bpy.context.object);o.name=name;o.data.materials.append(m)
    if rot:o.rotation_euler=rot
    mod=o.modifiers.new('Edge rounding','BEVEL');mod.width=.009;mod.segments=3
    o.modifiers.new('Normals','WEIGHTED_NORMAL')
    return o
def line(name, pts, radius, m, cyclic=False):
    c=bpy.data.curves.new(name,'CURVE');c.dimensions='3D';c.resolution_u=16;c.bevel_depth=radius;c.bevel_resolution=3
    s=c.splines.new('POLY');s.points.add(len(pts)-1)
    for p,co in zip(s.points,pts):p.co=(*co,1)
    s.use_cyclic_u=cyclic
    o=bpy.data.objects.new(name,c);COL.objects.link(o);o.data.materials.append(m);return o
def rod(name,a,b,r,m):
    o=cyl(name,(Vector(a)+Vector(b))/2,r,(Vector(b)-Vector(a)).length,m)
    o.rotation_euler=(Vector(b)-Vector(a)).to_track_quat('Z','Y').to_euler();return o
def text(name, value, loc, size, m, rot=(pi/2,0,0), align='CENTER'):
    c=bpy.data.curves.new(name,'FONT');c.body=value;c.size=size;c.align_x=align;c.extrude=.001;c.bevel_depth=.0007
    o=bpy.data.objects.new(name,c);COL.objects.link(o);o.location=loc;o.rotation_euler=rot;o.data.materials.append(m);return o
def area(name, loc, target, power, color, size):
    d=bpy.data.lights.new(name,'AREA');d.energy=power;d.color=color;d.shape='DISK';d.size=size
    o=bpy.data.objects.new(name,d);COL.objects.link(o);o.location=loc;o.rotation_euler=(Vector(target)-o.location).to_track_quat('-Z','Y').to_euler();return o
sage=mat('Sage green painted plaster',(.29,.43,.39));texture(sage,95,.12)
cream=mat('Warm ivory trim',(.84,.79,.66));white=mat('Cotton off white',(.88,.86,.76));texture(white,150,.18)
oak=mat('Honey oak',(.49,.27,.115));darkwood=mat('Walnut',(.17,.075,.032));black=mat('Charcoal',(.022,.033,.041))
metal=mat('Brushed steel',(.3,.34,.36),.28,.8);teal=mat('Petrol blue duvet',(.055,.22,.26));texture(teal,160,.25)
terra=mat('Terracotta',(.63,.20,.095));mustard=mat('Golden ochre',(.85,.51,.11));pink=mat('Neon coral', (1,.08,.22),.28,0,6)
blue=mat('Blue book',(.085,.26,.42));paper=mat('Paper',(.86,.81,.67));green=mat('Plant green',(.10,.28,.11))
rubber=mat('Soft rubber',(.018,.022,.027));screen=mat('Screen midnight blue',(.015,.10,.17),.25,0,.65)
# Directional wood grain, requiring no image textures.
n=oak.node_tree.nodes;l=oak.node_tree.links;p=n.get('Principled BSDF')
coord=n.new('ShaderNodeTexCoord');mapping=n.new('ShaderNodeVectorMath');mapping.operation='MULTIPLY';mapping.inputs[1].default_value=(3,55,4)
noise=n.new('ShaderNodeTexNoise');noise.inputs['Scale'].default_value=3;noise.inputs['Detail'].default_value=2
ramp=n.new('ShaderNodeValToRGB');ramp.color_ramp.elements[0].color=(.24,.10,.035,1);ramp.color_ramp.elements[1].color=(.64,.40,.19,1)
l.new(coord.outputs['Generated'],mapping.inputs[0]);l.new(mapping.outputs['Vector'],noise.inputs['Vector']);l.new(noise.outputs['Fac'],ramp.inputs[0]);l.new(ramp.outputs[0],p.inputs['Base Color'])
# Open front and right sides make every prop visible in a dollhouse view.
box('Foundation',(0,0,-.14),(5.6,4.7,.27),darkwood,.055)
floor_mats=[mat('Oak floor shade %02d'%i,(.34+i*.024,.22+i*.017,.125+i*.010)) for i in range(7)]
for row in range(18):
    y=-2.22+row*.25
    bounds=[-2.7]+[x for x in [-3.5+(row%3)*.43+j*1.36 for j in range(7)] if -2.7<x<2.7]+[2.7]
    for a,b in zip(bounds,bounds[1:]):box('Individual oak floorboard',((a+b)/2,y,.011),(b-a-.009,.241,.035),random.choice(floor_mats),.004)
box('Left wall',(-2.79,0,1.5),(.18,4.65,3),sage)
box('Back wall left',(-.88,2.34,1.5),(3.82,.18,3),sage)
box('Back wall right',(2.51,2.34,1.5),(.56,.18,3),sage)
box('Door wall lintel',(1.63,2.34,2.65),(1.2,.18,.7),sage)
box('Left skirting',(-2.675,0,.12),(.055,4.5,.18),cream,.005)
box('Back skirting',(-.9,2.22,.12),(3.6,.055,.18),cream,.005)
box('Left wall cap',(-2.79,0,3.025),(.23,4.67,.08),cream)
box('Back wall cap',(0,2.34,3.025),(5.76,.23,.08),cream)
collection('Closed entrance door')
box('Closed oak door',(1.63,2.315,1.13),(1.13,.085,2.24),oak,.014)
for x in [1.02,2.24]:box('Door frame',(x,2.23,1.17),(.09,.13,2.35),cream,.008)
box('Door frame header',(1.63,2.23,2.34),(1.31,.13,.10),cream,.008)
for z,h in [(.63,.76),(1.64,.88)]:box('Inset door panel',(1.63,2.263,z),(.89,.02,h),darkwood,.018);box('Door raised panel',(1.63,2.245,z),(.85,.022,h-.04),oak,.018)
cyl('Handle rose',(2.04,2.18,1.10),.046,.03,metal,(pi/2,0,0))
rod('Door lever',(2.04,2.13,1.10),(1.88,2.13,1.10),.018,metal)
box('Room number plaque',(1.63,2.19,1.98),(.24,.018,.12),black,.013)
text('Room number','207',(1.63,2.174,1.955),.07,cream)
collection('Bed and soft furnishings')
for x in [-2.35,-1.18]:
    for y in [-.75,1.65]:box('Bed leg',(x,y,.24),(.09,.09,.45),darkwood)
box('Solid wood bed frame',(-1.77,.49,.43),(1.43,2.57,.24),oak,.04)
box('Headboard',(-1.77,1.79,.80),(1.43,.12,1.02),oak,.045)
box('Mattress',(-1.77,.49,.64),(1.35,2.43,.27),white,.13)
for x in [-2.08,-1.47]:
    o=box('Soft sleeping pillow',(x,1.32,.85),(.58,.43,.16),white,.075);o.rotation_euler[2]=random.uniform(-.09,.09)
    line('Pillow stitched edge',[(x-.24,1.10,.86),(x+.24,1.10,.86),(x+.28,1.14,.86)],.005,cream)
# Draped duvet: gently wrinkled top and hanging sides.
verts=[];faces=[];nx=46;ny=58
for j in range(ny):
    y=-.89+1.94*j/(ny-1)
    for i in range(nx):
        u=i/(nx-1);x=-2.55+1.57*u
        edge=max(0,(abs(u-.5)-.405)/.095)
        front=max(0,(-.64-y)/.25)
        z=.82-.31*edge**.7-.25*front+.020*sin(u*35+y*9)+.012*sin(y*30+u*16)
        verts.append((x,y,z))
for j in range(ny-1):
    for i in range(nx-1):a=j*nx+i;faces.append((a,a+1,a+nx+1,a+nx))
mesh=bpy.data.meshes.new('Duvet cloth mesh');mesh.from_pydata(verts,[],faces);mesh.update()
o=bpy.data.objects.new('Rumpled draped blue duvet',mesh);COL.objects.link(o);o.data.materials.append(teal)
o.modifiers.new('Cloth thickness','SOLIDIFY').thickness=.018
for p in mesh.polygons:p.use_smooth=True
for y in [.69,.72,.75]:line('Duvet stitch',[(x,y,.844+.020*sin((x+2.55)/1.57*35+y*9)) for x in [-2.39+i*.025 for i in range(50)]],.003,cream)
o=box('Ochre throw pillow',(-1.98,1.04,.98),(.43,.17,.39),mustard,.075,(-.18,0,-.14))
# Folded blanket across the foot, with short tassels.
box('Folded rust blanket',(-1.76,-.42,.867),(1.28,.43,.085),terra,.038)
for x in [-2.34+i*.052 for i in range(23)]:line('Blanket fringe',[(x,-.64,.87),(x+.006,-.70,.835)],.007,terra)
box('Under bed storage crate',(-1.70,-.69,.21),(.77,.6,.27),blue,.025)
box('Storage handle',(-1.7,-.996,.23),(.2,.009,.047),cream,.014)
collection('Study desk and laptop')
box('Desktop',(-.09,1.58,.84),(1.76,.74,.10),oak,.03)
for x in [-.84,.66]:
    for y in [1.30,1.87]:box('Desk steel leg',(x,y,.42),(.045,.045,.8),black,.007)
box('Desk drawer',(.50,1.57,.65),(.43,.60,.24),cream)
box('Drawer pull',(.50,1.26,.66),(.15,.035,.023),metal,.008)
box('Desk mat',(-.12,1.52,.897),(1.02,.49,.012),teal,.025)
box('Laptop aluminum base',(-.12,1.48,.919),(.64,.43,.033),metal,.016)
box('Keyboard black inset',(-.12,1.54,.940),(.54,.23,.008),black,.006)
for r in range(4):
    for k in range(11):box('Laptop key',(-.36+k*.047,1.46+r*.045,.947),(.034,.031,.005),metal,.004)
box('Laptop trackpad',(-.12,1.33,.940),(.19,.09,.004),darkwood,.008)
# Screen is open with a slight backwards tilt.
screen_root=bpy.data.objects.new('Laptop screen hinge',None);COL.objects.link(screen_root);screen_root.location=(-.12,1.68,.94);screen_root.rotation_euler[0]=math.radians(-12)
def screenpart(o):o.parent=screen_root;return o
screenpart(box('Laptop lid',(0,0,.215),(.64,.027,.43),black,.014))
screenpart(box('Lit laptop display',(0,-.017,.216),(.588,.007,.369),screen,.006))
screenpart(text('Laptop screen title','BOZZARD',(-.246,-.023,.337),.043,cream,align='LEFT'))
for i in range(7):
    screenpart(box('Code on laptop',(-.115,-.023,.281-i*.032),(.22+random.random()*.13,.002,.008),teal if i%2 else cream,.001))
screenpart(box('Screen sidebar',(.207,-.023,.20),(.085,.002,.21),blue,.003))
cyl('Webcam',(-.12,1.717,1.366),.008,.003,black,(pi/2,0,0))
# Workbooks, pen pot, coffee and desk lamp.
for i,m in enumerate([terra,blue,paper]):box('Stacked textbook',(.55,1.68,.926+i*.046),(.30,.23,.041),m,.006,rot=(0,0,.05*(i-1)))
text('Book title','NOTES',(.54,1.63,1.073),.044,cream,(0,0,0))
cyl('Pen cup',(-.77,1.80,1.0),.054,.20,terra)
for i in range(5):rod('Pencil',(-.79+i*.011,1.8,1.01),(-.81+i*.019,1.8+random.uniform(-.02,.02),1.21+random.uniform(-.02,.03)),.005,mustard)
cyl('Coffee mug',(.57,1.32,1.006),.066,.17,cream)
cyl('Coffee surface',(.57,1.32,1.094),.054,.003,darkwood)
line('Mug handle',[(.64+.047*cos(a),1.32,1.01+.056*sin(a)) for a in [i*2*pi/32 for i in range(33)]],.012,cream)
cyl('Lamp base',(-.68,1.41,.919),.105,.035,black)
rod('Desk lamp lower arm',(-.68,1.41,.94),(-.72,1.49,1.27),.016,black)
rod('Desk lamp upper arm',(-.72,1.49,1.27),(-.50,1.48,1.46),.016,black)
bpy.ops.mesh.primitive_cone_add(vertices=32,radius1=.115,radius2=.053,depth=.13,location=(-.48,1.48,1.43));o=move(bpy.context.object);o.name='Lamp shade';o.data.materials.append(mustard)
bulb=mat('Lamp warm diffuser',(1,.68,.30),.4,0,3)
cyl('Lamp illuminated face',(-.48,1.48,1.36),.095,.008,bulb)
area('Desk reading light',(-.48,1.48,1.35),(-.4,1.5,.87),16,(1,.61,.29),.20)
# Chair offset from desk, inviting and visible.
box('Chair upholstered seat',(.08,.56,.51),(.53,.50,.105),terra,.07)
box('Chair back',(.08,.32,.81),(.54,.08,.48),terra,.08,(-.10,0,0))
for x in [-.12,.28]:
    for y in [.37,.75]:rod('Chair leg',(x,y,.49),(x+(x-.08)*.35,y+(y-.56)*.4,.055),.023,black)
for x in [-.14,.30]:rod('Chair back support',(x,.4,.48),(x,.32,.93),.017,black)
collection('Neon sign and wall decor')
box('Neon dark backing',(-1.62,2.207,2.28),(1.66,.055,.53),black,.09)
o=text('Bozzard neon sign','Bozzard',(-1.62,2.163,2.14),.37,pink)
o.data.extrude=.003;o.data.bevel_depth=.005
# Outline glyphs read as real neon glass tubes.
o.data.fill_mode='NONE';o.data.bevel_resolution=5
line('Neon power cord',[(-2.39,2.17,2.15),(-2.39,2.17,1.92),(-2.51,2.17,1.82),(-2.51,2.17,.27)],.008,black)
box('Neon transformer',(-2.51,2.16,.32),(.10,.07,.16),black)
area('Neon pink wall wash',(-1.62,1.98,2.33),(-1.62,2.34,2.3),35,(1,.045,.17),1.1)
# Pinboard above the laptop, plus taped prints on side wall.
box('Pinboard frame',(.15,2.19,2.02),(1.20,.06,.75),oak)
cork=mat('Cork',(.43,.27,.14));texture(cork,120,.32)
box('Cork board',(.15,2.151,2.02),(1.12,.014,.67),cork,.006)
for i,(x,z,w,h,m) in enumerate([(-.2,2.08,.32,.4,paper),(.20,2.17,.31,.23,blue),(.45,1.90,.25,.25,mustard),(.12,1.87,.25,.20,cream)]):
    box('Pinned note',(x,2.13,z),(w,.008,h),m,.002,rot=(0,random.uniform(-.07,.07),0));uv('Push pin',(x,2.116,z+h/2-.023),(.013,.009,.013),terra)
text('Class timetable','MON  /  FRI',(-.2,2.114,2.19),.028,black)
for i in range(6):box('Timetable rows',(-.2,2.115,2.12-i*.035),(.23,.002,.004),teal,.001)
text('Sticky reminder','MAKE',(.45,2.112,1.92),.04,black);text('Sticky reminder 2','STUFF',(.45,2.112,1.865),.04,black)
collection('Window and shelf')
# Bright recessed-looking window on the left wall; four frosted blue panes.
glass=mat('Blue daylight glass',(.32,.58,.65),.22,0,.25)
box('Window dark recess',(-2.67,-.40,1.96),(.06,1.28,1.35),darkwood,.008)
box('Window glass',(-2.628,-.4,1.96),(.018,1.15,1.22),glass,.003)
for y in [-1.04,.24]:box('Window vertical frame',(-2.58,y,1.96),(.13,.075,1.41),cream,.008)
for z in [1.28,1.96,2.64]:box('Window horizontal frame',(-2.58,-.4,z),(.13,1.35,.068),cream,.008)
box('Window central mullion',(-2.58,-.4,1.96),(.13,.045,1.35),cream,.005)
box('Window sill',(-2.50,-.40,1.24),(.38,1.48,.065),oak)
# Partly raised blind with pull cord.
for z in [2.51+i*.045 for i in range(4)]:box('Blind slat',(-2.49,-.4,z),(.045,1.24,.034),white,.006)
line('Blind pull cord',[(-2.48,.20,2.60),(-2.48,.20,1.72)],.004,cream)
uv('Blind pull knob',(-2.48,.20,1.70),(.015,.015,.033),darkwood)
box('Wall shelf',(-2.43,1.15,1.85),(.51,1.06,.06),oak)
for y in [.77,1.53]:line('Shelf bracket',[(-2.67,y,1.60),(-2.22,y,1.81),(-2.67,y,1.81)],.013,black)
for i in range(7):
    h=random.uniform(.20,.33);box('Shelf book',(-2.44,.78+i*.075,1.9+h/2),(.24,.052,h),[terra,blue,cream,mustard][i%4],.005)
def plant(x,y,z,scale=1):
    bpy.ops.mesh.primitive_cone_add(vertices=32,radius1=.09*scale,radius2=.125*scale,depth=.20*scale,location=(x,y,z+.1*scale));o=move(bpy.context.object);o.name='Terracotta plant pot';o.data.materials.append(terra)
    cyl('Pot soil',(x,y,z+.202*scale),.108*scale,.01,darkwood)
    for i in range(9):
        a=i*2.4;end=(x+.12*scale*cos(a),y+.12*scale*sin(a),z+(.29+.11*random.random())*scale)
        rod('Plant stem',(x,y,z+.2*scale),end,.004*scale,green)
        leaf=uv('Plant leaf',end,(.035*scale,.085*scale,.017*scale),green);leaf.rotation_euler=(.3,a,a)
plant(-2.40,1.47,1.89,.8)
plant(-2.43,-.83,1.28,.78)
# Radiator under the window.
for y in [-.94+i*.105 for i in range(11)]:box('Radiator fin',(-2.57,y,.69),(.16,.078,.59),cream,.025)
for z in [.44,.94]:rod('Radiator pipe',(-2.62,-1,z),(-2.62,.2,z),.023,cream)
collection('Rug and floor clutter')
rug=mat('Woven oatmeal rug',(.57,.52,.40));texture(rug,180,.4)
box('Large woven rug',(.3,-.73,.052),(2.42,2.10,.025),rug,.025)
for x in [-.78,1.38]:box('Rug rust border',(x,-.73,.067),(.08,2.0,.003),terra,.002)
for y in [-1.71,.24]:box('Rug dark border',(.30,y,.068),(2.3,.06,.003),teal,.002)
for x in [-.86+i*.05 for i in range(48)]:
    for y,d in [(-1.79,-1),(.33,1)]:line('Rug fringe',[(x,y,.051),(x+.009,y+d*.067,.048)],.005,rug)
collection('Skateboard')
skate=bpy.data.objects.new('Skateboard complete',None);COL.objects.link(skate);skate.location=(.35,-1.36,.15);skate.rotation_euler=(0,0,-.34)
def skatepart(o):o.parent=skate;return o
skatepart(box('Rounded maple skateboard deck',(0,0,.053),(.84,.23,.041),oak,.105))
skatepart(box('Grip tape',(0,0,.077),(.79,.214,.008),black,.10))
skatepart(box('Skate deck stripe',(.18,0,.083),(.035,.205,.002),terra,.001))
for x in [-.27,.27]:
    skatepart(cyl('Truck axle',(x,0,-.025),.013,.29,metal,(pi/2,0,0)))
    skatepart(box('Truck mount',(x,0,.002),(.09,.10,.045),metal,.012))
    for y in [-.14,.14]:skatepart(cyl('Cream skateboard wheel',(x,y,-.037),.058,.045,cream,(pi/2,0,0)))
    for a in [-.021,.021]:
        for b in [-.043,.043]:skatepart(cyl('Deck screw',(x+a,b,.084),.007,.003,metal))
collection('Acoustic guitar')
# Extruded guitar silhouette with waist, bouts and a real six-string neck.
guitar=bpy.data.objects.new('Guitar complete',None);COL.objects.link(guitar);guitar.location=(2.13,.62,.12);guitar.rotation_euler=(.11,-.16,-.12)
def gp(o):o.parent=guitar;return o
outline=[(0,.02),(-.15,.035),(-.235,.13),(-.25,.25),(-.21,.34),(-.12,.40),(-.115,.46),(-.18,.52),(-.17,.62),(-.10,.68),(0,.695),(.10,.68),(.17,.62),(.18,.52),(.115,.46),(.12,.40),(.21,.34),(.25,.25),(.235,.13),(.15,.035)]
vs=[(x,y,z) for y in [-.09,.09] for x,z in outline];N=len(outline)
fs=[tuple(reversed(range(N))),tuple(range(N,2*N))]+[(i,(i+1)%N,(i+1)%N+N,i+N) for i in range(N)]
me=bpy.data.meshes.new('Guitar body silhouette');me.from_pydata(vs,[],fs);me.update();o=bpy.data.objects.new('Acoustic guitar body',me);COL.objects.link(o);o.data.materials.append(oak);gp(o)
mod=o.modifiers.new('Rounded guitar body','BEVEL');mod.width=.025;mod.segments=4;o.modifiers.new('Normals','WEIGHTED_NORMAL')
gp(line('Guitar ivory binding',[(x,-.104,z) for x,z in outline],.009,cream,True))
gp(cyl('Sound hole',(0,-.111,.465),.076,.006,black,(pi/2,0,0)))
gp(line('Sound hole rosette',[(.089*cos(a),-.119,.465+.089*sin(a)) for a in [i*2*pi/64 for i in range(65)]],.005,darkwood))
gp(box('Guitar neck',(0,0,.9),(.07,.07,.58),darkwood,.008))
gp(box('Rosewood fretboard',(0,-.05,.9),(.08,.025,.57),black,.005))
gp(box('Guitar headstock',(0,0,1.25),(.112,.061,.20),oak,.024))
gp(box('Guitar bridge',(0,-.126,.23),(.21,.031,.042),darkwood,.01))
for i in range(14):gp(rod('Guitar fret',(-.04,-.069,.635+i*.036),(.04,-.069,.635+i*.036),.002,metal))
for i in range(6):
    x=-.025+i*.01;gp(line('Guitar string',[(x,-.147,.23),(x,-.080,1.31)],.0009,metal))
for z in [1.2,1.26,1.32]:
    for x in [-.077,.077]:gp(uv('Tuning peg',(x,0,z),(.022,.015,.013),metal))
rod('Guitar stand upright',(2.14,.81,.06),(2.14,.81,.66),.015,black)
for x in [1.88,2.34]:rod('Guitar stand foot',(2.14,.81,.10),(x,.42,.065),.018,black)
collection('Laundry basket and clothes')
# Open slatted basket, with a visible pile of laundry and rolled rim.
bx,by=2.04,-1.09
cyl('Laundry basket bottom',(bx,by,.075),.30,.04,cream)
for i in range(40):
    a=i*2*pi/40
    rod('Basket woven upright',(bx+.285*cos(a),by+.285*sin(a),.08),(bx+.34*cos(a),by+.34*sin(a),.64),.009,cream)
for z in [.12+i*.056 for i in range(10)]:
    r=.285+(z-.08)/.56*.055
    line('Basket horizontal weave',[(bx+r*cos(a),by+r*sin(a),z) for a in [j*2*pi/80 for j in range(80)]],.010,cream,True)
line('Basket rolled rim',[(bx+.34*cos(a),by+.34*sin(a),.65) for a in [j*2*pi/80 for j in range(80)]],.023,cream,True)
for i in range(13):
    x=bx+random.uniform(-.19,.19);y=by+random.uniform(-.19,.19);z=.41+i*.016
    o=uv('Crumpled laundry',(x,y,z),(.15,.12,.065),[white,teal,terra,blue][i%4]);o.rotation_euler=(random.random(),random.random(),random.random())
# A shirt hanging over the near basket rim.
verts=[];faces=[]
for j in range(20):
    t=j/19
    for i in range(14):
        u=i/13;verts.append((bx-.14+.28*u,by-.29-.11*sin(t*pi/2),.66-.33*t+.012*sin(u*25+t*8)))
for j in range(19):
    for i in range(13):a=j*14+i;faces.append((a,a+1,a+15,a+14))
me=bpy.data.meshes.new('Hanging shirt cloth');me.from_pydata(verts,[],faces);me.update();o=bpy.data.objects.new('T shirt draped over basket',me);COL.objects.link(o);o.data.materials.append(blue);o.modifiers.new('Fabric thickness','SOLIDIFY').thickness=.007
for p in me.polygons:p.use_smooth=True
collection('Personal details')
# Backpack beside the desk.
box('Canvas backpack',(.93,1.03,.34),(.39,.22,.53),teal,.095,rot=(.05,-.15,-.15))
box('Backpack front pocket',(.95,.893,.27),(.29,.075,.23),blue,.04)
line('Backpack carry loop',[(.84,1.03,.59),(.84,1.03,.67),(1.0,1.03,.67),(1.0,1.03,.59)],.015,black)
line('Backpack zip',[(.81,.85,.38),(.96,.84,.4),(1.08,.86,.37)],.004,metal)
# Pair of sneakers near the foot of the bed.
for x,y,a in [(-1.08,-1.51,-.2),(-.81,-1.68,-.06)]:
    o=box('Sneaker rubber sole',(x,y,.11),(.19,.38,.055),cream,.07,rot=(0,0,a))
    o=uv('Sneaker upper',(x,y+.016,.17),(.09,.17,.083),terra);o.rotation_euler[2]=a
    uv('Sneaker opening',(x,y+.08,.23),(.052,.057,.014),black)
    for j in range(4):rod('Sneaker lace',(x-.045,y-.07+j*.03,.237),(x+.045,y-.07+j*.03,.237),.004,cream)
# Side table, books, phone.
box('Nightstand top',(-2.16,-1.14,.62),(.72,.48,.065),oak)
for x in [-2.44,-1.88]:
    for y in [-1.31,-.98]:box('Nightstand leg',(x,y,.32),(.035,.035,.60),black,.005)
box('Nightstand lower shelf',(-2.16,-1.14,.22),(.69,.45,.04),oak)
for i,m in enumerate([blue,terra,paper]):box('Bedside book',(-2.16,-1.14,.26+i*.035),(.37,.27,.034),m,.005)
box('Phone on bedside table',(-2.19,-1.14,.665),(.12,.235,.018),black,.013,rot=(0,0,.18))
box('Phone screen',(-2.19,-1.14,.676),(.101,.202,.004),screen,.008,rot=(0,0,.18))
# Framed portrait on the left wall, using the user's supplied artwork.
import runpy
runpy.run_path(os.path.join(OUT,'add_portrait.py'))['add_portrait'](OUT)
runpy.run_path(os.path.join(OUT,'add_portrait.py'))['add_photo'](OUT)
# Wall socket and cable give the desk a plausible connection.
box('Power outlet',(.54,2.19,.37),(.16,.035,.09),cream,.01)
for x in [.50,.58]:cyl('Outlet socket',(x,2.165,.37),.019,.008,black,(pi/2,0,0))
line('Laptop charger cable',[(.51,2.15,.37),(.46,2.09,.16),(.19,1.96,.12),(-.05,1.92,.87),(-.43,1.57,.92)],.006,black)
collection('Lighting and cameras')
area('Large warm key',(1,-3,7),(0,0,0),650,(1,.82,.65),5)
area('Cool window light',(-3,-.4,3),(-.5,.2,.2),400,(.58,.78,1),2.5)
area('Soft front fill',(4,-1,3),(0,1,1),200,(.8,.91,1),4)
world=bpy.context.scene.world or bpy.data.worlds.new('Studio');bpy.context.scene.world=world;world.use_nodes=True
world.node_tree.nodes['Background'].inputs[0].default_value=(.12,.17,.20,1);world.node_tree.nodes['Background'].inputs[1].default_value=.35
ground=mat('Backdrop',(.105,.15,.16))
box('Studio ground',(0,0,-.34),(200,200,.12),ground,0)
def camera(name,loc,target,ortho):
    d=bpy.data.cameras.new(name);o=bpy.data.objects.new(name,d);COL.objects.link(o);o.location=loc;o.rotation_euler=(Vector(target)-o.location).to_track_quat('-Z','Y').to_euler();d.type='ORTHO';d.ortho_scale=ortho;return o
scene=bpy.context.scene
scene.camera=camera('Hero cutaway camera',(8,-11,8.3),(0,.05,1.03),8.8)
camera('Interior detail camera',(5,-8,5),(0,.5,1.1),6.9)
scene.render.engine='CYCLES';scene.cycles.samples=48;scene.cycles.use_denoising=True
scene.render.compositor_device='CPU'
scene.render.resolution_x=1600;scene.render.resolution_y=1440;scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG';scene.render.filepath=os.path.join(OUT,'bozzard_dorm_preview.png')
scene.view_settings.view_transform='AgX'
# Blender 5 compositor uses a node group rather than scene.node_tree.
ng=bpy.data.node_groups.new('Neon bloom compositor','CompositorNodeTree')
ng.interface.new_socket(name='Image',in_out='OUTPUT',socket_type='NodeSocketColor')
scene.compositing_node_group=ng
rl=ng.nodes.new('CompositorNodeRLayers');gl=ng.nodes.new('CompositorNodeGlare');gl.inputs['Type'].default_value='Fog Glow';gl.inputs['Quality'].default_value='High';gl.inputs['Strength'].default_value=.28
out=ng.nodes.new('NodeGroupOutput');ng.links.new(rl.outputs['Image'],gl.inputs['Image']);ng.links.new(gl.outputs['Image'],out.inputs['Image'])
# Open the saved file directly in the composed camera view.
for screen_ui in bpy.data.screens:
    for ar in screen_ui.areas:
        if ar.type=='VIEW_3D':ar.spaces.active.region_3d.view_perspective='CAMERA';ar.spaces.active.shading.type='MATERIAL'
bpy.ops.object.select_all(action='DESELECT')
scene['Design']='Bozzard / Room 207 — furnished university dorm'
scene['Contents']='Closed door, bed, skateboard, Bozzard neon sign, acoustic guitar, open laptop, laundry basket'
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT,'bozzard_dorm.blend'))
bpy.ops.render.render(write_still=True)
print('DORM_COMPLETE',len(bpy.data.objects),'objects')
