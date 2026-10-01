struct FrameUniform {
    view_projection: mat4x4<f32>,
    previous_view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    viewport: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient_color: vec4<f32>,
    fog_color: vec4<f32>, fog_density: vec4<f32>, fog_height: vec4<f32>,
    misc: vec4<f32>, // x = elapsed seconds for shader graphs
};
@group(0) @binding(11) var<uniform> frame: FrameUniform;
