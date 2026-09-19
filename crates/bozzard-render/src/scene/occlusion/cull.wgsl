struct Candidate {
    rectangle: vec4<u32>,
    nearest: f32,
    indices: u32,
    instances: u32,
    padding: u32,
}
struct Indirect {
    indices: u32,
    instances: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}
@group(0) @binding(0) var pyramid: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> candidates: array<Candidate>;
@group(0) @binding(2) var<storage, read_write> arguments: array<Indirect>;
@group(0) @binding(3) var<uniform> count: vec4<u32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= count.x { return; }
    let candidate = candidates[index];
    var instances = candidate.instances;
    // Near-plane crossings and batches containing a nonprojectable surface
    // have a negative nearest depth and always take the reference path.
    if candidate.nearest >= 0.0 {
        let extent = max(candidate.rectangle.z - candidate.rectangle.x + 1u,
                         candidate.rectangle.w - candidate.rectangle.y + 1u);
        var level = 0u;
        // At most five cells per axis. A single very coarse cell would often
        // include unrelated background and lose useful wall occlusion.
        while (4u << level) < extent { level++; }
        level = min(level, textureNumLevels(pyramid) - 1u);
        let size = textureDimensions(pyramid, level);
        let lo = min(candidate.rectangle.xy >> vec2<u32>(level), size - 1u);
        let hi = min(candidate.rectangle.zw >> vec2<u32>(level), size - 1u);
        var farthest = 0.0;
        for (var y = lo.y; y <= hi.y; y++) {
            for (var x = lo.x; x <= hi.x; x++) {
                farthest = max(farthest, textureLoad(pyramid, vec2<u32>(x,y), level).x);
            }
        }
        // Strict separation preserves equal-depth ordering and absorbs projection
        // roundoff. Empty/background pixels (depth 1) can never hide geometry.
        if farthest + 0.00002 < candidate.nearest { instances = 0u; }
    }
    arguments[index] = Indirect(candidate.indices, instances, 0u, 0, 0u);
}
