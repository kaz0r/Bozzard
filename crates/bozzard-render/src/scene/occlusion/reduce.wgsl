@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var destination: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(destination);
    if any(id.xy >= size) { return; }
    let source_size = textureDimensions(source);
    let start = id.xy * 2u;
    var depth = 0.0;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let p = min(start + vec2<u32>(x,y), source_size - 1u);
            depth = max(depth, textureLoad(source, p, 0).x);
        }
    }
    textureStore(destination, id.xy, vec4<f32>(depth));
}
