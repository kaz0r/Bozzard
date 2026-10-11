// Trail Renderer ribbons: camera-facing strips built on the CPU, unlit, depth tested
// against the opaque scene and softened toward both long edges.
struct Camera {
    view_projection: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexIn {
    @location(0) position: vec3<f32>,
    // -1 on one long edge, +1 on the other.
    @location(1) edge: f32,
    @location(2) color: vec4<f32>,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) edge: f32,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.view_projection * vec4<f32>(input.position, 1.0);
    out.edge = input.edge;
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let soft = 1.0 - smoothstep(0.55, 1.0, abs(input.edge));
    return vec4<f32>(input.color.rgb, input.color.a * soft);
}
