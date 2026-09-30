"""Validate exported ship halves, station footprint and scene attachment points."""
import json
import math
from pathlib import Path

from validate_machine_expansion import read_glb, values

ROOT=Path(__file__).resolve().parents[3]
ASSETS=ROOT / "examples/earth-factory/scenes/assets"


def validate():
    manifest=json.loads((ROOT / "assets/travel-ship/manifest.json").read_text())
    entries=manifest["models"]
    assert {e["id"] for e in entries}=={"landing-station","survey-ship-lower","survey-ship-upper","survey-ship-exhaust"}
    assert len(entries)==4
    assert manifest["power_socket"]==[0,1.26,0]
    assert manifest["fuel_input_tile"]==[2,1]
    assert manifest["station_footprint"]==[4,4]
    shared={"Survey ship ivory armor","Scorched graphite interiors",
            "Survey ship gold identification","Survey ship cyan service lines"}
    for entry in entries:
        slug=entry["id"]
        doc,binary=read_glb(ASSETS / entry["mesh"])
        assert not any(k in doc for k in ["images","animations","skins","cameras"]),slug
        assert all("uri" not in b for b in doc["buffers"]),slug
        if slug!="survey-ship-exhaust":
            assert shared <= {m["name"] for m in doc["materials"]},slug
        points,triangles=[],0
        for mesh in doc["meshes"]:
            for primitive in mesh["primitives"]:
                assert primitive.get("mode",4)==4,slug
                vertices=values(doc,binary,primitive["attributes"]["POSITION"],3)
                normals=values(doc,binary,primitive["attributes"]["NORMAL"],3)
                indices=[v[0] for v in values(doc,binary,primitive["indices"],1)]
                assert len(indices)%3==0 and max(indices)<len(vertices),slug
                assert all(math.isfinite(v) for p in vertices for v in p),slug
                assert all(abs(sum(v*v for v in n)-1)<.001 for n in normals),slug
                for a,b,c in zip(indices[::3],indices[1::3],indices[2::3]):
                    ab=[vertices[b][i]-vertices[a][i] for i in range(3)]
                    ac=[vertices[c][i]-vertices[a][i] for i in range(3)]
                    cross=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]]
                    assert sum(v*v for v in cross)>1e-14,(slug,"degenerate triangle")
                triangles+=len(indices)//3
                points.extend(vertices)
        assert triangles==entry["triangles"]<6000,slug
        assert len(doc["meshes"])==entry["surfaces"]<=7,slug
        for node in doc["nodes"]:
            assert node.get("translation",[0,0,0])==[0,0,0],slug
            assert node.get("scale",[1,1,1])==[1,1,1],slug
            assert node.get("rotation",[0,0,0,1])==[0,0,0,1],slug
        low=[min(p[i] for p in points) for i in range(3)]
        high=[max(p[i] for p in points) for i in range(3)]
        assert all(abs(a-d)<.00001 for bound,declared in zip([low,high],entry["bounds"])
                   for a,d in zip(bound,declared)),slug
        if slug=="landing-station":
            assert min(low[0],low[2])>=-1.55 and max(high[0],high[2])<=2.55,slug
            assert abs(high[1]-manifest["power_socket"][1])<.001,slug
        elif slug!="survey-ship-exhaust":
            # Translated into the existing 4x4 platform, without a scale change.
            assert low[0]>=-1.4 and high[0]<=1.4 and low[2]>=-.9 and high[2]<=1.4,slug
            assert low[1]>=0,slug
        print(f"{slug}: {triangles} triangles; geometry, shared pivot and materials OK")
    scene=json.loads((ASSETS.parent / "earth.json").read_text())
    objects={o["id"]:o for o in scene["objects"]}
    for object_id,slug in [("site-0","landing-station"),("rocket-lower-0","survey-ship-lower"),
                           ("rocket-upper-0","survey-ship-upper"),("rocket-exhaust","survey-ship-exhaust")]:
        obj=objects[object_id]
        assert obj["drawable"]["mesh"]=={"asset":slug}
        assert (ASSETS.parent / scene["assets"][slug]["path"]).is_file(),slug
        assert obj["transform"]["scale"]==[1,1,1]
        if object_id!="site-0":
            assert obj["parent"]=="rocket-rig"
            assert obj["transform"]["translation"]==manifest["ship_position"]
        else:
            assert "parent" not in obj
    for i,pos in enumerate(manifest["navigation_lights"]):
        assert objects[f"rocket-light-{i}"]["transform"]["translation"]==pos
        assert objects[f"rocket-light-{i}"]["parent"]=="rocket-rig"
    print("Validated upright rocket, construction stages, stationary launch pad and flight effect")


if __name__=="__main__":
    validate()
