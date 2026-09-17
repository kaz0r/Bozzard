struct Params { scale: f32, add: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> values: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= arrayLength(&values) { return; }
    values[id.x] = values[id.x] * params.scale + params.add;
}
