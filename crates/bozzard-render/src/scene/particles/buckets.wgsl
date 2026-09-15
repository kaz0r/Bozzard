struct Record { depth:f32,index:u32,low:u32,high:u32 }
@group(0) @binding(0) var<storage,read> records:array<Record>;
@group(0) @binding(1) var<storage,read> depths:array<f32>;
@group(0) @binding(2) var<storage,read_write> draws:array<vec4<u32>>;
@group(0) @binding(3) var<uniform> counts:vec4<u32>;
fn split(depth:f32)->u32 {
    var low=0u;var high=counts.x;
    while low<high {let middle=(low+high)/2u;if records[middle].depth>depth {low=middle+1u;}else{high=middle;}}
    return low;
}
@compute @workgroup_size(64) fn buckets(@builtin(global_invocation_id) id:vec3<u32>){
    let i=id.x;if i>counts.y{return;}
    var first=0u;if i>0u {first=split(depths[i-1u]);}
    var last=counts.x;if i<counts.y {last=split(depths[i]);}
    // first_vertex indexes a storage buffer in the vertex shader, requiring no optional indirect-first-instance feature.
    draws[i]=vec4<u32>((last-first)*6u,1u,first*6u,0u);
}
