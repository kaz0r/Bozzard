struct Settings { mvp: mat4x4<f32>, color: vec4<f32>, encoding: vec4<f32> }
@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(2) uv: vec2<f32>) -> Vertex {
    var out: Vertex;
    out.position = settings.mvp * vec4<f32>(position, 1.0);
    out.uv = uv;
    return out;
}
@fragment fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    let texel=textureSample(atlas,glyph_sampler,in.uv);
    var color = settings.color.rgb * texel.rgb;
    if settings.encoding.x > 0.5 {
        color = select(1.055 * pow(color, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * color, color <= vec3<f32>(0.0031308));
    }
    return vec4<f32>(color, settings.color.a * texel.a);
}
