#[test]
fn demo_effects_validate_in_basic_and_pbr_pipelines() {
    let shared = [
        include_str!("../src/scene/environment_sample.wgsl"),
        include_str!("../src/scene/shadow_sample.wgsl"),
        include_str!("../src/scene/local_lights.wgsl"),
        include_str!("../src/scene/gi.wgsl"),
        include_str!("../src/scene/effects.wgsl"),
        include_str!("../src/scene/fog.wgsl"),
    ]
    .join("\n");
    for shader in [
        include_str!("../src/scene.wgsl"),
        include_str!("../src/pbr.wgsl"),
    ] {
        let source = format!("{shared}\n{shader}");
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
