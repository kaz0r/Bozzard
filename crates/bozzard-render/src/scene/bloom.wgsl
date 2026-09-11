@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: vec4<f32>;
@group(0) @binding(3) var low: texture_2d<f32>;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index:u32) -> Vertex {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    return Vertex(vec4<f32>(p[index],0.0,1.0),p[index]*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5));
}
fn filtered_source(uv:vec2<f32>) -> vec3<f32> {
    let step = 1.0/vec2<f32>(textureDimensions(source));
    // Normalized four bilinear taps cover a 2x2 reduction and avoid nearest-sample flicker.
    return (textureSampleLevel(source,linear_sampler,uv+step*vec2<f32>(-0.5,-0.5),0.0).rgb
        +textureSampleLevel(source,linear_sampler,uv+step*vec2<f32>(0.5,-0.5),0.0).rgb
        +textureSampleLevel(source,linear_sampler,uv+step*vec2<f32>(-0.5,0.5),0.0).rgb
        +textureSampleLevel(source,linear_sampler,uv+step*vec2<f32>(0.5,0.5),0.0).rgb)*0.25;
}
@fragment fn prefilter(in:Vertex) -> @location(0) vec4<f32> {
    let color = max(filtered_source(in.uv),vec3<f32>(0.0));
    let brightness = max(color.r,max(color.g,color.b));
    let knee = max(settings.y,0.00001);
    let soft = clamp(brightness-settings.x+knee,0.0,2.0*knee);
    let contribution = max(brightness-settings.x,soft*soft/(4.0*knee))/max(brightness,0.00001);
    return vec4<f32>(color*contribution,1.0);
}
@fragment fn downsample(in:Vertex) -> @location(0) vec4<f32> { return vec4<f32>(filtered_source(in.uv),1.0); }
@fragment fn upsample(in:Vertex) -> @location(0) vec4<f32> {
    let step = 1.0/vec2<f32>(textureDimensions(low));
    var blurred=vec3<f32>(0.0);
    for(var y=-1;y<=1;y++) { for(var x=-1;x<=1;x++) {
        let weight=select(1.0,2.0,x==0)*select(1.0,2.0,y==0);
        blurred += textureSampleLevel(low,linear_sampler,in.uv+vec2<f32>(f32(x),f32(y))*step,0.0).rgb*weight;
    } }
    let high=textureSampleLevel(source,linear_sampler,in.uv,0.0).rgb;
    // Convex combination preserves constant radiance regardless of pyramid depth.
    return vec4<f32>(mix(high,blurred/16.0,settings.z),1.0);
}
