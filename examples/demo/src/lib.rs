//! Shared simulation for the native player and the headless executable.
mod prefabs;
use bozzard_app::{App, Entity, Plugin};
use bozzard_scene::{GameplayInput, GameplayState, Scene, SceneInstance, Spin, Transform};
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub fn load_document(path: Option<&Path>) -> anyhow::Result<Scene> {
    match path {
        Some(path) => Scene::from_json(&std::fs::read_to_string(path)?),
        None => scene_document(),
    }
}

/// Rebase file references when saving elsewhere; assets themselves stay at their source paths.
pub fn save_document_from(scene: &Scene, path: &Path, source: Option<&Path>) -> anyhow::Result<()> {
    save_document(&prepare_document_from(scene, path, source)?, path)
}

/// Prepare rebased references without replacing the destination document.
pub fn prepare_document_from(
    scene: &Scene,
    path: &Path,
    source: Option<&Path>,
) -> anyhow::Result<Scene> {
    use anyhow::{Context, ensure};
    if scene.assets.is_empty() {
        scene.validate()?;
        return Ok(scene.clone());
    }
    scene.validate()?;
    let parent = |path: &Path| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    };
    let root = source
        .map(parent)
        .unwrap_or_else(|| Path::new(".").to_path_buf());
    let destination = parent(path);
    std::fs::create_dir_all(&destination)?;
    let destination = destination.canonicalize()?;
    let mut saved = scene.clone();
    for asset in saved.assets.values_mut() {
        let target = root
            .join(&asset.path)
            .canonicalize()
            .with_context(|| format!("resolving asset '{}' for save", asset.path))?;
        let from: Vec<_> = destination.components().collect();
        let to: Vec<_> = target.components().collect();
        let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
        ensure!(
            common > 0 && from.first() == to.first(),
            "cannot save relative asset references across filesystem roots"
        );
        let mut relative = std::path::PathBuf::new();
        for _ in common..from.len() {
            relative.push("..");
        }
        for component in &to[common..] {
            relative.push(component.as_os_str());
        }
        asset.path = relative
            .to_str()
            .context("asset path is not UTF-8")?
            .replace('\\', "/");
    }
    Ok(saved)
}

/// Validate first, then replace through a sibling temporary file so failed saves keep the old file.
pub fn save_document(scene: &Scene, path: &Path) -> anyhow::Result<()> {
    save_json(&scene.to_json()?, path)
}

