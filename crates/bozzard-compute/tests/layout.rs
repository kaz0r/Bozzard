use bozzard_compute::{BindingKind, Kernel};
use serde_json::json;

const KERNEL: &str = r#"
struct Particle { position: vec3f, weight: f32, velocity: vec3f }
struct Params { time: f32, offset: vec3f }
@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(2) var<storage, read_write> particles: array<Particle>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= arrayLength(&particles) { return; }
    particles[id.x].position += params.offset * params.time;
}
"#;

#[test]
fn reflected_layout_packs_wgsl_padding_and_bounds_partial_workgroups() {
    let kernel = Kernel::parse(KERNEL).unwrap();
    let entry = kernel.entry("main").unwrap();
    assert_eq!(entry.groups_for_extent([65, 2, 1]).unwrap(), [2, 2, 1]);
    assert!(entry.groups_for_extent([0, 1, 1]).is_err());
    assert!(kernel.entry("missing").is_err());
    let params = entry.parameters().unwrap();
    let p = json!({"time": 2.0, "offset": [1.0, 2.0, 3.0]});
    let packed = params.pack(&p).unwrap();
    assert_eq!(packed.len(), 32);
    assert_eq!(&packed[4..16], &[0; 12]);
    assert_eq!(f32::from_le_bytes(packed[16..20].try_into().unwrap()), 1.0);
    assert_eq!(params.unpack(&packed, 4).unwrap(), p);
    assert!(params.unpack(&packed, 3).is_err());
    let BindingKind::Storage { layout, writable } = &entry.binding("particles").unwrap().kind
    else {
        panic!()
    };
    assert!(writable);
    assert_eq!(layout.buffer_size(3).unwrap(), 96);
    let particle = json!({"position": [1.0, 2.0, 3.0], "weight": 4.0, "velocity": [5.0, 6.0, 7.0]});
    let data = json!([particle, particle]);
    let bytes = layout.pack(&data).unwrap();
    assert_eq!(bytes.len(), 64);
    assert_eq!(&bytes[28..32], &[0; 4]);
    assert_eq!(layout.unpack(&bytes, 14).unwrap(), data);
    assert!(layout.unpack(&bytes[..63], 100).is_err());
    assert!(layout.buffer_size(u32::MAX).is_err());
}

#[test]
fn integer_ranges_struct_fields_matrix_columns_and_runtime_tails_are_checked() {
    let kernel = Kernel::parse(
        r#"
struct Item { signed: i32, unsigned: u32, matrix: mat2x3f }
struct Data { header: vec4u, items: array<Item> }
@group(0) @binding(0) var<storage, read_write> data: Data;
@compute @workgroup_size(1) fn main() { data.items[0].signed += i32(data.header.x); }
"#,
    )
    .unwrap();
    let BindingKind::Storage { layout, .. } =
        &kernel.entry("main").unwrap().binding("data").unwrap().kind
    else {
        panic!()
    };
    let mut value = json!({"header": [1, 2, 3, 4], "items": [{"signed": -2147483648i64, "unsigned": 4294967295u64, "matrix": [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]}]});
    let bytes = layout.pack(&value).unwrap();
    assert_eq!(bytes.len(), 64);
    assert_eq!(layout.unpack(&bytes, 12).unwrap(), value);
    value["items"][0]["unsigned"] = json!(4294967296u64);
    assert!(layout.pack(&value).is_err());
    value["items"][0]["unsigned"] = json!(1.5);
    assert!(layout.pack(&value).is_err());
    value["items"][0]["unsigned"] = json!(0);
    value["extra"] = json!(1);
    assert!(layout.pack(&value).is_err());
    assert!(layout.unpack(&bytes[..16], 100).is_err());
}

#[test]
fn reflection_is_per_entry_and_errors_include_source_context() {
    let kernel = Kernel::parse(
        r#"
@group(0) @binding(0) var<storage, read_write> first: array<u32>;
@group(0) @binding(1) var<storage, read_write> second: array<u32>;
@compute @workgroup_size(1) fn a() { first[0] = 1u; }
@compute @workgroup_size(1) fn b() { second[0] = 2u; }
"#,
    )
    .unwrap();
    assert_eq!(kernel.entry("a").unwrap().bindings.len(), 1);
    assert_eq!(kernel.entry("a").unwrap().bindings[0].name, "first");
    assert_eq!(kernel.entry("b").unwrap().bindings[0].name, "second");
    let error = Kernel::parse("@compute @workgroup_size(1) fn main() { nope; }")
        .unwrap_err()
        .to_string();
    assert!(error.contains("nope") && error.contains("1:"), "{error}");
    assert!(
        Kernel::parse("@fragment fn main() -> @location(0) vec4f { return vec4f(0.); }").is_err()
    );
    assert!(Kernel::parse(KERNEL.replace("@group(1)", "@group(4)")).is_err());
    assert!(
        Kernel::parse(KERNEL.replace("var<uniform> params", "var<uniform> unexpected")).is_err()
    );
    assert!(Kernel::parse(" ".repeat(1024 * 1024 + 1)).is_err());
}

#[test]
fn textures_and_samplers_have_explicit_kinds_and_unsupported_features_fail() {
    let source = r#"
@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var input: texture_2d<f32>;
@group(0) @binding(2) var linear: sampler;
@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3u) {
  textureStore(output, id.xy, textureSampleLevel(input, linear, vec2f(0.5), 0.));
}

"#;
    let kernel = Kernel::parse(source).unwrap();
    let e = kernel.entry("main").unwrap();
    assert!(matches!(e.bindings[0].kind, BindingKind::StorageTexture(_)));
    assert!(matches!(e.bindings[1].kind, BindingKind::SampledTexture));
    assert!(matches!(e.bindings[2].kind, BindingKind::Sampler));
    assert!(Kernel::parse(source.replace("rgba8unorm", "r32uint")).is_err());
    assert!(e.pack_parameters(&json!({})).unwrap().is_empty());
    assert!(e.pack_parameters(&json!({"unexpected": 1})).is_err());
}

#[test]
fn runtime_tail_respects_struct_minimum_binding_span_without_inventing_elements() {
    let kernel = Kernel::parse(
        r#"
struct Data { header: vec4u, values: array<f32> }
@group(0) @binding(0) var<storage, read_write> data: Data;
@compute @workgroup_size(1) fn main() { data.values[0] = f32(data.header.x); }
"#,
    )
    .unwrap();
    let BindingKind::Storage { layout, .. } =
        &kernel.entry("main").unwrap().binding("data").unwrap().kind
    else {
        panic!()
    };
    assert_eq!(layout.minimum_size(), 32);
    assert!(layout.buffer_size(1).is_err());
    assert_eq!(layout.buffer_size(4).unwrap(), 32);
    let value = json!({"header": [1, 2, 3, 4], "values": [1., 2., 3., 4., 5.]});
    let bytes = layout.pack(&value).unwrap();
    assert_eq!(bytes.len(), 36);
    assert_eq!(layout.unpack(&bytes, 9).unwrap(), value);
}
