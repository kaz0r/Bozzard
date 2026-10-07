//! The generated voxel pagoda garden (tools/gen_pagoda.py) opens, batches by
//! asset and plays: the camera rig orbits, koi and clouds move, Space/N toggles
//! night, and Stop restores the authored document.
use bozzard_assets::AssetData;
use bozzard_editor::Editor;
use bozzard_render::MeshKind;
use bozzard_scene::{GameplayInput, Layer, Scene};
use glam::Vec3;
use std::{path::Path, time::Duration};

fn scene_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/pagoda-garden/scenes/pagoda.json")
}

#[test]
fn pagoda_garden_assets_are_outward_single_colour_voxel_meshes() {
    let editor = Editor::open(&scene_path()).unwrap();
    editor.assets.require_ready().unwrap();
    let scene = editor.scene().clone();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    let (mut trees, mut clouds, mut emissive) = (0, 0, 0);
    for entry in editor.assets.entries() {
        let Some(data) = entry.data() else {
            panic!("{} did not load", entry.id);
        };
        let AssetData::Mesh(mesh) = data else {
            assert!(matches!(data, AssetData::Script(_)), "{}", entry.id);
            continue;
        };
        assert!(
            mesh.warnings.is_empty(),
            "{}: {:?}",
            entry.id,
            mesh.warnings
        );
        // Every voxel face winds counter-clockwise around its outward normal.
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from_slice(&mesh.vertices[triangle[i] as usize][..3]));
            let normal = Vec3::from_slice(&mesh.vertices[triangle[0] as usize][3..6]);
            assert!((b - a).cross(c - a).dot(normal) > 0., "{}", entry.id);
        }
        for part in &mesh.parts {
            let shading = part.shading.as_ref().expect("glTF PBR surface");
            assert_eq!(shading.material.metallic, 0.);
            if shading.material.emissive_factor.iter().any(|v| *v > 0.) {
                emissive += 1;
            }
        }
        if entry.id.starts_with("tree-") {
            // One colour per asset keeps instanced tree parts in source order.
            assert_eq!(mesh.parts.len(), 1, "{}", entry.id);
            trees += 1;
        } else if entry.id.starts_with("cloud-") {
            assert!(mesh.parts.iter().all(|p| p.color[3] < 1.), "{}", entry.id);
            clouds += 1;
        } else if entry.id == "garden" || entry.id == "pagoda" {
            assert!(mesh.parts.len() >= 15, "{}: {}", entry.id, mesh.parts.len());
        }
    }
    assert!(trees > 60, "{trees}");
    assert_eq!(clouds, 7);
    assert!(
        emissive >= 6,
        "lanterns, lamps, gold and water highlights glow"
    );
}

#[test]
fn pagoda_garden_renders_in_batch_order_and_plays_day_night() {
    let mut editor = Editor::open(&scene_path()).unwrap();
    editor.assets.require_ready().unwrap();
    let scene = editor.scene().clone();
    let rendered = editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    assert!(rendered.lighting.shadows);
    assert_eq!(rendered.lights.len(), 5);
    assert!(rendered.environment.background);
    assert!(rendered.fog.enabled);
    // Objects render in ID order; IDs group each mesh's instances contiguously so
    // the batch planner keeps source order at its lower bound.
    let mut seen = std::collections::BTreeSet::new();
    let mut previous: Option<&MeshKind> = None;
    for item in &rendered.items {
        let key = match &item.mesh {
            MeshKind::ModelPart(id, part) => format!("{id}#{part}"),
            other => format!("{other:?}"),
        };
        if previous.is_none_or(|p| p != &item.mesh) {
            assert!(seen.insert(key.clone()), "{key} is not contiguous");
        }
        previous = Some(&item.mesh);
    }
    let cubes = rendered
        .items
        .iter()
        .filter(|i| i.mesh == MeshKind::Cube)
        .count();
    assert!(
        cubes > 400,
        "flowers, petal carpets and koi are stock cubes: {cubes}"
    );

    editor.start_play().unwrap();
    let advance = |editor: &mut Editor, frames: usize, input: GameplayInput| {
        for _ in 0..frames {
            editor.play.as_mut().unwrap().set_gameplay_input(input);
            editor.advance(Duration::from_secs_f64(1. / 60.));
        }
        editor.play.as_ref().unwrap().check_simulation().unwrap();
    };
    advance(&mut editor, 120, GameplayInput::default());
    let day = editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    assert_ne!(
        day.view_projection, rendered.view_projection,
        "the camera rig orbits"
    );
    let moved = |id_prefix: &str| {
        let play = editor.play.as_ref().unwrap();
        scene
            .objects
            .iter()
            .filter(|o| o.id.starts_with(id_prefix))
            .any(|object| {
                let entity = play.instance().entity(&object.id).unwrap();
                let now = play
                    .app
                    .world
                    .get::<bozzard_scene::Transform>(entity)
                    .unwrap();
                Vec3::from_array(now.translation)
                    .distance(Vec3::from_array(object.transform.translation))
                    > 0.05
            })
    };
    assert!(moved("f-cube-b-koi-"), "koi swim");
    assert!(moved("g-cloud-"), "clouds drift");
    assert_eq!(day.lighting.sun_intensity, scene.lighting.sun_intensity);

    let night_key = GameplayInput {
        keys: bozzard_scene::keys::bit("N"),
        ..Default::default()
    };
    advance(&mut editor, 1, night_key);
    advance(&mut editor, 2, GameplayInput::default());
    let night = editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    assert!(
        night.lighting.sun_intensity < 0.5,
        "{}",
        night.lighting.sun_intensity
    );
    assert!(night.environment.star_intensity > 0.);
    assert!(night.fog.color.iter().sum::<f32>() < day.fog.color.iter().sum::<f32>());
    advance(&mut editor, 1, night_key);
    advance(&mut editor, 2, GameplayInput::default());
    let morning = editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    assert_eq!(morning.lighting.sun_intensity, scene.lighting.sun_intensity);
    assert_eq!(morning.environment.star_intensity, 0.);

    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
}
