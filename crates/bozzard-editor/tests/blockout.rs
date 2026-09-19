use anyhow::{Result, ensure};
use bozzard_assets::{
    blockout::{Blockout, BrushPrimitive},
    job::Job,
};
use bozzard_editor::Editor;
use bozzard_scene::{Layer, Scene, Transform, TriangleMesh};
use glam::Vec3;
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

struct Temp(PathBuf);
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
        ensure!(Instant::now() < deadline, "geometry job timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn blockout_reuses_geometry_preserves_existing_collision_and_publishes_one_stroke() -> Result<()> {
    let temp =
        Temp(std::env::temp_dir().join(format!("bozzard-blockout-editor-{}", std::process::id())));
    fs::create_dir(&temp.0)?;
    let mut editor = Editor::new(
        Scene::from_json(r#"{"version":1,"name":"Blockout","views":{},"objects":[]}"#)?,
        &temp.0.join("scene.json"),
    )?;
    let brush = Blockout {
        version: 1,
        primitive: BrushPrimitive::Ramp,
    };
    let job = editor.blockout_job(
        brush,
        vec![Transform {
            scale: [2., 3., 4.],
            ..Default::default()
        }],
    )?;
    let first = editor.accept_geometry(wait(&job)?)?;
    let hit = editor
        .collisions()?
        .raycast(Vec3::new(0., 10., 0.), Vec3::NEG_Y, 20., None)?
        .unwrap();
    assert!((hit.position.y - 1.5).abs() < 1e-5);
    // View the slope from above; a low camera sees the ramp's front wall first.
    let projection =
        glam::camera::rh::proj::directx::perspective(60_f32.to_radians(), 1., 0.1, 100.)
            * glam::camera::rh::view::look_at_mat4(
                Vec3::new(0., 12., 8.),
                Vec3::new(0., 1.5, 0.),
                Vec3::Y,
            );
    let (pick, point) = editor
        .pick_point_with_projection(Layer::ThreeD, projection, [0.; 2])?
        .unwrap();
    assert_eq!(pick.object, first);
    assert!(point.abs_diff_eq(Vec3::new(0., 1.5, 0.), 1e-4));
    assert_eq!(
        editor.pick_surface_with_projection(Layer::ThreeD, projection, [0.; 2])?,
        Some(pick)
    );
    let asset = editor.scene().assets.keys().next().unwrap().clone();
    let data = editor
        .assets
        .get(editor.assets.handle(&asset).unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    let mut scene = editor.scene().clone();
    let collider = scene.objects[0].mesh_collider.as_mut().unwrap();
    collider.layers = 4;
    collider.mesh = TriangleMesh::new(vec![[[-1., 0.1, -1.], [0., 0.1, 1.], [1., 0.1, -1.]]])?;
    editor.apply("Custom collision", scene.clone())?;
    let job = editor.blockout_job(
        brush,
        vec![
            Transform {
                translation: [5., 0., 0.],
                ..Default::default()
            },
            Transform {
                translation: [8., 0., 0.],
                ..Default::default()
            },
        ],
    )?;
    let group = editor.accept_geometry(wait(&job)?)?;
    assert_eq!(editor.scene().assets.len(), 1);
    assert_eq!(editor.scene().objects.len(), 4);
    assert_eq!(
        editor
            .scene()
            .objects
            .iter()
            .filter(|o| o.parent.as_deref() == Some(&group))
            .count(),
        2
    );
    assert_eq!(
        editor.scene().objects[0].mesh_collider,
        scene.objects[0].mesh_collider
    );
    assert!(Arc::ptr_eq(
        &data,
        &editor
            .assets
            .get(editor.assets.handle(&asset).unwrap())
            .unwrap()
            .shared_data()
            .unwrap()
    ));
    let after = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &scene);
    editor.redo()?;
    assert_eq!(editor.scene(), &after);
    let path = editor.path.clone();
    editor.save(&path)?;
    assert_eq!(Editor::open(&path)?.scene(), &after);
    Ok(())
}
