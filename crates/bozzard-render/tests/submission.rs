use bozzard_render::*;
use glam::{Mat4, Vec3};

fn scene(count: usize) -> RenderScene {
    let side = (count as f32).sqrt().ceil() as usize;
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: vec![],
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -1.,
            side as f32,
            -1.,
            side as f32,
            0.1,
            30.,
        ),
        items: (0..count)
            .map(|i| DrawItem {
                motion_id: i as u64 + 1,
                model: Mat4::from_translation(Vec3::new((i % side) as f32, (i / side) as f32, -5.))
                    * Mat4::from_scale(Vec3::splat(0.7)),
                mesh: MeshKind::Quad,
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [0.2 + (i % 7) as f32 * 0.1, 0.5, 0.3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                },
            })
            .collect(),
        lighting: Lighting {
            shadows: false,
            ..Default::default()
        },
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
    }
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 160, 160, |target| {
        renderer.draw(gpu, target, [160; 2], scene)
    })
}
fn compare(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scene: &RenderScene,
) -> anyhow::Result<()> {
    let a = capture(gpu, &mut renderers[0], scene)?;
    let b = capture(gpu, &mut renderers[1], scene)?;
    assert_eq!(a.rgba, b.rgba, "retained/native commands changed pixels");
    assert_eq!(
        renderers[0].frame_stats().color_triangles,
        renderers[1].frame_stats().color_triangles
    );
    Ok(())
}
#[test]
fn exact_resource_state_caching_reduces_binds_without_changing_draws() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer.set_instancing_enabled(false);
        renderer.set_render_bundles_enabled(false);
        renderer.set_native_multi_draw_enabled(false);
        renderer
    });
    renderers[0].set_state_caching_enabled(false);
    let mut scene = scene(80);
    // Give both paths the same consumed outputs and two vertex streams, so the
    // counts isolate binding reuse from unused-motion-output elimination.
    scene.display.temporal_aa.enabled = true;
    scene.display.reflections.enabled = true;
    for _ in 0..2 {
        compare(&gpu, &mut renderers, &scene)?;
        let before = renderers[0].frame_stats();
        let after = renderers[1].frame_stats();
        assert_eq!((before.color_draws, after.color_draws), (80, 80));
        assert_eq!((before.pipeline_binds, after.pipeline_binds), (80, 1));
        assert_eq!((before.vertex_binds, after.vertex_binds), (160, 2));
        assert_eq!((before.index_binds, after.index_binds), (80, 1));
        assert!(after.material_binds < before.material_binds);
    }
    println!(
        "resource_state_proof unchanged_draws=80 pipeline_binds=80->1 vertex_binds=160->2 index_binds=80->1 group_binds={}->{} exact_pixels=true",
        renderers[0].frame_stats().material_binds,
        renderers[1].frame_stats().material_binds,
    );
    Ok(())
}

