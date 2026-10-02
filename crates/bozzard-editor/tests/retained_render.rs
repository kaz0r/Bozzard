//! Paired native profiles include scene extraction as well as renderer preparation.
use anyhow::{Result, ensure};
use bozzard_editor::Editor;
use bozzard_render::{FrameStats, Gpu, RenderScene, SceneRenderer, wgpu};
use bozzard_scene::{Layer, Scene, blueprint::BlackboardValue, blueprint::Value};
use glam::Mat4;
use serde_json::json;
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "native full-pipeline parity; accepts software graphics adapters"]
fn retained_factory_pixels_and_checkpoints_match_reference() -> Result<()> {
    use bozzard_scene::{Drawable, Texture, Transform};
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    let mut editor = dense_factory()?;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&bozzard_render::instance(
        bozzard_render::Backend::native(),
    )))?;
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_surface_preparation_caching_enabled(false);
    upload_assets(&editor, &gpu, &mut renderers)?;
    let play = editor.play.as_mut().unwrap();
    let camera_id = &play.instance().document().views[&Layer::ThreeD];
    let camera_pose = play.instance().global_transforms(&play.app.world)?[camera_id];
    let mut root = play.with_instance(|instance, world| {
        instance.spawn_prefab(world, "machine-kiln", [0., 0.08, 0.])
    })?;
    let (drawable_id, original_drawable) = {
        let play = editor.play.as_ref().unwrap();
        play.instance().document().prefabs[&root]
            .members
            .values()
            .find_map(|id| {
                let entity = play.instance().entity(id)?;
                play.app
                    .world
                    .get::<Drawable>(entity)
                    .map(|drawable| (id.clone(), drawable.clone()))
            })
            .expect("the real kiln prefab provides a drawable")
    };
    let mut held_frames = None;
    for phase in 0_usize..8 {
        if phase >= 2 {
            let play = editor.play.as_mut().unwrap();
            play.app.step();
            play.check_simulation()?;
        }
        {
            let play = editor.play.as_mut().unwrap();
            match phase {
                2 => {
                    let entity = play.instance().entity(&root).unwrap();
                    let mut transform = play.app.world.get_mut::<Transform>(entity).unwrap();
                    transform.translation[0] += 2.;
                    transform.rotation_degrees[1] = 45.;
                    let entity = play.instance().entity(&drawable_id).unwrap();
                    play.app.world.get_mut::<Drawable>(entity).unwrap().color = [0.8, 0.3, 0.6];
                }
                3 => {
                    let entity = play.instance().entity(&drawable_id).unwrap();
                    let _ = play.app.world.remove::<Drawable>(entity)?;
                }
                4 => {
                    let entity = play.instance().entity(&drawable_id).unwrap();
                    let mut drawable = original_drawable.clone();
                    drawable.texture = Texture::Checker;
                    let _ = play.app.world.insert(entity, drawable)?;
                }
                5 => {
                    root = play.with_instance(|instance, world| {
                        instance.destroy_prefab(world, &root)?;
                        instance.spawn_prefab(world, "machine-kiln", [3., 0.08, 1.])
                    })?;
                }
                6 => {
                    play.with_instance(|instance, world| instance.destroy_prefab(world, &root))?;
                }
                _ => {}
            }
        }
        let expected_state = state(&editor)?;
        // The first two frames have identical inputs; subsequent frames rotate
        // only the inspection camera while real factory motion and edits advance.
        let pose = Some(Mat4::from_rotation_y(phase.saturating_sub(1) as f32 * 0.1) * camera_pose);
        let reference = extract(&editor, false, pose)?;
        let retained = extract(&editor, true, pose)?;
        if phase == 1 {
            let stats = retained.stats().unwrap();
            ensure!(
                stats.material_reuses > 0 && stats.pooled_items > 0,
                "warm adapter records were not reused"
            );
        }
        compare_pixels(
            &gpu,
            &mut renderers,
            [reference.scene(), retained.scene()],
            [640, 400],
            &format!("edit phase {phase}"),
        )?;
        if phase == 1 {
            ensure!(
                renderers[1].frame_stats().surface_records_reused > 0,
                "warm surfaces were not reused"
            );
            // A later extraction cannot recycle or overwrite these live records.
            held_frames = Some((reference, retained));
        }
        ensure!(
            state(&editor)? == expected_state,
            "rendering changed gameplay at phase {phase}"
        );
    }
    if let Some((reference, retained)) = held_frames {
        compare_pixels(
            &gpu,
            &mut renderers,
            [reference.scene(), retained.scene()],
            [640, 400],
            "live frozen frame after later edits",
        )?;
    }
    for heading in [0_f32, 90., 180., 270.] {
        let expected_state = state(&editor)?;
        let pose = Some(Mat4::from_rotation_y(heading.to_radians()) * camera_pose);
        let reference = extract(&editor, false, pose)?;
        let retained = extract(&editor, true, pose)?;
        compare_pixels(
            &gpu,
            &mut renderers,
            [reference.scene(), retained.scene()],
            [640, 400],
            &format!("camera heading {heading}"),
        )?;
        ensure!(
            state(&editor)? == expected_state,
            "capture changed gameplay"
        );
    }
    println!(
        "retained_factory_parity_ok: warm reuse, motion, camera, component removal/reinsert, prefab despawn/respawn, frozen frame; exact pixels and checkpoints"
    );
    Ok(())
}

