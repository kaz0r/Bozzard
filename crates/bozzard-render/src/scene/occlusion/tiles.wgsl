@group(0) @binding(0) var source: texture_depth_2d;
@group(0) @binding(1) var destination: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(destination)) { return; }
    let source_size = textureDimensions(source);
    let start = id.xy * 8u;
    var depth = 0.0;
    for (var y = 0u; y < 8u; y++) {
        for (var x = 0u; x < 8u; x++) {
            let p = start + vec2<u32>(x,y);
            if any(p >= source_size) { depth = 1.0; }
            else { depth = max(depth, textureLoad(source, p, 0)); }
        }
    }
    textureStore(destination, id.xy, vec4<f32>(depth));
}
