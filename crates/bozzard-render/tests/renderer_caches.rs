//! Renderer cache retention: asset publication, text, temporal targets,
//! particles, sky shading and mipmap uploads.
use bozzard_render::*;
use glam::{Mat4, Vec3};
use std::sync::Arc;

const SIZE: [u32; 2] = [640, 400];
const MESHES: usize = 160;

fn camera() -> Mat4 {
    glam::camera::rh::proj::directx::perspective(0.9, 1.6, 0.1, 100.)
        * glam::camera::rh::view::look_at_mat4(Vec3::new(0., 14., 22.), Vec3::ZERO, Vec3::Y)
}
fn material(i: usize) -> Material {
    Material {
        metallic: Some(0.1),
        roughness: Some(0.7),
        tint: [0.3 + (i % 5) as f32 * 0.1, 0.6, 0.4],
        lit: true,
        texture: TextureKind::Checker,
        uv_scale: [1.; 2],
        surface_overrides: Default::default(),
        shader: None,
    }
}
/// `count` lit cubes over `MESHES` distinct uploaded meshes: enough color
/// draws for retained render bundles, plus sun shadows and a sky.
fn grid(count: usize) -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: vec![],
        view_projection: camera(),
        items: (0..count)
            .map(|i| DrawItem {
                motion_id: i as u64 + 1,
                model: Mat4::from_translation(Vec3::new(
                    (i % 40) as f32 * 0.9 - 17.5,
                    0.4,
                    (i / 40) as f32 * 0.9 - 14.,
                )) * Mat4::from_scale(Vec3::splat(0.6)),
                mesh: MeshKind::Imported(format!("mesh-{}", i % MESHES)),
                material: material(i),
            })
            .collect(),
        lighting: Lighting::default(),
        lights: vec![],
        environment: EnvironmentSettings::default(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings::default(),
    }
}
fn upload_meshes(gpu: &Gpu, renderer: &mut SceneRenderer) -> anyhow::Result<()> {
    for mesh in 0..MESHES {
        let s = 0.5 + mesh as f32 * 0.002;
        let vertices: Vec<[f32; 8]> = [
            [-s, -s, s],
            [s, -s, s],
            [s, s, s],
            [-s, s, s],
            [-s, -s, -s],
            [s, -s, -s],
            [s, s, -s],
            [-s, s, -s],
        ]
        .iter()
        .map(|p| {
            let n = Vec3::from(*p).normalize();
            [p[0], p[1], p[2], n.x, n.y, n.z, p[0] + 0.5, p[1] + 0.5]
        })
        .collect();
        let indices = [
            0, 1, 2, 0, 2, 3, 5, 4, 7, 5, 7, 6, 1, 5, 6, 1, 6, 2, 4, 0, 3, 4, 3, 7, 3, 2, 6, 3, 6,
            7, 4, 5, 1, 4, 1, 0,
        ];
        renderer.upload_mesh(gpu, &format!("mesh-{mesh}"), &vertices, &indices)?;
    }
    Ok(())
}
fn particles(count: usize) -> Vec<Particle> {
    (0..count)
        .map(|i| Particle {
            simulation: None,
            id: i as u64 + 1,
            position: Vec3::new(
                (i % 64) as f32 * 0.5 - 16.,
                1. + (i / 64 % 8) as f32 * 0.4,
                (i / 512) as f32 * 0.6 - 4.,
            ),
            velocity: Vec3::new(0., 0.5, 0.),
            size: 0.4,
            rotation: i as f32 * 0.1,
            color: [0.7, 0.6, 0.5],
            opacity: 0.4,
            kind: ParticleKind::Smoke,
            softness: 0.3,
            trail_length: 0.,
            seed: (i % 97) as f32 / 97.,
        })
        .collect()
}
fn volume(resolution: u32) -> IrradianceVolume {
    let probes = (resolution * resolution * resolution) as usize;
    IrradianceVolume {
        min: [-20., -1., -16.],
        max: [20., 8., 16.],
        resolution: [resolution; 3],
        intensity: 1.,
        normal_bias: 0.2,
        probes: Arc::new(
            (0..probes * 41)
                .map(|i| match i % 41 {
                    0 => [0.2, 0.25, 0.3, 1.],
                    1..=8 => [0.01, 0.02, 0.01, 0.],
                    _ => [4., 16., 4., 16.],
                })
                .collect(),
        ),
    }
}
fn label(text: &str) -> DrawItem {
    DrawItem {
        motion_id: 0,
        model: Mat4::IDENTITY,
        mesh: MeshKind::Text(TextMesh {
            text: text.into(),
            screen: Some(ScreenText {
                anchor: [0.1, 0.1],
                offset: [0.; 2],
            }),
            font_size: 18.,
            ..Default::default()
        }),
        material: Material {
            lit: false,
            texture: TextureKind::Text,
            ..material(0)
        },
    }
}
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.)
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, SIZE[0], SIZE[1], |target| {
        renderer.draw(gpu, target, SIZE, scene)
    })
}
/// Absent references are written; present ones must match byte for byte.
fn reference(name: &str, frame: &Frame) -> anyhow::Result<()> {
    if let Some(directory) = std::env::var_os("BOZZARD_CACHE_REFERENCE_DIR") {
        std::fs::create_dir_all(&directory)?;
        let path = std::path::PathBuf::from(directory).join(format!("{name}.rgba"));
        if path.exists() {
            anyhow::ensure!(
                std::fs::read(&path)? == frame.rgba,
                "historical pixels differ for {name}"
            );
        } else {
            std::fs::write(path, &frame.rgba)?;
        }
    }
    Ok(())
}
#[derive(Default)]
struct Totals {
    cpu: Vec<f64>,
    wall: Vec<f64>,
    encode: Vec<f64>,
    shadow_maps: usize,
    object_buffers: usize,
    instance_buffers: usize,
    records_built: usize,
    plan_rebuilds: usize,
    bundle_compilations: usize,
    bundle_replays: usize,
    indirect_runs: usize,
    pipeline_binds: usize,
    shadow_draws: usize,
    post_bind_groups: usize,
}
impl Totals {
    fn add(&mut self, stats: &FrameStats, wall: f64) {
        self.cpu.push(stats.cpu_ms);
        self.wall.push(wall);
        self.encode.push(stats.encode_ms);
        self.shadow_maps += stats.shadow_maps_rendered;
        self.object_buffers += stats.object_buffer_allocations;
        self.instance_buffers += stats.instance_buffer_allocations;
        self.records_built += stats.surface_records_built;
        self.plan_rebuilds += stats.batch_plan_rebuilds;
        self.bundle_compilations += stats.render_bundle_compilations;
        self.bundle_replays += stats.render_bundle_replays;
        self.indirect_runs += stats.multi_draw_indirect_runs;
        self.pipeline_binds += stats.pipeline_binds;
        self.shadow_draws += stats.shadow_draws;
        self.post_bind_groups += stats.post_bind_groups;
    }
    fn print(self, workload: &str, frames: usize) {
        println!(
            "renderer_caches workload={workload} frames={frames} cpu_median_ms={:.3} synchronized_median_ms={:.3} encode_median_ms={:.3} shadow_maps={} shadow_draws={} object_buffer_allocations={} instance_buffer_allocations={} surface_records_built={} plan_rebuilds={} bundle_compilations={} bundle_replays={} indirect_runs={} pipeline_binds={} post_bind_groups={}",
            median(self.cpu),
            median(self.wall),
            median(self.encode),
            self.shadow_maps,
            self.shadow_draws,
            self.object_buffers,
            self.instance_buffers,
            self.records_built,
            self.plan_rebuilds,
            self.bundle_compilations,
            self.bundle_replays,
            self.indirect_runs,
            self.pipeline_binds,
            self.post_bind_groups,
        );
    }
}

