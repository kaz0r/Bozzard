@group(0) @binding(0) var hdr_scene: texture_2d<f32>;
struct DisplaySettings {
    transform: vec4<f32>, // exposure, tone mapping, software sRGB, bloom intensity
    aa: vec4<f32>, // enabled (off for raw diagnostics), padding
}
@group(0) @binding(1) var<uniform> settings: DisplaySettings;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var bloom_sampler: sampler;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(p[index],0.0,1.0);
}
fn srgb(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055*pow(linear, vec3<f32>(1.0/2.4))-0.055, linear*12.92, linear <= vec3<f32>(0.0031308));
}
fn display_color(uv: vec2<f32>) -> vec3<f32> {
    var radiance = max(textureSampleLevel(hdr_scene,bloom_sampler,uv,0.0).rgb, vec3<f32>(0.0));
    if settings.transform.w > 0.0 {
        radiance += textureSampleLevel(bloom,bloom_sampler,uv,0.0).rgb*settings.transform.w;
    }
    var color = radiance*settings.transform.x;
    if settings.transform.y > 0.5 { color = color / (vec3<f32>(1.0)+color); }
    return color;
}
fn luma(color: vec3<f32>) -> f32 {
    // Perceptual contrast, independent of whether the output target encodes sRGB.
    return dot(srgb(color), vec3<f32>(0.299, 0.587, 0.114));
}
fn fxaa(uv: vec2<f32>, texel: vec2<f32>, center: vec3<f32>) -> vec3<f32> {
    let nw = luma(display_color(uv + vec2<f32>(-1.0,-1.0)*texel));
    let ne = luma(display_color(uv + vec2<f32>( 1.0,-1.0)*texel));
    let sw = luma(display_color(uv + vec2<f32>(-1.0, 1.0)*texel));
    let se = luma(display_color(uv + vec2<f32>( 1.0, 1.0)*texel));
    let m = luma(center);
    let low = min(m, min(min(nw, ne), min(sw, se)));
    let high = max(m, max(max(nw, ne), max(sw, se)));
    if high - low < max(0.0312, high*0.125) { return center; }
    var direction = vec2<f32>(-(nw + ne - sw - se), nw + sw - ne - se);
    let reduce = max((nw + ne + sw + se)*0.03125, 0.0078125);
    direction = clamp(direction/(min(abs(direction.x), abs(direction.y)) + reduce),
        vec2<f32>(-8.0), vec2<f32>(8.0))*texel;
    let a = 0.5*(display_color(uv - direction/6.0) + display_color(uv + direction/6.0));
    let b = a*0.5 + 0.25*(display_color(uv - direction*0.5) + display_color(uv + direction*0.5));
    let lb = luma(b);
    return select(b, a, lb < low || lb > high);
}
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = textureLoad(hdr_scene, vec2<i32>(position.xy),0);
    if settings.aa.x < 0.5 {
        // Preserve the original raw path: unfiltered, nonnegative linear RGB and original alpha.
        return vec4<f32>(max(sample.rgb, vec3<f32>(0.0)), sample.a);
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(hdr_scene));
    let uv = position.xy*texel;
    var color = fxaa(uv, texel, display_color(uv));
    if settings.transform.z > 0.5 { color = srgb(color); }
    return vec4<f32>(color,sample.a);
}