#[test]
fn native_arena_reduces_10000_copies_from_157_draws_to_10_and_retains_uploads() -> anyhow::Result<()>
{
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_native_instance_arena_enabled(false);
    for renderer in &mut renderers {
        renderer.set_render_bundles_enabled(false);
        renderer.set_native_multi_draw_enabled(false);
    }
    let mut scene = scene(10_000);
    compare(&gpu, &mut renderers, &scene)?;
    let native = renderers[1].frame_stats().native_instance_arena;
    println!(
        "arena_proof native={native} reference_draws={} candidate_draws={} object_allocations={} instance_id_bytes={}",
        renderers[0].frame_stats().color_draws,
        renderers[1].frame_stats().color_draws,
        renderers[1].frame_stats().object_buffer_allocations,
        renderers[1].frame_stats().instance_id_bytes,
    );
    assert_eq!(renderers[0].frame_stats().color_draws, 157);
    assert_eq!(
        renderers[1].frame_stats().color_draws,
        if native { 10 } else { 157 }
    );
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    assert_eq!(
        renderers[1].frame_stats().native_object_membership_reused,
        native
    );
    scene.items[1123].material.tint = [0.9, 0.1, 0.3];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 256);
    assert_eq!(
        renderers[1].frame_stats().native_object_membership_reused,
        native
    );
    if native {
        let mut inserted = scene.items[0].clone();
        inserted.motion_id = 99112233;
        inserted.model *= Mat4::from_translation(Vec3::new(0.2, 0.2, 0.));
        scene.items.insert(0, inserted);
        compare(&gpu, &mut renderers, &scene)?;
        assert!(!renderers[1].frame_stats().native_object_membership_reused);
        assert_eq!(
            renderers[1].frame_stats().instance_uniform_bytes,
            256,
            "early insertion preserves unchanged stable object rows"
        );
        scene.items.remove(0);
        compare(&gpu, &mut renderers, &scene)?;
        assert!(!renderers[1].frame_stats().native_object_membership_reused);
        assert_eq!(
            renderers[1].frame_stats().instance_uniform_bytes,
            0,
            "early removal changes only compact instance IDs"
        );
        assert!(renderers[1].frame_stats().instance_id_bytes > 0);
        compare(&gpu, &mut renderers, &scene)?;
        assert!(renderers[1].frame_stats().native_object_membership_reused);
        println!(
            "arena_membership_proof 10000_warm_hash_insertions=0 exact_pixels=true edits_reuse=true insertion_removal_remap=true"
        );
    }
    renderers[1].set_native_instance_arena_enabled(false);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().color_draws, 157);
    renderers[1].set_native_instance_arena_enabled(true);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().native_instance_arena, native);
    Ok(())
}
#[test]
fn per_instance_numeric_graph_values_match_portable_arrays_through_parameter_edits()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_native_instance_arena_enabled(false);
    let surface = "fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { var s=default_material_surface(uv,normal_uv,mr_uv,ao_uv,emissive_uv,world_normal,tangent,world,view,front,time); s.base*=graph_numeric(0u).xyz; return s; }";
    let source = |red| {
        std::sync::Arc::new(ShaderSource {
            id: 991234,
            opaque_sort_id: 991234,
            surface: surface.into(),
            numeric_parameters: std::sync::Arc::from([[red, 0.8, 1., 0.]]),
        })
    };
    let mut scene = scene(132);
    let a = source(0.2);
    let b = source(0.9);
    for (i, item) in scene.items.iter_mut().enumerate() {
        item.material.shader = Some(if i % 2 == 0 { a.clone() } else { b.clone() });
    }
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &renderers {
        assert_eq!(
            renderer.frame_stats().graph_parameter_validation_objects,
            132
        );
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().graph_parameter_bytes, 0);
    for renderer in &renderers {
        assert_eq!(renderer.frame_stats().graph_parameter_validation_objects, 0);
    }
    scene.items[90].material.shader = Some(source(0.5));
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &renderers {
        assert_eq!(renderer.frame_stats().graph_parameter_validation_objects, 1);
    }
    assert_eq!(renderers[1].frame_stats().graph_parameter_bytes, 256);
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    // Equal values in a different Arc are also previously validated values.
    scene.items[90].material.shader = Some(source(0.5));
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &renderers {
        assert_eq!(renderer.frame_stats().graph_parameter_validation_objects, 0);
    }
    for invalid in [
        source(f32::NAN),
        std::sync::Arc::new(ShaderSource {
            id: 991234,
            opaque_sort_id: 991234,
            surface: surface.into(),
            numeric_parameters: std::sync::Arc::from([[0.; 4]; 17]),
        }),
    ] {
        scene.items[90].material.shader = Some(invalid);
        for renderer in &mut renderers {
            let error = capture(&gpu, renderer, &scene).err().unwrap();
            assert!(
                error
                    .to_string()
                    .contains("invalid graph numeric parameters")
            );
        }
        scene.items[90].material.shader = Some(source(0.5));
        compare(&gpu, &mut renderers, &scene)?;
    }
    println!(
        "graph_validation_proof cold_scans=132 warm_scans=0 edited_scans=1 edited_bytes=256 equal_arc_scans=0 invalid_and_reverted_exact=true"
    );
    Ok(())
}
#[test]
fn retained_bundles_replay_exact_pixels_and_rebuild_when_resources_or_ranges_change()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_render_bundles_enabled(false);
    let mut scene = scene(192);
    for (i, item) in scene.items.iter_mut().enumerate() {
        let id = format!("bundle-material-{i}");
        for renderer in &mut renderers {
            renderer.upload_image(&gpu, &id, 1, 1, &[i as u8, 255 - i as u8, 70, 255])?;
        }
        item.material.texture = TextureKind::Imported(id);
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().render_bundle_compilations, 1);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().render_bundle_compilations, 0);
    assert_eq!(renderers[1].frame_stats().render_bundle_replays, 1);
    scene.items[90].material.tint = [0.9, 0.1, 0.2];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(
        renderers[1].frame_stats().render_bundle_compilations,
        0,
        "contents-only uniform edits keep recorded commands"
    );
    for renderer in &mut renderers {
        renderer.upload_image(&gpu, "bundle-material-90", 1, 1, &[40, 120, 230, 255])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().render_bundle_compilations, 1);
    scene.items.remove(0);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().render_bundle_compilations, 1);
    renderers[1].set_render_bundles_enabled(false);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().render_bundle_replays, 0);
    Ok(())
}
#[test]
fn native_multi_draw_retains_nonzero_first_instance_and_dirty_arguments() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_native_multi_draw_enabled(false);
    for renderer in &mut renderers {
        renderer.set_render_bundles_enabled(false);
    }
    let mut scene = scene(8192);
    compare(&gpu, &mut renderers, &scene)?;
    let supported = renderers[1].frame_stats().native_instance_arena
        && gpu
            .device
            .features()
            .contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT);
    println!(
        "multi_draw_proof native_arena={} native_multi_draw={supported} logical_draws={} indirect_runs={} indirect_commands={} argument_bytes={}",
        renderers[1].frame_stats().native_instance_arena,
        renderers[1].frame_stats().color_draws,
        renderers[1].frame_stats().multi_draw_indirect_runs,
        renderers[1].frame_stats().multi_draw_indirect_draws,
        renderers[1].frame_stats().multi_draw_indirect_bytes,
    );
    assert_eq!(
        renderers[1].frame_stats().multi_draw_indirect_runs,
        usize::from(supported)
    );
    assert_eq!(
        renderers[1].frame_stats().multi_draw_indirect_draws,
        if supported { 8 } else { 0 }
    );
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().multi_draw_indirect_bytes, 0);
    scene.items.pop();
    compare(&gpu, &mut renderers, &scene)?;
    if supported {
        assert!(renderers[1].frame_stats().multi_draw_indirect_bytes > 0);
    }
    Ok(())
}

