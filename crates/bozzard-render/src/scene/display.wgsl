@group(0) @binding(0) var hdr_scene: texture_2d<f32>;
struct DisplaySettings {
    transform: vec4<f32>, // exposure, tone mapping, software sRGB, bloom intensity
    aa: vec4<f32>, // enabled, simulation time, padding
    grade: vec4<f32>, // temperature, tint, saturation, contrast
    lift: vec4<f32>,
    gamma: vec4<f32>,
    gain: vec4<f32>,
    vignette: vec4<f32>, // intensity, roundness, feather
    grain: vec4<f32>, // intensity, pixel size
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
    let balance = exp2(vec3<f32>(settings.grade.x*0.5 + settings.grade.y*0.15, -settings.grade.y*0.3, -settings.grade.x*0.5 + settings.grade.y*0.15));
    var color = radiance*settings.transform.x*balance;
    if settings.transform.y > 1.5 {
        // Bozzard filmic curve: soft toe/shoulder, normalized at scene-linear white 8.
        // Map luminance first to preserve bright fire/neon hue, then compress into gamut.
        let y = dot(color,vec3<f32>(0.2126,0.7152,0.0722));
        let mapped = clamp((y*(y+0.08))/(y*y+0.55*y+0.1)*1.05971535,0.0,1.0);
        color *= mapped/max(y,0.000001);
        let peak = max(color.r,max(color.g,color.b));
        if peak>1.0 { color = mix(vec3<f32>(mapped),color,(1.0-mapped)/max(peak-mapped,0.000001)); }
    } else if settings.transform.y > 0.5 { color = color / (vec3<f32>(1.0)+color); }
    color = max(color*settings.gain.rgb+settings.lift.rgb,vec3<f32>(0));
    // Bound before the maximum gamma exponent, even for an exposure-only HDR preview.
    if any(settings.gamma.rgb != vec3<f32>(1)) { color = pow(min(color,vec3<f32>(1e8)),vec3<f32>(1)/settings.gamma.rgb); }
    // Contrast around middle gray in log luminance preserves dark scene detail.
    if settings.grade.w != 1.0 { color = 0.18*pow(min(color/0.18,vec3<f32>(1e16)),vec3<f32>(settings.grade.w)); }
    let gray = dot(color,vec3<f32>(0.2126,0.7152,0.0722));
    return max(mix(vec3<f32>(gray),color,settings.grade.z),vec3<f32>(0));
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
fn hash(pixel: vec2<u32>, frame: u32) -> f32 {
    var h = pixel.x*374761393u + pixel.y*668265263u + frame*1442695041u;
    h = (h^(h>>13u))*1274126177u;
    return f32(h^(h>>16u))/4294967295.0;
}
fn from_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c+0.055)/1.055,vec3<f32>(2.4)),c/12.92,c<=vec3<f32>(0.04045));
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
    if settings.vignette.x > 0.0 {
        let aspect = f32(textureDimensions(hdr_scene).x)/f32(textureDimensions(hdr_scene).y);
        let shape = mix(vec2<f32>(1),vec2<f32>(aspect,1),settings.vignette.y);
        let radius = length((uv-0.5)*2.0*shape)/length(shape);
        let edge = smoothstep(1.0-settings.vignette.z,1.0,radius);
        color *= 1.0-settings.vignette.x*edge;
    }
    if settings.grain.x > 0.0 {
        var perceptual = clamp(srgb(color),vec3<f32>(0),vec3<f32>(1));
        let noise = hash(vec2<u32>(floor(position.xy/settings.grain.y)),u32(floor(settings.aa.y*24.0)))-0.5;
        let midtone = 4.0*perceptual*(vec3<f32>(1)-perceptual);
        perceptual = clamp(perceptual+noise*settings.grain.x*midtone,vec3<f32>(0),vec3<f32>(1));
        color = from_srgb(perceptual);
    }
    if settings.transform.z > 0.5 { color = srgb(color); }
    return vec4<f32>(color,sample.a);
}
