// Linear-color output, straight alpha. Bounds checks cover the final partial workgroup.
struct Params { time: f32, amplitude: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output);
    if any(id.xy >= size) { return; }
    let uv = (vec2f(id.xy) + 0.5) / vec2f(size);
    let drift = params.time * 0.7;
    let wave = sin(uv.x * 25.0 + drift + sin(uv.y * 14.0 - drift))
        + sin(uv.y * 38.0 - drift * 1.3 + sin(uv.x * 11.0 + drift));
    let light = smoothstep(0.3, 1.8, wave) * params.amplitude;
    let deep = vec3f(0.008, 0.055, 0.14);
    let shallow = vec3f(0.08, 0.62, 0.68);
    let color = mix(deep, shallow, light) + pow(max(wave * 0.5, 0.0), 12.0) * 0.2;
    textureStore(output, id.xy, vec4f(color, 1.0));
}
