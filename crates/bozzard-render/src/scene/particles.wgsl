struct ParticleUniform {
    view_projection: mat4x4<f32>, inverse_view_projection: mat4x4<f32>,
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient: vec4<f32>, viewport: vec4<f32>,
}
@group(0) @binding(0) var<uniform> object: ParticleUniform;
@group(0) @binding(1) var opaque_depth: texture_depth_2d;
struct ParticleVertex {
    @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) world:vec3<f32>,
    @location(2) color:vec4<f32>, @location(3) params:vec4<f32>,
}
@vertex fn vs_main(@builtin(vertex_index) index:u32,
    @location(0) position_size:vec4<f32>, @location(1) velocity_rotation:vec4<f32>,
    @location(2) color_opacity:vec4<f32>, @location(3) kind_soft_trail_seed:vec4<f32>) -> ParticleVertex {
    let corners=array<vec2<f32>,6>(vec2<f32>(-1,-1),vec2<f32>(1,-1),vec2<f32>(1,1),vec2<f32>(-1,-1),vec2<f32>(1,1),vec2<f32>(-1,1));
    let q=corners[index];let angle=velocity_rotation.w;
    let rotated=vec2<f32>(q.x*cos(angle)-q.y*sin(angle),q.x*sin(angle)+q.y*cos(angle));
    var world=position_size.xyz+(object.right.xyz*rotated.x+object.up.xyz*rotated.y)*position_size.w*0.5;
    if kind_soft_trail_seed.x>1.5 {
        let velocity=velocity_rotation.xyz;let speed=length(velocity);
        let axis=select(object.up.xyz,velocity/max(speed,0.00001),speed>0.0001);
        let cross_axis=cross(object.forward.xyz,axis);
        let right=select(object.right.xyz,cross_axis/max(length(cross_axis),0.00001),length(cross_axis)>0.001);
        world=position_size.xyz-velocity*kind_soft_trail_seed.z*0.5+right*q.x*position_size.w*0.5+axis*q.y*(position_size.w+speed*kind_soft_trail_seed.z)*0.5;
    } else if kind_soft_trail_seed.x>0.5 {
        world=position_size.xyz+(object.right.xyz*rotated.x+object.up.xyz*rotated.y*(0.2+0.8*abs(sin(angle*1.7))))*position_size.w*0.5;
    }
    return ParticleVertex(object.view_projection*vec4<f32>(world,1),q*0.5+0.5,world,color_opacity,kind_soft_trail_seed);
}
fn hash2(p:vec2<f32>)->f32 {return fract(sin(dot(p,vec2<f32>(127.1,311.7)))*43758.5453);}
fn noise(p:vec2<f32>)->f32 {
    let a=floor(p);let b=fract(p);let t=b*b*(3.0-2.0*b);
    return mix(mix(hash2(a),hash2(a+vec2<f32>(1,0)),t.x),mix(hash2(a+vec2<f32>(0,1)),hash2(a+vec2<f32>(1,1)),t.x),t.y);
}
fn world_at(uv:vec2<f32>,depth:f32)->vec3<f32> {
    let h=object.inverse_view_projection*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),depth,1);
    return h.xyz/select(0.000001,h.w,abs(h.w)>0.000001);
}
struct ParticleOutput { @location(0) color:vec4<f32>, @location(1) reactive:vec4<f32> }
@fragment fn fs_main(in:ParticleVertex)->ParticleOutput {
    let pixel=clamp(vec2<i32>(in.position.xy),vec2<i32>(0),vec2<i32>(textureDimensions(opaque_depth))-1);
    let z=textureLoad(opaque_depth,pixel,0);
    if in.position.z>=z {discard;}
    let uv=in.position.xy/object.viewport.xy;
    let surface=world_at(uv,z);
    let gap=dot(surface-in.world,object.forward.xyz);
    let soft=smoothstep(0.0,in.params.y,max(gap,0.0));
    let q=in.uv*2.0-1.0;
    var alpha=0.0;
    var color=in.color.rgb;
    if in.params.x<0.5 {
        let p=in.uv*4.0+in.params.w*27.0;
        let density=noise(p)*0.55+noise(p*2.1)*0.3+noise(p*4.2)*0.15;
        alpha=pow(max(1.0-dot(q,q),0.0),1.6)*smoothstep(0.12,0.78,density)*1.6;
    } else if in.params.x<1.5 {
        alpha=1.0-smoothstep(0.45,1.0,abs(q.x)+abs(q.y));
    } else {
        alpha=exp(-q.x*q.x*6.0)*smoothstep(0.0,0.5,in.uv.y)*(1.0-smoothstep(0.8,1.0,in.uv.y));
        color*=8.0;
    }
    if in.params.x<1.5 {
        var illumination=object.ambient.rgb;
        illumination+=object.sun_color.rgb*object.sun.w*0.12*sun_visibility(in.world,vec3<f32>(0));
        for(var i=0u;i<u32(local_lights.count.x);i++) {
            let light=local_lights.lights[i];let offset=light.position_range.xyz-in.world;
            // Finite scattering keeps billboards near the point source from
            // turning into emissive discs as inverse-square radiance rises.
            let radiance=local_radiance(light,offset);
            illumination+=(radiance/(vec3<f32>(1.0)+radiance/12.0))*0.035*local_visibility(light,in.world,vec3<f32>(0));
        }
        color*=illumination;
    }
    alpha*=in.color.a*soft;
    if alpha<0.001 {discard;}
    return ParticleOutput(vec4<f32>(min(color,vec3<f32>(60000)),clamp(alpha,0.0,1.0)),vec4<f32>(0,0,0,clamp(alpha*3.0,0.0,1.0)));
}
