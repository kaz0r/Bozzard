#[path = "../src/scene/host.rs"]
mod host;
#[test]
fn demo_effects_validate_in_basic_and_pbr_pipelines() {
    for pbr in [false, true] {
        let source = host::host_text(pbr);
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
