struct SkyVertex { @builtin(position) position: vec4<f32>, @location(0) ndc: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) i:u32) -> SkyVertex {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    var out: SkyVertex; out.position=vec4<f32>(p[i],1.0,1.0); out.ndc=p[i]; return out;
}
@fragment fn fs_main(in: SkyVertex) -> @location(0) vec4<f32> {
    let near = environment.inverse_view_projection*vec4<f32>(in.ndc,0.0,1.0);
    let far = environment.inverse_view_projection*vec4<f32>(in.ndc,1.0,1.0);
    let d = normalize(far.xyz/far.w-near.xyz/near.w);
    let t = sqrt(abs(d.y));
    let weights = select(vec3<f32>(0.0,1.0-t,t),vec3<f32>(t,1.0-t,0.0),d.y>=0.0);
    return vec4<f32>(environment_color(weights),1.0);
}
