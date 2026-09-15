//! Every shipped scene survives a save and load, and no component becomes an unrecognized extra.
//!
//! The registry owns how each component is read and written; this is the check that its hooks
//! cover everything the real scenes actually contain.
use bozzard_scene::Scene;
use std::path::{Path, PathBuf};

fn scenes() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenes");
    let mut paths: Vec<_> = std::fs::read_dir(&directory)
        .expect("example scenes")
        .map(|entry| entry.expect("scene entry").path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 20, "expected the example scenes");
    paths
}

#[test]
fn every_scene_round_trips_without_losing_a_component() {
    for path in scenes() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let json = std::fs::read_to_string(&path).unwrap();
        let scene = Scene::from_json(&json).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        let saved = scene
            .to_json()
            .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        let reloaded = Scene::from_json(&saved).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        assert_eq!(reloaded, scene, "{name} changed on the way out and back");
        for object in &scene.objects {
            assert!(
                object.extras.is_empty(),
                "{name}: '{}' has unrecognized components {:?}",
                object.id,
                object.extras.keys().collect::<Vec<_>>()
            );
        }
        // The saved form still lists every component as the object's own key.
        for object in &scene.objects {
            for entry in bozzard_scene::components() {
                if (entry.present)(object) {
                    assert!(
                        saved.contains(&format!("\"{}\"", entry.name)),
                        "{name}: '{}' lost its {} component when saved",
                        object.id,
                        entry.name
                    );
                }
            }
        }
    }
}