/// Atomic replacement shared by scene and portable blueprint exports.
pub fn save_json(json: &str, path: &Path) -> anyhow::Result<()> {
    static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".bozzard-save-{}-{}.tmp",
        std::process::id(),
        NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn scene_document() -> anyhow::Result<Scene> {
    Scene::from_json(include_str!("../scenes/scene-lab.json"))
}

#[derive(Default)]
struct SimulationStatus {
    error: Option<String>,
}

pub struct SceneDemo {
    pub app: App,
}

impl SceneDemo {
    pub fn instance(&self) -> &SceneInstance {
        self.app
            .world
            .resource::<SceneInstance>()
            .expect("scene instance")
    }
    pub fn with_instance<R>(
        &mut self,
        f: impl FnOnce(&mut SceneInstance, &mut bozzard_app::World) -> R,
    ) -> R {
        let mut instance = self
            .app
            .world
            .remove_resource::<SceneInstance>()
            .expect("scene instance");
        let result = f(&mut instance, &mut self.app.world);
        self.app.world.insert_resource(instance);
        result
    }
    pub fn accepts_gameplay_input(&self) -> bool {
        self.gameplay().is_some() || self.instance().has_blueprints()
    }
    pub fn gameplay(&self) -> Option<&GameplayState> {
        self.app.world.resource::<GameplayState>()
    }
    /// Preserve queued edges until a fixed tick; neutral input clears them on focus loss.
    pub fn set_gameplay_input(&mut self, input: GameplayInput) {
        let previous = self
            .app
            .world
            .resource::<GameplayInput>()
            .copied()
            .unwrap_or_default();
        self.app.world.insert_resource(GameplayInput {
            movement: input.movement,
            jump: previous.jump || input.jump,
            orbit: [
                previous.orbit[0] + input.orbit[0],
                previous.orbit[1] + input.orbit[1],
            ],
        });
    }
    pub fn clear_gameplay_input(&mut self) {
        self.app.world.insert_resource(GameplayInput::default());
    }
    pub fn check_simulation(&self) -> anyhow::Result<()> {
        if let Some(error) = self
            .app
            .world
            .resource::<SimulationStatus>()
            .and_then(|status| status.error.as_ref())
        {
            anyhow::bail!("simulation failed: {error}");
        }
        Ok(())
    }
    pub fn new_with_prefabs(document: &Scene, path: Option<&Path>) -> anyhow::Result<Self> {
        let (scene, templates) = prefabs::load(document, path)?;
        let mut demo = Self::new(&scene)?;
        demo.with_instance(|instance, _| -> anyhow::Result<()> {
            for (asset, prefab) in templates {
                instance.register_prefab(asset, prefab)?;
            }
            Ok(())
        })?;
        Ok(demo)
    }
    pub fn new(document: &Scene) -> anyhow::Result<Self> {
        let mut app = App::default();
        let instance = document.spawn(&mut app.world)?;
        app.add_system(|world, _, tick| {
            if world
                .resource::<SimulationStatus>()
                .is_some_and(|status| status.error.is_some())
            {
                return;
            }
            let dynamic: std::collections::HashSet<_> = world
                .query::<bozzard_scene::Gravity>()
                .filter(|(e, g)| {
                    g.enabled && world.get::<bozzard_scene::PlayerController>(*e).is_none()
                })
                .map(|(e, _)| e)
                .collect();
            for (entity, transform, spin) in world
                .query_pair_mut::<Transform, Spin>()
                .expect("distinct components")
            {
                if dynamic.contains(&entity) {
                    continue;
                }
                for axis in 0..3 {
                    transform.rotation_degrees[axis] = (transform.rotation_degrees[axis]
                        + spin.0[axis] * tick.delta.as_secs_f32())
                    .rem_euclid(360.0);
                }
            }
        });
        app.world.insert_resource(instance);
        app.world.insert_resource(SimulationStatus::default());
        app.add_system(move |world, _, tick| {
            // Freeze on simulation failure rather than silently advancing a broken world.
            if world
                .resource::<SimulationStatus>()
                .is_some_and(|status| status.error.is_some())
            {
                return;
            }
            let dt = tick.delta.as_secs_f32();
            let input = world
                .resource::<GameplayInput>()
                .copied()
                .unwrap_or_default();
            let mut gravity_instance = world
                .remove_resource::<SceneInstance>()
                .expect("scene instance");
            let error = gravity_instance
                .advance_display(dt)
                .and_then(|()| gravity_instance.gameplay_motion(world, dt))
                .and_then(|()| gravity_instance.step_gravity(world, dt))
                .and_then(|()| gravity_instance.gameplay_interactions(world))
                .and_then(|()| gravity_instance.step_blueprints(world, dt, input))
                .and_then(|()| gravity_instance.step_particles(world, dt))
                .err()
                .map(|error| format!("{error:#}"));
            world.insert_resource(GameplayInput {
                movement: input.movement,
                ..Default::default()
            });
            world.insert_resource(gravity_instance);
            world.insert_resource(SimulationStatus { error });
        });
        Ok(Self { app })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Position(pub [f32; 3]);
#[derive(Debug, Clone, Copy)]
pub struct Velocity(pub [f32; 3]);

pub struct MovementPlugin;
impl Plugin for MovementPlugin {
    fn name(&self) -> &'static str {
        "bozzard.demo.movement"
    }
    fn build(&self, app: &mut App) {
        app.add_system(|world, _, tick| {
            for (_, position, velocity) in world
                .query_pair_mut::<Position, Velocity>()
                .expect("distinct component types")
            {
                for axis in 0..3 {
                    position.0[axis] += velocity.0[axis] * tick.delta.as_secs_f32();
                }
            }
        });
    }
}

pub fn demo() -> (App, Entity) {
    let mut app = App::default();
    app.add_plugin(MovementPlugin).unwrap();
    let entity = app.world.spawn();
    app.world.insert(entity, Position([0.0; 3])).unwrap();
    app.world.insert(entity, Velocity([0.1, 0.0, 0.0])).unwrap();
    (app, entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gameplay_edges_accumulate_until_tick_and_clear_on_focus_loss() {
        let document = Scene::from_json(include_str!("../scenes/first-trail.json")).unwrap();
        let mut demo = SceneDemo::new(&document).unwrap();
        demo.set_gameplay_input(GameplayInput {
            orbit: [10.0, 0.0],
            jump: true,
            ..Default::default()
        });
        demo.set_gameplay_input(GameplayInput {
            orbit: [10.0, 0.0],
            ..Default::default()
        });
        demo.app.step();
        demo.check_simulation().unwrap();
        let yaw = demo.gameplay().unwrap().yaw;
        assert!((yaw - 356.0).abs() < 0.001);
        demo.app.step();
        assert_eq!(demo.gameplay().unwrap().yaw, yaw);
        demo.set_gameplay_input(GameplayInput {
            movement: [1.0, 0.0],
            orbit: [100.0, 0.0],
            jump: true,
        });
        demo.clear_gameplay_input();
        demo.app.step();
        assert_eq!(demo.gameplay().unwrap().yaw, yaw);
        assert_eq!(
            demo.app.world.resource::<GameplayInput>().unwrap().movement,
            [0.0; 2]
        );
    }
    #[test]
    fn scene_animation_and_save_reload_preserve_parented_objects() {
        let mut demo = SceneDemo::new(&scene_document().unwrap()).unwrap();
        let before = demo.instance().global_transforms(&demo.app.world).unwrap()["satellite"];
        for _ in 0..120 {
            demo.app.step();
        }
        let after = demo.instance().global_transforms(&demo.app.world).unwrap()["satellite"];
        assert_ne!(before, after);
        let saved = demo.instance().capture(&demo.app.world).unwrap();
        let other = SceneDemo::new(&Scene::from_json(&saved.to_json().unwrap()).unwrap()).unwrap();
        assert_eq!(
            other
                .instance()
                .global_transforms(&other.app.world)
                .unwrap()["satellite"],
            after
        );
    }
    #[test]
    fn simulation_moves_on_all_three_axes() {
        let (mut app, e) = demo();
        app.world.insert(e, Velocity([0.1, -0.2, 0.3])).unwrap();
        for _ in 0..120 {
            app.step();
        }
        for (actual, expected) in app
            .world
            .get::<Position>(e)
            .unwrap()
            .0
            .iter()
            .zip([0.2, -0.4, 0.6])
        {
            assert!((actual - expected).abs() < 1e-5);
        }
    }
}