#[test]
#[ignore = "release-mode renderer cache benchmark; run explicitly"]
fn renderer_cache_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("renderer cache benchmark target"),
            size: wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    const FRAMES: usize = 90;
    const WARMUP: usize = 10;
    let selected = std::env::var("BOZZARD_CACHE_WORKLOADS").ok();
    for workload in [
        "stream",
        "text_toggle",
        "taa_play",
        "taa_paused",
        "taa_blur",
        "particles",
        "sky",
    ] {
        if selected
            .as_deref()
            .is_some_and(|names| !names.split(',').any(|name| name == workload))
        {
            continue;
        }
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        upload_meshes(&gpu, &mut renderer)?;
        let mut scene = grid(1280);
        match workload {
            "taa_play" | "taa_paused" | "taa_blur" => {
                scene.display.temporal_aa.enabled = true;
                scene.display.motion_blur.enabled = workload == "taa_blur";
                scene.display.bloom.enabled = true;
                scene.display.auto_exposure.enabled = true;
                scene.display.depth_of_field.enabled = true;
                scene.display.depth_of_field.focus_distance = 20.;
                scene.particles = particles(4096);
                scene.gi = Some(volume(8));
            }
            "particles" => scene.particles = particles(512),
            "sky" => {
                // A camera-facing wall covers most of the view in front of the sky.
                for (i, item) in scene.items.iter_mut().enumerate() {
                    item.model = Mat4::from_translation(Vec3::new(
                        (i % 40) as f32 * 0.9 - 17.5,
                        (i / 40) as f32 * 0.9 - 6.,
                        -2.,
                    )) * Mat4::from_rotation_x(-0.55)
                        * Mat4::from_scale(Vec3::splat(0.95));
                }
                scene.lighting.shadows = false;
            }
            _ => {}
        }
        let mut totals = Totals::default();
        for frame in 0..FRAMES {
            match workload {
                "stream" => {
                    renderer.upload_image(
                        &gpu,
                        &format!("stream-{frame}"),
                        16,
                        16,
                        &[200; 1024],
                    )?;
                    if frame > 0 {
                        renderer.remove_asset(&format!("stream-{}", frame - 1));
                    }
                }
                "text_toggle" => {
                    if frame % 2 == 0 {
                        scene.items.push(label("Score"));
                    } else {
                        scene.items.pop();
                    }
                }
                "taa_play" | "taa_blur" | "particles" => {
                    scene.display.time_seconds = frame as f32 / 60.;
                }
                _ => {}
            }
            let start = std::time::Instant::now();
            renderer.draw(&gpu, &target, SIZE, &scene)?;
            gpu.wait()?;
            let wall = start.elapsed().as_secs_f64() * 1000.;
            if frame >= WARMUP {
                totals.add(&renderer.frame_stats(), wall);
            }
        }
        totals.print(workload, FRAMES - WARMUP);
        for index in 0..3 {
            if matches!(workload, "taa_play" | "taa_blur" | "particles") {
                scene.display.time_seconds = (FRAMES + index) as f32 / 60.;
            }
            reference(
                &format!("{workload}-{index}"),
                &capture(&gpu, &mut renderer, &scene)?,
            )?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "release-mode mipmap upload benchmark; run explicitly"]
fn mipmap_upload_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let images: Vec<Vec<u8>> = (0..8)
        .map(|seed| {
            (0..1024 * 1024)
                .flat_map(|i: u32| {
                    let v = (i.wrapping_mul(2_654_435_761).wrapping_add(seed * 977) >> 24) as u8;
                    [v, v / 2, 255 - v, 255]
                })
                .collect()
        })
        .collect();
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0., 0.5, 0., 0., 0., 1., 0.5, 0.],
    ];
    let indices: Vec<u32> = (0..8).flat_map(|_| [0, 1, 2]).collect();
    let mut wall = Vec::new();
    let mut cpu = Vec::new();
    for repeat in 0..12 {
        let parts: Vec<ModelPart> = images
            .iter()
            .enumerate()
            .map(|(i, rgba)| ModelPart {
                source_key: "0000000000000000",
                start: i as u32 * 3,
                count: 3,
                color: [1.; 4],
                alpha_cutoff: None,
                image: Some(ModelImage {
                    width: 1024,
                    height: 1024,
                    rgba,
                }),
                shading: None,
            })
            .collect();
        gpu.wait()?;
        let start = std::time::Instant::now();
        renderer.upload_model(&gpu, "mipmapped", &vertices, &indices, &parts)?;
        gpu.wait()?;
        if repeat >= 2 {
            wall.push(start.elapsed().as_secs_f64() * 1000.);
            cpu.push(
                renderer
                    .model_upload_stats("mipmapped")
                    .unwrap()
                    .cpu_upload_ms,
            );
        }
    }
    println!(
        "mipmap_upload images=8 size=1024 levels=11 upload_cpu_median_ms={:.3} synchronized_median_ms={:.3}",
        median(cpu),
        median(wall)
    );
    Ok(())
}

