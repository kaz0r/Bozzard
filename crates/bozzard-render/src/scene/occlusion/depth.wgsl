// Only known fully opaque stock surfaces enter this pass. Mirrored one-sided
// surfaces use the same facing rule as the color shader.
struct Occluder { mvp: mat4x4<f32>, flags: vec4<f32>, }
@group(0) @binding(0) var<uniform> objects: array<Occluder, 32>;
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) flags: vec2<f32>,
}
@vertex fn vs_main(@location(0) position: vec3<f32>, @builtin(instance_index) index: u32) -> VertexOutput {
    var result: VertexOutput;
    result.position = objects[index].mvp * vec4<f32>(position, 1.0);
    result.flags = objects[index].flags.xy;
    return result;
}
@fragment fn fs_main(in: VertexOutput, @builtin(front_facing) front: bool) {
    if in.flags.x < 0.5 && front != (in.flags.y > 0.0) { discard; }
}
