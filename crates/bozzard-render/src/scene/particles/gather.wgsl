struct Particle { position_size:vec4<f32>, velocity_rotation:vec4<f32>, color_opacity:vec4<f32>, kind_soft_trail_seed:vec4<f32> }
struct Record { depth:f32,index:u32,low:u32,high:u32 }
@group(0) @binding(0) var<storage,read> records:array<Record>;
@group(0) @binding(1) var<storage,read> unsorted:array<Particle>;
@group(0) @binding(2) var<storage,read_write> sorted:array<Particle>;
@group(0) @binding(3) var<uniform> counts:vec4<u32>;
@compute @workgroup_size(64) fn gather(@builtin(global_invocation_id) id:vec3<u32>){
    if id.x<counts.x {sorted[id.x]=unsorted[records[id.x].index];}
}
