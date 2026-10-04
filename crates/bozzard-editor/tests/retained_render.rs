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
    profile(Workload::Active, false)
}

#[test]
#[ignore = "release-mode retained-scene profile; requires a hardware graphics adapter"]
fn profile_earth_factory_retained_frozen() -> Result<()> {
    profile(Workload::Frozen, false)
}

#[test]
#[ignore = "release-mode retained-scene profile; requires a hardware graphics adapter"]
fn profile_earth_factory_retained_camera() -> Result<()> {
    profile(Workload::Camera, false)
}

#[test]
#[ignore = "native batch-order parity; accepts software graphics adapters"]
fn batch_planning_factory_pixels_match_rebuilt_plans_during_motion() -> Result<()> {
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    let mut editor = dense_factory()?;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&bozzard_render::instance(
        bozzard_render::Backend::native(),
    )))?;
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_incremental_batch_planning_enabled(false);
    for renderer in &mut renderers {
        renderer.set_occlusion_enabled(false);
    }
    upload_assets(&editor, &gpu, &mut renderers)?;
    let play = editor.play.as_ref().unwrap();
    let camera_id = &play.instance().document().views[&Layer::ThreeD];
    let camera_pose = play.instance().global_transforms(&play.app.world)?[camera_id];
    let mut reused = 0;
    let mut recertified = 0;
    for frame in 0..20 {
        let play = editor.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
        let expected = state(&editor)?;
        let pose = Some(Mat4::from_rotation_y(frame as f32 * 0.08) * camera_pose);
        let input = extract(&editor, true, pose)?;
        compare_geometry_pixels(
            &gpu,
            &mut renderers,
            [input.scene(), input.scene()],
            [640, 400],
            &format!("batch-planning motion frame {frame}"),
            false,
        )?;
        let stats = renderers[1].frame_stats();
        reused += usize::from(stats.batch_plan_reused);
        recertified += stats.batch_plan_recertifications;
        ensure!(
            state(&editor)? == expected,
            "rendering changed gameplay at frame {frame}"
        );
    }
    ensure!(
        reused > 10 && recertified > 0,
        "parity must exercise retained and renewed plans"
    );
    println!(
        "batch_planning_factory_parity_ok: 20 moving frames, exact pixels, geometry and checkpoints; {reused} reuses, {recertified} renewals"
    );
    Ok(())
}

/// Same retained extraction and surface cache on both sides: only changed-plan
/// rebuilding versus incremental planning differs. No user saves are accessed.
#[test]
#[ignore = "release-mode paired batch-planning profile; requires a hardware graphics adapter"]
fn profile_earth_factory_batch_planning() -> Result<()> {
    for workload in [Workload::Active, Workload::Frozen, Workload::Camera] {
        profile(workload, true)?;
    }
    Ok(())
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
    geometry_equal(a, b);
    assert_eq!(a.color_draws, b.color_draws);
}

fn geometry_equal(a: FrameStats, b: FrameStats) {
    assert_eq!(a.scene_items, b.scene_items);
    assert_eq!(a.surfaces, b.surfaces);
    assert_eq!(a.visible_items, b.visible_items);
    assert_eq!(a.visible_surfaces, b.visible_surfaces);
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
    compare_geometry_pixels(gpu, renderers, scenes, size, context, true)
}

fn compare_geometry_pixels(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scenes: [&RenderScene; 2],
    size: [u32; 2],
    context: &str,
    identical_draw_counts: bool,
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
    if identical_draw_counts {
        counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
    } else {
        geometry_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
    }
    Ok(())
}