fn gpu() -> anyhow::Result<Gpu> {
    pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))
}
/// A renderer without history, for exact comparison after cache reuse.
fn fresh(
    gpu: &Gpu,
    scene: &RenderScene,
    setup: impl FnOnce(&mut SceneRenderer) -> anyhow::Result<()>,
) -> anyhow::Result<Frame> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(gpu, &mut renderer)?;
    setup(&mut renderer)?;
    capture(gpu, &mut renderer, scene)?;
    capture(gpu, &mut renderer, scene)
}
fn retained(stats: FrameStats, label: &str) {
    assert_eq!(stats.shadow_maps_rendered, 0, "{label}: shadow maps");
    assert_eq!(stats.object_buffer_allocations, 0, "{label}: objects");
    assert_eq!(stats.instance_buffer_allocations, 0, "{label}: instances");
    assert_eq!(stats.surface_records_built, 0, "{label}: preparation");
    assert_eq!(stats.batch_plan_rebuilds, 0, "{label}: plan");
    assert_eq!(stats.render_bundle_compilations, 0, "{label}: bundles");
}
fn cube(size: f32) -> (Vec<[f32; 8]>, Vec<u32>) {
    cuboid(Vec3::splat(-size), Vec3::splat(size))
}
fn cuboid(min: Vec3, max: Vec3) -> (Vec<[f32; 8]>, Vec<u32>) {
    let center = (min + max) * 0.5;
    let vertices = (0..8)
        .map(|i| {
            let p = Vec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            );
            let n = (p - center).normalize();
            [p.x, p.y, p.z, n.x, n.y, n.z, 0.5, 0.5]
        })
        .collect();
    let indices = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6,
        1, 3, 5, 3, 7, 5,
    ];
    (vertices, indices)
}

