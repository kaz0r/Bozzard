//! `--record DIR`: run a scene without a window and write one offscreen frame per video
//! frame, simulating in real time at the fixed 60 Hz tick. A camera path file can fly the
//! view; the scene's own camera is used otherwise. Frames are binary PPMs named
//! `frame_00000.ppm`, ready for `ffmpeg -framerate FPS -i frame_%05d.ppm`.
use super::*;
use bozzard_render::capture_offscreen;
use glam::{Mat4, Vec3};

/// Timed camera keys, interpolated with Catmull-Rom splines through position and target.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraPath {
    keys: Vec<CameraKey>,
    #[serde(default = "default_near")]
    near: f32,
    #[serde(default = "default_far")]
    far: f32,
}
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraKey {
    /// Seconds from the first recorded frame.
    time: f32,
    position: [f32; 3],
    target: [f32; 3],
    #[serde(default = "default_fov")]
    fov: f32,
}
fn default_near() -> f32 {
    0.1
}
fn default_far() -> f32 {
    1000.0
}
fn default_fov() -> f32 {
    55.0
}

impl CameraPath {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let path: Self = serde_json::from_str(&std::fs::read_to_string(path)?)
            .with_context(|| format!("reading camera path {}", path.display()))?;
        ensure!(
            !path.keys.is_empty(),
            "a camera path needs at least one key"
        );
        ensure!(
            path.keys.windows(2).all(|pair| pair[0].time < pair[1].time),
            "camera keys must have increasing times"
        );
        ensure!(
            path.keys.iter().all(|key| {
                key.time.is_finite()
                    && Vec3::from(key.position).is_finite()
                    && Vec3::from(key.target).is_finite()
                    && Vec3::from(key.position) != Vec3::from(key.target)
                    && (1.0..=170.0).contains(&key.fov)
            }) && path.near > 0.0
                && path.far > path.near,
            "camera keys need finite, distinct position and target, a 1–170° field of view and 0 < near < far"
        );
        Ok(path)
    }
    /// The camera's world pose and vertical field of view at `time`.
    fn at(&self, time: f32) -> (Mat4, f32) {
        let keys = &self.keys;
        let last = keys.len() - 1;
        let segment = keys
            .windows(2)
            .position(|pair| time < pair[1].time)
            .unwrap_or(last.saturating_sub(1));
        let (a, b) = (segment, (segment + 1).min(last));
        let span = keys[b].time - keys[a].time;
        let t = if span > 0.0 {
            ((time - keys[a].time) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let spline = |field: fn(&CameraKey) -> [f32; 3]| {
            let p = |i: usize| Vec3::from(field(&keys[i]));
            let (p0, p1, p2, p3) = (p(a.saturating_sub(1)), p(a), p(b), p((b + 1).min(last)));
            let t2 = t * t;
            let t3 = t2 * t;
            0.5 * (2.0 * p1
                + (p2 - p0) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
        };
        let (eye, target) = (spline(|k| k.position), spline(|k| k.target));
        let forward = (target - eye).normalize_or_zero();
        let up = if forward.cross(Vec3::Y).length_squared() < 1e-6 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let fov = keys[a].fov + (keys[b].fov - keys[a].fov) * t * t * (3.0 - 2.0 * t);
        (
            glam::camera::rh::view::look_at_mat4(eye, target, up).inverse(),
            fov,
        )
    }
}

pub(crate) fn run(document: &Scene, options: &Options, directory: &Path) -> Result<()> {
    ensure!(
        options.record_fps > 0 && 60 % options.record_fps == 0,
        "--record-fps must divide the 60 Hz simulation tick (1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30 or 60)"
    );
    let path = options
        .record_camera
        .as_deref()
        .map(CameraPath::load)
        .transpose()?;
    let (mut demo, mut assets) = start_session(document, options)?;
    settings::load(&mut demo, options);
    demo.set_threaded_simulation(false)?;
    for _ in 0..options.capture_ticks {
        demo.app.step();
        demo.check_simulation()?;
    }
    let gpu = pollster::block_on(Gpu::request(
        &instance(options.backend),
        None,
        options.software,
    ))?;
    if options.hardware {
        gpu.require_hardware()?;
    }
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    renderer.set_hud_scale(1.);
    let [width, height] = options.capture_size;
    let layer = options.layer;
    let aspect = width as f32 / height as f32;
    let cache = bozzard_render_assets::RenderSceneCache::default();
    std::fs::create_dir_all(directory)?;
    let ticks = 60 / options.record_fps;
    let started = std::time::Instant::now();
    for index in 0..options.record_frames {
        if index > 0 {
            for _ in 0..ticks {
                demo.app.step();
                demo.check_simulation()?;
            }
        }
        let pose = path.as_ref().map(|path| {
            let (pose, fov) = path.at(index as f32 / options.record_fps as f32);
            (pose, fov, path.near, path.far)
        });
        if let Some((_, fov, near, far)) = pose {
            // The path's lens replaces the view camera's; its pose rides the inspection pose,
            // so LOD, volumes and fog follow the flown camera.
            let camera = demo
                .instance()
                .document()
                .views
                .get(&layer)
                .and_then(|id| demo.instance().entity(id))
                .context("the scene has no camera for this view")?;
            if let Some(mut lens) = demo.app.world.get_mut::<bozzard_scene::Camera>(camera) {
                *lens = bozzard_scene::Camera::Perspective {
                    vertical_fov_degrees: fov,
                    near,
                    far,
                };
            }
        }
        let mut scene = presentation::extract_frame_from(
            &demo,
            assets.store(),
            &cache,
            layer,
            aspect,
            pose.map(|p| p.0),
        )?;
        settings::apply_quality_to_frame(settings::applied(&demo).0.quality, &mut scene);
        if options.record_ui {
            let ui =
                demo.instance()
                    .ui_frame(&demo.app.world, layer, [width as f32, height as f32])?;
            scene.append_items(bozzard_render_assets::widget_items(&ui, assets.store())?);
        }
        assets.upload_required(&gpu, &mut renderer, &scene, options.gpu_memory_mib << 20)?;
        let frame = capture_offscreen(&gpu, width, height, |view| {
            renderer.draw(&gpu, view, [width, height], &scene)
        })?;
        frame.write_ppm(&directory.join(format!("frame_{index:05}.ppm")))?;
    }
    println!(
        "record_ok directory={} frames={} fps={} size={width}x{height} seconds={:.1}",
        directory.display(),
        options.record_frames,
        options.record_fps,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> CameraPath {
        serde_json::from_str(
            r#"{"keys":[
                {"time":0,"position":[0,0,10],"target":[0,0,0],"fov":40},
                {"time":2,"position":[10,0,0],"target":[0,0,0],"fov":60}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn keys_are_hit_exactly_and_the_camera_looks_at_its_target() {
        let path = path();
        let (start, fov) = path.at(0.0);
        assert!((start.w_axis.truncate() - Vec3::new(0., 0., 10.)).length() < 1e-4);
        assert_eq!(fov, 40.0);
        let (end, fov) = path.at(2.0);
        assert!((end.w_axis.truncate() - Vec3::new(10., 0., 0.)).length() < 1e-4);
        assert_eq!(fov, 60.0);
        // Cameras look down their local -Z.
        let forward = end.transform_vector3(-Vec3::Z);
        assert!((forward - -Vec3::X).length() < 1e-4, "{forward}");
        // Past the last key the camera holds still.
        assert_eq!(path.at(5.0).0, end);
        let (middle, fov) = path.at(1.0);
        assert_eq!(fov, 50.0);
        assert!(middle.w_axis.truncate().length() > 6.0);
    }

    #[test]
    fn broken_paths_are_rejected() {
        let dir = std::env::temp_dir().join(format!("bozzard-camera-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, json) in [
            ("empty", r#"{"keys":[]}"#),
            (
                "backwards",
                r#"{"keys":[{"time":1,"position":[0,0,1],"target":[0,0,0]},{"time":0,"position":[0,0,2],"target":[0,0,0]}]}"#,
            ),
            (
                "degenerate",
                r#"{"keys":[{"time":0,"position":[0,0,0],"target":[0,0,0]}]}"#,
            ),
        ] {
            let file = dir.join(format!("{name}.json"));
            std::fs::write(&file, json).unwrap();
            assert!(CameraPath::load(&file).is_err(), "{name}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
