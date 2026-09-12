struct Lens {
    inverse_view_projection: mat4x4<f32>,
    origin: vec4<f32>,
    forward: vec4<f32>,
    lens: vec4<f32>, // focus distance, half-resolution CoC scale, max radius, unused
}
@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var geometry_depth: texture_depth_2d;
@group(0) @binding(2) var<uniform> camera: Lens;
@group(0) @binding(3) var prefiltered: texture_2d<f32>;
@group(0) @binding(4) var far_field: texture_2d<f32>;
@group(0) @binding(5) var near_field: texture_2d<f32>;
@group(0) @binding(6) var linear_sampler: sampler;
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));
    return vec4<f32>(p[i],0,1);
}
fn pixel(p: vec2<i32>) -> vec2<i32> { return clamp(p,vec2<i32>(0),vec2<i32>(textureDimensions(hdr))-1); }
fn depth(p: vec2<i32>) -> f32 { return textureLoad(geometry_depth,pixel(p),0); }
fn coc(p: vec2<i32>) -> f32 {
    let uv = (vec2<f32>(pixel(p))+0.5)/vec2<f32>(textureDimensions(hdr));
    let h = camera.inverse_view_projection*vec4<f32>(uv*vec2<f32>(2,-2)+vec2<f32>(-1,1),depth(p),1);
    let world = h.xyz/select(0.000001,h.w,abs(h.w)>0.000001);
    let z = max(dot(world-camera.origin.xyz,camera.forward.xyz),0.001);
    return clamp((1.0-camera.lens.x/z)*camera.lens.y,-camera.lens.z,camera.lens.z);
}
// Keep the nearest layer at silhouettes; mixing the bright background into a
// foreground sample would spread it over a sharp object in the gather pass.
@fragment fn prefilter_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let base = vec2<i32>(frag.xy)*2;
    var nearest = base;
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let p = base+vec2<i32>(x,y);
        if depth(p)<depth(nearest) {nearest=p;}
    } }
    let circle = coc(nearest);
    var sum = vec3<f32>(0);
    var total = 0.0;
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let p = base+vec2<i32>(x,y);
        let weight = exp(-abs(coc(p)-circle)*4.0);
        sum += textureLoad(hdr,pixel(p),0).rgb*weight;
        total += weight;
    } }
    return vec4<f32>(sum/max(total,0.00001),circle);
}
struct Layers { @location(0) far: vec4<f32>, @location(1) near: vec4<f32> }
@fragment fn gather_main(@builtin(position) frag: vec4<f32>) -> Layers {
    let size = vec2<f32>(textureDimensions(prefiltered));
    let center = textureLoad(prefiltered,vec2<i32>(frag.xy),0);
    let radius = camera.lens.z;
    var far_sum = vec3<f32>(0);
    var far_weight = 0.0;
    var near_sum = vec3<f32>(0);
    var near_weight = 0.0;
    // Equal-area golden-angle disk. No animated jitter or temporal history.
    for(var i=0u;i<96u;i++) {
        let r = sqrt((f32(i)+0.5)/96.0)*radius;
        let angle = f32(i)*2.39996323;
        let offset = vec2<f32>(cos(angle),sin(angle))*r;
        let sample = textureSampleLevel(prefiltered,linear_sampler,(frag.xy+offset)/size,0.0);
        var far_radius = min(max(center.a,0.0),max(sample.a,0.0));
        // Reconstruct the visible background underneath a defocused foreground.
        // Otherwise blending a soft coverage mask over its original sharp color
        // leaves the object's center sharp even though its silhouette expands.
        if center.a < -0.5 {
            far_radius = select(0.0,-center.a,sample.a>center.a+0.5);
        }
        if far_radius>0.25 {
            let weight = (1.0-smoothstep(max(far_radius-0.5,0.0),far_radius+0.5,r))/max(far_radius*far_radius,1.0);
            far_sum += sample.rgb*weight;
            far_weight += weight;
        }
        // Foreground disks can cover background, but cannot cover a nearer surface.
        let near_radius = max(-sample.a,0.0);
        if near_radius>0.5 && sample.a<=center.a+0.5 {
            let weight = (1.0-smoothstep(max(near_radius-0.5,0.0),near_radius+0.5,r))/max(near_radius*near_radius,1.0);
            near_sum += sample.rgb*weight;
            near_weight += weight;
        }
    }
    let coverage = clamp(near_weight*radius*radius/96.0,0.0,1.0);
    let far_color = select(center.rgb,far_sum/max(far_weight,0.00001),far_weight>0.00001);
    let near_color = near_sum/max(near_weight,0.00001);
    return Layers(vec4<f32>(far_color,center.a),vec4<f32>(near_color*coverage,coverage));
}
@fragment fn composite_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let source = textureLoad(hdr,vec2<i32>(frag.xy),0);
    let circle = coc(vec2<i32>(frag.xy));
    let p = frag.xy*0.5-0.5;
    let base = vec2<i32>(floor(p));
    let fraction = fract(p);
    var far_sum = vec3<f32>(0);
    var far_weight = 0.0;
    var near_sum = vec4<f32>(0);
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let q = clamp(base+vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(textureDimensions(far_field))-1);
        let spatial = select(1.0-fraction.x,fraction.x,x==1)*select(1.0-fraction.y,fraction.y,y==1);
        let far = textureLoad(far_field,q,0);
        let weight = spatial*exp(-abs(circle-far.a)/max(0.25,abs(circle)*0.25));
        far_sum += far.rgb*weight;
        far_weight += weight;
        near_sum += textureLoad(near_field,q,0)*spatial;
    } }
    var color = source.rgb;
    if abs(circle)>0.25 && far_weight>0.00001 {
        color = mix(color,far_sum/far_weight,smoothstep(0.25,1.25,abs(circle)));
    }
    color = color*(1.0-near_sum.a)+near_sum.rgb;
    return vec4<f32>(min(max(color,vec3<f32>(0)),vec3<f32>(60000)),source.a);
}
