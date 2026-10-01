// Shared layout for color and shadow passes, including their instance arrays.
struct ObjectUniform {
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    parameters: vec4<f32>, // UV scale, lighting enabled, alpha cutoff
    model: mat4x4<f32>,
    raster: vec4<f32>, // determinant sign, double-sided flag, light-mask halves
    surface_factors: vec4<f32>,
    previous_model: mat4x4<f32>,
};