fn profile(workload: Workload, batch_planning: bool) -> Result<()> {
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
    if batch_planning {
        renderers[0].set_incremental_batch_planning_enabled(false);
    } else {
        renderers[0].set_surface_preparation_caching_enabled(false);
    }
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
            let input = extract(&editor, batch_planning || mode == 1, pose)?;
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
        if batch_planning {
            geometry_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        } else {
            counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        }
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
            let input = extract(&editor, batch_planning || mode == 1, pose)?;
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
                    "{}{}-heading{heading:.0}-{}.ppm",
                    if batch_planning {
                        "batch-planning-"
                    } else {
                        ""
                    },
                    workload.name(),
                    if mode == 0 { "reference" } else { "retained" },
                )))?;
            }
        }
        if batch_planning {
            geometry_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        } else {
            counts_equal(renderers[0].frame_stats(), renderers[1].frame_stats());
        }
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
                if mode == 0 && !batch_planning {
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
            let mut rebuild_reasons = std::collections::BTreeMap::new();
            for sample in samples {
                if let Some(reason) = sample.renderer.batch_plan_rebuild_reason {
                    *rebuild_reasons.entry(reason).or_insert(0_usize) += 1;
                }
            }
            json!({
                "mode": if batch_planning {
                    if mode == 0 { "rebuild_changed_plans" } else { "incremental_plans" }
                } else if mode == 0 { "owned_reference_full_path" } else { "retained_full_path" },
                "batch_planning": stage(|sample| sample.renderer.batch_plan_ms),
                "visibility": stage(|sample| sample.renderer.visibility_ms),
                "occlusion_prepare": stage(|sample| sample.renderer.occlusion_prepare_ms),
                "batch_plan_rebuild_reasons": rebuild_reasons,
                "batch_plan_reused_frames": samples.iter().filter(|sample| sample.renderer.batch_plan_reused).count(),
                "batch_plan_recertified_frames": samples.iter().filter(|sample| sample.renderer.batch_plan_recertifications > 0).count(),
                "frame_samples": samples.iter().map(|sample| json!({
                    "batch_plan_ms": sample.renderer.batch_plan_ms,
                    "renderer_cpu_ms": sample.renderer.cpu_ms,
                    "renderer_prepare_ms": sample.renderer.prepare_ms,
                    "batch_plan_rebuild_reason": sample.renderer.batch_plan_rebuild_reason,
                    "batch_plan_reused": sample.renderer.batch_plan_reused,
                    "batch_plan_recertifications": sample.renderer.batch_plan_recertifications,
                    "batch_bounds_updates": sample.renderer.batch_bounds_updates,
                    "batch_order_checks": sample.renderer.batch_order_checks,
                    "color_draws": sample.renderer.color_draws,
                    "color_triangles": sample.renderer.color_triangles,
                })).collect::<Vec<_>>(),
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
        "comparison": if batch_planning { "batch_planning" } else { "retained_render_scenes" },
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "adapter": info.name,
        "backend": info.backend.to_str(),
        "viewport": size,
        "warmup_frames": 12,
        "measured_frames": 60,
        "alternating_mode_order": true,
        "exact_pixels": true,
        "submitted_counts_equal": !batch_planning,
        "submitted_geometry_equal": true,
        "scene_and_checkpoint_preserved": true,
        "results": results,
    });
    println!("retained_factory_profile={report}");
    if let Some(path) = output {
        std::fs::write(
            path.join(format!(
                "{}{}.json",
                if batch_planning {
                    "batch-planning-"
                } else {
                    ""
                },
                workload.name()
            )),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(())
}

/// Same retained scene input, stock batching and shadow caches on both sides.
/// Only opaque shader-graph eligibility differs. No user saves are accessed.
#[test]
#[ignore = "release-mode graph-instancing profile; requires a hardware graphics adapter"]
fn profile_earth_factory_graph_instancing() -> Result<()> {
    for workload in [Workload::Active, Workload::Frozen, Workload::Camera] {
        profile_batching(workload, BatchingComparison::GraphInstancing)?;
    }
    Ok(())
}

/// Both sides retain the same color plan, graph eligibility and shadow caches.
#[test]
#[ignore = "release-mode batch-preparation profile; requires a hardware graphics adapter"]
fn profile_earth_factory_batch_preparation() -> Result<()> {
    for workload in [Workload::Active, Workload::Frozen, Workload::Camera] {
        profile_batching(workload, BatchingComparison::BatchPreparation)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchingComparison {
    GraphInstancing,
    BatchPreparation,
    ResourceMetadata,
}
impl BatchingComparison {
    fn configure(self, renderer: &mut SceneRenderer) {
        match self {
            Self::GraphInstancing => renderer.set_shader_graph_instancing_enabled(false),
            Self::BatchPreparation => renderer.set_batch_preparation_caching_enabled(false),
            Self::ResourceMetadata => renderer.set_resource_metadata_caching_enabled(false),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::GraphInstancing => "shader_graph_instancing",
            Self::BatchPreparation => "batch_preparation",
            Self::ResourceMetadata => "resource_metadata",
        }
    }
    fn modes(self) -> [&'static str; 2] {
        match self {
            Self::GraphInstancing => ["individual_graphs", "instanced_graphs"],
            Self::BatchPreparation => ["per_frame_preparation", "retained_preparation"],
            Self::ResourceMetadata => ["per_frame_resources", "retained_resources"],
        }
    }
    fn output_env(self) -> &'static str {
        match self {
            Self::GraphInstancing => "BOZZARD_GRAPH_INSTANCING_OUTPUT",
            Self::BatchPreparation => "BOZZARD_BATCH_PREPARATION_OUTPUT",
            Self::ResourceMetadata => "BOZZARD_RESOURCE_METADATA_OUTPUT",
        }
    }
}

#[test]
#[ignore = "release-mode resource-metadata profile; requires a hardware graphics adapter"]
fn profile_earth_factory_resource_metadata() -> Result<()> {
    for workload in [Workload::Active, Workload::Frozen, Workload::Camera] {
        profile_batching(workload, BatchingComparison::ResourceMetadata)?;
    }
    Ok(())
}

fn preparation_work(stats: &FrameStats) -> serde_json::Value {
    json!({
        "surfaces": stats.surfaces,
        "mesh_validation_checks": stats.mesh_validation_checks,
        "resource_metadata_hits": stats.resource_metadata_hits,
        "resource_metadata_builds": stats.resource_metadata_builds,
        "resource_metadata_bypasses": stats.resource_metadata_bypasses,
        "resource_bounds_lookups": stats.resource_bounds_lookups,
        "resource_metadata_bytes": stats.resource_metadata_bytes,
        "object_binding_allocations": stats.object_binding_allocations,
        "graph_source_checks": stats.graph_source_checks,
        "graph_instance_checks": stats.graph_instance_checks,
        "object_uniform_source_checks": stats.object_uniform_source_checks,
        "object_matrix_checks": stats.object_matrix_checks,
        "object_uniform_builds": stats.object_uniform_builds,
        "light_mask_checks": stats.light_mask_checks,
        "light_mask_cache_hits": stats.light_mask_cache_hits,
        "light_mask_builds": stats.light_mask_builds,
        "individual_uniform_candidates": stats.individual_uniform_candidates,
        "object_uniform_writes": stats.object_uniform_writes,
        "scratch_bounds_bytes": stats.scratch_bounds_bytes,
        "scratch_visibility_bytes": stats.scratch_visibility_bytes,
        "scratch_visible_items_bytes": stats.scratch_visible_items_bytes,
        "scratch_individual_bytes": stats.scratch_individual_bytes,
        "scratch_transparent_bytes": stats.scratch_transparent_bytes,
        "scratch_capacity_bytes": stats.preparation_scratch_bytes(),
    })
}

fn batch_frame_sample(stats: &FrameStats, wall: f64) -> serde_json::Value {
    json!({
        "renderer_cpu_ms": stats.cpu_ms, "synchronized_ms": wall,
        "renderer_prepare_ms": stats.prepare_ms,
        "preparation_stages_ms": stats.preparation_stages().into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        "preparation_work": preparation_work(stats),
        "color_draws": stats.color_draws, "color_triangles": stats.color_triangles,
        "shadow_draws": stats.shadow_draws, "shadow_triangles": stats.shadow_triangles,
        "sun_depth_copies": stats.sun_depth_copies, "batching": stats.batching,
        "graph_instanced_compilations": stats.graph_instanced_compilations,
        "batch_diagnostics_ms": stats.batch_diagnostics_ms,
        "instance_prepare_ms": stats.instance_prepare_ms,
        "shadow_batch_plan_ms": stats.shadow_batch_plan_ms,
        "shadow_instance_prepare_ms": stats.shadow_instance_prepare_ms,
        "batch_diagnostics_reused": stats.batch_diagnostics_reused,
        "shadow_batch_plan_reused": stats.shadow_batch_plan_reused,
        "instance_uniform_bytes": stats.instance_uniform_bytes,
        "shadow_instance_uniform_bytes": stats.shadow_instance_uniform_bytes,
        "instance_buffer_bytes": stats.instance_buffer_bytes,
        "shadow_instance_buffer_bytes": stats.shadow_instance_buffer_bytes,
    })
}

fn profile_batching(workload: Workload, comparison: BatchingComparison) -> Result<()> {
    let preparation = comparison != BatchingComparison::GraphInstancing;
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
    comparison.configure(&mut renderers[0]);
    for renderer in &mut renderers {
        renderer.set_profiling_enabled(true);
    }
    upload_assets(&editor, &gpu, &mut renderers)?;
    let size = [1280, 800];
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("graph instancing factory profile"),
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
    let mut samples: [Vec<(FrameStats, f64)>; 2] = Default::default();
    let mut gpu_samples: [Vec<f64>; 2] = Default::default();
    let mut submitted_shadow_triangles_equal = true;
    for frame in 0..72 {
        if workload == Workload::Active {
            let play = editor.play.as_mut().unwrap();
            play.app.step();
            play.check_simulation()?;
        }
        let checkpoint = [0, 12, 36, 71]
            .contains(&frame)
            .then(|| state(&editor))
            .transpose()?;
        let input = extract(&editor, true, workload.inspection_pose(frame, camera_pose))?;
        for mode in if frame % 2 == 0 { [0, 1] } else { [1, 0] } {
            let started = Instant::now();
            renderers[mode].draw(&gpu, &target, size, input.scene())?;
            gpu.wait()?;
            let synchronized = started.elapsed().as_secs_f64() * 1000.;
            let profiles = renderers[mode].poll_gpu_profiles(&gpu)?;
            if frame >= 12 {
                samples[mode].push((renderers[mode].frame_stats(), synchronized));
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
            }
        }
        let a = renderers[0].frame_stats();
        let b = renderers[1].frame_stats();
        for stats in [a, b] {
            let stages = stats.preparation_stages();
            ensure!(
                stages.iter().all(|(_, ms)| ms.is_finite() && *ms >= 0.),
                "invalid CPU stage sample"
            );
            let sum: f64 = stages.iter().map(|(_, ms)| ms).sum();
            ensure!(
                (sum - stats.prepare_ms).abs() <= stats.prepare_ms.max(1.) * 1e-9,
                "preparation stage overlap/gap: {sum} vs {}",
                stats.prepare_ms
            );
            ensure!(
                stats.resource_metadata_hits
                    + stats.resource_metadata_builds
                    + stats.resource_metadata_bypasses
                    == stats.surfaces
                    && stats.resource_bounds_lookups
                        == stats.resource_metadata_builds + stats.resource_metadata_bypasses,
                "resource work counters do not partition surfaces"
            );
            ensure!(
                stats.light_mask_checks == stats.light_mask_cache_hits + stats.light_mask_builds,
                "light-mask work counters do not partition checks"
            );
        }
        assert_eq!(
            (
                a.scene_items,
                a.surfaces,
                a.visible_items,
                a.visible_surfaces,
                a.color_triangles
            ),
            (
                b.scene_items,
                b.surfaces,
                b.visible_items,
                b.visible_surfaces,
                b.color_triangles
            )
        );
        if preparation {
            counts_equal(a, b);
            assert_eq!(
                serde_json::to_value(a.batching)?,
                serde_json::to_value(b.batching)?
            );
        }
        // Static-depth caching can submit fewer casters or a depth-copy triangle.
        // Keep normal caches enabled and report this difference, not a false claim
        // that both renderers necessarily submit identical shadow geometry.
        submitted_shadow_triangles_equal &= a.shadow_triangles == b.shadow_triangles;
        if let Some(checkpoint) = checkpoint {
            ensure!(state(&editor)? == checkpoint, "rendering changed gameplay");
        }
    }
    if workload != Workload::Active {
        ensure!(
            state(&editor)? == frozen_state,
            "frozen rendering advanced gameplay"
        );
    }
    let output = std::env::var_os(comparison.output_env()).map(PathBuf::from);
    if let Some(path) = &output {
        std::fs::create_dir_all(path)?;
    }
    for heading in [0_f32, 90., 180., 270.] {
        let checkpoint = state(&editor)?;
        let input = extract(
            &editor,
            true,
            Some(Mat4::from_rotation_y(heading.to_radians()) * camera_pose),
        )?;
        let mut reference = None;
        for (mode, renderer) in renderers.iter_mut().enumerate() {
            let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                renderer.draw(&gpu, target, size, input.scene())
            })?;
            ensure!(
                capture
                    .rgba
                    .chunks_exact(4)
                    .any(|pixel| pixel[..3] != capture.rgba[..3]),
                "capture contains no world"
            );
            if let Some(reference) = &reference {
                ensure!(
                    capture.rgba == *reference,
                    "graph pixels differ at heading {heading}"
                );
            } else {
                reference = Some(capture.rgba.clone());
            }
            if let Some(path) = &output {
                capture.write_ppm(&path.join(format!(
                    "{}-heading{heading:.0}-{mode}.ppm",
                    workload.name()
                )))?;
            }
        }
        ensure!(state(&editor)? == checkpoint, "capture changed gameplay");
    }
    let results: Vec<_> = samples.iter().enumerate().map(|(mode, samples)| {
        let stage = |value: fn(&FrameStats) -> f64| distribution(samples.iter().map(|(stats, _)| value(stats)).collect());
        let preparation_stages: std::collections::BTreeMap<_, _> = samples[0].0.preparation_stages()
            .into_iter().enumerate().map(|(index, (name, _))| {
                (name, distribution(samples.iter().map(|(stats, _)| stats.preparation_stages()[index].1).collect()))
            }).collect();
        json!({
            "preparation_stages": preparation_stages,
            "mode": comparison.modes()[mode],
            "renderer_cpu": stage(|s| s.cpu_ms), "renderer_prepare": stage(|s| s.prepare_ms),
            "renderer_encode": stage(|s| s.encode_ms), "renderer_submit": stage(|s| s.submit_ms),
            "batch_planning": stage(|s| s.batch_plan_ms),
            "batch_diagnostics": stage(|s| s.batch_diagnostics_ms),
            "instance_prepare": stage(|s| s.instance_prepare_ms),
            "shadow_batch_plan": stage(|s| s.shadow_batch_plan_ms),
            "shadow_instance_prepare": stage(|s| s.shadow_instance_prepare_ms),
            "instance_upload_bytes": samples.iter().map(|(s, _)| s.instance_uniform_bytes).sum::<usize>(),
            "shadow_instance_upload_bytes": samples.iter().map(|(s, _)| s.shadow_instance_uniform_bytes).sum::<usize>(),
            "instance_buffer_allocations": samples.iter().map(|(s, _)| s.instance_buffer_allocations).sum::<usize>(),
            "diagnostics_reused_frames": samples.iter().filter(|(s, _)| s.batch_diagnostics_reused).count(),
            "shadow_plan_reused_frames": samples.iter().filter(|(s, _)| s.shadow_batch_plan_reused).count(),
            "synchronized": distribution(samples.iter().map(|(_, wall)| *wall).collect()),
            "gpu_pass_sum": distribution(std::mem::take(&mut gpu_samples[mode])),
            "last_measured_frame": samples.last().unwrap().0,
            "frame_samples": samples.iter().map(|(stats, wall)| batch_frame_sample(stats, *wall)).collect::<Vec<_>>(),
        })
    }).collect();
    let report = json!({
        "comparison": comparison.name(), "workload": workload.name(),
        "adapter": info.name, "backend": info.backend.to_str(),
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "viewport": size, "warmup_frames": 12, "measured_frames": 60,
        "alternating_mode_order": true, "shared_retained_input": true,
        "timing_excludes_extraction": true, "exact_pixels": true,
        "submitted_color_geometry_equal": true, "submitted_shadow_triangles_equal": submitted_shadow_triangles_equal,
        "scene_and_checkpoint_preserved": true, "results": results,
    });
    println!("graph_instancing_factory_profile={report}");
    if let Some(path) = output {
        std::fs::write(
            path.join(format!("{}.json", workload.name())),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(())
}

#[test]
#[ignore = "native graph-instancing motion parity; accepts software graphics adapters"]
fn graph_instancing_factory_pixels_and_checkpoints_match_during_motion() -> Result<()> {
    factory_batching_motion_parity(BatchingComparison::GraphInstancing)
}

#[test]
#[ignore = "native batch-preparation motion parity; accepts software graphics adapters"]
fn batch_preparation_factory_pixels_and_checkpoints_match_during_motion() -> Result<()> {
    factory_batching_motion_parity(BatchingComparison::BatchPreparation)
}

#[test]
#[ignore = "native resource-metadata motion parity; accepts software graphics adapters"]
fn resource_metadata_factory_pixels_and_checkpoints_match_during_motion() -> Result<()> {
    factory_batching_motion_parity(BatchingComparison::ResourceMetadata)
}

fn factory_batching_motion_parity(comparison: BatchingComparison) -> Result<()> {
    let preparation = comparison != BatchingComparison::GraphInstancing;
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    let mut editor = dense_factory()?;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&bozzard_render::instance(
        bozzard_render::Backend::native(),
    )))?;
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    comparison.configure(&mut renderers[0]);
    for renderer in &mut renderers {
        renderer.set_occlusion_enabled(false);
        // Verify identical caster submissions independently of cache heuristics.
        renderer.set_shadow_preparation_caching_enabled(false);
    }
    upload_assets(&editor, &gpu, &mut renderers)?;
    let play = editor.play.as_ref().unwrap();
    let camera_id = &play.instance().document().views[&Layer::ThreeD];
    let camera_pose = play.instance().global_transforms(&play.app.world)?[camera_id];
    let mut batched_frames = 0;
    for frame in 0..20 {
        let play = editor.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
        let checkpoint = state(&editor)?;
        let pose = Some(Mat4::from_rotation_y(frame as f32 * 0.08) * camera_pose);
        let input = extract(&editor, true, pose)?;
        let mut reference = None;
        for renderer in &mut renderers {
            let capture = bozzard_render::capture_offscreen(&gpu, 640, 400, |target| {
                renderer.draw(&gpu, target, [640, 400], input.scene())
            })?;
            if let Some(reference) = &reference {
                ensure!(
                    capture.rgba == *reference,
                    "graph pixels differ at frame {frame}"
                );
            } else {
                reference = Some(capture.rgba);
            }
        }
        let a = renderers[0].frame_stats();
        let b = renderers[1].frame_stats();
        assert_eq!(
            (
                a.surfaces,
                a.visible_surfaces,
                a.color_triangles,
                a.shadow_triangles
            ),
            (
                b.surfaces,
                b.visible_surfaces,
                b.color_triangles,
                b.shadow_triangles
            )
        );
        if preparation {
            counts_equal(a, b);
            assert_eq!(a.batching, b.batching);
        }
        batched_frames += usize::from(
            b.batching.graph_instanced_surfaces > 0
                && (preparation || b.color_draws < a.color_draws),
        );
        ensure!(
            state(&editor)? == checkpoint,
            "rendering changed gameplay at frame {frame}"
        );
    }
    ensure!(
        batched_frames == 20,
        "motion parity must actually exercise graph instances"
    );
    println!(
        "{}_factory_parity_ok: 20 moving frames, exact pixels, geometry and checkpoints",
        if comparison == BatchingComparison::GraphInstancing {
            "graph_instancing"
        } else {
            comparison.name()
        }
    );
    Ok(())
}
