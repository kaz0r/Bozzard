struct LocalLight {
    position_range: vec4<f32>, color_intensity: vec4<f32>,
    direction_outer: vec4<f32>, cone: vec4<f32>,
};
struct LocalLights { count: vec4<f32>, lights: array<LocalLight, 32> };
@group(2) @binding(3) var<uniform> local_lights: LocalLights;
struct SpotShadows { maps: array<ShadowUniform, 8> };
@group(2) @binding(6) var spot_shadow_maps: texture_depth_2d_array;
@group(2) @binding(7) var<uniform> spot_shadows: SpotShadows;

struct PointShadows { maps: array<ShadowUniform, 24> };
@group(2) @binding(8) var point_shadow_maps: texture_depth_2d_array;
@group(2) @binding(9) var<uniform> point_shadows: PointShadows;

fn point_shadow_face(ray: vec3<f32>) -> u32 {
    let a = abs(ray);
    if a.x >= a.y && a.x >= a.z { return select(1u, 0u, ray.x >= 0.0); }
    if a.y >= a.z { return select(3u, 2u, ray.y >= 0.0); }
    return select(5u, 4u, ray.z >= 0.0);
}
fn filtered_local_visibility(maps: texture_depth_2d_array, map: ShadowUniform,
    slot: u32, world: vec3<f32>) -> f32 {
    let projected = map.matrix * vec4<f32>(world, 1.0);
    if projected.w <= 0.0 { return 1.0; }
    let p = projected.xyz / projected.w;
    let uv = p.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || p.z < 0.0 || p.z > 1.0 { return 1.0; }
    var visibility = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            visibility += textureSampleCompareLevel(maps, shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y)) * map.settings.w, i32(slot), p.z);
        }
    }
    return visibility / 9.0;
}
fn local_visibility(light: LocalLight, world: vec3<f32>, geometric_normal: vec3<f32>) -> f32 {
    // cone.z stores shadow-light slot + 1, independently for each light kind.
    if light.cone.z < 0.5 { return 1.0; }
    let offset = light.position_range.xyz - world;
    let distance = length(offset);
    let to_light = offset / max(distance, 0.000001);
    if distance >= light.position_range.w { return 1.0; }
    let slot = u32(light.cone.z) - 1u;
    if light.cone.y > 0.5 {
        if dot(-to_light, light.direction_outer.xyz) < light.direction_outer.w { return 1.0; }
        let map = spot_shadows.maps[slot];
        let normal_offset = geometric_normal * map.settings.y * (1.0 - max(dot(geometric_normal, to_light), 0.0));
        return filtered_local_visibility(spot_shadow_maps, map, slot,
            world + normal_offset + to_light * map.settings.x);
    }
    let base = slot * 6u;
    let settings = point_shadows.maps[base].settings;
    // Select after world-space bias, so offsets across an edge use the correct face.
    let biased_world = world + geometric_normal * settings.y * (1.0 - max(dot(geometric_normal, to_light), 0.0)) + to_light * settings.x;
    let face = base + point_shadow_face(biased_world - light.position_range.xyz);
    return filtered_local_visibility(point_shadow_maps, point_shadows.maps[face], face, biased_world);
}

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
        result += local_radiance(light, offset) * max(dot(normal, direction), 0.0) / 3.14159265 * local_visibility(light, world, normal);
    }
    return result;
}