#[test]
fn native_geometry_arena_combines_distinct_meshes_with_exact_rebased_indices() -> anyhow::Result<()>
{
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_native_multi_draw_enabled(false);
    for renderer in &mut renderers {
        renderer.set_render_bundles_enabled(false);
    }
    let mut scene = scene(16);
    for mesh in 0..8 {
        let id = format!("native-arena-mesh-{mesh}");
        let top = 0.25 + mesh as f32 * 0.035;
        let vertices = [
            [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
            [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
            [0., top, 0., 0., 0., 1., 0.5, 0.],
        ];
        for renderer in &mut renderers {
            renderer.upload_mesh(&gpu, &id, &vertices, &[0, 1, 2])?;
        }
        for item in &mut scene.items[mesh * 2..mesh * 2 + 2] {
            item.mesh = MeshKind::Imported(id.clone());
        }
    }
    compare(&gpu, &mut renderers, &scene)?;
    let supported = renderers[1].frame_stats().native_instance_arena
        && gpu
            .device
            .features()
            .contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT);
    assert_eq!(
        renderers[1].frame_stats().multi_draw_indirect_draws,
        if supported { 8 } else { 0 }
    );
    assert_eq!(
        renderers[1].frame_stats().multi_draw_indirect_runs,
        usize::from(supported)
    );
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().multi_draw_indirect_bytes, 0);
    scene.items[3].material.tint = [0.8, 0.9, 0.2];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().multi_draw_indirect_bytes, 0);
    Ok(())
}

