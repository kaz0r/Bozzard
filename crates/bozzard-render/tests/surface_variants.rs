use bozzard_render::*;
use glam::{Mat4, Vec3};

fn input() -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        particles: vec![],
        gi: None,
        shader_time: 0.,
        fog: Default::default(),
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        lighting: Lighting {
            shadows: false,
            ..Default::default()
        },
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-4., 4., -4., 4., 0.1, 30.),
        items: (0..8)
            .map(|i| DrawItem {
                motion_id: i + 1,
                model: Mat4::from_translation(Vec3::new(
                    (i % 4) as f32 * 1.6 - 2.4,
                    (i / 4) as f32 * 2. - 1.,
                    -5.,
                )) * Mat4::from_scale(Vec3::new(if i % 2 == 0 { 1. } else { -1. }, 1., 1.)),
                mesh: MeshKind::ModelPart("material".into(), 0),
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [0.7, 0.5, 0.4],
                    uv_scale: [1.; 2],
                    texture: TextureKind::ModelPart("material".into(), 0),
                    lit: true,
                    shader: None,
                },
            })
            .collect(),
    }
}
fn upload(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    mask: u8,
    double_sided: bool,
) -> anyhow::Result<()> {
    let vertices = [
        [-0.7, -0.7, 0., 0., 0., 1., 0., 1.],
        [0.7, -0.7, 0., 0., 0., 1., 1., 1.],
        [0.7, 0.7, 0., 0., 0., 1., 1., 0.],
        [-0.7, 0.7, 0., 0., 0., 1., 0., 0.],
    ];
    let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
    let map = || MaterialMap {
        image: ModelImage {
            width: 1,
            height: 1,
            rgba: &[130, 150, 210, 255],
        },
        sampler: Default::default(),
    };
    renderer.upload_model(
        gpu,
        "material",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            source_key: "0000000000000000",
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: Some(0.5),
            image: Some(ModelImage {
                width: 1,
                height: 1,
                rgba: &[190, 170, 140, 255],
            }),
            shading: Some(ModelShading {
                vertex_start: 0,
                vertices: &attributes,
                metallic: 0.4,
                roughness: 0.65,
                normal_scale: 0.7,
                occlusion_strength: 0.6,
                emissive_factor: [0.03, 0.02, 0.01],
                double_sided,
                base_color_sampler: Default::default(),
                normal: (mask & 1 != 0).then(map),
                metallic_roughness: (mask & 2 != 0).then(map),
                occlusion: (mask & 4 != 0).then(map),
                emissive: (mask & 8 != 0).then(map),
            }),
        }],
    )
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 160, 160, |target| {
        renderer.draw(gpu, target, [160; 2], scene)
    })
}
#[test]
fn map_masks_winding_unlit_and_consumed_outputs_preserve_native_pixels() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut pair = std::array::from_fn::<_, 2, _>(|_| {
        SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
    });
    pair[0].set_shader_optimizations_enabled(false);
    for renderer in &mut pair {
        renderer.set_occlusion_enabled(false);
    }
    let mut scene = input();
    let mut comparisons = 0;
    for maps in 0..16 {
        for double_sided in [false, true] {
            for renderer in &mut pair {
                upload(&gpu, renderer, maps, double_sided)?;
            }
            for lit in [true, false] {
                for item in &mut scene.items {
                    item.material.lit = lit;
                }
                let a = capture(&gpu, &mut pair[0], &scene)?;
                let b = capture(&gpu, &mut pair[1], &scene)?;
                assert_eq!(
                    a.rgba, b.rgba,
                    "maps={maps} double_sided={double_sided} lit={lit}"
                );
                assert!(a.rgba.chunks_exact(4).any(|p| p[..3] != a.rgba[..3]));
                comparisons += 1;
            }
        }
    }
    for (taa, reflections, expected_targets) in
        [(true, false, 2), (false, true, 2), (true, true, 3)]
    {
        scene.display.temporal_aa.enabled = taa;
        scene.display.reflections.enabled = reflections;
        scene.display.reflections.strength = 1.;
        for renderer in &mut pair {
            renderer.reset_display_history();
        }
        for frame in 0..3 {
            scene.display.time_seconds = frame as f32 / 60.;
            let a = capture(&gpu, &mut pair[0], &scene)?;
            let b = capture(&gpu, &mut pair[1], &scene)?;
            assert_eq!(
                a.rgba, b.rgba,
                "taa={taa} reflections={reflections} frame={frame}"
            );
            assert_eq!(pair[1].frame_stats().auxiliary_targets, expected_targets);
            comparisons += 1;
        }
    }
    println!(
        "surface_variant_proof comparisons={comparisons} missing_map_masks=16 mirrored_and_double_sided=true exact_pixels=true"
    );
    Ok(())
}

#[test]
fn stable_rows_and_normal_matrices_survive_material_edits_and_early_insertions()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    renderer.set_instancing_enabled(false);
    renderer.set_occlusion_enabled(false);
    let mut scene = input();
    upload(&gpu, &mut renderer, 0, false)?;
    capture(&gpu, &mut renderer, &scene)?;
    capture(&gpu, &mut renderer, &scene)?;
    scene.items[3].material.tint = [0.9, 0.7, 0.2];
    capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().object_uniform_builds, 1);
    assert_eq!(renderer.frame_stats().normal_matrix_builds, 0);
    let mut new = scene.items[0].clone();
    new.motion_id = 900;
    new.model *= Mat4::from_translation(Vec3::Z * -0.5);
    scene.items.insert(0, new);
    capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().object_buffer_allocations, 1);
    assert_eq!(renderer.frame_stats().normal_matrix_builds, 1);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 1);
    scene.items.remove(0);
    capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().object_buffer_allocations, 0);
    assert_eq!(renderer.frame_stats().normal_matrix_builds, 0);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 0);
    println!(
        "stable_object_proof material_edit_normal_builds=0 insertion_allocations=1 removal_writes=0"
    );
    Ok(())
}