#[test]
#[ignore = "release-mode retained-scene profile; requires a hardware graphics adapter"]
fn profile_earth_factory_retained_active() -> Result<()> {
    profile(Workload::Active)
}

#[test]
#[ignore = "release-mode retained-scene profile; requires a hardware graphics adapter"]
fn profile_earth_factory_retained_frozen() -> Result<()> {
    profile(Workload::Frozen)
}

#[test]
#[ignore = "release-mode retained-scene profile; requires a hardware graphics adapter"]
fn profile_earth_factory_retained_camera() -> Result<()> {
    profile(Workload::Camera)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workload {
    Active,
    Frozen,
    Camera,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Active => "active_factory",
            Self::Frozen => "frozen_factory",
            Self::Camera => "camera_only_factory",
        }
    }

    fn inspection_pose(self, frame: usize, initial: Mat4) -> Option<Mat4> {
        (self == Self::Camera).then(|| Mat4::from_rotation_y(frame as f32 * 0.02) * initial)
    }
}

struct Sample {
    extraction: f64,
    retirement: f64,
    total_cpu: f64,
    synchronized: f64,
    renderer: FrameStats,
    adapter: Option<bozzard_render_assets::RenderSceneStats>,
}

/// Reference frames own their ordinary records; retained frames return storage on drop.
enum Input {
    Reference(RenderScene),
    Retained(bozzard_render_assets::RenderFrame),
}

impl Input {
    fn scene(&self) -> &RenderScene {
        match self {
            Self::Reference(scene) => scene,
            Self::Retained(frame) => frame,
        }
    }

    fn stats(&self) -> Option<bozzard_render_assets::RenderSceneStats> {
        match self {
            Self::Reference(_) => None,
            Self::Retained(frame) => Some(frame.stats()),
        }
    }
}

fn extract(editor: &Editor, retained: bool, pose: Option<Mat4>) -> Result<Input> {
    if retained {
        Ok(Input::Retained(editor.render_frame_from_camera(
            Layer::ThreeD,
            1.6,
            pose,
        )?))
    } else {
        Ok(Input::Reference(editor.render_from_camera(
            Layer::ThreeD,
            1.6,
            pose,
        )?))
    }
}

fn state(editor: &Editor) -> Result<(Scene, String)> {
    let play = editor.play.as_ref().unwrap();
    Ok((
        play.instance().capture(&play.app.world)?,
        play.instance().save_game_json(&play.app.world)?,
    ))
}

fn dense_factory() -> Result<Editor> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    let controller = scene
        .objects
        .iter_mut()
        .find(|object| object.id == "controller")
        .unwrap();
    for (name, value) in [("title_open", false), ("creative", true)] {
        controller
            .blackboard
            .insert(name.into(), BlackboardValue::Scalar(Value::Bool(value)));
    }
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    editor.apply("Retained rendering profile", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_start(me)", "fn normal_start(me)")
        .replace("fn on_update(me, dt)", "fn normal_update(me, dt)");
    editor.play.as_mut().unwrap().with_instance(|instance, _| {
        instance.register_script(
            "earth-factory".into(),
            format!("{source}\n{}", include_str!("fixtures/dense_lighting.rhai")),
        )
    })?;
    for _ in 0..60 {
        let play = editor.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
    }
    Ok(editor)
}

