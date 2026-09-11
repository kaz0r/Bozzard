@group(0) @binding(0) var hdr_scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: vec4<f32>;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var bloom_sampler: sampler;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(p[index],0.0,1.0);
}
fn srgb(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055*pow(linear, vec3<f32>(1.0/2.4))-0.055, linear*12.92, linear <= vec3<f32>(0.0031308));
}
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = textureLoad(hdr_scene, vec2<i32>(position.xy),0);
    var radiance = max(sample.rgb, vec3<f32>(0.0));
    if settings.w > 0.0 {
        let uv = position.xy / vec2<f32>(textureDimensions(hdr_scene));
        radiance += textureSampleLevel(bloom,bloom_sampler,uv,0.0).rgb*settings.w;
    }
    var color = radiance*settings.x;
    if settings.y > 0.5 { color = color / (vec3<f32>(1.0)+color); }
    if settings.z > 0.5 { color = srgb(color); }
    return vec4<f32>(color,sample.a);
}
