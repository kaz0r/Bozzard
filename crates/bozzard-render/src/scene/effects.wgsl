// Shared by the basic and imported PBR pipelines. Effects replace the surface
// appearance, not its alpha/culling/depth behavior. IDs live in surface_factors.z.
fn demo_effect(base: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>, world: vec3<f32>) -> vec3<f32> {
    let n = normalize(normal);
    if object.surface_factors.z < 1.5 {
        return n * 0.5 + 0.5;
    }
    if object.surface_factors.z < 2.5 {
        let cell = floor(uv * 8.0);
        let odd = (cell.x + cell.y) - 2.0 * floor((cell.x + cell.y) * 0.5);
        return base * select(0.12, 1.0, odd < 1.0);
    }
    // ponytail: sun-only artistic bands; use the standard material for full PBR/local lighting.
    let amount = max(dot(n, object.sun.xyz), 0.0) * sun_visibility(world, n);
    let band = min(floor(amount * 3.0), 2.0) / 2.0;
    return base * (0.2 + 0.8 * band);
}
