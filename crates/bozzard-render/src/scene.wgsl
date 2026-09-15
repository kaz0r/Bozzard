struct ObjectUniform {
    mvp: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    parameters: vec4<f32>, // UV scale, lighting enabled, alpha cutoff
    model: mat4x4<f32>, inverse_view_projection: mat4x4<f32>, viewport: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient_color: vec4<f32>,
    surface_factors: vec4<f32>,
    fog_color: vec4<f32>, fog_density: vec4<f32>, fog_height: vec4<f32>,
    previous_mvp: mat4x4<f32>, misc: vec4<f32>, // x = elapsed seconds for shader graphs
};
@group(0) @binding(0) var<uniform> object: ObjectUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;
// Extra material maps; procedural meshes bind neutral placeholders so shader
// graphs sample the same five slots on every mesh flavor.
@group(0) @binding(3) var normal_texture: texture_2d<f32>;
@group(0) @binding(4) var normal_sampler: sampler;
@group(0) @binding(5) var mr_texture: texture_2d<f32>;
@group(0) @binding(6) var mr_sampler: sampler;
@group(0) @binding(7) var ao_texture: texture_2d<f32>;
@group(0) @binding(8) var ao_sampler: sampler;
@group(0) @binding(9) var emissive_texture: texture_2d<f32>;
@group(0) @binding(10) var emissive_sampler: sampler;

struct VertexOutput {
    @location(3) previous:vec4<f32>,
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>, @location(8) previous_position: vec3<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.previous=object.previous_mvp*vec4<f32>(previous_position,1.0);
    out.position = object.mvp * vec4<f32>(position, 1.0);
    out.normal = (object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.world = (object.model * vec4<f32>(position, 1.0)).xyz;
    out.uv = uv * object.parameters.xy;
    return out;
}

/// Surface parameters shared by stock materials and shader graph modules.
/// `normal` is the final world-space normal; shader graphs override it directly.
struct SurfaceParams {
    base: vec3<f32>, metallic: f32, roughness: f32,
    emissive: vec3<f32>, alpha: f32, normal: vec3<f32>, ao: f32,
};

fn default_material_surface(uv: vec2<f32>, normal_uv: vec2<f32>, mr_uv: vec2<f32>, ao_uv: vec2<f32>, emissive_uv: vec2<f32>, world_normal: vec3<f32>, tangent: vec4<f32>, world: vec3<f32>, view: vec3<f32>, front: bool, time: f32) -> SurfaceParams {
    let texel = textureSample(color_texture, color_sampler, uv);
    return SurfaceParams(
        texel.rgb * object.tint.rgb,
        clamp(object.surface_factors.x, 0.0, 1.0),
        clamp(select(1.0, object.surface_factors.y, object.surface_factors.y >= 0.0), 0.045, 1.0),
        vec3<f32>(0.0), texel.a * object.tint.a, world_normal, 1.0,
    );
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front: bool) -> SurfaceOutput {
    let ndc = in.position.xy / object.viewport.xy * vec2<f32>(2,-2) + vec2<f32>(-1,1);
    let near = object.inverse_view_projection * vec4<f32>(ndc, 0, 1);
    let view = normalize(near.xyz / near.w - in.world);
    let params = default_material_surface(in.uv, in.uv, in.uv, in.uv, in.uv, normalize(in.normal), vec4<f32>(0.0), in.world, view, front, object.misc.x);
    if params.alpha <= 0.00001 || params.alpha < object.parameters.w { discard; }
    let base = params.base;
    let alpha = params.alpha;
    if object.surface_factors.z > 0.5 {
        let effect = demo_effect(base, in.normal, in.uv, in.world);
        if object.surface_factors.z < 1.5 { return surface_output(vec4<f32>(effect, alpha),in.position,in.previous,in.normal,1.0,vec3<f32>(0),1.0,1.0); }
        return surface_output(vec4<f32>(apply_fog(effect, in.world, in.position.xy), alpha),in.position,in.previous,in.normal,1.0,vec3<f32>(0),1.0,1.0);
    }
    if object.parameters.z>0.5 && (object.surface_factors.y>=0.0 || object.surface_factors.x>=0.0) {
        let n=params.normal;
        let v=view;
        let roughness=params.roughness;let metallic=params.metallic;
        let f0=mix(vec3<f32>(0.04),base,metallic);
        var color=direct_brdf(base,metallic,roughness,n,v,object.sun.xyz)*object.sun.w*object.sun_color.rgb*sun_visibility(in.world,n);
        for(var i=0u;i<u32(local_lights.count.x);i++) {
            let light=local_lights.lights[i];let offset=light.position_range.xyz-in.world;
            color+=direct_brdf(base,metallic,roughness,n,v,local_direction(light,offset))*local_radiance(light,offset)*local_visibility(light,in.world,n);
        }
        color+=base*(1.0-metallic)*(object.sun_color.w*object.ambient_color.rgb+gi_diffuse(in.world,n)*(1.0-f0));
        color+=specular_environment(reflect(-v,n),roughness,max(dot(n,v),0.0001),f0);
        return surface_output(vec4<f32>(apply_fog(min(color,vec3<f32>(60000)),in.world,in.position.xy),alpha),in.position,in.previous,n,roughness,f0,1.0,0.0);
    }
    let diffuse = local_diffuse(in.world, params.normal) + object.sun_color.w * object.ambient_color.rgb + gi_diffuse(in.world,params.normal)
        + object.sun_color.rgb * object.sun.w * max(dot(params.normal, object.sun.xyz), 0.0) / 3.14159265 * sun_visibility(in.world, params.normal);
    let light = mix(vec3<f32>(1.0), diffuse, object.parameters.z);
    return surface_output(vec4<f32>(apply_fog(min(base * light + params.emissive, vec3<f32>(60000.0)), in.world, in.position.xy), alpha),in.position,in.previous,params.normal,1.0,vec3<f32>(0),1.0,0.0);
}


struct SurfaceOutput {
    @location(0) color:vec4<f32>,
    @location(1) normal_roughness:vec4<f32>,
    @location(2) motion_depth_reactive:vec4<f32>,
    @location(3) fresnel_occlusion:vec4<f32>,
}
fn surface_output(color:vec4<f32>,position:vec4<f32>,previous:vec4<f32>,normal:vec3<f32>,roughness:f32,f0:vec3<f32>,ao:f32,reactive:f32)->SurfaceOutput {
    let current_uv=position.xy/object.viewport.xy;
    let previous_ndc=previous.xyz/max(previous.w,0.000001);
    let previous_uv=previous_ndc.xy*vec2<f32>(0.5,-0.5)+0.5;
    let valid=previous.w>0.00001 && previous_ndc.z>=0.0 && previous_ndc.z<=1.0;
    return SurfaceOutput(color,vec4<f32>(normalize(normal),roughness),
        vec4<f32>(select(vec2<f32>(0),current_uv-previous_uv,valid),select(1.0,log2(max(1.0-previous_ndc.z,0.00000001)),valid),max(reactive,object.surface_factors.w)),vec4<f32>(f0,ao));
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
