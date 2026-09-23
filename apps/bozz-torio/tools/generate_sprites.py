"""Draw Bozz-torio's 100 original 32px pixel-art sprites into a 64px atlas."""
from PIL import Image, ImageDraw
from pathlib import Path
from math import cos, sin, pi
import json

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "sprites.png"
OUT.parent.mkdir(parents=True, exist_ok=True)
S = 32
COLS = 10
ROWS = 10
atlas = Image.new("RGBA", (COLS*S*2, ROWS*S*2), (0,0,0,0))
INK = "#1b2832"
SHADE = "#3d4d57"
CREAM = "#f5e7c5"
COPPER = "#d97849"
GOLD = "#edba69"
CYAN = "#5ed4cb"
RED = "#ee866b"
BLUE = "#668eac"

def sprite(index, paint):
    img=Image.new("RGBA",(S,S),(0,0,0,0)); d=ImageDraw.Draw(img)
    paint(d)
    img=img.resize((S*2,S*2),Image.Resampling.NEAREST)
    atlas.alpha_composite(img,((index%COLS)*S*2,(index//COLS)*S*2))

def polygon(d,coords,fill,outline=INK,width=1):
    d.polygon(coords,fill=fill)
    d.line(coords+[coords[0]],fill=outline,width=width,joint="curve")

def ore(d,color,highlight):
    polygon(d,[(4,24),(8,14),(14,9),(21,11),(28,19),(24,27),(12,29)],SHADE)
    polygon(d,[(6,22),(12,14),(18,13),(19,22),(13,26)],color)
    polygon(d,[(19,13),(24,17),(26,22),(20,23)],highlight)
    d.line([(10,18),(13,16),(16,17)],fill=CREAM,width=1)

def ingot(d,base,top):
    polygon(d,[(5,22),(9,13),(24,13),(28,22),(24,26),(8,26)],base)
    polygon(d,[(9,13),(24,13),(27,20),(6,20)],top)
    d.line([(9,17),(22,17)],fill=CREAM,width=1)
    d.line([(9,23),(24,23)],fill=INK,width=1)

def gear(d):
    pts=[]
    for i in range(32):
        a=2*pi*i/32-pi/2
        r=13 if i%4 in (0,1) else 10
        pts.append((16+int(r*cos(a)),16+int(r*sin(a))))
    polygon(d,pts,GOLD)
    d.ellipse((11,11,21,21),fill=SHADE,outline=INK,width=2)
    d.ellipse((14,14,18,18),fill=CREAM)

def circuit(d):
    d.rounded_rectangle((5,5,27,27),radius=2,fill="#3b917c",outline=INK,width=2)
    d.rectangle((11,11,21,21),fill="#284e60",outline=CYAN)
    for y in (9,16,23):
        d.line([(3,y),(9,y),(11,12 if y<16 else 20)],fill=GOLD,width=2)
        d.line([(22,12 if y<16 else 20),(24,y),(29,y)],fill=GOLD,width=2)
    d.rectangle((13,13,17,17),fill=CYAN)

def chassis(d,fill):
    d.rounded_rectangle((3,5,29,28),radius=3,fill=fill,outline=INK,width=2)
    d.rectangle((5,25,27,28),fill=SHADE,outline=INK)
    d.rectangle((7,7,25,22),fill="#293d47",outline=INK)

def miner(d):
    chassis(d,BLUE)
    polygon(d,[(9,13),(21,13),(18,20),(12,20)],GOLD)
    d.rectangle((13,9,17,13),fill=CREAM,outline=INK)
    d.line([(9,23),(24,23)],fill=CYAN,width=2)
    d.rectangle((4,5,8,9),fill=RED,outline=INK)

def furnace(d):
    chassis(d,COPPER)
    polygon(d,[(11,21),(10,17),(16,9),(21,16),(20,21)],GOLD)
    polygon(d,[(14,21),(13,18),(17,13),(18,21)],RED)
    d.rectangle((7,22,25,25),fill=INK)

def assembler(d):
    chassis(d,"#469887")
    d.ellipse((11,11,21,21),fill=SHADE,outline=GOLD,width=2)
    for x in (8,23):
        d.line([(x,9),(x,17),(14 if x==8 else 18,17)],fill=CYAN,width=3)
    d.ellipse((15,15,18,18),fill=CREAM)

def belt(d):
    d.rounded_rectangle((2,7,30,25),radius=3,fill=SHADE,outline=INK,width=2)
    for x in (6,13,20,27):
        d.line([(x,8),(x,24)],fill=BLUE,width=2)
    polygon(d,[(11,13),(20,13),(20,10),(27,16),(20,22),(20,19),(11,19)],GOLD)

def splitter(d):
    chassis(d,"#717b89")
    d.line([(7,17),(16,17),(23,10)],fill=GOLD,width=3)
    d.line([(16,17),(23,23)],fill=CYAN,width=3)
    polygon(d,[(20,10),(26,6),(25,13)],GOLD)
    polygon(d,[(20,22),(26,27),(26,20)],CYAN)

def hub(d):
    d.rounded_rectangle((2,2,30,30),radius=3,fill="#866ca1",outline=INK,width=2)
    d.rectangle((5,5,27,27),fill="#3b4457",outline=CREAM,width=2)
    d.polygon([(16,8),(24,16),(16,24),(8,16)],fill=GOLD)
    d.polygon([(16,11),(21,16),(16,21),(11,16)],fill="#552f68")

def patch(d,color):
    d.ellipse((4,5,29,28),fill="#354c47",outline=INK,width=2)
    for x,y,sz in ((8,12,6),(19,8,6),(15,19,8)):
        polygon(d,[(x,y+sz),(x+1,y+2),(x+sz,y),(x+sz+2,y+sz)],color)

def ground(d,fill):
    d.rectangle((0,0,31,31),fill=fill)
    d.line([(0,31),(31,31),(31,0)],fill="#263941",width=1)
    for x,y in ((7,7),(24,23),(18,4)):
        d.point((x,y),fill="#5a7771")

def coal(d):
    ore(d,"#363d4b","#6f7790")
    for x,y in ((11,16),(19,20),(22,14)):
        d.point((x,y),fill=CYAN)

def generator(d):
    chassis(d,"#ad7851")
    d.ellipse((8,9,24,24),fill="#2e5260",outline=GOLD,width=2)
    polygon(d,[(18,9),(12,17),(17,17),(14,23),(22,14),(17,14)],GOLD)
    d.rectangle((4,4,9,9),fill=COPPER,outline=INK)

def power_pole(d):
    d.ellipse((7,25,25,30),fill=SHADE,outline=INK,width=2)
    d.rectangle((14,7,18,27),fill="#6f8292",outline=INK)
    d.line([(6,10),(26,10)],fill=GOLD,width=3)
    for x in (7,24):
        d.ellipse((x-2,7,x+2,13),fill=CYAN,outline=INK)
    d.ellipse((12,2,20,9),fill=GOLD,outline=INK)

def lightning(d):
    polygon(d,[(17,2),(8,18),(15,18),(12,30),(25,12),(18,12)],GOLD)

def upgrade(d):
    d.rounded_rectangle((3,3,29,29),radius=4,fill=SHADE,outline=CYAN,width=2)
    polygon(d,[(16,5),(23,14),(19,14),(19,24),(13,24),(13,14),(9,14)],GOLD)

PALETTES=[
    ("#304d4b","#678f81","#253e40"), ("#395754","#83a38b","#2a4544"),
    ("#46584c","#9ea87a","#34483d"), ("#4b5147","#afa482","#393f3a"),
    ("#4b4542","#bd8f72","#393638"), ("#51433e","#cf9673","#3e3434"),
    ("#3b4c57","#79a1a5","#293e4b"), ("#46515d","#9aa5b6","#323d4b"),
]
def ground_variant(d,biome,detail):
    base,light,dark=PALETTES[biome]
    d.rectangle((0,0,31,31),fill=base)
    d.line([(0,31),(31,31),(31,0)],fill=dark,width=1)
    if detail in (0,4):
        for x,y in ((5,6),(21,19),(27,8)):
            d.point((x,y),fill=light)
    elif detail in (1,5):
        d.line([(5,22),(9,19),(12,21)],fill=light,width=1)
        d.line([(20,8),(23,6)],fill=light,width=1)
    elif detail in (2,6):
        d.ellipse((8,7,12,10),fill=dark,outline=light)
        d.point((25,23),fill=light)
    else:
        d.line([(6,5),(10,5),(11,7)],fill=light,width=1)
        d.line([(18,23),(23,24)],fill=dark,width=2)
    if detail >= 4:
        d.rectangle((15,14,16,15),fill=light)

def decoration(d,index):
    colors=[CYAN,GOLD,COPPER,BLUE,RED,CREAM]
    color=colors[index%len(colors)]
    if index%3==0:
        polygon(d,[(7,24),(11,9),(18,6),(25,22),(19,27)],SHADE)
        d.line([(14,11),(19,9)],fill=color,width=2)
    elif index%3==1:
        d.ellipse((5,8,26,28),fill="#35544e",outline=INK,width=2)
        for x,y in ((10,11),(17,8),(22,15)):
            d.line([(16,24),(x,y)],fill=color,width=2)
    else:
        d.rounded_rectangle((6,8,26,27),radius=3,fill=SHADE,outline=INK,width=2)
        d.rectangle((10,12,22,22),fill=color,outline=INK)

paint=[lambda d:ore(d,"#849cb3","#bdd1d8"),lambda d:ore(d,COPPER,GOLD),
       lambda d:ingot(d,BLUE,"#a6d1dc"),lambda d:ingot(d,COPPER,GOLD),gear,circuit,
       miner,furnace,assembler,belt,splitter,hub,
       lambda d:patch(d,"#8eafc2"),lambda d:patch(d,COPPER),
       lambda d:ground(d,"#314e4c"),lambda d:ground(d,"#385550")]
for i,fn in enumerate(paint): sprite(i,fn)
extras=[coal,lambda d:patch(d,"#798096"),generator,power_pole,lightning,upgrade]
for fn in extras:
    sprite(len(paint),fn)
    paint.append(fn)
for biome in range(8):
    for detail in range(8):
        fn=lambda d,b=biome,v=detail:ground_variant(d,b,v)
        sprite(len(paint),fn)
        paint.append(fn)
for variant in range(14):
    fn=lambda d,v=variant:decoration(d,v)
    sprite(len(paint),fn)
    paint.append(fn)
assert len(paint)==100
atlas.save(OUT)
# The authored UI Image for the conveyor chooses one of these four exact
# quarter-turns, keeping its preview aligned with the placed Sprite component.
conveyor = atlas.crop((9*S*2, 0, 10*S*2, S*2))
preview = Image.new("RGBA", (4*S*2, S*2), (0,0,0,0))
for index, angle in enumerate((90, 0, 270, 180)):  # North, East, South, West
    preview.alpha_composite(conveyor.rotate(angle), (index*S*2, 0))
preview.save(OUT.parent / "conveyor_preview.png")
names=["Iron ore","Copper ore","Iron ingot","Copper ingot","Gear","Circuit","Miner","Furnace","Assembler","Conveyor","Splitter","Delivery hub","Iron deposit","Copper deposit","Factory floor A","Factory floor B","Coal ore","Coal seam","Generator","Power pole","Electricity","Upgrade"]
names += [f"Terrain biome {b+1} variation {v+1}" for b in range(8) for v in range(8)]
names += [f"World decoration {v+1}" for v in range(14)]
(OUT.parent/"sprite_manifest.json").write_text(json.dumps({"columns":COLS,"rows":ROWS,"frame_size":64,"sprites":names},indent=2)+"\n")
print(f"Wrote {OUT} ({atlas.width} × {atlas.height}, {len(paint)} original sprites)")
