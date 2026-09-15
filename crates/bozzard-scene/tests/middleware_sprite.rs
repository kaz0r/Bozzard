use bozzard_ecs::World;
use bozzard_scene::{
    Layer, Scene,
    middleware::{
        registry,
        signals::{Kind, Signals},
        sprite::{Atlas, Clip, Control, FrameEvent, Runtime, Sprite, Tilemap},
    },
};
use std::{collections::BTreeSet, sync::Arc};
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"2D content","views":{},"assets":{"atlas":{"kind":"image","path":"atlas.png"}},"objects":[{"id":"sprite","name":"Sprite","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},{"id":"map","name":"Map","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap()
}
#[test]
fn sprite_clips_emit_crossed_frames_and_save_resume_exactly() {
    let mut scene = scene();
    registry::set(
        &mut scene.objects[0],
        &Sprite {
            image: "atlas".into(),
            atlas: Atlas {
                columns: 4,
                rows: 1,
            },
            initial: "Walk".into(),
            clips: Arc::new(vec![Clip {
                name: "Walk".into(),
                fps: 4.,
                frames: vec![0, 1, 2, 3],
                events: vec![FrameEvent {
                    frame: 2,
                    name: "Step".into(),
                }],
                ..Default::default()
            }]),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.step_sprites(&mut world, 0.6).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["sprite"].frame,
        2
    );
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("sprite", Kind::Sprite)
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["Step"]
    );
    let visual = instance.sprite_frame(&world, Layer::TwoD).unwrap();
    assert_eq!(visual[0].quads[0][4..], [0.5, 0., 0.25, 1.]);
    let save = instance.save_game_json(&world).unwrap();
    instance.step_sprites(&mut world, 1.).unwrap();
    instance.load_game_json(&mut world, &save).unwrap();
    instance.step_sprites(&mut world, 0.25).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["sprite"].frame,
        3
    );
    instance
        .control_sprite(&mut world, "sprite", Control::Pause)
        .unwrap();
    instance.step_sprites(&mut world, 2.).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["sprite"].frame,
        3
    );
    instance
        .control_sprite(&mut world, "sprite", Control::Frame(1))
        .unwrap();
    assert_eq!(
        instance.sprite_frame(&world, Layer::TwoD).unwrap()[0].quads[0][4],
        0.25
    );
    assert!(
        instance
            .control_sprite(&mut world, "sprite", Control::Frame(4))
            .is_err()
    );
}
#[test]
fn tile_painting_batches_geometry_merges_solids_and_deduplicates_query_results() {
    let mut scene = scene();
    registry::set(
        &mut scene.objects[1],
        &Tilemap {
            image: "atlas".into(),
            dimensions: [4, 4],
            cells: Arc::new(vec![1; 16]),
            solid: Arc::new(BTreeSet::from([1])),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let visuals = instance.sprite_frame(&world, Layer::TwoD).unwrap();
    assert_eq!(visuals.len(), 1);
    assert_eq!(visuals[0].quads.len(), 16);
    assert!(Arc::ptr_eq(
        &visuals[0].quads,
        &instance.sprite_frame(&world, Layer::TwoD).unwrap()[0].quads
    ));
    assert_eq!(instance.query_geometry(&world).unwrap().boxes.len(), 1);
    instance.set_tile(&mut world, "map", 1, 1, 0).unwrap();
    assert_eq!(
        instance.sprite_frame(&world, Layer::TwoD).unwrap()[0]
            .quads
            .len(),
        15
    );
    let geometry = instance.query_geometry(&world).unwrap();
    assert!(geometry.boxes.len() > 1);
    assert_eq!(
        geometry
            .overlap_sphere(glam::Vec3::new(2., -2., 0.), 5., None, 1)
            .unwrap(),
        ["map"]
    );
    assert!(
        instance.collisions(&world).unwrap().overlaps.is_empty(),
        "one compound map must not overlap itself"
    );
    let hole = geometry
        .raycast(glam::Vec3::new(1.5, -1.5, 2.), -glam::Vec3::Z, 4., None)
        .unwrap();
    assert!(hole.is_none());
    assert!(instance.set_tile(&mut world, "map", 4, 0, 1).is_err());
}

#[test]
fn solid_tiles_support_rigid_bodies_and_tile_edits_update_physics() {
    let mut scene = scene();
    registry::set(
        &mut scene.objects[1],
        &Tilemap {
            image: "atlas".into(),
            dimensions: [3, 1],
            cells: Arc::new(vec![1; 3]),
            solid: Arc::new(BTreeSet::from([1])),
            ..Default::default()
        },
    )
    .unwrap();
    scene.objects.push(bozzard_scene::Object {
        id: "body".into(),
        name: "Body".into(),
        transform: bozzard_scene::Transform {
            translation: [1.5, 3., 0.],
            ..Default::default()
        },
        collider: Some(Default::default()),
        gravity: Some(Default::default()),
        ..Default::default()
    });
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..240 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    let body = instance.entity("body").unwrap();
    assert!(
        world
            .get::<bozzard_scene::Transform>(body)
            .unwrap()
            .translation[1]
            > 0.4,
        "body must rest on the tiles"
    );
    for x in 0..3 {
        instance.set_tile(&mut world, "map", x, 0, 0).unwrap();
    }
    for _ in 0..120 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    assert!(
        world
            .get::<bozzard_scene::Transform>(body)
            .unwrap()
            .translation[1]
            < -1.,
        "removing the floor must update Rapier"
    );
}
