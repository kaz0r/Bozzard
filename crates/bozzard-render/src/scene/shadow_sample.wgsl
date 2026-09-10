struct ShadowUniform { matrix: mat4x4<f32>, settings: vec4<f32> };
@group(2) @binding(0) var<uniform> shadow: ShadowUniform;
@group(2) @binding(1) var shadow_map: texture_depth_2d;
@group(2) @binding(2) var shadow_sampler: sampler_comparison;
fn sun_visibility(world: vec3<f32>, geometric_normal: vec3<f32>) -> f32 {
    if shadow.settings.z < 0.5 { return 1.0; }
    let offset = geometric_normal * shadow.settings.y * (1.0 - max(dot(geometric_normal, object.sun.xyz), 0.0));
    let projected = shadow.matrix * vec4<f32>(world + offset, 1.0);
    let p = projected.xyz / projected.w;
    let uv = p.xy * vec2<f32>(0.5,-0.5) + vec2<f32>(0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || p.z < 0.0 || p.z > 1.0 { return 1.0; }
    var visibility = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            visibility += textureSampleCompareLevel(shadow_map, shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y))*shadow.settings.w, p.z-shadow.settings.x);
        }
    }
    return visibility / 9.0;
}
