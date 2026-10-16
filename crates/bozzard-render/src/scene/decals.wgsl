// Decals: each box is drawn once per covered pixel (its back faces); the fragment reads the
// opaque depth, rebuilds the surface position, and paints the image where that position
// lies inside the box. Lit decals use the same sun, shadow, ambient and local-light terms
// as a simple lit surface, then the scene fog.
struct FrameUniform {
    view_projection: mat4x4<f32>,
    previous_view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    viewport: vec4<f32>,
    sun: vec4<f32>, sun_color: vec4<f32>, ambient_color: vec4<f32>,
    fog_color: vec4<f32>, fog_density: vec4<f32>, fog_height: vec4<f32>,
    misc: vec4<f32>,
};
// Named `object` for the shared shadow, local-light and fog functions.
@group(0) @binding(0) var<uniform> object: FrameUniform;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
struct DecalRecord {
    volume: mat4x4<f32>,
    inverse_volume: mat4x4<f32>,
    color: vec4<f32>,
    // x: smallest accepted facing, y: lit, w: -1 for a mirrored volume.
    params: vec4<f32>,
};
@group(0) @binding(2) var<storage, read> decals: array<DecalRecord>;
@group(1) @binding(0) var decal_image: texture_2d<f32>;
@group(1) @binding(1) var decal_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VertexOut {
    // Unit cube corners by bit (x = bit 0, y = bit 1, z = bit 2), wound outward.
    let corners = array<u32, 36>(
        1u, 3u, 7u, 1u, 7u, 5u, 0u, 4u, 6u, 0u, 6u, 2u, 2u, 6u, 7u, 2u, 7u, 3u,
        0u, 1u, 5u, 0u, 5u, 4u, 4u, 5u, 7u, 4u, 7u, 6u, 0u, 2u, 3u, 0u, 3u, 1u,
    );
    let corner = corners[vertex];
    let local = vec3<f32>(f32(corner & 1u), f32((corner >> 1u) & 1u), f32((corner >> 2u) & 1u)) - 0.5;
    var out: VertexOut;
    out.clip = object.view_projection * decals[instance].volume * vec4<f32>(local, 1.0);
    out.index = instance;
    return out;
}

fn decal_world(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let h = object.inverse_view_projection * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), depth, 1.0);
    return h.xyz / select(0.000001, h.w, abs(h.w) > 0.000001);
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let record = decals[in.index];
    let pixel = clamp(vec2<i32>(in.clip.xy), vec2<i32>(0), vec2<i32>(textureDimensions(scene_depth)) - 1);
    let depth = textureLoad(scene_depth, pixel, 0);
    let world = decal_world(in.clip.xy / object.viewport.xy, depth);
    let local = (record.inverse_volume * vec4<f32>(world, 1.0)).xyz;
    let uv = local.xz + 0.5;
    // Derivatives before any discard, while every pixel of the quad is still running.
    let world_dx = dpdx(world);
    let world_dy = dpdy(world);
    let texel = textureSampleGrad(decal_image, decal_sampler, uv, dpdx(uv), dpdy(uv));
    // Back faces only, so each covered pixel is painted once; a mirrored box flips winding.
    if front != (record.params.w < 0.0) { discard; }
    if depth >= 1.0 || any(abs(local) > vec3<f32>(0.5)) { discard; }
    var normal = normalize(cross(world_dy, world_dx));
    let eye = object.inverse_view_projection * vec4<f32>(0.0, 0.0, 1.0, 0.0);
    let toward = select(-eye.xyz, eye.xyz / eye.w - world, abs(eye.w) > 0.000001 * length(eye.xyz));
    if dot(normal, toward) < 0.0 { normal = -normal; }
    let up = normalize((record.volume * vec4<f32>(0.0, 1.0, 0.0, 0.0)).xyz);
    let facing = (dot(normal, up) - record.params.x) / max(1.0 - record.params.x, 0.001);
    // Fade over the last quarter of the angle limit and near the ends of the projection.
    let fade = clamp(facing * 4.0, 0.0, 1.0) * (1.0 - smoothstep(0.4, 0.5, abs(local.y)));
    let alpha = texel.a * record.color.a * fade;
    if alpha < 0.002 { discard; }
    var color = texel.rgb * record.color.rgb;
    if record.params.y > 0.5 {
        let diffuse = local_diffuse(world, normal)
            + object.sun_color.w * object.ambient_color.rgb
            + object.sun_color.rgb * object.sun.w * max(dot(normal, object.sun.xyz), 0.0) / 3.14159265
                * sun_visibility(world, normal);
        color *= diffuse;
    }
    return vec4<f32>(apply_fog(min(color, vec3<f32>(60000.0)), world, in.clip.xy), alpha);
}
