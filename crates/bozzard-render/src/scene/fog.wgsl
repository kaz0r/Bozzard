// Mean of exp(-falloff * max(y-base, 0)) along a segment. Sorting the
// endpoints avoids exponential overflow; the small-delta branch avoids cancellation.
fn fog_height_mean(a: f32, b: f32, base: f32, falloff: f32) -> f32 {
    let lo = (min(a,b)-base)*falloff;
    let hi = (max(a,b)-base)*falloff;
    let delta = hi-lo;
    if delta < 0.001 { return exp(-max((lo+hi)*0.5,0.0)); }
    let below = clamp(-lo/delta,0.0,1.0);
    let above = exp(-max(lo,0.0)) * (1.0-exp(-(max(hi,0.0)-max(lo,0.0)))) / delta;
    return clamp(below+above,0.0,1.0);
}

fn apply_fog(color: vec3<f32>, world: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    if object.fog_color.w < 0.5 { return color; }
    let ndc = pixel / object.viewport.xy * vec2<f32>(2.0,-2.0) + vec2<f32>(-1.0,1.0);
    let near = object.inverse_view_projection * vec4<f32>(ndc,0.0,1.0);
    let origin = near.xyz / near.w;
    let distance = length(world-origin);
    let segment = max(distance-object.fog_density.y,0.0);
    if segment <= 0.0 { return color; }
    let start = mix(origin,world,min(object.fog_density.y/max(distance,0.000001),1.0));
    let height = fog_height_mean(start.y,world.y,object.fog_density.w,object.fog_height.x);
    let optical_depth = segment * (object.fog_density.x + object.fog_density.z*height);
    return mix(color,object.fog_color.rgb,1.0-exp(-min(optical_depth,80.0)));
}