#[test]
fn unrelated_asset_publication_keeps_renderer_caches() -> anyhow::Result<()> {
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let mut scene = grid(320);
    scene.items[0].material.texture = TextureKind::Imported("decal".into());
    scene.items[0].material.lit = false;
    scene.items[1].mesh = MeshKind::Imported("spare".into());
    let decal = |renderer: &mut SceneRenderer, rgba: &[u8]| {
        renderer.upload_image(&gpu, "decal", 1, 1, rgba)
    };
    decal(&mut renderer, &[255, 0, 0, 255])?;
    let (vertices, indices) = cube(0.5);
    renderer.upload_mesh(&gpu, "spare", &vertices, &indices)?;
    renderer.upload_image(&gpu, "unused", 1, 1, &[0, 255, 0, 255])?;
    capture(&gpu, &mut renderer, &scene)?;
    let before = capture(&gpu, &mut renderer, &scene)?;
    assert!(renderer.frame_stats().render_bundle_replays > 0);

    // A new ID, or one the last frame did not draw, cannot be in any cache.
    renderer.upload_image(&gpu, "fresh", 2, 2, &[90; 16])?;
    assert_eq!(capture(&gpu, &mut renderer, &scene)?.rgba, before.rgba);
    retained(renderer.frame_stats(), "new image");
    renderer.remove_asset("unused");
    renderer.remove_asset("never-uploaded");
    assert_eq!(capture(&gpu, &mut renderer, &scene)?.rgba, before.rgba);
    retained(renderer.frame_stats(), "unreferenced removal");

    // A drawn but unlit texture rebinds its users and keeps every shadow map.
    decal(&mut renderer, &[0, 0, 255, 255])?;
    let recolored = capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().shadow_maps_rendered, 0);
    assert_ne!(recolored.rgba, before.rgba);
    let reference = fresh(&gpu, &scene, |renderer| {
        decal(renderer, &[0, 0, 255, 255])?;
        renderer.upload_mesh(&gpu, "spare", &vertices, &indices)
    })?;
    assert_eq!(
        recolored.rgba, reference.rgba,
        "same-ID texture replacement"
    );

    // A drawn caster still refreshes shadows; an evicted former caster does not.
    let (larger, larger_indices) = cube(0.9);
    renderer.upload_mesh(&gpu, "spare", &larger, &larger_indices)?;
    let reshaped = capture(&gpu, &mut renderer, &scene)?;
    assert!(renderer.frame_stats().shadow_maps_rendered > 0);
    let reference = fresh(&gpu, &scene, |renderer| {
        decal(renderer, &[0, 0, 255, 255])?;
        renderer.upload_mesh(&gpu, "spare", &larger, &larger_indices)
    })?;
    assert_eq!(reshaped.rgba, reference.rgba, "same-ID caster replacement");
    scene.items[1].mesh = MeshKind::Imported("mesh-1".into());
    capture(&gpu, &mut renderer, &scene)?;
    let settled = capture(&gpu, &mut renderer, &scene)?;
    renderer.remove_asset("spare");
    assert_eq!(capture(&gpu, &mut renderer, &scene)?.rgba, settled.rgba);
    retained(renderer.frame_stats(), "former caster eviction");
    Ok(())
}

