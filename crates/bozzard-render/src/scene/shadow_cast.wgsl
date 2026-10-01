struct ShadowUniform { matrix: mat4x4<f32>, settings: vec4<f32> };
@group(0) @binding(0) var<uniform> object: ObjectUniform;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;
@group(1) @binding(0) var<uniform> shadow: ShadowUniform;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = shadow.matrix * object.model * vec4<f32>(position,1.0);
    out.uv = uv * object.parameters.xy;
    return out;
}
@fragment fn fs_main(in: VertexOutput, @builtin(front_facing) front: bool) {
    let alpha = textureSample(color_texture,color_sampler,in.uv).a * object.tint.a;
    if alpha <= 0.00001 || alpha < object.parameters.w { discard; }
    if object.raster.y < 0.5 && front != (object.raster.x > 0.0) { discard; }
}
