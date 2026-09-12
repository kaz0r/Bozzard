struct ReflectionUniform {
    view_projection:mat4x4<f32>, inverse_view_projection:mat4x4<f32>,
    viewport:vec4<f32>, // width height cutoff strength
    trace:vec4<f32>, // steps maximum distance thickness
    fog_color:vec4<f32>,fog_density:vec4<f32>,fog_height:vec4<f32>,
}
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var scene_depth:texture_depth_2d;
@group(0) @binding(2) var normals:texture_2d<f32>;
@group(0) @binding(3) var fresnel:texture_2d<f32>;
@group(0) @binding(4) var source_sampler:sampler;
@group(0) @binding(5) var<uniform> object:ReflectionUniform;
struct Vertex { @builtin(position) position:vec4<f32> }
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Vertex {
    let p=array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));return Vertex(vec4<f32>(p[index],0,1));
}
fn pixel(uv:vec2<f32>)->vec2<i32> {return clamp(vec2<i32>(uv*object.viewport.xy),vec2<i32>(0),vec2<i32>(object.viewport.xy)-1);}
fn world_at(uv:vec2<f32>,z:f32)->vec3<f32> {
    let h=object.inverse_view_projection*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),z,1);return h.xyz/h.w;
}
fn gap_at(uv:vec2<f32>,world:vec3<f32>)->f32 {
    let near=world_at(uv,0.0);let surface=world_at(uv,textureLoad(scene_depth,pixel(uv),0));
    let direction=normalize(surface-near);return dot(world-surface,direction);
}
@fragment fn fs_main(in:Vertex)->@location(0) vec4<f32> {
    let uv=in.position.xy/object.viewport.xy;let p=pixel(uv);let base=textureLoad(source,p,0);
    let packed=textureLoad(normals,p,0);let material=textureLoad(fresnel,p,0);let z=textureLoad(scene_depth,p,0);
    if z>=0.999999 || length(packed.xyz)<0.5 || packed.w>=object.viewport.z || max(max(material.r,material.g),material.b)<0.001 {return base;}
    let world=world_at(uv,z);let n=normalize(packed.xyz);let v=normalize(world_at(uv,0.0)-world);let ray=reflect(-v,n);
    if dot(n,v)<=0.0 {return base;}
    let origin=world+n*max(0.015,object.trace.z*0.15);
    let a=object.view_projection*vec4<f32>(origin,1);
    var finish=origin+ray*object.trace.y;
    let far=object.view_projection*vec4<f32>(finish,1);
    if far.z<0.001 {finish=mix(origin,finish,clamp((a.z-0.001)/(a.z-far.z),0.0,1.0)*0.99);}
    let b=object.view_projection*vec4<f32>(finish,1);
    if a.w<=0.0 || b.w<=0.0 {return base;}
    let uv_a=a.xy/a.w*vec2<f32>(0.5,-0.5)+0.5;let uv_b=b.xy/b.w*vec2<f32>(0.5,-0.5)+0.5;
    let delta=(uv_b-uv_a)*object.viewport.xy;let pixels=max(abs(delta.x),abs(delta.y));
    if pixels<2.0 {return base;}
    let steps=min(object.trace.x,ceil(pixels));let start=2.0/pixels;
    let qa=origin/a.w;let qb=finish/b.w;let ka=1.0/a.w;let kb=1.0/b.w;
    var previous_t=0.0;var previous_gap=-1.0;var hit_uv=vec2<f32>(0);var hit_t=0.0;var found=false;
    for(var i=0u;i<u32(steps);i++) {
        let t=start+(1.0-start)*(f32(i)+0.5)/steps;let sample_uv=mix(uv_a,uv_b,t);
        if any(sample_uv<=vec2<f32>(0))||any(sample_uv>=vec2<f32>(1)){break;}
        let point=mix(qa,qb,t)/mix(ka,kb,t);let gap=gap_at(sample_uv,point);
        if gap>=0.0 && previous_gap<0.0 {
            var lo=previous_t;var hi=t;
            for(var j=0;j<6;j++) {
                let mid=(lo+hi)*0.5;let test_uv=mix(uv_a,uv_b,mid);let test_world=mix(qa,qb,mid)/mix(ka,kb,mid);
                if gap_at(test_uv,test_world)>0.0{hi=mid;}else{lo=mid;}
            }
            hit_t=hi;hit_uv=mix(uv_a,uv_b,hi);let hit_world=mix(qa,qb,hi)/mix(ka,kb,hi);
            let error=gap_at(hit_uv,hit_world);let hit_normal=textureLoad(normals,pixel(hit_uv),0).xyz;
            if error>=0.0 && error<object.trace.z && dot(hit_normal,ray)<-0.05 && distance(hit_world,world)>0.08 {found=true;break;}
        }
        previous_t=t;previous_gap=gap;
    }
    if !found {return base;}
    let edge=min(min(hit_uv.x,hit_uv.y),min(1.0-hit_uv.x,1.0-hit_uv.y));
    let confidence=smoothstep(0.0,0.08,edge)*(1.0-smoothstep(0.7,1.0,hit_t))*(1.0-smoothstep(object.viewport.z*0.7,object.viewport.z,packed.w))*object.viewport.w;
    let hit_depth=textureLoad(scene_depth,pixel(hit_uv),0);let encoded_depth=log2(max(1.0-hit_depth,0.00000001));
    var reflected=vec3<f32>(0);var total=0.0;
    let radius=packed.w*packed.w*18.0;
    for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){
        let sample_uv=clamp(hit_uv+vec2<f32>(f32(x),f32(y))*radius/object.viewport.xy,vec2<f32>(0),vec2<f32>(1));
        let depth=textureLoad(scene_depth,pixel(sample_uv),0);
        let weight=1.0-smoothstep(0.03,0.2,abs(log2(max(1.0-depth,0.00000001))-encoded_depth));
        reflected+=textureSampleLevel(source,source_sampler,sample_uv,0).rgb*weight;total+=weight;
    }}
    reflected/=max(total,0.001);
    let nv=max(dot(n,v),0.0001);let brdf=textureSampleLevel(environment_brdf,environment_sampler,vec2<f32>(nv,packed.w),0).rg;
    let reflection=reflected*(material.rgb*brdf.x+brdf.y);
    let fallback=specular_environment(ray,packed.w,nv,material.rgb);
    let transmission=apply_fog(vec3<f32>(1),world,in.position.xy)-apply_fog(vec3<f32>(0),world,in.position.xy);
    return vec4<f32>(clamp(base.rgb+(reflection-fallback)*confidence*material.a*transmission,vec3<f32>(0),vec3<f32>(60000)),base.a);
}