#[test]
fn replaced_lit_receiver_geometry_refits_sun_shadows() -> anyhow::Result<()> {
    // Lit transparent surfaces never write shadow depth, but they extend the
    // fitted sun box. The replacement is a pane far below its old extent, past
    // the old far plane, where the grid's shadow falls on it only once the box
    // is fitted again; sampling beyond the far plane reads as lit.
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let mut scene = grid(320);
    scene.items.push(DrawItem {
        motion_id: 10_000,
        model: Mat4::from_translation(Vec3::new(-6., -1., -11.)),
        mesh: MeshKind::Imported("veil".into()),
        material: Material {
            texture: TextureKind::Imported("glass".into()),
            ..material(0)
        },
    });
    let glass = |renderer: &mut SceneRenderer| {
        renderer.upload_image(&gpu, "glass", 1, 1, &[200, 220, 255, 160])
    };
    glass(&mut renderer)?;
    let (small, small_indices) = cube(0.5);
    renderer.upload_mesh(&gpu, "veil", &small, &small_indices)?;
    capture(&gpu, &mut renderer, &scene)?;
    let before = capture(&gpu, &mut renderer, &scene)?;
    let (large, large_indices) = cuboid(Vec3::new(-8., -12.5, -18.), Vec3::new(8., -12., -2.));
    renderer.upload_mesh(&gpu, "veil", &large, &large_indices)?;
    let moved = capture(&gpu, &mut renderer, &scene)?;
    let reference = fresh(&gpu, &scene, |renderer| {
        glass(renderer)?;
        renderer.upload_mesh(&gpu, "veil", &large, &large_indices)
    })?;
    assert_ne!(moved.rgba, before.rgba);
    assert_eq!(moved.rgba, reference.rgba, "lit receiver replacement");
    Ok(())
}

#[test]
fn caster_replaced_while_shadows_were_off_is_redrawn() -> anyhow::Result<()> {
    // Shadow depth plans and packed caster geometry are refreshed only on
    // frames that render shadows, so they can predate the last frame.
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let mut scene = grid(320);
    scene.items[1].mesh = MeshKind::Imported("spare".into());
    let (small, small_indices) = cube(0.5);
    renderer.upload_mesh(&gpu, "spare", &small, &small_indices)?;
    capture(&gpu, &mut renderer, &scene)?;
    let before = capture(&gpu, &mut renderer, &scene)?;
    scene.lighting.shadows = false;
    scene.items[1].mesh = MeshKind::Imported("mesh-1".into());
    capture(&gpu, &mut renderer, &scene)?;
    let (large, large_indices) = cube(1.5);
    renderer.upload_mesh(&gpu, "spare", &large, &large_indices)?;
    scene.lighting.shadows = true;
    scene.items[1].mesh = MeshKind::Imported("spare".into());
    let restored = capture(&gpu, &mut renderer, &scene)?;
    let reference = fresh(&gpu, &scene, |renderer| {
        renderer.upload_mesh(&gpu, "spare", &large, &large_indices)
    })?;
    assert_ne!(restored.rgba, before.rgba);
    assert_eq!(
        restored.rgba, reference.rgba,
        "caster replaced while shadows were off"
    );
    Ok(())
}

