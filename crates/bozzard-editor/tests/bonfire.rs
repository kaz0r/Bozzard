use bozzard_editor::Editor;
use bozzard_scene::{Blueprint, GameplayInput, Light, Transform};
use std::{collections::BTreeSet, path::PathBuf};

#[test]
#[ignore = "long-running CPU-only bonfire profiling; run explicitly with --ignored --nocapture"]
fn bonfire_soak() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/bonfire-lab.json");
    let scene = bozzard_demo::load_document(Some(&path)).unwrap();
    let mut demo = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    for block in 0..8 {
        let start = std::time::Instant::now();
        for _ in 0..300 {
            demo.app.step();
            demo.check_simulation().unwrap();
            assert!(demo.instance().document().prefabs.len() <= 16);
            assert_eq!(
                demo.app.world.len(),
                scene.objects.len() + demo.instance().document().prefabs.len()
            );
            assert_eq!(
                demo.app.world.query::<Transform>().count(),
                demo.app.world.len()
            );
        }
        let memory = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|s| s.starts_with("VmRSS:"))
                    .map(str::to_owned)
            });
        eprintln!(
            "bonfire_soak ticks={} ms/tick={:.3} entities={} prefabs={} {memory:?}",
            (block + 1) * 300,
            start.elapsed().as_secs_f64() * 1000. / 300.,
            demo.app.world.len(),
            demo.instance().document().prefabs.len()
        );
    }
}

#[test]
fn bonfire_spawns_real_independent_embers_destroys_them_and_drains_when_paused() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/bonfire-lab.json");
    let mut editor = Editor::open(&path).unwrap(); // Also decodes both emissive cube assets.
    editor.assets.require_ready().unwrap();
    let authored = editor.scene().clone();
    assert!(authored.lighting.sun_intensity < 0.1 && authored.lighting.ambient_intensity < 0.02);
    assert!(authored.display.bloom.enabled);
    assert_eq!(
        authored
            .objects
            .iter()
            .filter(|o| o.light.is_some())
            .count(),
        1
    );
    let prefab = bozzard_scene::Prefab::from_json(include_str!(
        "../../../examples/demo/scenes/assets/bonfire/ember.prefab.json"
    ))
    .unwrap();
    assert_eq!(
        prefab.objects[0].blueprints[0].graph,
        Blueprint::from_json(include_str!(
            "../../../examples/demo/scenes/assets/Blueprints/ember-lifetime.blueprint.json"
        ))
        .unwrap()
    );
    let base = authored.objects.len();
    for (owner, file) in [
        ("bonfire", "bonfire-spawn"),
        ("firelight", "bonfire-flicker"),
    ] {
        let graph = Blueprint::from_json(
            &std::fs::read_to_string(
                path.parent()
                    .unwrap()
                    .join(format!("assets/Blueprints/{file}.blueprint.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            authored
                .objects
                .iter()
                .find(|o| o.id == owner)
                .unwrap()
                .blueprints[0]
                .graph,
            graph
        );
    }
    editor.start_play().unwrap();
    let demo = editor.play.as_mut().unwrap();
    let mut seen = BTreeSet::new();
    let mut first = None;
    let mut previous_size = f32::INFINITY;
    let mut brightness = (f32::INFINITY, 0_f32);
    for tick in 1..=720 {
        demo.app.step();
        demo.check_simulation().unwrap();
        let document = demo.instance().document();
        let count = document.prefabs.len();
        assert!(
            count <= 16,
            "unbounded ember population at tick {tick}: {count}"
        );
        if tick > 200 {
            assert!(
                (15..=16).contains(&count),
                "emission stopped at tick {tick}"
            );
        }
        assert_eq!(demo.app.world.len(), base + count);
        for id in document.prefabs.keys() {
            seen.insert(id.clone());
            let entity = demo.instance().entity(id).unwrap();
            first.get_or_insert_with(|| (id.clone(), entity));
            let transform = demo.app.world.get::<Transform>(entity).unwrap();
            assert!((1.24..4.5).contains(&transform.translation[1]));
            assert!(transform.scale[0] > 0. && transform.scale[0] <= 0.135);
            if first.as_ref().unwrap().0 == *id {
                assert!(transform.scale[0] <= previous_size);
                previous_size = transform.scale[0];
                if tick == 90 {
                    assert!(transform.translation[1] > 2.5);
                }
            }
        }
        let light = demo
            .app
            .world
            .get::<Light>(demo.instance().entity("firelight").unwrap())
            .unwrap();
        brightness.0 = brightness.0.min(light.intensity);
        brightness.1 = brightness.1.max(light.intensity);
    }
    assert!(seen.len() >= 65);
    assert!(brightness.0 >= 40. && brightness.0 < 43. && brightness.1 > 53. && brightness.1 <= 56.);
    let (id, entity) = first.unwrap();
    assert!(demo.instance().entity(&id).is_none());
    assert!(!demo.app.world.contains(entity)); // Real destruction, not Set Visible.
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    for _ in 0..200 {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
    assert!(demo.instance().document().prefabs.is_empty());
    assert_eq!(demo.app.world.len(), base);
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    for _ in 0..60 {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
    assert!(!demo.instance().document().prefabs.is_empty());
    assert!(
        demo.instance()
            .document()
            .prefabs
            .keys()
            .all(|id| !seen.contains(id))
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
    editor.start_play().unwrap();
    assert_eq!(editor.play.as_ref().unwrap().app.world.len(), base);
    assert!(
        editor
            .play
            .as_ref()
            .unwrap()
            .instance()
            .document()
            .prefabs
            .is_empty()
    );
}
