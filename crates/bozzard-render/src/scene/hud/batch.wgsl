struct Settings { mvp: mat4x4<f32>, color: vec4<f32>, encoding: vec4<f32> }
struct Uniforms { items: array<Settings,170> }
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) @interpolate(flat) object: u32 }
fn vertex(position:vec3<f32>, uv:vec2<f32>, object:u32)->Vertex {
    var out:Vertex;
    out.position=uniforms.items[object].mvp*vec4<f32>(position,1.0);
    out.uv=uv; out.object=object; return out;
}
@vertex fn vs_main(@location(0) position:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) object:u32)->Vertex { return vertex(position,uv,object); }
@vertex fn vs_single(@location(0) position:vec3<f32>, @location(2) uv:vec2<f32>)->Vertex { return vertex(position,uv,0u); }
@fragment fn fs_main(in:Vertex)->@location(0) vec4<f32> {
    let settings=uniforms.items[in.object];
    let texel=textureSample(atlas,glyph_sampler,in.uv);
    var coverage=texel.a;
    if settings.encoding.y>0.5 {
        let size=vec2<f32>(textureDimensions(atlas));
        let low=clamp(in.uv*size+vec2<f32>(0.5),vec2<f32>(0.0),vec2<f32>(1.0));
        let high=clamp((vec2<f32>(1.0)-in.uv)*size+vec2<f32>(0.5),vec2<f32>(0.0),vec2<f32>(1.0));
        coverage*=low.x*low.y*high.x*high.y;
    }
    var color=settings.color.rgb*texel.rgb;
    if settings.encoding.x>0.5 { color=select(1.055*pow(color,vec3<f32>(1.0/2.4))-0.055,12.92*color,color<=vec3<f32>(0.0031308)); }
    return vec4<f32>(color,settings.color.a*coverage);
}
