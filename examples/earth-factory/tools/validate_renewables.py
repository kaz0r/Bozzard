"""Verify indexed geometry, tile footprints and registered renewable prefabs."""
import json, math
from pathlib import Path
from validate_machine_expansion import read_glb, values
ROOT=Path(__file__).resolve().parents[3]
ASSETS=ROOT/'examples/earth-factory/scenes/assets'

def main():
    models=json.loads((ROOT/'assets/renewables/manifest.json').read_text())['models']
    assert [m['kind'] for m in models]==[40,41,42]
    scene=json.loads((ASSETS.parent/'earth.json').read_text())
    for model in models:
        points=[];triangles=0;primitives=0
        parts=[(model['mesh'],[0,0,0],model['triangles'])]
        if model['kind']==42: parts.append((model['rotor_mesh'],model['rotor_pivot'],model['rotor_triangles']))
        for path,pivot,expected in parts:
            doc,data=read_glb(ASSETS/path);part_triangles=0
            assert not any(k in doc for k in ['images','animations','skins','cameras'])
            assert all('uri' not in b for b in doc['buffers'])
            for mesh in doc['meshes']:
                for primitive in mesh['primitives']:
                    primitives+=1
                    pos=values(doc,data,primitive['attributes']['POSITION'],3)
                    norms=values(doc,data,primitive['attributes']['NORMAL'],3)
                    idx=[v[0] for v in values(doc,data,primitive['indices'],1)]
                    assert len(idx)%3==0 and min(idx)>=0 and max(idx)<len(pos)
                    assert all(math.isfinite(x) for p in pos for x in p)
                    assert all(abs(sum(x*x for x in n)-1)<.001 for n in norms)
                    for a,b,c in zip(idx[::3],idx[1::3],idx[2::3]):
                        u=[pos[b][i]-pos[a][i] for i in range(3)];v=[pos[c][i]-pos[a][i] for i in range(3)]
                        cross=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]]
                        assert sum(x*x for x in cross)>1e-14,model['id']
                    points += [[p[i]+pivot[i] for i in range(3)] for p in pos]
                    part_triangles+=len(idx)//3
            assert part_triangles==expected
            triangles+=part_triangles
        assert triangles<1500 and primitives<=6
        lo=[min(p[i] for p in points) for i in range(3)];hi=[max(p[i] for p in points) for i in range(3)]
        assert lo[1]>=-.00001 and lo[0]>=-.5 and lo[2]>=-.5 and hi[2]<=.5
        assert hi[0]<= (1.5 if model['kind']==41 else .5)
        assert model['footprint']==([[0,0],[1,0]] if model['kind']==41 else [[0,0]])
        assert model['id'] in scene['assets']
        prefab=json.loads((ASSETS/model['prefab']).read_text())
        assert len(prefab['objects'])==(2 if model['kind']==42 else 1)
        assert prefab['objects'][0]['transform']['translation']==[0,0,0]
        if model['kind']==42:
            rotor=prefab['objects'][1]
            assert rotor['parent']=='root' and rotor['transform']['translation']==model['rotor_pivot']
            assert rotor['script_manager']['scripts'][0]['script']=='wind-rotor'
        print(f"{model['name']}: {triangles} indexed triangles, {primitives} material groups; footprint OK")

if __name__=='__main__':main()
