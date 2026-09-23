"""Draw Bozz-torio's original 32px pixel-art sprites into a 64px atlas."""
from PIL import Image, ImageDraw
from pathlib import Path
from math import cos, sin, pi

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "sprites.png"
OUT.parent.mkdir(parents=True, exist_ok=True)
S = 32
atlas = Image.new("RGBA", (5*S*2, 4*S*2), (0,0,0,0))
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
    atlas.alpha_composite(img,((index%5)*S*2,(index//5)*S*2))

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

paint=[lambda d:ore(d,"#849cb3","#bdd1d8"),lambda d:ore(d,COPPER,GOLD),
       lambda d:ingot(d,BLUE,"#a6d1dc"),lambda d:ingot(d,COPPER,GOLD),gear,circuit,
       miner,furnace,assembler,belt,splitter,hub,
       lambda d:patch(d,"#8eafc2"),lambda d:patch(d,COPPER),
       lambda d:ground(d,"#314e4c"),lambda d:ground(d,"#385550")]
for i,fn in enumerate(paint): sprite(i,fn)
atlas.save(OUT)
print(f"Wrote {OUT} ({atlas.width} × {atlas.height}, {len(paint)} original sprites)")
