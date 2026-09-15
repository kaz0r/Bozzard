use bozzard_assets::job::{Job, Progress};
use bozzard_editor::Editor;
use bozzard_scene::{Layer, Scene, Transform};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let p = std::env::temp_dir().join(format!(
                "bozzard-gi-editor-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&p) {
                Ok(()) => return Self(p),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn editor(path: &std::path::Path) -> Editor {
    let mut s =
        Scene::from_json(include_str!("../../../examples/demo/scenes/gi-lab.json")).unwrap();
    s.gi.volume.resolution = [2; 3];
    s.gi.volume.samples = 64;
    s.gi.volume.bounces = 1;
    Editor::new(s, path).unwrap()
}
fn wait<T: Send + 'static>(job: &Job<T>) -> anyhow::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = job.poll() {
            return r;
        }
        assert!(Instant::now() < deadline, "bake timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn bake_history_save_play_and_static_invalidation() {
    let dir = Temp::new();
    let mut e = editor(&dir.0.join("scene.json"));
    let original = e.scene().clone();
    let job = e.bake_gi_job().unwrap();
    e.accept_gi(wait(&job).unwrap()).unwrap();
    assert!(e.gi_current());
    assert_eq!(e.undo_label(), Some("Bake global illumination"));
    assert!(e.render(Layer::ThreeD, 1.).unwrap().gi.is_some());
    assert!(e.render(Layer::TwoD, 1.).unwrap().gi.is_none());
    e.undo().unwrap();
    assert_eq!(*e.scene(), original);
    e.redo().unwrap();
    assert!(e.gi_current());
    let baked = e.scene().clone();
    e.save(&dir.0.join("nested/scene.json")).unwrap();
    let reopened = Editor::open(&dir.0.join("nested/scene.json")).unwrap();
    assert!(reopened.gi_current());
    assert_eq!(reopened.scene().gi, baked.gi);
    e.start_play().unwrap();
    assert!(e.render(Layer::ThreeD, 1.).unwrap().gi.is_some());
    let play = e.play.as_mut().unwrap();
    let entity = play.instance().entity("tall-box").unwrap();
    play.app
        .world
        .get_mut::<Transform>(entity)
        .unwrap()
        .translation[0] += 1.;
    assert!(e.render(Layer::ThreeD, 1.).unwrap().gi.is_none());
    e.stop_play();
    assert!(e.gi_current());
    let mut scene = e.scene().clone();
    scene.display.exposure_ev = 2.;
    scene.gi.intensity = 0.7;
    scene.objects[0].transform.translation[0] += 1.;
    e.apply("Display and camera", scene).unwrap();
    assert!(e.gi_current());
    let mut scene = e.scene().clone();
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == "floor")
        .unwrap()
        .drawable
        .as_mut()
        .unwrap()
        .color = [0.1; 3];
    e.apply("Material", scene).unwrap();
    assert!(!e.gi_current());
    assert!(e.render(Layer::ThreeD, 1.).unwrap().gi.is_none());
    e.undo().unwrap();
    assert!(e.gi_current());
    let mut scene = e.scene().clone();
    scene.gi.baked = None;
    scene.gi.enabled = false;
    e.apply("Clear bake", scene).unwrap();
    assert!(!e.gi_current());
    e.undo().unwrap();
    assert!(e.gi_current());
}
#[test]
fn cancelled_and_stale_bakes_never_publish() {
    let dir = Temp::new();
    let mut e = editor(&dir.0.join("scene.json"));
    let original = e.scene().clone();
    let job = e.bake_gi_job().unwrap();
    job.cancel();
    assert!(wait(&job).is_err());
    assert_eq!(*e.scene(), original);
    let job = e.bake_gi_job().unwrap();
    let ready = wait(&job).unwrap();
    let mut scene = e.scene().clone();
    scene.name = "Changed while baking".into();
    e.apply("Rename", scene).unwrap();
    assert!(e.accept_gi(ready).is_err());
    assert!(e.scene().gi.baked.is_none());
    let job = e.bake_gi_job().unwrap();
    let ready = wait(&job).unwrap();
    e.start_play().unwrap();
    assert!(e.accept_gi(ready).is_err());
    assert!(e.bake_gi_job().is_err());
    e.stop_play();
    let job = e.bake_gi_job().unwrap();
    let ready = wait(&job).unwrap();
    e.save(&dir.0.join("elsewhere/scene.json")).unwrap();
    assert!(e.accept_gi(ready).is_err());
    e.fit_gi_volume().unwrap();
    assert_eq!(e.undo_label(), Some("Fit GI volume"));
    assert!(e.scene().gi.volume.min[0] < -3.1);
    let baked = bozzard_assets::gi::bake(
        e.scene(),
        &e.assets,
        e.scene().gi.volume,
        &Progress::default(),
    )
    .unwrap();
    assert!(baked.probes.iter().any(|p| p[3] > 0.));
}