#[test]
fn zero_local_light_contributions_preserve_diffuse_and_stock_brdf_pixels() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut pair = std::array::from_fn::<_, 2, _>(|_| {
        SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
    });
    pair[0].set_shader_optimizations_enabled(false);
    for renderer in &mut pair {
        renderer.set_occlusion_enabled(false);
        renderer.set_local_light_culling_enabled(false);
        upload(&gpu, renderer, 0, true)?;
    }
    let mut scene = input();
    let mut comparisons = 0;
    for full_aux in [false, true] {
        scene.display.temporal_aa.enabled = full_aux;
        scene.display.reflections.enabled = full_aux;
        for renderer in &mut pair {
            renderer.reset_display_history();
            upload(&gpu, renderer, if full_aux { 15 } else { 0 }, true)?;
        }
        for flavor in 0..3 {
            for item in &mut scene.items {
                item.mesh = if flavor == 0 {
                    MeshKind::ModelPart("material".into(), 0)
                } else {
                    MeshKind::Quad
                };
                item.material.texture = if flavor == 0 {
                    TextureKind::ModelPart("material".into(), 0)
                } else {
                    TextureKind::White
                };
                item.material.metallic = (flavor == 2).then_some(0.3);
                item.material.roughness = (flavor == 2).then_some(0.65);
            }
            let mut captures = Vec::new();
            for case in 0..5 {
                scene.display.time_seconds = comparisons as f32 / 60.;
                scene.lights = vec![LocalLight {
                    directional: false,
                    position: if case == 1 {
                        [0., 0., -8.]
                    } else {
                        [0., 0., -2.]
                    },
                    direction: if case == 4 {
                        [1., 0., 0.]
                    } else {
                        [0., 0., -1.]
                    },
                    color: [0.8, 0.6, 0.4],
                    intensity: 25.,
                    range: if case == 2 { 3.01 } else { 12. },
                    spot_angles: match case {
                        3 => Some([12., 12.]),
                        4 => Some([20., 35.]),
                        _ => None,
                    },
                    shadows: Some(LocalShadowSettings::default()),
                }];
                let a = capture(&gpu, &mut pair[0], &scene)?;
                let b = capture(&gpu, &mut pair[1], &scene)?;
                assert_eq!(
                    a.rgba, b.rgba,
                    "flavor={flavor},case={case},full_aux={full_aux}"
                );
                captures.push(a.rgba);
                comparisons += 1;
            }
            assert_ne!(captures[0], captures[1], "positive light must contribute");
        }
    }
    println!(
        "zero_local_light_proof comparisons={comparisons} diffuse_and_two_stock_brdf_hosts=true generic_and_specialized=true point_range_backface_hard_soft_cone_shadows=true exact_pixels=true"
    );
    Ok(())
}

#[test]
fn cached_object_sources_preserve_signed_zero_for_custom_graphs() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut pair = std::array::from_fn::<_, 2, _>(|_| {
        SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
    });
    pair[0].set_state_caching_enabled(false);
    let mut scene = input();
    scene.items.truncate(1);
    scene.items[0].mesh = MeshKind::Quad;
    scene.items[0].material.texture = TextureKind::White;
    scene.items[0].material.lit = false;
    scene.items[0].material.shader = Some(std::sync::Arc::new(ShaderSource {
        id: 543219,
        opaque_sort_id: 543219,
        numeric_parameters: std::sync::Arc::from([]),
        surface: "fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { var s=default_material_surface(uv,normal_uv,mr_uv,ao_uv,emissive_uv,world_normal,tangent,world,view,front,time); s.base=select(vec3<f32>(1.,0.,0.),vec3<f32>(0.,1.,0.),(bitcast<u32>(object.parameters.x)&0x80000000u)!=0u); return s; }".into(),
    }));
    let shader = scene.items[0].material.shader.as_deref().unwrap();
    for renderer in &mut pair {
        let bad_shader = ShaderSource {
            id: 9876123,
            opaque_sort_id: 9876123,
            surface: "invalid wgsl".into(),
            numeric_parameters: std::sync::Arc::from([]),
        };
        let invalid = [
            ShaderWarmup {
                shader,
                pbr: false,
                transparent: false,
                instanced: false,
                output_mask: 0,
                cull_mode: None,
            },
            ShaderWarmup {
                shader: &bad_shader,
                pbr: false,
                transparent: false,
                instanced: false,
                output_mask: 0,
                cull_mode: None,
            },
        ];
        assert!(renderer.prewarm_shader_variants(&gpu, &invalid).is_err());
        let request = [ShaderWarmup {
            shader,
            pbr: false,
            transparent: false,
            instanced: false,
            output_mask: 0,
            cull_mode: None,
        }];
        assert_eq!(renderer.prewarm_shader_variants(&gpu, &request)?, 1);
        assert_eq!(renderer.prewarm_shader_variants(&gpu, &request)?, 0);
    }
    let mut previous = None;
    for value in [0., -0., 0.] {
        scene.items[0].material.uv_scale[0] = value;
        let a = capture(&gpu, &mut pair[0], &scene)?;
        let b = capture(&gpu, &mut pair[1], &scene)?;
        assert_eq!(a.rgba, b.rgba);
        assert_eq!(pair[1].frame_stats().surface_variant_compilations, 0);
        if let Some(previous) = previous {
            assert_ne!(b.rgba, previous);
        }
        previous = Some(b.rgba);
    }
    println!(
        "signed_zero_proof exact_custom_graph_pixels=true transitions=2 prewarmed_first_frame_compiles=0"
    );
    Ok(())
}
