struct EnvironmentUniform {
    zenith: vec4<f32>, horizon: vec4<f32>, ground: vec4<f32>, inverse_view_projection: mat4x4<f32>,
};
@group(3) @binding(0) var<uniform> environment: EnvironmentUniform;
@group(3) @binding(1) var irradiance_map: texture_cube<f32>;
@group(3) @binding(2) var reflection_map: texture_cube<f32>;
@group(3) @binding(3) var environment_sampler: sampler;
@group(3) @binding(4) var environment_brdf: texture_2d<f32>;
fn environment_color(weights: vec3<f32>) -> vec3<f32> {
    return (weights.x*environment.zenith.rgb + weights.y*environment.horizon.rgb + weights.z*environment.ground.rgb)*environment.zenith.w;
}
fn diffuse_environment(n: vec3<f32>) -> vec3<f32> {
    if environment.zenith.w == 0.0 { return vec3<f32>(0.0); }
    return environment_color(textureSampleLevel(irradiance_map,environment_sampler,n,0.0).rgb);
}
fn specular_environment(r: vec3<f32>, roughness: f32, nv: f32, f0: vec3<f32>) -> vec3<f32> {
    if environment.zenith.w == 0.0 { return vec3<f32>(0.0); }
    let weights = textureSampleLevel(reflection_map,environment_sampler,r,roughness*7.0).rgb;
    let brdf = textureSampleLevel(environment_brdf,environment_sampler,vec2<f32>(nv,roughness),0.0).rg;
    return environment_color(weights)*(f0*brdf.x+brdf.y);
}
