use anyhow::{Result, ensure};
use bozzard_assets::job::Job;
use bozzard_editor::{Editor, FoliageSettings, PrefabCommand};
use bozzard_scene::Scene;
use glam::Vec3;
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
            "bozzard-foliage-{}-{}",
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
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        ensure!(Instant::now() < deadline, "scatter preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn editor(temp: &Temp) -> Result<Editor> {
    Editor::new(
        Scene::from_json(
            r#"{"version":1,"name":"Foliage","views":{},"objects":[
      {"id":"ground","name":"Ground","transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"collider":{"size":[32,1,32]}},
      {"id":"tree","name":"Tree","transform":{"translation":[20,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
      {"id":"leaf","name":"Leaf","parent":"tree","transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[0.5,2,0.5]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0.2,0.8,0.3],"uv_scale":[1,1]}}
    ]}"#,
        )?,
        &temp.0.join("scene.json"),
    )
}

#[test]
fn seeded_scatter_preserves_prefabs_spacing_assets_and_one_step_history() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    editor.select_object(Some("tree".into()));
    let job = editor.prefab_job(PrefabCommand::Create)?;
    editor.accept_prefab(wait(&job)?)?;
    let before = editor.scene().clone();
    let settings = FoliageSettings {
        count: 30,
        radius: 7.,
        spacing: 1.,
        seed: 42,
        ..Default::default()
    };
    let job = editor.scatter_foliage_job("tree", "ground", settings.clone())?;
    let (group, placed, requested) = editor.accept_foliage(wait(&job)?)?;
    assert_eq!((placed, requested), (30, 30));
    assert_eq!(editor.scene().objects.len(), before.objects.len() + 61);
    assert_eq!(editor.scene().assets, before.assets);
    assert_eq!(editor.scene().prefabs.len(), before.prefabs.len() + 30);
    let roots: Vec<_> = editor
        .scene()
        .objects
        .iter()
        .filter(|o| o.parent.as_deref() == Some(&group))
        .collect();
    assert_eq!(roots.len(), 30);
    for (i, root) in roots.iter().enumerate() {
        let p = Vec3::from_array(root.transform.translation);
        assert!(p.y.abs() < 1e-5 && p.length() <= 7.);
        for other in roots.iter().skip(i + 1) {
            assert!(p.distance(Vec3::from_array(other.transform.translation)) >= 1. - 1e-5);
        }
        assert_eq!(
            editor
                .scene()
                .objects
                .iter()
                .filter(|o| o.parent.as_deref() == Some(&root.id))
                .count(),
            1
        );
    }
    let after = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &before);
    editor.redo()?;
    assert_eq!(editor.scene(), &after);
    editor.undo()?;
    let job = editor.scatter_foliage_job("tree", "ground", settings)?;
    editor.accept_foliage(wait(&job)?)?;
    assert_eq!(
        editor.scene(),
        &after,
        "same seed and inputs must reproduce every transform and ID"
    );
    let path = editor.path.clone();
    editor.save(&path)?;
    assert_eq!(Editor::open(&path)?.scene(), &after);
    Ok(())
}

#[test]
fn stale_cancelled_or_unsuitable_scatter_keeps_the_scene_unchanged() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let before = editor.scene().clone();
    let settings = FoliageSettings {
        count: 3,
        ..Default::default()
    };
    let job = editor.scatter_foliage_job("tree", "ground", settings.clone())?;
    let prepared = wait(&job)?;
    job.cancel();
    assert!(editor.accept_foliage(prepared).is_err());
    assert_eq!(editor.scene(), &before);
    let job = editor.scatter_foliage_job("tree", "ground", settings)?;
    let prepared = wait(&job)?;
    let mut changed = before.clone();
    changed.name = "Changed".into();
    editor.apply("Change scene", changed.clone())?;
    assert!(editor.accept_foliage(prepared).is_err());
    assert_eq!(editor.scene(), &changed);
    let job = editor.scatter_foliage_job(
        "tree",
        "ground",
        FoliageSettings {
            count: 3,
            center: [100., 100.],
            ..Default::default()
        },
    )?;
    assert!(wait(&job).is_err());
    assert_eq!(editor.scene(), &changed);
    assert!(
        editor
            .scatter_foliage_job(
                "tree",
                "ground",
                FoliageSettings {
                    scale: [2., 1.],
                    ..Default::default()
                }
            )
            .is_err()
    );
    Ok(())
}
