use anyhow::{Result, ensure};
use bozzard_assets::{
    CookSource,
    job::{Job, Progress},
    terrain::{Terrain, TerrainPaint, TerrainPaintBrush},
};
use bozzard_editor::{Editor, TerrainRequest};
use bozzard_render::{Frame, Gpu, SceneRenderer, wgpu};
use bozzard_scene::{AssetKind, Layer, Scene};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "bozzard-terrain-painting-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        if Instant::now() >= deadline {
            job.cancel();
            anyhow::bail!("terrain paint preparation timed out");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn capture(gpu: &Gpu, editor: &Editor) -> Result<Frame> {
    editor.assets.require_ready()?;
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(gpu, &mut renderer, &entry.id, data)?;
        }
    }
    let scene = editor.render(Layer::ThreeD, 1.)?;
    bozzard_render::capture_offscreen(gpu, 128, 128, |target| {
        renderer.draw(gpu, target, [128; 2], &scene)
    })
}

#[test]
fn painted_terrain_cooked_relocation_matches_raw_with_exact_pixels() -> Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("scene.json");
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Paint proof","views":{"3d":"camera"},"objects":[{
            "id":"camera","name":"Camera",
            "transform":{"translation":[0,8,8],"rotation_degrees":[-45,0,0],"scale":[1,1,1]},
            "camera":{"projection":"perspective","vertical_fov_degrees":50,"near":0.1,"far":100}
        }]}"#,
    )?;
    scene.lighting.shadows = false;
    scene.lighting.ambient_intensity = 0.25;
    let mut editor = Editor::new(scene, &path)?;
    let mut terrain = Terrain::flat([17; 2], [8.; 2])?;
    terrain.paint = Some(TerrainPaint::new(terrain.heights.len()));
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain,
        position: [0.; 3],
    })?;
    let ground = editor.accept_terrain(wait(&job)?)?;
    let initial = editor.scene().clone();
    let original_collider = initial
        .objects
        .iter()
        .find(|object| object.id == ground)
        .unwrap()
        .mesh_collider
        .clone();
    let source_path = temp.0.join(&initial.assets.values().next().unwrap().path);
    let original_bytes = fs::read(&source_path)?;
    let original_mesh = editor.selected_mesh().unwrap();
    let vertices = original_mesh.vertices.clone();
    let indices = original_mesh.indices.clone();

    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let before = capture(&gpu, &editor)?;
    let source = editor.terrain_source(&ground)?;
    let mut terrain = source.terrain.clone();
    for (layer, center, radius, strength) in [(1, [0., 0.], 3.5, 1.), (2, [-1.5, 1.], 2., 0.9)] {
        assert!(terrain.paint(TerrainPaintBrush {
            layer,
            center,
            radius,
            strength,
        })?);
    }
    assert_eq!(terrain.heights, source.terrain.heights);
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    assert_eq!(fs::read(&source_path)?, original_bytes);
    let mesh = editor.selected_mesh().unwrap();
    assert_eq!(mesh.vertices, vertices);
    assert_eq!(mesh.indices, indices);
    assert_eq!(
        editor
            .scene()
            .objects
            .iter()
            .find(|object| object.id == ground)
            .unwrap()
            .mesh_collider,
        original_collider
    );
    let after = capture(&gpu, &editor)?;
    let changed_pixels = before
        .rgba
        .chunks_exact(4)
        .zip(after.rgba.chunks_exact(4))
        .filter(|(before, after)| before != after)
        .count();
    ensure!(
        changed_pixels > 200,
        "painting must visibly change the frame"
    );
    let painted = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &initial);
    assert_eq!(capture(&gpu, &editor)?.rgba, before.rgba);
    editor.redo()?;
    assert_eq!(editor.scene(), &painted);
    assert_eq!(capture(&gpu, &editor)?.rgba, after.rgba);
    editor.save(&path)?;
    assert_eq!(capture(&gpu, &Editor::open(&path)?)?.rgba, after.rgba);

    // Cook with lossless embedded maps, relocate, then make all authoring revisions unavailable.
    let cooked_root = temp.0.join("relocated");
    fs::create_dir(&cooked_root)?;
    let mut cooked_scene = editor.scene().clone();
    for (id, asset) in &mut cooked_scene.assets {
        assert_eq!(asset.kind, AssetKind::Mesh);
        let bytes = CookSource::read(
            AssetKind::Mesh,
            &temp.0.join(&asset.path),
            &Progress::default(),
        )?
        .cook(&[], &Progress::default())?;
        asset.path = format!("{id}.bmesh");
        fs::write(cooked_root.join(&asset.path), bytes)?;
    }
    let cooked_path = cooked_root.join("scene.json");
    fs::write(&cooked_path, cooked_scene.to_json()?)?;
    fs::remove_dir_all(temp.0.join("assets"))?;
    let reopened = Editor::open(&cooked_path)?;
    assert_eq!(capture(&gpu, &reopened)?.rgba, after.rgba);
    println!(
        "terrain_painting changed_pixels={changed_pixels} geometry_vertices={} triangles={} history_exact_rgba=true saved_exact_rgba=true relocated_cooked_exact_rgba=true authoring_sources_unavailable=true",
        vertices.len(),
        indices.len() / 3
    );
    Ok(())
}
