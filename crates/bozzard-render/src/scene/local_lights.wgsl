struct LocalLight {
    position_range: vec4<f32>, color_intensity: vec4<f32>,
    direction_outer: vec4<f32>, cone: vec4<f32>,
};
struct LocalLights { count: vec4<f32>, lights: array<LocalLight, 32> };
@group(2) @binding(3) var<uniform> local_lights: LocalLights;

// cone.y: 0 = point, 1 = spot, 2 = directional.
fn local_direction(light: LocalLight, offset: vec3<f32>) -> vec3<f32> {
    if light.cone.y > 1.5 { return -light.direction_outer.xyz; }
    return offset / max(length(offset), 0.000001);
}
fn local_radiance(light: LocalLight, offset: vec3<f32>) -> vec3<f32> {
    if light.cone.y > 1.5 { return light.color_intensity.rgb * light.color_intensity.w; }
    let distance2 = dot(offset, offset);
    let ratio2 = distance2 / (light.position_range.w * light.position_range.w);
    let window = max(1.0 - ratio2 * ratio2, 0.0);
    var attenuation = window * window / max(distance2, 0.0001);
    if light.cone.y > 0.5 {
        let direction = offset / max(sqrt(distance2), 0.000001);
        let angle = dot(light.direction_outer.xyz, -direction);
        let width = light.cone.x - light.direction_outer.w;
        // Equal angles define a hard-edged cone; never divide by zero.
        var cone = select(0.0, 1.0, angle >= light.direction_outer.w);
        if width > 0.000001 { cone = clamp((angle - light.direction_outer.w) / width, 0.0, 1.0); }
        attenuation *= cone * cone;
    }
    return light.color_intensity.rgb * light.color_intensity.w * attenuation;
}
fn local_diffuse(world: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < u32(local_lights.count.x); i++) {
        let light = local_lights.lights[i];
        let offset = light.position_range.xyz - world;
        let direction = local_direction(light, offset);
        result += local_radiance(light, offset) * max(dot(normal, direction), 0.0) / 3.14159265;
    }
    return result;
}
