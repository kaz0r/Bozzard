struct ObjectUniform {
    mvp: mat4x4<f32>, normal: mat4x4<f32>, tint: vec4<f32>, parameters: vec4<f32>,
    model: mat4x4<f32>, inverse_view_projection: mat4x4<f32>, viewport: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient_color: vec4<f32>,
    surface_factors: vec4<f32>,
};
struct MaterialUniform { factors: vec4<f32>, emissive: vec4<f32> };
@group(0) @binding(0) var<uniform> object: ObjectUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;
@group(1) @binding(0) var<uniform> material: MaterialUniform;
@group(1) @binding(1) var normal_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_sampler: sampler;
@group(1) @binding(3) var mr_texture: texture_2d<f32>;
@group(1) @binding(4) var mr_sampler: sampler;
@group(1) @binding(5) var ao_texture: texture_2d<f32>;
@group(1) @binding(6) var ao_sampler: sampler;
@group(1) @binding(7) var emissive_texture: texture_2d<f32>;
@group(1) @binding(8) var emissive_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>, @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>, @location(3) uv: vec2<f32>,
    @location(4) normal_uv: vec2<f32>, @location(5) mr_uv: vec2<f32>,
    @location(6) ao_uv: vec2<f32>, @location(7) emissive_uv: vec2<f32>,
};
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>, @location(4) normal_uv: vec2<f32>, @location(5) mr_uv: vec2<f32>,
    @location(6) ao_uv: vec2<f32>, @location(7) emissive_uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = object.mvp * vec4<f32>(position,1.0);
    out.world = (object.model * vec4<f32>(position,1.0)).xyz;
    out.normal = (object.normal * vec4<f32>(normal,0.0)).xyz;
    out.tangent = vec4<f32>((object.model * vec4<f32>(tangent.xyz,0.0)).xyz, tangent.w * object.viewport.z);
    out.uv = uv * object.parameters.xy;
    out.normal_uv = normal_uv * object.parameters.xy;
    out.mr_uv = mr_uv * object.parameters.xy;
    out.ao_uv = ao_uv * object.parameters.xy;
    out.emissive_uv = emissive_uv * object.parameters.xy;
    return out;
}
@fragment fn fs_main(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let texel = textureSample(color_texture,color_sampler,in.uv);
    let mr = textureSample(mr_texture,mr_sampler,in.mr_uv);
    let sampled_normal = textureSample(normal_texture,normal_sampler,in.normal_uv).xyz * 2.0 - 1.0;
    let ao = mix(1.0,textureSample(ao_texture,ao_sampler,in.ao_uv).r,material.factors.w);
    let emissive = textureSample(emissive_texture,emissive_sampler,in.emissive_uv).rgb * material.emissive.xyz;
    let facing = front == (object.viewport.z > 0.0);
    if !facing && material.emissive.w < 0.5 { discard; }
    let alpha = texel.a * object.tint.a;
    if alpha <= 0.00001 || alpha < object.parameters.w { discard; }
    let base = texel.rgb * object.tint.rgb;
    if object.parameters.z < 0.5 { return vec4<f32>(base,alpha); }
    var n = normalize(in.normal);
    let t = normalize(in.tangent.xyz - n * dot(n,in.tangent.xyz));
    let b = cross(n,t) * in.tangent.w;
    let mapped = normalize(vec3<f32>(sampled_normal.xy * material.factors.z, sampled_normal.z));
    n = normalize(mat3x3<f32>(t,b,n) * mapped);
    if !facing { n = -n; }
    // Unproject this pixel onto the near plane. The resulting view ray works for
    // both perspective and orthographic cameras, including editor navigation.
    let ndc = in.position.xy / object.viewport.xy * vec2<f32>(2.0,-2.0) + vec2<f32>(-1.0,1.0);
    let near = object.inverse_view_projection * vec4<f32>(ndc,0.0,1.0);
    let view_ray = near.xyz / near.w - in.world;
    let v = view_ray / max(length(view_ray),0.000001);
    let metallic = clamp(select(material.factors.x, object.surface_factors.x, object.surface_factors.x >= 0.0) * mr.b,0.0,1.0);
    let roughness = clamp(select(material.factors.y, object.surface_factors.y, object.surface_factors.y >= 0.0) * mr.g,0.045,1.0);
    let nv = max(dot(n,v),0.0001);
    let f0 = mix(vec3<f32>(0.04),base,metallic);
    let shadow_normal = normalize(in.normal) * select(-1.0, 1.0, facing);
    let visibility_sun = sun_visibility(in.world, shadow_normal);
    var direct = direct_brdf(base, metallic, roughness, n, v, object.sun.xyz) * object.sun.w * object.sun_color.rgb * visibility_sun;
    for (var i = 0u; i < u32(local_lights.count.x); i++) {
        let light = local_lights.lights[i];
        let offset = light.position_range.xyz - in.world;
        let l = offset / max(length(offset), 0.000001);
        direct += direct_brdf(base, metallic, roughness, n, v, l) * local_radiance(light, offset) * local_visibility(light, in.world, shadow_normal);
    }
    let ibl_diffuse = gi_diffuse(in.world,n)*base*(1.0-f0)*(1.0-metallic);
    let ibl_specular = specular_environment(reflect(-v,n),roughness,nv,f0);
    let indirect = base*(1.0-metallic)*object.sun_color.w*object.ambient_color.rgb*ao + (ibl_diffuse+ibl_specular)*ao;
    return vec4<f32>(min(direct+indirect+emissive, vec3<f32>(60000.0)),alpha);
}

fn direct_brdf(base: vec3<f32>, metallic: f32, roughness: f32, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let h = (v+l) / max(length(v+l),0.000001);
    let nl = max(dot(n,l),0.0);
    let nv = max(dot(n,v),0.0001);
    let nh = max(dot(n,h),0.0);
    let vh = max(dot(v,h),0.0);
    let a2 = pow(roughness,4.0);
    let denominator = nh*nh*(a2-1.0)+1.0;
    let distribution = a2 / max(3.14159265*denominator*denominator,0.000001);
    let visibility = 0.5 / max(nl*sqrt(nv*nv*(1.0-a2)+a2) + nv*sqrt(nl*nl*(1.0-a2)+a2),0.0001);
    let f0 = mix(vec3<f32>(0.04),base,metallic);
    let fresnel = f0 + (1.0-f0)*pow(1.0-vh,5.0);
    let diffuse = (1.0-fresnel)*(1.0-metallic)*base/3.14159265;
    return (diffuse + distribution*visibility*fresnel)*nl;
}
