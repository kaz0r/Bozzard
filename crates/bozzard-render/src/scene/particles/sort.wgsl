struct Record {depth:f32,index:u32,low:u32,high:u32}
@group(0) @binding(0) var<storage,read_write> records:array<Record>;
@group(0) @binding(1) var<uniform> stage:vec4<u32>;
fn before(a:Record,b:Record)->bool {
    return a.depth>b.depth || (a.depth==b.depth && (a.high<b.high || (a.high==b.high && a.low<b.low)));
}
@compute @workgroup_size(64) fn sort(@builtin(global_invocation_id) id:vec3<u32>) {
    let i=id.x;let other=i^stage.y;if i>=stage.z || other<=i || other>=stage.z {return;}
    let a=records[i];let b=records[other];
    let swap=select(before(a,b),before(b,a),(i&stage.x)==0u);
    if swap {records[i]=b;records[other]=a;}
}