#[test]
fn freshness_tracks_public_asset_reload_and_preview_revision() -> anyhow::Result<()> {
    let dir = Temp::new();
    let mesh_path = dir.0.join("source.obj");
    let mesh = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    std::fs::write(&mesh_path, mesh)?;
    let mut e = editor(&dir.0.join("scene.json"));
    let mut scene = e.scene().clone();
    scene.assets.insert(
        "mesh".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Mesh,
            path: "source.obj".into(),
        },
    );
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == "floor")
        .unwrap()
        .drawable
        .as_mut()
        .unwrap()
        .mesh = bozzard_scene::Mesh::Asset("mesh".into());
    e.apply("Mesh", scene)?;
    let mut scene = e.scene().clone();
    scene.gi.baked = Some(std::sync::Arc::new(bozzard_scene::BakedGi::new(
        bozzard_assets::gi::source(&scene, &e.assets, scene.gi.volume)?,
        scene.gi.volume,
        std::sync::Arc::new(vec![
            [0.; 4];
            scene.gi.volume.probe_count()
                * bozzard_scene::GI_PROBE_STRIDE
        ]),
    )?));
    scene.gi.enabled = true;
    e.apply("GI fixture", scene)?;
    let mut preview = bozzard_editor::EffectsPreview::new(&e)?;
    for _ in 0..2 {
        assert!(e.gi_current());
        assert!(e.render(Layer::ThreeD, 1.)?.gi.is_some());
        assert!(preview.render(&e, Layer::ThreeD, 1.)?.gi.is_some());
    }
    let revision = e.asset_revision();
    std::fs::write(&mesh_path, mesh.replace("0 1 0", "0 2 0"))?;
    assert_eq!(e.assets.refresh().len(), 1);
    assert_eq!(revision, e.asset_revision()); // Bypasses Editor's publication counter.
    assert!(!e.gi_current());
    assert!(e.render(Layer::ThreeD, 1.)?.gi.is_none());
    assert!(preview.render(&e, Layer::ThreeD, 1.)?.gi.is_none());
    std::fs::write(&mesh_path, mesh)?;
    e.assets.refresh();
    assert!(e.gi_current());
    Ok(())
}

#[test]
fn immutable_document_snapshots_track_edits_undo_and_redo() -> anyhow::Result<()> {
    let dir = Temp::new();
    let mut e = editor(&dir.0.join("scene.json"));
    let original = e.scene_snapshot();
    assert!(std::sync::Arc::ptr_eq(&original, &e.scene_snapshot()));
    let mut next = (*original).clone();
    next.name = "Renamed".into();
    e.apply("Rename", next.clone())?;
    let changed = e.scene_snapshot();
    assert_eq!(*changed, next);
    assert!(!std::sync::Arc::ptr_eq(&original, &changed));
    e.undo()?;
    assert_eq!(*e.scene_snapshot(), *original);
    e.redo()?;
    assert_eq!(*e.scene_snapshot(), *changed);
    assert_eq!(*original, *editor(&dir.0.join("other.json")).scene());
    Ok(())
}
