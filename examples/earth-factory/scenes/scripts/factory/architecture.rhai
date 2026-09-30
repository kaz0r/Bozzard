// Pure structure addressing and enclosure rules, shared by solo and host authority.
import "factory-grid" as grid;
import "factory-debris" as debris;
fn structure(kind) { kind>=30.0 && kind<=39.0 }
fn floor_kind(kind) { kind==30.0 || kind==31.0 }
fn roof_kind(kind) { kind==36.0 || kind==37.0 }
fn slug(kind) { ["concrete-floor","wood-floor","concrete-wall","brick-wall","metal-wall","wood-wall","concrete-roof","wood-roof","sliding-door","window"][kind.to_int()-30] }
fn key(x,z) { x.to_string()+","+z.to_string() }
fn edge_key(x,z,axis) { key(x,z)+","+axis.to_string() }
fn address(x,z,kind,direction) {
    let slot=if floor_kind(kind) {0}else if roof_kind(kind) {1}else{2+direction%2};
    if slot>=2 && direction==2 {x-=1;}
    if slot>=2 && direction==3 {z-=1;}
    let cx=floor((x.to_float()+7.0)/15.0).to_int();let cz=floor((z.to_float()+7.0)/15.0).to_int();
    #{region:grid::chunk_id(cx,cz),slot:grid::index(x-cx*15,z-cz*15)*4+slot,x:x,z:z,axis:slot-2}
}
fn page(pages,planet,region) { grid::unpack_numbers(pages[planet*289+region],900) }
fn value(pages,planet,x,z,kind,direction) {
    let a=address(x,z,kind,direction);
    if a.region<0 || a.region>=289 {return 0.0;}
    if pages[planet*289+a.region]=="" {return 0.0;}
    page(pages,planet,a.region)[a.slot]
}
fn barrier(pages,planet,x,z,direction) { value(pages,planet,x,z,32.0,direction) }
fn keep_clear(seed,planet,x,z,demonstration,developer) {
    (!demonstration && x>=-1 && x<=2 && z>=-1 && z<=2) ||
        (!demonstration && !developer && debris::blocked(seed,planet,x,z))
}
fn edit(pages,planet,x,z,kind,direction,remove,seed,demonstration,developer) {
    let a=address(x,z,kind,direction);
    let radius=if planet==1 {6}else{8};let limit=radius*15+7;
    if a.x.abs()>limit || a.z.abs()>limit || (developer && (a.x < -7 || a.z < -7 || a.x>22 || a.z>22)) {
        return #{ok:false,message:"Keep structures within the planet boundary.",region:0,slot:0,data:[]};
    }
    let data=page(pages,planet,a.region);
    let layer=a.slot%4;let error="";
    if remove {
        if data[a.slot]==0.0 {error="No structure on this floor, roof or edge.";}
        else if layer==0 && data[a.slot+1]!=0.0 {error="Remove the roof before its foundation.";}
        else {data[a.slot]=0.0;}
    } else if keep_clear(seed,planet,x,z,demonstration,developer) ||
        (layer>=2 && keep_clear(seed,planet,a.x+if a.axis==0 {1}else{0},a.z+if a.axis==1 {1}else{0},demonstration,developer)) ||
        keep_clear(seed,planet,a.x,a.z,demonstration,developer) {error="Keep the landing site and wreckage clear.";}
    else if data[a.slot]!=0.0 {error="Remove the existing piece with X first.";}
    else if layer==1 && data[a.slot-1]==0.0 {error="Place a foundation before its roof.";}
    else if layer>=2 && value(pages,planet,a.x,a.z,30.0,0)==0.0 &&
        value(pages,planet,a.x+if a.axis==0 {1}else{0},a.z+if a.axis==1 {1}else{0},30.0,0)==0.0 {error="Walls, doors and windows need an adjacent foundation.";}
    else {data[a.slot]=kind;}
    #{ok:error=="",message:error,region:a.region,slot:a.slot,data:data}
}
fn topology(pages,planet) {
    let floors=#{};let edges=#{};
    for region in 0..289 {
        if pages[planet*289+region]=="" {continue;}
        let data=page(pages,planet,region);
        let cx=region%17-8;let cz=region/17-8;
        for cell in 0..225 {
            let x=cx*15+grid::cell_x(cell);let z=cz*15+grid::cell_z(cell);
            if data[cell*4]>0.0 {floors[key(x,z)]=[x,z,data[cell*4+1],region,cell];}
            for axis in 0..2 {if data[cell*4+2+axis]>0.0 {edges[edge_key(x,z,axis)]=data[cell*4+2+axis];}}
        }
    }
    let membership=#{};let rooms=[];let visited=#{};
    for start in floors.keys() {
        if visited.contains(start) {continue;}
        let queue=[start];let group=[];let sealed=true;let n=0;
        visited[start]=true;
        while n<queue.len() {
            let at=floors[queue[n]];n+=1;group.push(at);
            if at[2]==0.0 {sealed=false;}
            for dir in 0..4 {
                let a=address(at[0],at[1],32.0,dir);
                if edges.contains(edge_key(a.x,a.z,a.axis)) {continue;}
                let next=key(at[0]+grid::step_x(dir),at[1]+grid::step_z(dir));
                if !floors.contains(next) {sealed=false;continue;}
                if !visited.contains(next) {visited[next]=true;queue.push(next);}
            }
        }
        let room=if sealed {rooms.len()+1}else{0};
        for at in group {membership[key(at[0],at[1])]=room;}
        if sealed {rooms.push(group);}
    }
    #{rooms:rooms,membership:membership,floors:floors,edges:edges}
}
