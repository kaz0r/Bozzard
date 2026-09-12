// Single-scattering integration. Shared light/shadow declarations precede this module.
struct VolumeUniform {
    inverse_view_projection: mat4x4<f32>,
    medium: vec4<f32>, // density, base height, height falloff, start distance
    albedo_phase: vec4<f32>,
    transport: vec4<f32>, // max distance, noise amount, noise scale, light intensity
    wind_time: vec4<f32>,
    sun: vec4<f32>, // direction toward sun, illuminance
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    viewport: vec4<f32>, // width, height, steps, unused
}
@group(0) @binding(0) var hdr_source: texture_2d<f32>;
@group(0) @binding(1) var geometry_depth: texture_depth_2d;
// The shared surface-shadow helper refers to object.sun. Only our volume helpers are used.
@group(0) @binding(2) var<uniform> object: VolumeUniform;
@group(0) @binding(3) var scattering: texture_2d<f32>;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));
    return vec4<f32>(positions[index],0,1);
}
fn volume_pixel(p: vec2<i32>) -> vec2<i32> {
    return clamp(p,vec2<i32>(0),vec2<i32>(textureDimensions(geometry_depth))-vec2<i32>(1));
}
fn volume_depth(p: vec2<i32>) -> f32 { return textureLoad(geometry_depth,volume_pixel(p),0); }
fn volume_world(p: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(volume_pixel(p))+0.5)/vec2<f32>(textureDimensions(geometry_depth));
    let h = object.inverse_view_projection*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),depth,1);
    return h.xyz / select(0.000001,h.w,abs(h.w)>0.000001);
}
fn ray_length(p: vec2<i32>) -> f32 {
    return min(length(volume_world(p,volume_depth(p))-volume_world(p,0.0)),object.transport.x);
}
// Conservative depth selection prevents half-resolution rays marching behind thin foregrounds.
fn trace_pixel(half_pixel: vec2<i32>) -> vec2<i32> {
    let first = volume_pixel(half_pixel*2);
    var chosen = first;
    var nearest = volume_depth(first);
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let p = volume_pixel(first+vec2<i32>(x,y));
        let depth = volume_depth(p);
        if depth < nearest { nearest = depth; chosen = p; }
    } }
    return chosen;
}
fn volume_hash(p: vec3<i32>) -> f32 {
    let v = bitcast<vec3<u32>>(p);
    var h = v.x*374761393u + v.y*668265263u + v.z*1442695041u;
    h = (h^(h>>13u))*1274126177u;
    return f32(h^(h>>16u))/4294967295.0;
}
fn value_noise(p: vec3<f32>) -> f32 {
    let cell = vec3<i32>(floor(p));
    let f = fract(p);
    let t = f*f*(vec3<f32>(3)-2.0*f);
    let a = mix(volume_hash(cell),volume_hash(cell+vec3<i32>(1,0,0)),t.x);
    let b = mix(volume_hash(cell+vec3<i32>(0,1,0)),volume_hash(cell+vec3<i32>(1,1,0)),t.x);
    let c = mix(volume_hash(cell+vec3<i32>(0,0,1)),volume_hash(cell+vec3<i32>(1,0,1)),t.x);
    let d = mix(volume_hash(cell+vec3<i32>(0,1,1)),volume_hash(cell+vec3<i32>(1,1,1)),t.x);
    return mix(mix(a,b,t.y),mix(c,d,t.y),t.z);
}
fn fog_density(world: vec3<f32>) -> f32 {
    var density = object.medium.x*exp(-object.medium.z*max(world.y-object.medium.y,0.0));
    if object.transport.y>0.0 {
        let p = (world-object.wind_time.xyz*object.wind_time.w)*object.transport.z;
        let noise = value_noise(p)*0.65+value_noise(p*2.07+vec3<f32>(7.1,3.7,1.3))*0.35;
        density *= mix(1.0,0.1+1.8*noise,object.transport.y);
    }
    return density;
}
fn phase(cosine: f32) -> f32 {
    let g = object.albedo_phase.w;
    let d = max(1.0+g*g-2.0*g*cosine,0.04);
    return (1.0-g*g)/(12.566370614*d*sqrt(d));
}
fn volume_sun_visibility(world: vec3<f32>) -> f32 {
    if shadow.settings.z<0.5 { return 1.0; }
    let h = shadow.matrix*vec4<f32>(world,1);
    let p = h.xyz/h.w;
    let uv = p.xy*vec2<f32>(0.5,-0.5)+0.5;
    if any(uv<vec2<f32>(0)) || any(uv>vec2<f32>(1)) || p.z<0.0 || p.z>1.0 { return 1.0; }
    return textureSampleCompareLevel(shadow_map,shadow_sampler,uv,p.z-shadow.settings.x);
}
fn volume_local_sample(maps: texture_depth_2d_array, map: ShadowUniform, slot: u32, world: vec3<f32>) -> f32 {
    let h = map.matrix*vec4<f32>(world,1);
    if h.w<=0.0 { return 1.0; }
    let p = h.xyz/h.w;
    let uv = p.xy*vec2<f32>(0.5,-0.5)+0.5;
    if any(uv<vec2<f32>(0)) || any(uv>vec2<f32>(1)) || p.z<0.0 || p.z>1.0 { return 1.0; }
    return textureSampleCompareLevel(maps,shadow_sampler,uv,i32(slot),p.z);
}
fn volume_local_visibility(light: LocalLight, world: vec3<f32>) -> f32 {
    if light.cone.z<0.5 { return 1.0; }
    let slot = u32(light.cone.z)-1u;
    let offset = light.position_range.xyz-world;
    let direction = offset/max(length(offset),0.000001);
    if light.cone.y>0.5 {
        let map = spot_shadows.maps[slot];
        return volume_local_sample(spot_shadow_maps,map,slot,world+direction*map.settings.x);
    }
    let base = slot*6u;
    let biased = world+direction*point_shadows.maps[base].settings.x;
    let face = base+point_shadow_face(biased-light.position_range.xyz);
    return volume_local_sample(point_shadow_maps,point_shadows.maps[face],face,biased);
}
fn illumination(world: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    var light = object.ambient.rgb;
    if object.sun.w>0.0 {
        light += object.sun_color.rgb*object.sun.w*phase(dot(ray,object.sun.xyz))*volume_sun_visibility(world);
    }
    for(var i=0u;i<u32(local_lights.count.x);i++) {
        let local = local_lights.lights[i];
        let offset = local.position_range.xyz-world;
        let distance2 = dot(offset,offset);
        if local.cone.y<1.5 && distance2>=local.position_range.w*local.position_range.w { continue; }
        var radiance = local_radiance(local,offset);
        if max(radiance.r,max(radiance.g,radiance.b))<=0.0 { continue; }
        // A finite 0.75-world-unit emitter avoids singular glints when a ray crosses a point light.
        if local.cone.y<1.5 { radiance *= max(distance2,0.0001)/max(distance2,0.5625); }
        light += radiance*phase(dot(ray,local_direction(local,offset)))*volume_local_visibility(local,world);
    }
    return light*object.transport.w;
}
fn trace_ray(pixel: vec2<i32>) -> vec4<f32> {
    let origin = volume_world(pixel,0.0);
    let delta = volume_world(pixel,0.9999)-origin;
    let ray = delta/max(length(delta),0.000001);
    let end = ray_length(pixel);
    let start = min(object.medium.w,end);
    let step_length = (end-start)/object.viewport.z;
    if step_length<=0.0 { return vec4<f32>(0,0,0,1); }
    let jitter = 0.15+0.7*volume_hash(vec3<i32>(pixel,0));
    var transmittance = 1.0;
    var light = vec3<f32>(0);
    for(var i=0u;i<u32(object.viewport.z);i++) {
        let p = origin+ray*(start+(f32(i)+jitter)*step_length);
        let density = fog_density(p);
        let step_transmittance = exp(-density*step_length);
        light += transmittance*(1.0-step_transmittance)*object.albedo_phase.rgb*illumination(p,ray);
        transmittance *= step_transmittance;
    }
    return vec4<f32>(min(light,vec3<f32>(60000)),transmittance);
}
@fragment fn trace_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return trace_ray(trace_pixel(vec2<i32>(frag.xy)));
}
@fragment fn composite_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(frag.xy);
    let end = ray_length(pixel);
    let p = (frag.xy-0.5)*0.5;
    let base = vec2<i32>(floor(p));
    let fraction = fract(p);
    var sum = vec4<f32>(0);
    var total = 0.0;
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let q = clamp(base+vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(textureDimensions(scattering))-vec2<i32>(1));
        let sample_end = ray_length(trace_pixel(q));
        let spatial = select(1.0-fraction.x,fraction.x,x==1)*select(1.0-fraction.y,fraction.y,y==1);
        let bilateral = exp(-abs(end-sample_end)/max(0.05,end*0.025));
        let weight = spatial*bilateral;
        sum += textureLoad(scattering,q,0)*weight;
        total += weight;
    } }
    // A thin silhouette may leave no representative half-resolution ray. Trace only
    // those pixels at full resolution to avoid both foreground leaks and dark outlines.
    var fog = sum/max(total,0.00001);
    if total<=0.00001 { fog = trace_ray(pixel); }
    let source = textureLoad(hdr_source,pixel,0);
    return vec4<f32>(min(max(source.rgb*fog.a+fog.rgb,vec3<f32>(0)),vec3<f32>(60000)),source.a);
}
