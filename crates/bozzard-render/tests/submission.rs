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
    scene.items[1123].material.tint = [0.9, 0.1, 0.3];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 256);
    if native {
        let mut inserted = scene.items[0].clone();
        inserted.motion_id = 99112233;
        inserted.model *= Mat4::from_translation(Vec3::new(0.2, 0.2, 0.));
        scene.items.insert(0, inserted);
        compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(
            renderers[1].frame_stats().instance_uniform_bytes,
            256,
            "early insertion preserves unchanged stable object rows"
        );
        scene.items.remove(0);
        compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(
            renderers[1].frame_stats().instance_uniform_bytes,
            0,
            "early removal changes only compact instance IDs"
        );
        assert!(renderers[1].frame_stats().instance_id_bytes > 0);
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
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().graph_parameter_bytes, 0);
    scene.items[90].material.shader = Some(source(0.5));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().graph_parameter_bytes, 256);
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
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
