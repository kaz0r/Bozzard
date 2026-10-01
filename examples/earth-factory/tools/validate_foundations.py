"""Geometry, snapping, prefab and save-schema checks for the indoor factory kit."""
import json,math
from pathlib import Path
from validate_machine_expansion import read_glb,values
ROOT=Path(__file__).resolve().parents[3]
ASSETS=ROOT/'examples/earth-factory/scenes/assets'
def validate():
    manifest=json.loads((ROOT/'assets/foundations/manifest.json').read_text())
    assert len(manifest['models'])==11
    assert {m['kind'] for m in manifest['models']}==set(range(30,40))|{0}
    for entry in manifest['models']:
        doc,data=read_glb(ASSETS/entry['mesh']);points=[];triangles=0
        assert all('uri' not in b for b in doc['buffers']),entry['id']
        assert not any(k in doc for k in ['images','animations','skins','cameras'])
        for node in doc['nodes']:
            assert node.get('translation',[0,0,0])==[0,0,0]
            assert node.get('scale',[1,1,1])==[1,1,1]
            assert node.get('rotation',[0,0,0,1])==[0,0,0,1]
        for mesh in doc['meshes']:
            for p in mesh['primitives']:
                pos=values(doc,data,p['attributes']['POSITION'],3);normals=values(doc,data,p['attributes']['NORMAL'],3)
                idx=[v[0] for v in values(doc,data,p['indices'],1)]
                assert len(idx)%3==0 and max(idx)<len(pos)
                assert all(math.isfinite(v) for point in pos for v in point)
                assert all(abs(sum(v*v for v in n)-1)<.001 for n in normals)
                for a,b,c in zip(idx[::3],idx[1::3],idx[2::3]):
                    u=[pos[b][i]-pos[a][i] for i in range(3)];v=[pos[c][i]-pos[a][i] for i in range(3)]
                    cross=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]]
                    assert sum(x*x for x in cross)>1e-14,(entry['id'],'degenerate triangle')
                points+=pos;triangles+=len(idx)//3
        assert triangles==entry['triangles']<6000
        lo=[min(p[i] for p in points) for i in range(3)];hi=[max(p[i] for p in points) for i in range(3)]
        assert lo[0]>=-.5 and hi[0]<=.5 and lo[2]>=-.50001 and hi[2]<=.50001
        if entry['kind'] in (30,31):assert .08<hi[1]<.115,'floor must clear grass and stay below the cursor'
        if entry['kind'] in (36,37):assert lo[1]>2.85 and hi[1]<3.2
        if entry['kind'] in (32,33,34,35,38,39):assert lo[1]>=.07 and 2.95<hi[1]<=3.05
        prefab=json.loads((ASSETS/entry['prefab']).read_text());assert len(prefab['objects'])==1
        assert prefab['objects'][0]['id']==prefab['root']
        assert prefab['objects'][0]['transform']['translation']==[0,0,0]
        print(f"{entry['name']}: {triangles} triangles, edge alignment and grounded geometry OK")
    scene=json.loads((ASSETS.parent/'earth.json').read_text())
    assert all(e['id'] in scene['assets'] for e in manifest['models'])
    controller=next(o for o in scene['objects'] if o['id']=='controller')
    assert len(controller['blackboard'])<=64
    assert controller['blackboard']['cache_structures']['list']['capacity']==578
    view=next(o for o in scene['objects'] if o['id']=='architecture-view')
    assert len(view['blackboard'])<=64
    for field in view['blackboard'].values():
        if 'list' not in field:continue
        data=field['list'];assert 1<=data['capacity']<=1024 and len(data['values'])<=data['capacity']
        assert all(data['element'] in value for value in data['values']),'transient cache element type mismatch'
    print('Validated all 10 building pieces, door leaf, game assets and persistent structure pages')
if __name__=='__main__':validate()