#[test]
fn native_lit_object_rows_and_numeric_graphs_match_portable_through_multilight_churn()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer.set_render_bundles_enabled(false);
        renderer.set_native_multi_draw_enabled(false);
        renderer
    });
    renderers[0].set_native_instance_arena_enabled(false);
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0., 0.5, 0., 0., 0., 1., 0.5, 0.],
    ];
    let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 3];
    let map = |rgba: &'static [u8]| MaterialMap {
        image: ModelImage {
            width: 1,
            height: 1,
            rgba,
        },
        sampler: Default::default(),
    };
    for renderer in &mut renderers {
        for (id, metallic, roughness) in
            [("native-lit-a", 0.15, 0.47), ("native-lit-b", 0.68, 0.73)]
        {
            renderer.upload_model(
                &gpu,
                id,
                &vertices,
                &[0, 1, 2],
                &[ModelPart {
                    source_key: "0000000000000000",
                    start: 0,
                    count: 3,
                    color: [1.; 4],
                    alpha_cutoff: None,
                    // The cubes use this same ModelPart texture key, so
                    // group 0 survives PBR→stock pipeline transitions while
                    // group 1 disappears and later Metal registers move.
                    image: Some(ModelImage {
                        width: 1,
                        height: 1,
                        rgba: if id == "native-lit-a" {
                            &[255; 4]
                        } else {
                            &[220, 185, 240, 255]
                        },
                    }),
                    shading: Some(ModelShading {
                        vertex_start: 0,
                        vertices: &attributes,
                        metallic,
                        roughness,
                        normal_scale: 0.65,
                        occlusion_strength: 0.8,
                        emissive_factor: [0.04, 0.02, 0.03],
                        double_sided: false,
                        base_color_sampler: Default::default(),
                        normal: Some(map(&[145, 128, 250, 255])),
                        metallic_roughness: Some(map(&[255, 185, 125, 255])),
                        occlusion: Some(map(&[205, 205, 205, 255])),
                        emissive: Some(map(&[25, 40, 15, 255])),
                    }),
                }],
            )?;
        }
    }
    let graph = |value: f32| {
        std::sync::Arc::new(ShaderSource {
            id: 779931,
            opaque_sort_id: 779931,
        surface: "fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { var s=default_material_surface(uv,normal_uv,mr_uv,ao_uv,emissive_uv,world_normal,tangent,world,view,front,time); s.emissive+=graph_numeric(0u).xyz; return s; }".into(),
        numeric_parameters: std::sync::Arc::from([[value, 0.025, 0.01, 0.]]),
    })
    };
    let mut scene = scene(300);
    scene.lighting = Lighting {
        shadows: true,
        shadow_resolution: 256,
        sun_intensity: 0.25,
        sun_direction: Vec3::new(0.3, 0.7, 1.).normalize().to_array(),
        ..Default::default()
    };
    scene.display.tone_mapping = true;
    scene.lights = (0..32)
        .map(|i| LocalLight {
            directional: false,
            position: [(i % 8) as f32 * 2.25, (i / 8) as f32 * 4.5, -2.2],
            direction: [0., 0., -1.],
            color: [0.3 + (i % 3) as f32 * 0.25, 0.4, 0.7],
            intensity: 4. + (i % 5) as f32,
            range: 5.5,
            spot_angles: (i % 4 == 0).then_some([35., 55.]),
            shadows: [0, 1, 16, 17]
                .contains(&i)
                .then_some(LocalShadowSettings::default()),
        })
        .collect();
    let mut original = std::collections::BTreeMap::new();
    for (i, item) in scene.items.iter_mut().enumerate() {
        item.model = Mat4::from_translation(Vec3::new((i % 18) as f32, (i / 18) as f32, -6.))
            * Mat4::from_rotation_y((i % 12) as f32 * 0.019)
            * Mat4::from_scale(Vec3::new(0.65, 0.65 + (i % 7) as f32 * 0.01, 0.5));
        original.insert(item.motion_id, item.model);
        item.material.lit = true;
        item.material.metallic = Some(0.1 + (i % 5) as f32 * 0.03);
        item.material.roughness = Some(0.45 + (i % 4) as f32 * 0.05);
        match i % 5 {
            0 => {
                item.mesh = MeshKind::Cube;
                item.material.texture = TextureKind::ModelPart("native-lit-a".into(), 0);
            }
            1 => item.mesh = MeshKind::Imported("native-lit-a".into()),
            2 => {
                item.mesh = MeshKind::Cube;
                item.material.texture = TextureKind::ModelPart("native-lit-a".into(), 0);
                item.material.shader = Some(graph(0.01));
            }
            3 => item.mesh = MeshKind::Imported("native-lit-b".into()),
            _ => {
                item.mesh = MeshKind::Imported("native-lit-a".into());
                item.material.shader = Some(graph(0.03));
            }
        }
    }
    let original_camera = scene.view_projection;
    let hidden_model = scene.items[36].model;
    let mut native = false;
    for tick in 0..72 {
        if tick >= 2 {
            for item in scene.items.iter_mut().filter(|item| item.motion_id <= 12) {
                item.model = original[&item.motion_id]
                    * Mat4::from_translation(Vec3::new((tick as f32 * 0.07).sin() * 0.08, 0., 0.))
                    * Mat4::from_rotation_z(tick as f32 * 0.001);
            }
            for item in scene
                .items
                .iter_mut()
                .filter(|item| item.material.shader.is_some())
            {
                item.material.shader = Some(graph(0.01 + (tick % 7) as f32 * 0.003));
            }
        }
        match tick {
            8 => {
                let mut item = scene.items[0].clone();
                item.motion_id = 900_001;
                item.model *= Mat4::from_translation(Vec3::new(0.1, 0.1, 0.1));
                scene.items.insert(0, item);
            }
            16 => {
                scene.items.remove(0);
            }
            24 => {
                scene.lights[20].position[0] += 0.4;
            }
            32 => {
                scene.items[36].model = Mat4::from_translation(Vec3::new(100., 100., -6.));
            }
            40 => {
                scene.items[36].model = hidden_model;
            }
            48 => {
                scene.items.remove(70);
            }
            56 => {
                let mut item = scene.items[0].clone();
                item.motion_id = 900_056;
                item.model = hidden_model * Mat4::from_translation(Vec3::new(0.1, 0.1, 0.));
                scene.items.insert(0, item);
            }
            64 => {
                scene.view_projection = original_camera * Mat4::from_rotation_y(0.015);
            }
            71 => {
                scene.view_projection = original_camera;
            }
            _ => {}
        }
        compare(&gpu, &mut renderers, &scene)?;
        let stats = renderers[1].frame_stats();
        native |= stats.native_instance_arena;
        assert!(stats.local_light_candidates > 0);
        assert!(
            stats.local_light_candidates < stats.local_light_slots,
            "fixture must vary object light masks"
        );
        if tick == 1 {
            assert_eq!(stats.instance_uniform_bytes, 0);
            assert_eq!(stats.graph_parameter_bytes, 0);
        }
    }
    println!(
        "native_lit_arena_proof native={native} 300_objects 32_local_lights stock_PBR_graph nonzero_group_ranges 72_frames exact_pixels=true"
    );
    Ok(())
}
#[test]
fn occlusion_bound_prepass_skips_projection_without_qualifying_occluders() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    // Reference without occlusion, full projection, and the conservative pre-pass.
    let mut renderers: [SceneRenderer; 3] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_occlusion_enabled(false);
    renderers[1].set_occlusion_bound_prepass_enabled(false);
    let mut scene = scene(4096);
    let projection = glam::camera::rh::proj::directx::perspective(0.9, 1., 0.1, 200.);
    for (i, item) in scene.items.iter_mut().enumerate() {
        item.mesh = MeshKind::Cube;
        item.model = Mat4::from_translation(Vec3::new(
            (i % 64) as f32 - 31.5,
            0.,
            -((i / 64) as f32) - 8.,
        )) * Mat4::from_scale(Vec3::splat(0.4));
    }
    let views = [
        Vec3::new(0., 6., 2.),
        Vec3::new(3., 7., 1.),
        Vec3::new(-4., 5., 3.),
    ];
    let run = |scene: &RenderScene, renderers: &mut [SceneRenderer; 3]| {
        let frames = renderers
            .iter_mut()
            .map(|renderer| capture(&gpu, renderer, scene))
            .collect::<anyhow::Result<Vec<_>>>()?;
        assert_eq!(
            frames[0].rgba, frames[1].rgba,
            "full projection changed pixels"
        );
        assert_eq!(
            frames[0].rgba, frames[2].rgba,
            "bound pre-pass changed pixels"
        );
        anyhow::Ok(())
    };
    let mut proof = None;
    for eye in views {
        scene.view_projection = projection
            * glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0., 0., -40.), Vec3::Y);
        run(&scene, &mut renderers)?;
        let full = renderers[1].frame_stats();
        let bounded = renderers[2].frame_stats();
        // Only near cubes whose bound reaches 2% of the view need exact corners.
        assert!(full.occlusion_projections > 1000, "{full:?}");
        assert!(
            bounded.occlusion_projections * 10 < full.occlusion_projections,
            "{bounded:?}"
        );
        assert!(bounded.occlusion_bound_rejections > 1000);
        assert_eq!(full.occlusion_depth_draws, 0);
        proof.get_or_insert((
            full.occlusion_projections,
            bounded.occlusion_projections,
            bounded.occlusion_bound_rejections,
        ));
    }
    // A wall in front of the field qualifies: both selections then agree exactly.
    let mut wall = scene.items[0].clone();
    wall.motion_id = 100_000;
    wall.mesh = MeshKind::Quad;
    wall.model =
        Mat4::from_translation(Vec3::new(0., 2., -14.)) * Mat4::from_scale(Vec3::new(40., 6., 1.));
    scene.items.push(wall);
    let mut depth_draws = 0;
    for eye in views {
        scene.view_projection = projection
            * glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0., 0., -40.), Vec3::Y);
        for _ in 0..3 {
            run(&scene, &mut renderers)?;
            let full = renderers[1].frame_stats();
            let bounded = renderers[2].frame_stats();
            assert_eq!(full.occlusion_candidates, bounded.occlusion_candidates);
            assert_eq!(full.occlusion_depth_draws, bounded.occlusion_depth_draws);
            assert_eq!(full.visible_surfaces, bounded.visible_surfaces);
            depth_draws += bounded.occlusion_depth_draws;
        }
    }
    assert!(depth_draws > 0, "the wall must qualify as an occluder");
    let (full, bounded, rejections) = proof.unwrap();
    println!(
        "occlusion_prepass_proof surfaces=4096 full_projections={full} bounded_projections={bounded} rejections={rejections} occluder_depth_draws={depth_draws}"
    );
    Ok(())
}
#[test]
#[ignore = "release-mode 140k-surface arena capacity check; run explicitly"]
fn native_arena_stays_native_past_former_128_mib_cliff() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers = std::array::from_fn(|_| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    renderers[0].set_native_instance_arena_enabled(false);
    let count = 140_000;
    let scene = scene(count);
    for frame in 0..3 {
        compare(&gpu, &mut renderers, &scene)?;
        let stats = renderers[1].frame_stats();
        let admitted = stats.native_arena_max_records >= count + count / 4 + 8192;
        println!(
            "arena_cliff_proof frame={frame} surfaces={count} max_records={} native={} portable_draws={} native_draws={}",
            stats.native_arena_max_records,
            stats.native_instance_arena,
            renderers[0].frame_stats().color_draws,
            stats.color_draws,
        );
        assert_eq!(stats.native_instance_arena, admitted);
        if admitted {
            assert!(stats.color_draws * 10 < renderers[0].frame_stats().color_draws);
        }
    }
    Ok(())
}
