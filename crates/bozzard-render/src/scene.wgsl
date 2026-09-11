struct ObjectUniform {
    mvp: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    parameters: vec4<f32>, // UV scale, lighting enabled, alpha cutoff
    model: mat4x4<f32>, inverse_view_projection: mat4x4<f32>, viewport: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient_color: vec4<f32>,
    surface_factors: vec4<f32>,
    fog_color: vec4<f32>, fog_density: vec4<f32>, fog_height: vec4<f32>,
};
@group(0) @binding(0) var<uniform> object: ObjectUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = object.mvp * vec4<f32>(position, 1.0);
    out.normal = (object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.world = (object.model * vec4<f32>(position, 1.0)).xyz;
    out.uv = uv * object.parameters.xy;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(color_texture, color_sampler, in.uv);
    let alpha = texel.a * object.tint.a;
    if alpha <= 0.00001 || alpha < object.parameters.w { discard; }
    let base = texel.rgb * object.tint.rgb;
    if object.surface_factors.z > 0.5 {
        let effect = demo_effect(base, in.normal, in.uv, in.world);
        if object.surface_factors.z < 1.5 { return vec4<f32>(effect, alpha); }
        return vec4<f32>(apply_fog(effect, in.world, in.position.xy), alpha);
    }
    let diffuse = local_diffuse(in.world, normalize(in.normal)) + object.sun_color.w * object.ambient_color.rgb + gi_diffuse(in.world,normalize(in.normal))
        + object.sun_color.rgb * object.sun.w * max(dot(normalize(in.normal), object.sun.xyz), 0.0) / 3.14159265 * sun_visibility(in.world, normalize(in.normal));
    let light = mix(vec3<f32>(1.0), diffuse, object.parameters.z);
    return vec4<f32>(apply_fog(min(base * light, vec3<f32>(60000.0)), in.world, in.position.xy), alpha);
}
