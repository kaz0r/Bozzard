use bozzard_assets::{AssetData, SimplifySettings, job::Job};
use bozzard_editor::{Editor, LodRequest};
use bozzard_scene::{Layer, Scene};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "bozzard-generated-lod-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("{e}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait<T: Send + 'static>(job: &Job<T>) -> anyhow::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "LOD worker timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn setup() -> anyhow::Result<(Temp, Editor)> {
    use std::fmt::Write;
    let temp = Temp::new();
    let mut obj = String::new();
    let size = 32;
    for y in 0..=size {
        for x in 0..=size {
            writeln!(obj, "v {} {} 0", x as f32 / 16. - 1., y as f32 / 16. - 1.)?;
        }
    }
    for y in 0..size {
        for x in 0..size {
            let a = y * (size + 1) + x + 1;
            let b = a + 1;
            let c = a + size + 1;
            let d = c + 1;
            writeln!(obj, "f {a} {b} {c}\nf {b} {d} {c}")?;
        }
    }
    fs::write(temp.0.join("mesh.obj"), obj)?;
    let scene = Scene::from_json(
        r#"{"version":1,"name":"Automatic LOD","views":{"3d":"camera"},"assets":{"mesh":{"kind":"mesh","path":"mesh.obj"}},"objects":[
      {"id":"camera","name":"Camera","transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},
      {"id":"mesh","name":"Grid","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":{"asset":"mesh"},"texture":"white","color":[0.8,0.3,0.1],"uv_scale":[1,1]}}
    ]}"#,
    )?;
    let editor = Editor::new(scene, &temp.0.join("scene.json"))?;
    Ok((temp, editor))
}
fn requests() -> Vec<LodRequest> {
    vec![
        LodRequest {
            switch: 1.,
            settings: SimplifySettings {
                ratio: 0.5,
                ..Default::default()
            },
        },
        LodRequest {
            switch: 2.,
            settings: SimplifySettings {
                ratio: 0.2,
                ..Default::default()
            },
        },
    ]
}
#[test]
fn generated_lods_publish_atomically_undo_redo_relocate_and_reject_stale_jobs() -> anyhow::Result<()>
{
    let (temp, mut editor) = setup()?;
    let before = editor.scene().clone();
    let bytes = fs::read(temp.0.join("mesh.obj"))?;
    let prepared = wait(&editor.generate_lods_job("mesh", requests())?)?;
    assert_eq!(editor.scene(), &before);
    assert_eq!(prepared.levels.len(), 2);
    let levels = editor.accept_lods(prepared)?;
    assert_eq!(levels[0].source_triangles, 2048);
    assert!(levels[1].triangles < levels[0].triangles && levels[0].triangles <= 1024);
    for level in &levels {
        let AssetData::Mesh(mesh) = editor
            .assets
            .get(editor.assets.handle(&level.asset).unwrap())
            .unwrap()
            .data()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(mesh.indices.len() / 3, level.triangles);
        assert!(
            mesh.parts.is_empty(),
            "plain OBJ must keep its Lambert material path"
        );
    }
    let generated = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &before);
    editor.redo()?;
    assert_eq!(editor.scene(), &generated);
    editor.save(&temp.0.join("scene.json"))?;
    let relocated = temp.0.join("relocated");
    fs::create_dir(&relocated)?;
    fs::rename(temp.0.join("assets"), relocated.join("assets"))?;
    fs::rename(temp.0.join("mesh.obj"), relocated.join("mesh.obj"))?;
    fs::rename(temp.0.join("scene.json"), relocated.join("scene.json"))?;
    editor = Editor::open(&relocated.join("scene.json"))?;
    assert_eq!(editor.scene(), &generated);
    assert_eq!(fs::read(relocated.join("mesh.obj"))?, bytes);
    let prepared = wait(&editor.generate_lods_job("mesh", requests())?)?;
    let stale_asset = prepared.levels[0].asset.clone();
    editor.create_empty()?;
    assert!(editor.accept_lods(prepared).is_err());
    assert!(!editor.scene().assets.contains_key(&stale_asset));
    assert_eq!(
        fs::read_dir(relocated.join("assets"))?.count(),
        1,
        "stale generated directory removed"
    );
    let job = editor.generate_lods_job("mesh", requests())?;
    job.cancel();
    assert!(wait(&job).is_err());
    assert_eq!(fs::read_dir(relocated.join("assets"))?.count(), 1);
    Ok(())
}

#[test]
fn generated_lod_reduces_native_gpu_work_without_changing_flat_surface_pixels() -> anyhow::Result<()>
{
    use bozzard_render::{Gpu, SceneRenderer, capture_offscreen, wgpu};
    let (_temp, mut editor) = setup()?;
    let mut scene = editor.scene().clone();
    scene.lighting.shadows = false;
    scene.environment.intensity = 0.;
    scene.environment.background = false;
    editor.apply("Test lighting", scene)?;
    let instance = wgpu::Instance::default();
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut residency = bozzard_render_assets::Residency::default();
    residency.sync(&gpu, &mut renderer, &editor.assets)?;
    let frame = editor.render(Layer::ThreeD, 1.)?;
    let reference = capture_offscreen(&gpu, 128, 128, |target| {
        renderer.draw(&gpu, target, [128; 2], &frame)
    })?;
    assert_eq!(renderer.frame_stats().color_triangles, 2048);
    let prepared = wait(&editor.generate_lods_job("mesh", requests())?)?;
    let levels = editor.accept_lods(prepared)?;
    residency.sync(&gpu, &mut renderer, &editor.assets)?;
    let frame = editor.render(Layer::ThreeD, 1.)?;
    let reduced = capture_offscreen(&gpu, 128, 128, |target| {
        renderer.draw(&gpu, target, [128; 2], &frame)
    })?;
    assert_eq!(
        renderer.frame_stats().color_triangles,
        levels[1].triangles as u64
    );
    assert_eq!(reference.rgba, reduced.rgba);
    eprintln!(
        "Generated LOD at 128x128: 2048 -> {} triangles, byte-identical pixels",
        levels[1].triangles
    );
    Ok(())
}