fn distribution(mut values: Vec<f64>) -> serde_json::Value {
    if values.is_empty() {
        return json!({ "samples": 0 });
    }
    values.sort_by(f64::total_cmp);
    let percentile = |percentage: usize| values[(values.len() * percentage).div_ceil(100) - 1];
    json!({
        "samples": values.len(),
        "median_ms": (values[(values.len() - 1) / 2] + values[values.len() / 2]) * 0.5,
        "p95_ms": percentile(95),
        "p99_ms": percentile(99),
    })
}

fn counts_equal(a: FrameStats, b: FrameStats) {
    assert_eq!(a.scene_items, b.scene_items);
    assert_eq!(a.surfaces, b.surfaces);
    assert_eq!(a.visible_items, b.visible_items);
    assert_eq!(a.visible_surfaces, b.visible_surfaces);
    assert_eq!(a.color_draws, b.color_draws);
    assert_eq!(a.color_triangles, b.color_triangles);
    assert_eq!(a.shadow_draws, b.shadow_draws);
    assert_eq!(a.shadow_triangles, b.shadow_triangles);
}

fn upload_assets(editor: &Editor, gpu: &Gpu, renderers: &mut [SceneRenderer; 2]) -> Result<()> {
    for renderer in renderers {
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(gpu, renderer, &entry.id, data)?;
            }
        }
    }
    Ok(())
}

fn compare_pixels(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scenes: [&RenderScene; 2],
    size: [u32; 2],
    context: &str,
) -> Result<()> {
    let mut reference = None;
    for (renderer, scene) in renderers.iter_mut().zip(scenes) {
        let capture = bozzard_render::capture_offscreen(gpu, size[0], size[1], |target| {
            renderer.draw(gpu, target, size, scene)
        })?;
        ensure!(
            capture
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel[..3] != capture.rgba[..3]),
            "{context}: capture contains no rendered world"
        );
        if let Some(reference) = &reference {
            ensure!(
                capture.rgba == *reference,
                "{context}: factory pixels differ"
            );
        } else {
            reference = Some(capture.rgba);
        }
    }
    counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
    Ok(())
}

