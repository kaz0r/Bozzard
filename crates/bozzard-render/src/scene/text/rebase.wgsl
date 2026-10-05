struct Metadata { items:array<vec4<u32>,1024> }
@group(0) @binding(0) var<storage,read_write> indices:array<u32>;
@group(0) @binding(1) var<uniform> metadata:Metadata;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let item=metadata.items[id.y];
    if id.x<item.y { indices[item.x+id.x]+=item.z; }
}
