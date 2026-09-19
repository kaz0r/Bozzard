use anyhow::{Result, ensure};
use bozzard_assets::{
    job::Job,
    terrain::{BrushMode, Terrain, TerrainBrush},
};
use bozzard_editor::{Editor, TerrainRequest};
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
            "bozzard-terrain-{}-{}",
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
        ensure!(Instant::now() < deadline, "terrain preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn editor(temp: &Temp) -> Result<Editor> {
    Editor::new(
        Scene::from_json(r#"{"version":1,"name":"Terrain","views":{},"objects":[]}"#)?,
        &temp.0.join("scene.json"),
    )
}
fn hit(editor: &Editor) -> Result<f32> {
    Ok(editor
        .collisions()?
        .raycast(Vec3::new(0., 10., 0.), Vec3::NEG_Y, 20., None)?
        .unwrap()
        .position
        .y)
}

#[test]
fn sculpt_revisions_keep_render_collision_history_and_saved_sources_consistent() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([33, 33], [16., 16.])?,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let initial = editor.scene().clone();
    let initial_path = temp.0.join(&initial.assets.values().next().unwrap().path);
    let bytes = fs::read(&initial_path)?;
    assert!(hit(&editor)?.abs() < 1e-5);
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    terrain.brush(TerrainBrush {
        mode: BrushMode::Raise,
        center: [0., 0.],
        radius: 3.,
        strength: 2.,
        target_height: 0.,
    })?;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    assert_eq!(editor.scene().assets.len(), 1);
    assert_ne!(editor.scene().assets, initial.assets);
    assert_eq!(fs::read(&initial_path)?, bytes);
    assert!((hit(&editor)? - 2.).abs() < 1e-5);
    assert_eq!(
        editor
            .selected_mesh()
            .unwrap()
            .vertices
            .iter()
            .map(|v| v[1])
            .fold(f32::NEG_INFINITY, f32::max),
        2.
    );
    let edited = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &initial);
    assert!(hit(&editor)?.abs() < 1e-5);
    editor.redo()?;
    assert_eq!(editor.scene(), &edited);
    assert!((hit(&editor)? - 2.).abs() < 1e-5);
    let path = editor.path.clone();
    editor.save(&path)?;
    let opened = Editor::open(&path)?;
    assert_eq!(
        opened
            .terrain_source(&id)?
            .terrain
            .heights
            .iter()
            .copied()
            .fold(0., f32::max),
        2.
    );
    assert!((hit(&opened)? - 2.).abs() < 1e-5);
    Ok(())
}

#[test]
fn stale_cancelled_and_externally_changed_terrain_never_publish() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let create = || TerrainRequest::Create {
        terrain: Terrain::flat([3, 3], [2., 2.]).unwrap(),
        position: [0.; 3],
    };
    let job = editor.terrain_job(create())?;
    let prepared = wait(&job)?;
    let mut changed = editor.scene().clone();
    changed.name = "Changed".into();
    editor.apply("Change scene", changed.clone())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(editor.scene(), &changed);
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 0);
    let job = editor.terrain_job(create())?;
    let prepared = wait(&job)?;
    job.cancel();
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 0);
    let job = editor.terrain_job(create())?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let source = editor.terrain_source(&id)?;
    let terrain = source.terrain.clone();
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    let prepared = wait(&job)?;
    let asset = temp
        .0
        .join(&editor.scene().assets.values().next().unwrap().path);
    let current = fs::read(&asset)?;
    fs::write(&asset, [current.as_slice(), b"\n"].concat())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 1);
    Ok(())
}

#[test]
fn terrain_and_brush_prefabs_keep_saved_geometry_after_later_sculpting() -> Result<()> {
    use bozzard_assets::blockout::{Blockout, BrushPrimitive};
    use bozzard_editor::PrefabCommand;
    use bozzard_scene::Transform;
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([9; 2], [8.; 2])?,
        position: [0.; 3],
    })?;
    let ground = editor.accept_terrain(wait(&job)?)?;
    let job = editor.blockout_job(
        Blockout {
            version: 1,
            primitive: BrushPrimitive::Ramp,
        },
        vec![Transform {
            translation: [3., 0., 3.],
            ..Default::default()
        }],
    )?;
    let brush = editor.accept_geometry(wait(&job)?)?;
    editor.reparent(&brush, Some(&ground))?;
    editor.select_object(Some(ground.clone()));
    let job = editor.prefab_job(PrefabCommand::Create)?;
    let prefab = editor.accept_prefab(wait(&job)?)?;
    let source = editor.terrain_source(&ground)?;
    let mut terrain = source.terrain.clone();
    terrain.brush(TerrainBrush {
        mode: BrushMode::Raise,
        center: [0.; 2],
        radius: 2.,
        strength: 2.,
        target_height: 0.,
    })?;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    let job = editor.prefab_job(PrefabCommand::Instantiate {
        asset: prefab,
        position: Some([10., 0., 0.]),
    })?;
    editor.accept_prefab(wait(&job)?)?;
    let instance = editor.selected.clone().unwrap();
    assert!(
        editor
            .terrain_source(&instance)?
            .terrain
            .heights
            .iter()
            .all(|h| *h == 0.)
    );
    assert_eq!(
        editor
            .terrain_source(&ground)?
            .terrain
            .sample([0.; 2])
            .unwrap()
            .0,
        2.
    );
    let collision = editor
        .collisions()?
        .raycast(Vec3::new(10., 10., 0.), Vec3::NEG_Y, 20., None)?
        .unwrap();
    assert!(collision.position.y.abs() < 1e-5);
    assert!(
        editor
            .scene()
            .objects
            .iter()
            .any(|o| o.parent.as_deref() == Some(&instance) && o.mesh_collider.is_some())
    );
    let path = editor.path.clone();
    editor.save(&path)?;
    let reopened = Editor::open(&path)?;
    assert_eq!(reopened.scene(), editor.scene());
    assert!(
        reopened
            .terrain_source(&instance)?
            .terrain
            .heights
            .iter()
            .all(|h| *h == 0.)
    );
    Ok(())
}
