struct TemporalUniform {
    inverse_vp:mat4x4<f32>, previous_vp:mat4x4<f32>,
    frame:vec4<f32>, // width height history-weight valid
    blur:vec4<f32>, // maximum pixels, shutter time scale, samples, repeated
    jitter:vec4<f32>, // current - previous jitter in UV
    options:vec4<f32>, // volume/heat reactive floor
}
@group(0) @binding(0) var current:texture_2d<f32>;
@group(0) @binding(1) var scene_depth:texture_depth_2d;
@group(0) @binding(2) var motion:texture_2d<f32>;
@group(0) @binding(3) var normals:texture_2d<f32>;
@group(0) @binding(4) var history:texture_2d<f32>;
@group(0) @binding(5) var history_surface:texture_2d<f32>;
@group(0) @binding(6) var linear_sampler:sampler;
@group(0) @binding(7) var<uniform> settings:TemporalUniform;
@group(0) @binding(8) var tile_motion:texture_2d<f32>;
struct Vertex { @builtin(position) position:vec4<f32> }
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Vertex {
    let p=array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));
    return Vertex(vec4<f32>(p[index],0,1));
}
fn pixel(p:vec2<i32>)->vec2<i32> {return clamp(p,vec2<i32>(0),vec2<i32>(settings.frame.xy)-1);}
fn compress(c:vec3<f32>)->vec3<f32> {return c/(1.0+dot(c,vec3<f32>(0.2126,0.7152,0.0722)));}
fn expand(c:vec3<f32>)->vec3<f32> {return c/max(1.0-dot(c,vec3<f32>(0.2126,0.7152,0.0722)),0.00001);}
struct Resolve { @location(0) color:vec4<f32>, @location(1) surface:vec4<f32> }
@fragment fn fs_taa(in:Vertex)->Resolve {
    let p=pixel(vec2<i32>(in.position.xy));let uv=in.position.xy/settings.frame.xy;
    let source=textureLoad(current,p,0);let z=textureLoad(scene_depth,p,0);
    let n=textureLoad(normals,p,0).xyz;
    let metadata=vec4<f32>(n,log2(max(1.0-z,0.00000001)));
    if settings.blur.w>0.5 && settings.frame.w>0.5 {
        return Resolve(textureLoad(history,p,0),metadata);
    }
    var velocity=textureLoad(motion,p,0);
    var previous_uv=uv-velocity.xy;
    if z>=0.999999 {
        let far=settings.inverse_vp*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),0.9999,1);
        let projected=settings.previous_vp*vec4<f32>(far.xyz/far.w,1);
        previous_uv=projected.xy/max(projected.w,0.00001)*vec2<f32>(0.5,-0.5)+0.5;
        velocity.z=-26.575425; // log2(1e-8), the cleared background depth.
    }
    let inside=all(previous_uv>vec2<f32>(0))&&all(previous_uv<vec2<f32>(1));
    let old_surface=textureLoad(history_surface,pixel(vec2<i32>(previous_uv*settings.frame.xy)),0);
    let depth_valid=abs(old_surface.w-velocity.z)<0.06;
    let normal_valid=dot(n,old_surface.xyz)>0.5 || (z>=0.999999 && length(old_surface.xyz)<0.01);
    var weight=select(0.0,settings.frame.z,inside&&depth_valid&&normal_valid&&settings.frame.w>0.5);
    var lower=vec3<f32>(1e10);var upper=vec3<f32>(0);
    var mean=vec3<f32>(0);var square=vec3<f32>(0);var reactive=velocity.w;
    for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
        let q=pixel(p+vec2<i32>(x,y));let sample_color=compress(textureLoad(current,q,0).rgb);
        lower=min(lower,sample_color);upper=max(upper,sample_color);mean+=sample_color;square+=sample_color*sample_color;
        reactive=max(reactive,textureLoad(motion,q,0).w);
    }}
    mean/=9.0;let sigma=sqrt(max(square/9.0-mean*mean,vec3<f32>(0)));
    lower=max(lower,mean-sigma*1.5);upper=min(upper,mean+sigma*1.5);
    let previous=compress(textureSampleLevel(history,linear_sampler,previous_uv,0).rgb);
    let clipped=clamp(previous,lower,upper);let fresh=compress(source.rgb);
    weight*=1.0-max(reactive,settings.options.x);
    // Fast luminance changes (fire/animated lights) lose history independently
    // of geometric motion, limiting bright trails after the source disappears.
    weight*=1.0-clamp(length(previous-fresh)*2.0,0.0,0.85);
    return Resolve(vec4<f32>(min(expand(mix(fresh,clipped,weight)),vec3<f32>(60000)),source.a),metadata);
}
fn velocity_pixels(p:vec2<i32>)->vec2<f32> {
    let packed=textureLoad(motion,pixel(p),0);
    if packed.w>0.1{return vec2<f32>(0);}
    let vector=(packed.xy-settings.jitter.xy)*settings.frame.xy*settings.blur.y;
    return vector*min(1.0,settings.blur.x/max(length(vector),0.00001));
}
@fragment fn fs_tilemax(in:Vertex)->@location(0) vec4<f32> {
    let tile=vec2<i32>(in.position.xy)*16;var largest=vec2<f32>(0);
    for(var y=0;y<16;y++){for(var x=0;x<16;x++){
        let vector=velocity_pixels(tile+vec2<i32>(x,y));
        if dot(vector,vector)>dot(largest,largest){largest=vector;}
    }}
    return vec4<f32>(largest,0,0);
}
@fragment fn fs_motion(in:Vertex)->@location(0) vec4<f32> {
    let p=pixel(vec2<i32>(in.position.xy));let uv=in.position.xy/settings.frame.xy;
    let source=textureLoad(current,p,0);let z=textureLoad(scene_depth,p,0);
    let packed=textureLoad(motion,p,0);
    if settings.blur.y<=0.0 || packed.w>0.1 {return source;}
    let center_vector=velocity_pixels(p);var vector=center_vector;
    let tile=p/16;let tile_size=vec2<i32>(textureDimensions(tile_motion));
    let reach=i32(clamp(ceil(settings.blur.x/32.0),1.0,4.0));
    for(var y=-reach;y<=reach;y++){for(var x=-reach;x<=reach;x++){
        let candidate=textureLoad(tile_motion,clamp(tile+vec2<i32>(x,y),vec2<i32>(0),tile_size-1),0).xy;
        if dot(candidate,candidate)>dot(vector,vector){vector=candidate;}
    }}
    if length(vector)<0.5{return source;}
    let delta=vector/settings.frame.xy;
    let center_moving=length(center_vector)>0.5;
    var sum=source.rgb;var total=1.0;
    for(var i=0u;i<u32(settings.blur.z);i++) {
        let t=(f32(i)+0.5)/settings.blur.z-0.5;let sample_uv=uv+delta*t;
        if any(sample_uv<vec2<f32>(0))||any(sample_uv>vec2<f32>(1)) {continue;}
        let q=pixel(vec2<i32>(sample_uv*settings.frame.xy));let other_z=textureLoad(scene_depth,q,0);
        let other=textureLoad(motion,q,0);let other_vector=velocity_pixels(q);
        let displacement=(sample_uv-uv)*settings.frame.xy;
        let speed=max(length(other_vector),0.00001);let direction=other_vector/speed;
        let along=abs(dot(displacement,direction));
        let across=length(displacement-direction*dot(displacement,direction));
        let reaches=length(other_vector)>0.5 && along<=speed*0.5+0.5 && across<1.5;
        let nearer=other_z<=z+0.00001;
        var sample_color=source.rgb;
        // A nearer moving surface can sweep into the background. A stationary
        // foreground keeps its depth protection even when a fast mover is behind it.
        if other.w<0.1 && ((reaches&&nearer)||(center_moving&&other_z>=z-0.00001)) {
            sample_color=textureSampleLevel(current,linear_sampler,sample_uv,0).rgb;
        }
        let weight=1.0-abs(t);sum+=sample_color*weight;total+=weight;
    }
    return vec4<f32>(sum/total,source.a);
}
