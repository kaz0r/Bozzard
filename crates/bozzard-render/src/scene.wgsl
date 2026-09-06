struct ObjectUniform {
    mvp: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    parameters: vec4<f32>, // UV scale, lighting enabled, padding
};
@group(0) @binding(0) var<uniform> object: ObjectUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = object.mvp * vec4<f32>(position, 1.0);
    out.normal = (object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv * object.parameters.xy;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(color_texture, color_sampler, in.uv).rgb * object.tint.rgb;
    let diffuse = 0.3 + 0.7 * max(dot(normalize(in.normal), normalize(vec3<f32>(0.4, 0.8, 0.6))), 0.0);
    let light = mix(1.0, diffuse, object.parameters.z);
    return vec4<f32>(base * light, 1.0);
}
