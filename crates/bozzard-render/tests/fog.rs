use bozzard_render::*;
use glam::{Mat4, Vec3};

#[test]
fn fog_shaders_validate() {
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
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}

fn capture(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    raw: bool,
) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 65, 65, |target| {
        if raw {
            renderer.draw_linear(gpu, target, [65, 65], scene)
        } else {
            renderer.draw(gpu, target, [65, 65], scene)
        }
    })
}
fn center(frame: &Frame) -> [u8; 4] {
    frame.rgba[(32 * 65 + 32) * 4..(32 * 65 + 32) * 4 + 4]
        .try_into()
        .unwrap()
}
fn srgb(v: f32) -> u8 {
    (255.
        * if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        })
    .round() as u8
}
fn check(frame: &Frame, linear: [f32; 3]) {
    let actual = center(frame);
    let expected = linear.map(srgb);
    assert!(
        actual[..3]
            .iter()
            .zip(expected)
            .all(|(a, e)| a.abs_diff(e) <= 2),
        "{actual:?} != {expected:?}"
    );
    assert_eq!(actual[3], 255);
}
// Independent midpoint integration oracle (not the shader's analytic formula).
fn fog_amount(scene: &RenderScene) -> f32 {
    let near = scene.view_projection.inverse().project_point3(Vec3::ZERO);
    let length = near.length();
    let segment = (length - scene.fog.start_distance).max(0.);
    let mut optical = 0.;
    for i in 0..10000 {
        let t = (scene.fog.start_distance + segment * (i as f32 + 0.5) / 10000.) / length;
        let y = near.y * (1. - t);
        optical += scene.fog.distance_density
            + scene.fog.height_density
                * (-scene.fog.height_falloff * (y - scene.fog.base_height).max(0.)).exp();
    }
    1. - (-optical * segment / 10000.).exp()
}

#[test]
fn fog_gpu_basic_pbr_distance_height_alpha_and_bypass() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let vertices = [
        [-1., -1., 0., 0., 0., 1., 0., 0.],
        [1., -1., 0., 0., 0., 1., 1., 0.],
        [1., 1., 0., 0., 0., 1., 1., 1.],
        [-1., 1., 0., 0., 0., 1., 0., 1.],
    ];
    let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
    let mut scene = RenderScene {
        fog: FogSettings {
            enabled: true,
            color: [1., 0., 0.],
            distance_density: std::f32::consts::LN_2 / 2.9,
            ..Default::default()
        },
        gi: None,
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: Mat4::IDENTITY,
        items: vec![DrawItem {
            model: Mat4::IDENTITY,
            mesh: MeshKind::Imported("fog".into()),
            material: Material {
                surface_overrides: Default::default(),
                tint: [0.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::White,
                lit: true,
            },
        }],
    };
    for pbr in [false, true] {
        for alpha in [1., 0.5] {
            renderer.upload_model(
                &gpu,
                "fog",
                &vertices,
                &[0, 1, 2, 0, 2, 3],
                &[ModelPart {
                    source_key: "",
                    start: 0,
                    count: 6,
                    color: [1., 1., 1., alpha],
                    alpha_cutoff: None,
                    image: None,
                    shading: pbr.then_some(ModelShading {
                        vertex_start: 0,
                        vertices: &attributes,
                        metallic: 0.,
                        roughness: 1.,
                        normal_scale: 1.,
                        occlusion_strength: 1.,
                        emissive_factor: [0.; 3],
                        double_sided: true,
                        base_color_sampler: Default::default(),
                        normal: None,
                        metallic_roughness: None,
                        occlusion: None,
                        emissive: None,
                    }),
                }],
            )?;
            for perspective in [false, true] {
                let projection = if perspective {
                    glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 20.)
                } else {
                    glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 20.)
                };
                for (angle, depth) in [(0., 1.), (0., 3.), (0., 8.), (0.6, 3.), (-0.6, 3.)] {
                    scene.view_projection = projection
                        * (Mat4::from_rotation_x(angle)
                            * Mat4::from_translation(Vec3::new(0., 0., depth)))
                        .inverse();
                    for (distance, height, base, falloff, start) in [
                        (std::f32::consts::LN_2 / 2.9, 0., 0., 1., 0.),
                        (0., 0.4, 0., 1., 0.),
                        (0.1, 0.4, -0.5, 2., 0.4),
                        (0., 0.4, 0., 0., 0.),
                        (0., 0.4, 0., 1000., 0.),
                        (0., 0., 0., 1., 0.),
                        (0.2, 0.4, 0., 1., 10.),
                    ] {
                        scene.fog.distance_density = distance;
                        scene.fog.height_density = height;
                        scene.fog.base_height = base;
                        scene.fog.height_falloff = falloff;
                        scene.fog.start_distance = start;
                        let amount = fog_amount(&scene);
                        check(
                            &capture(&gpu, &mut renderer, &scene, false)?,
                            [
                                amount * alpha + 0.018 * (1. - alpha),
                                0.025 * (1. - alpha),
                                0.04 * (1. - alpha),
                            ],
                        );
                    }
                }
            }
        }
    }
    scene.fog.distance_density = 1.;
    scene.fog.start_distance = 0.;
    let raw = capture(&gpu, &mut renderer, &scene, true)?;
    scene.fog.enabled = false;
    assert_eq!(raw.rgba, capture(&gpu, &mut renderer, &scene, true)?.rgba);
    scene.items[0].material.texture = TextureKind::Normals;
    let normals = capture(&gpu, &mut renderer, &scene, false)?;
    scene.fog.enabled = true;
    assert_eq!(
        normals.rgba,
        capture(&gpu, &mut renderer, &scene, false)?.rgba
    );
    scene.items[0].material.texture = TextureKind::White;
    scene.items[0].material.lit = false;
    let amount = fog_amount(&scene);
    check(
        &capture(&gpu, &mut renderer, &scene, false)?,
        [amount * 0.5 + 0.009, 0.0125, 0.02],
    );
    scene.fog.enabled = false;
    check(
        &capture(&gpu, &mut renderer, &scene, false)?,
        [0.009, 0.0125, 0.02],
    );
    scene.fog.enabled = true;
    renderer.upload_model(
        &gpu,
        "fog",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            source_key: "",
            start: 0,
            count: 6,
            color: [1., 1., 1., 0.5],
            alpha_cutoff: Some(0.75),
            image: None,
            shading: None,
        }],
    )?;
    check(
        &capture(&gpu, &mut renderer, &scene, false)?,
        [0.018, 0.025, 0.04],
    );
    scene.fog.height_density = f32::NAN;
    assert!(capture(&gpu, &mut renderer, &scene, false).is_err());
    Ok(())
}
