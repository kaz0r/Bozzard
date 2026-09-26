struct SkyVertex { @builtin(position) position: vec4<f32>, @location(0) ndc: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) i:u32) -> SkyVertex {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    var out: SkyVertex; out.position=vec4<f32>(p[i],1.0,1.0); out.ndc=p[i]; return out;
}
fn star_hash(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell,vec2<f32>(127.1,311.7)))*43758.5453);
}
fn stars(uv: vec2<f32>) -> vec3<f32> {
    let cell = floor(uv);
    let seed = star_hash(cell);
    let center = vec2<f32>(star_hash(cell+17.0),star_hash(cell+59.0))*0.6+0.2;
    let radius = mix(0.013,0.027,star_hash(cell+83.0));
    let aa = max(length(fwidth(uv))*0.5,0.001);
    let point = 1.0-smoothstep(radius-aa,radius+aa,length(fract(uv)-center));
    let brightness = select(0.0,mix(0.35,1.0,seed)*point,seed>0.45);
    return mix(vec3<f32>(0.62,0.76,1.0),vec3<f32>(1.0,0.91,0.75),star_hash(cell+101.0))*brightness;
}
@fragment fn fs_main(in: SkyVertex) -> @location(0) vec4<f32> {
    let near = environment.inverse_view_projection*vec4<f32>(in.ndc,0.0,1.0);
    let far = environment.inverse_view_projection*vec4<f32>(in.ndc,1.0,1.0);
    let d = normalize(far.xyz/far.w-near.xyz/near.w);
    let t = sqrt(abs(d.y));
    let weights = select(vec3<f32>(0.0,1.0-t,t),vec3<f32>(t,1.0-t,0.0),d.y>=0.0);
    var color = environment_color(weights);
    if environment.horizon.w > 0.0 {
        // Perspective skies follow the view direction. An orthographic camera has
        // parallel rays, so use a distant screen-aligned field that stays fixed on
        // pan/zoom instead of collapsing the entire sky onto a single star sample.
        var uv = vec2<f32>(atan2(d.z,d.x),asin(clamp(d.y,-1.0,1.0)))*120.0;
        if abs(near.w-far.w) < 0.00001 { uv = in.position.xy/52.0; }
        color += stars(uv)*environment.horizon.w;
    }
    return vec4<f32>(color,1.0);
}