fn profile(workload: Workload) -> Result<()> {
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    let mut editor = dense_factory()?;
    let play = editor.play.as_ref().unwrap();
    let camera_id = &play.instance().document().views[&Layer::ThreeD];
    let camera_pose = play.instance().global_transforms(&play.app.world)?[camera_id];
    let frozen_state = state(&editor)?;
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    gpu.require_hardware()?;
    let info = gpu.adapter.get_info();
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_surface_preparation_caching_enabled(false);
    for renderer in &mut renderers {
        renderer.set_profiling_enabled(true);
    }
    upload_assets(&editor, &gpu, &mut renderers)?;
    let size = [1280, 800];
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("retained factory profile"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
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
    let mut samples: [Vec<Sample>; 2] = Default::default();
    let mut gpu_samples: [Vec<f64>; 2] = Default::default();
    for frame in 0..72 {
        if workload == Workload::Active {
            let play = editor.play.as_mut().unwrap();
            play.app.step();
            play.check_simulation()?;
        }
        let expected_state = [0, 12, 36, 71]
            .contains(&frame)
            .then(|| state(&editor))
            .transpose()?;
        let pose = workload.inspection_pose(frame, camera_pose);
        for mode in if frame % 2 == 0 { [0, 1] } else { [1, 0] } {
            let started = Instant::now();
            let input = extract(&editor, mode == 1, pose)?;
            let extraction = started.elapsed().as_secs_f64() * 1000.;
            renderers[mode].draw(&gpu, &target, size, input.scene())?;
            let renderer = renderers[mode].frame_stats();
            let adapter = input.stats();
            gpu.wait()?;
            let retiring = Instant::now();
            // Include frame retirement in full-path timing. Retained frames return
            // their payloads to the pool; legacy records release their allocations.
            drop(input);
            let retirement = retiring.elapsed().as_secs_f64() * 1000.;
            let synchronized = started.elapsed().as_secs_f64() * 1000.;
            let profiles = renderers[mode].poll_gpu_profiles(&gpu)?;
            if frame >= 12 {
                for profile in profiles {
                    if !profile.failed
                        && profile.omitted == 0
                        && let Some(times) = profile
                            .passes
                            .iter()
                            .map(|pass| pass.milliseconds)
                            .collect::<Option<Vec<_>>>()
                    {
                        gpu_samples[mode].push(times.into_iter().sum());
                    }
                }
                samples[mode].push(Sample {
                    extraction,
                    retirement,
                    total_cpu: extraction + renderer.cpu_ms + retirement,
                    synchronized,
                    renderer,
                    adapter,
                });
            }
        }
        counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        if let Some(expected) = expected_state {
            ensure!(
                state(&editor)? == expected,
                "rendering changed gameplay at frame {frame}"
            );
        }
    }
    if workload != Workload::Active {
        ensure!(
            state(&editor)? == frozen_state,
            "frozen rendering advanced gameplay"
        );
    }
    let output = std::env::var_os("BOZZARD_RETAINED_RENDER_OUTPUT").map(PathBuf::from);
    if let Some(path) = &output {
        std::fs::create_dir_all(path)?;
    }
    // Reference and retained extraction each feed independent renderer histories.
    // Captures are outside all timing and use the same authoritative world state.
    for heading in [0_f32, 90., 180., 270.] {
        let expected_state = state(&editor)?;
        let pose = Some(Mat4::from_rotation_y(heading.to_radians()) * camera_pose);
        let mut reference = None;
        for (mode, renderer) in renderers.iter_mut().enumerate() {
            let input = extract(&editor, mode == 1, pose)?;
            let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                renderer.draw(&gpu, target, size, input.scene())
            })?;
            if let Some(reference) = &reference {
                ensure!(
                    capture.rgba == *reference,
                    "factory pixels differ at {heading} degrees"
                );
            } else {
                reference = Some(capture.rgba.clone());
            }
            if let Some(path) = &output {
                capture.write_ppm(&path.join(format!(
                    "{}-heading{heading:.0}-{}.ppm",
                    workload.name(),
                    if mode == 0 { "reference" } else { "retained" },
                )))?;
            }
        }
        counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        ensure!(
            state(&editor)? == expected_state,
            "capture changed gameplay"
        );
    }
    let results = samples
        .iter()
        .enumerate()
        .map(|(mode, samples)| {
            let stage =
                |value: fn(&Sample) -> f64| distribution(samples.iter().map(value).collect());
            let adapter_stage = |value: fn(&bozzard_render_assets::RenderSceneStats) -> f64| {
                if mode == 0 {
                    serde_json::Value::Null
                } else {
                    distribution(
                        samples
                            .iter()
                            .map(|sample| value(sample.adapter.as_ref().unwrap()))
                            .collect(),
                    )
                }
            };
            let final_sample = samples.last().unwrap();
            let adapter = final_sample.adapter.as_ref();
            json!({
                "mode": if mode == 0 { "owned_reference_full_path" } else { "retained_full_path" },
                "extraction": stage(|sample| sample.extraction),
                "frame_retirement": stage(|sample| sample.retirement),
                "adapter": adapter_stage(|stats| stats.adapter_ms),
                "asset_scan": adapter_stage(|stats| stats.asset_scan_ms),
                "material_prepare": adapter_stage(|stats| stats.material_prepare_ms),
                "surface_prepare": stage(|sample| sample.renderer.surface_prepare_ms),
                "total_cpu": stage(|sample| sample.total_cpu),
                "renderer_cpu": stage(|sample| sample.renderer.cpu_ms),
                "renderer_prepare": stage(|sample| sample.renderer.prepare_ms),
                "renderer_encode": stage(|sample| sample.renderer.encode_ms),
                "renderer_submit": stage(|sample| sample.renderer.submit_ms),
                "synchronized": stage(|sample| sample.synchronized),
                "gpu_pass_sum": distribution(std::mem::take(&mut gpu_samples[mode])),
                "last_measured_frame": {
                    "adapter": adapter.map(|stats| json!({
                        "material_reuses": stats.material_reuses,
                        "material_rebuilds": stats.material_rebuilds,
                        "pooled_items": stats.pooled_items,
                        "assets_changed": stats.assets_changed,
                        "retained_frames": stats.retained_frames,
                    })),
                    "renderer": final_sample.renderer,
                },
            })
        })
        .collect::<Vec<_>>();
    let report = json!({
        "workload": workload.name(),
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "adapter": info.name,
        "backend": info.backend.to_str(),
        "viewport": size,
        "warmup_frames": 12,
        "measured_frames": 60,
        "alternating_mode_order": true,
        "exact_pixels": true,
        "submitted_counts_equal": true,
        "scene_and_checkpoint_preserved": true,
        "results": results,
    });
    println!("retained_factory_profile={report}");
    if let Some(path) = output {
        std::fs::write(
            path.join(format!("{}.json", workload.name())),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(())
}
