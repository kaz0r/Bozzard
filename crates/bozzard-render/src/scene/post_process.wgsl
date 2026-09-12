// Depth-derived SSAO and emissive-driven heat shimmer, before bloom and display mapping.
struct Settings {
    inverse_view_projection: mat4x4<f32>,
    ao: vec4<f32>, // intensity, world radius, world bias, enabled
    heat: vec4<f32>, // displacement at 1080p, threshold, speed, rise
    frame: vec4<f32>, // time, width, height, unused
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var<uniform> settings: Settings;
@group(0) @binding(3) var linear_sampler: sampler;
@group(0) @binding(4) var occlusion: texture_2d<f32>;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));
    return vec4<f32>(p[index],0,1);
}
fn bounded_pixel(pixel: vec2<i32>) -> vec2<i32> {
    return clamp(pixel, vec2<i32>(0), vec2<i32>(textureDimensions(scene_depth))-vec2<i32>(1));
}
fn depth_at(pixel: vec2<i32>) -> f32 { return textureLoad(scene_depth,bounded_pixel(pixel),0); }
fn unproject(pixel: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(bounded_pixel(pixel))+0.5)/vec2<f32>(textureDimensions(scene_depth));
    let h = settings.inverse_view_projection*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),depth,1);
    return h.xyz / select(0.000001,h.w,abs(h.w)>0.000001);
}
fn world_at(pixel: vec2<i32>) -> vec3<f32> { return unproject(pixel,depth_at(pixel)); }
fn linear_depth(pixel: vec2<i32>) -> f32 { return min(length(world_at(pixel)-unproject(pixel,0.0)),60000.0); }
@fragment fn ao_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = bounded_pixel(vec2<i32>(frag.xy)*2);
    if depth_at(pixel) >= 0.999999 { return vec4<f32>(1,0,0,1); }
    let p = world_at(pixel);
    let left = p-world_at(pixel-vec2<i32>(1,0));
    let right = world_at(pixel+vec2<i32>(1,0))-p;
    let up = p-world_at(pixel-vec2<i32>(0,1));
    let down = world_at(pixel+vec2<i32>(0,1))-p;
    // Favor the derivative on the same surface to avoid silhouette halos.
    let dx = select(left,right,dot(right,right)<dot(left,left));
    let dy = select(up,down,dot(down,down)<dot(up,up));
    var normal = cross(dy,dx);
    let distance = linear_depth(pixel);
    if dot(normal,normal) < 1e-16 { return vec4<f32>(1,distance,0,1); }
    normal = normalize(normal);
    if dot(normal,unproject(pixel,0.0)-p)<0.0 { normal = -normal; }
    let radius = settings.ao.y;
    let pixel_radius = clamp(radius/max(max(length(dx),length(dy)),0.00001),2.0,96.0);
    var sum = 0.0;
    var count = 0.0;
    // Fixed spiral: no temporal noise, frame-history dependency, or camera-motion trails.
    for(var i=0; i<24; i++) {
        let angle = f32(i)*2.39996323;
        let r = sqrt((f32(i)+0.5)/24.0)*pixel_radius;
        let q = pixel+vec2<i32>(round(vec2<f32>(cos(angle),sin(angle))*r));
        if any(q < vec2<i32>(0)) || any(q >= vec2<i32>(textureDimensions(scene_depth))) { continue; }
        count += 1.0;
        if depth_at(q) >= 0.999999 { continue; }
        let delta = world_at(q)-p;
        let d = length(delta);
        if d < 0.00001 || d > radius { continue; }
        let horizon = max(dot(normal,delta)-settings.ao.z,0.0)/d;
        let falloff = 1.0-smoothstep(radius*0.25,radius,d);
        sum += horizon*falloff;
    }
    let ao = clamp(1.0-settings.ao.x*sum/max(count,1.0)*4.0,0.15,1.0);
    return vec4<f32>(ao,distance,0,1);
}
fn ao_at(pixel: vec2<i32>) -> f32 {
    if settings.ao.w < 0.5 || depth_at(pixel)>=0.999999 { return 1.0; }
    let p = vec2<f32>(pixel)*0.5;
    let base = vec2<i32>(floor(p));
    let fraction = fract(p);
    let depth = linear_depth(pixel);
    var total = 0.0;
    var weights = 0.0;
    for(var y=0; y<2; y++) { for(var x=0; x<2; x++) {
        let q = clamp(base+vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(textureDimensions(occlusion))-vec2<i32>(1));
        let sample = textureLoad(occlusion,q,0);
        let spatial = select(1.0-fraction.x,fraction.x,x==1)*select(1.0-fraction.y,fraction.y,y==1);
        let bilateral = exp(-abs(sample.g-depth)/max(settings.ao.y*0.2,0.001));
        let weight = spatial*bilateral;
        total += sample.r*weight;
        weights += weight;
    } }
    return select(1.0,total/max(weights,0.00001),weights>0.00001);
}
@fragment fn composite_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(frag.xy);
    let size = vec2<f32>(textureDimensions(source));
    let uv = frag.xy/size;
    var sample_uv = uv;
    if settings.heat.x > 0.0 {
        var mask = 0.0;
        let depth = depth_at(pixel);
        // Search below the fragment: hot emissive regions produce a rising plume.
        for(var i=0;i<6;i++) {
            let offset = f32(i)/5.0*settings.heat.w;
            let hot_uv = uv+vec2<f32>(0,offset);
            if hot_uv.y>1.0 { continue; }
            let hot = textureSampleLevel(source,linear_sampler,hot_uv,0.0).rgb;
            let brightness = max(hot.r,max(hot.g,hot.b));
            let hot_depth = depth_at(vec2<i32>(hot_uv*size));
            let visible = select(0.0,1.0,hot_depth<=depth+0.00001);
            let energy = smoothstep(settings.heat.y,settings.heat.y+max(settings.heat.y,1.0),brightness);
            mask = max(mask,energy*(1.0-f32(i)/7.0)*visible);
        }
        let t = settings.frame.x*settings.heat.z;
        let wave = vec2<f32>(sin(uv.y*190.0-t*5.0+sin(uv.x*93.0+t*1.7)),cos(uv.x*137.0+uv.y*71.0-t*3.0))*vec2<f32>(1,0.4);
        let candidate = clamp(uv+wave*mask*settings.heat.x*(size.y/1080.0)/size,vec2<f32>(0.5)/size,vec2<f32>(1)-vec2<f32>(0.5)/size);
        // Never smear a closer silhouette over background heat.
        let tolerance = max(0.00002,max(abs(depth_at(pixel+vec2<i32>(1,0))-depth),abs(depth_at(pixel+vec2<i32>(0,1))-depth))*2.0);
        if abs(depth_at(vec2<i32>(candidate*size))-depth)<=tolerance { sample_uv = candidate; }
    }
    let original = textureLoad(source,pixel,0);
    let color = textureSampleLevel(source,linear_sampler,sample_uv,0.0).rgb;
    let ao = ao_at(bounded_pixel(vec2<i32>(sample_uv*size)));
    // Preserve intense HDR highlights while grounding the shaded scene.
    let highlight = smoothstep(1.0,4.0,max(color.r,max(color.g,color.b)));
    return vec4<f32>(color*mix(ao,1.0,highlight),original.a);
}