#[test]
fn model_replacement_adds_surfaces_named_before_they_existed() -> anyhow::Result<()> {
    // An item may name a surface the resident model does not have yet; it
    // draws nothing, so no surface of the last frame refers to the model.
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let (vertices, indices) = cube(0.5);
    let part = || ModelPart {
        source_key: "0000000000000000",
        start: 0,
        count: indices.len() as u32,
        color: [1.; 4],
        alpha_cutoff: None,
        image: None,
        shading: None,
    };
    let one = [part()];
    let two = [part(), part()];
    renderer.upload_model(&gpu, "parts", &vertices, &indices, &one)?;
    let mut scene = grid(320);
    scene.items.push(DrawItem {
        motion_id: 10_000,
        model: Mat4::from_translation(Vec3::new(0., 3., 0.)) * Mat4::from_scale(Vec3::splat(4.)),
        mesh: MeshKind::ModelPart("parts".into(), 1),
        material: material(0),
    });
    capture(&gpu, &mut renderer, &scene)?;
    let before = capture(&gpu, &mut renderer, &scene)?;
    renderer.upload_model(&gpu, "parts", &vertices, &indices, &two)?;
    let after = capture(&gpu, &mut renderer, &scene)?;
    let reference = fresh(&gpu, &scene, |renderer| {
        renderer.upload_model(&gpu, "parts", &vertices, &indices, &two)
    })?;
    assert_ne!(before.rgba, reference.rgba);
    assert_eq!(after.rgba, reference.rgba, "surface added by replacement");
    Ok(())
}

#[test]
fn text_atlas_changes_keep_shadow_maps() -> anyhow::Result<()> {
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let mut scene = grid(320);
    capture(&gpu, &mut renderer, &scene)?;
    let empty = capture(&gpu, &mut renderer, &scene)?;
    scene.items.push(label("Score 10"));
    let labelled = capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().shadow_maps_rendered, 0);
    assert_eq!(renderer.frame_stats().surface_records_built, 0);
    assert_eq!(labelled.rgba, fresh(&gpu, &scene, |_| Ok(()))?.rgba);
    // Many new glyphs grow the atlas and replace its view.
    let glyphs: String = (32..0x500).filter_map(char::from_u32).collect();
    scene.items.push(label(&glyphs));
    let grown = capture(&gpu, &mut renderer, &scene)?;
    assert_eq!(renderer.frame_stats().shadow_maps_rendered, 0);
    assert_eq!(grown.rgba, fresh(&gpu, &scene, |_| Ok(()))?.rgba);
    scene.items.truncate(320);
    assert_eq!(capture(&gpu, &mut renderer, &scene)?.rgba, empty.rgba);
    assert_eq!(renderer.frame_stats().shadow_maps_rendered, 0);
    assert_eq!(renderer.frame_stats().surface_records_built, 0);
    Ok(())
}

#[test]
fn temporal_ping_pong_reuses_post_bind_groups() -> anyhow::Result<()> {
    let gpu = gpu()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload_meshes(&gpu, &mut renderer)?;
    let mut scene = grid(320);
    scene.display.temporal_aa.enabled = true;
    scene.display.bloom.enabled = true;
    scene.display.auto_exposure.enabled = true;
    scene.display.depth_of_field.enabled = true;
    for blur in [false, true] {
        scene.display.motion_blur.enabled = blur;
        let mut created = Vec::new();
        for frame in 0..6 {
            scene.display.time_seconds = frame as f32 / 60.;
            capture(&gpu, &mut renderer, &scene)?;
            created.push(renderer.frame_stats().post_bind_groups);
        }
        // Both history indices bind once; later frames alternate between them.
        assert_eq!(created[2..], [0; 4], "motion blur {blur}: {created:?}");
    }
    Ok(())
}
