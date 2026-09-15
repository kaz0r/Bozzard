use bozzard_ecs::World;
use bozzard_scene::{
    Scene, Transform,
    middleware::{
        curve::Curve,
        registry,
        tween::{Control, Property, Track, Tween},
    },
};
use std::sync::Arc;
fn setup(autoplay: bool) -> (Scene, World, bozzard_scene::SceneInstance) {
    let mut scene = Scene::from_json(r#"{"version":1,"name":"motion","views":{},"objects":[{"id":"cube","name":"Cube","transform":{"translation":[7,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    let mut track = Track::new(Property::Translation);
    track.channels[0] = Curve::linear(0., 10., 2.);
    registry::set(
        &mut scene.objects[0],
        &Tween {
            autoplay,
            duration: 2.,
            tracks: Arc::new(vec![track]),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    (scene, world, instance)
}
#[test]
fn motion_components_roundtrip_and_play_pause_seek_complete_and_restore() {
    let (scene, mut world, mut instance) = setup(false);
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    let entity = instance.entity("cube").unwrap();
    instance.step_tweens(&mut world, 0.5).unwrap();
    assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 7.);
    instance
        .control_tween(&mut world, "cube", Control::Play { restart: true })
        .unwrap();
    instance.step_tweens(&mut world, 0.5).unwrap();
    assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 2.5);
    instance
        .control_tween(&mut world, "cube", Control::Pause)
        .unwrap();
    instance.step_tweens(&mut world, 0.5).unwrap();
    assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 2.5);
    instance
        .control_tween(&mut world, "cube", Control::Seek(1.))
        .unwrap();
    instance.step_tweens(&mut world, 0.).unwrap();
    assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 5.);
    instance
        .control_tween(&mut world, "cube", Control::Play { restart: false })
        .unwrap();
    let checkpoint = instance.save_game_json(&world).unwrap();
    instance.step_tweens(&mut world, 1.).unwrap();
    assert!(
        world
            .resource::<bozzard_scene::middleware::tween::Runtime>()
            .unwrap()
            .finished
            .contains("cube")
    );
    instance.step_tweens(&mut world, 0.1).unwrap();
    assert!(
        world
            .resource::<bozzard_scene::middleware::tween::Runtime>()
            .unwrap()
            .finished
            .is_empty()
    );
    instance.load_game_json(&mut world, &checkpoint).unwrap();
    instance.step_tweens(&mut world, 0.5).unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("cube").unwrap())
            .unwrap()
            .translation[0],
        7.5
    );
}
#[test]
fn malformed_known_motion_and_removed_targets_fail_before_spawn() {
    let (mut scene, _, _) = setup(true);
    scene.objects[0].extras.get_mut("tween").unwrap()["typo"] = true.into();
    assert!(Scene::from_json(&serde_json::to_string(&scene).unwrap()).is_err());
    let (mut scene, _, _) = setup(true);
    let mut tween: Tween = registry::get(&scene.objects[0]).unwrap().unwrap();
    Arc::make_mut(&mut tween.tracks)[0].target =
        bozzard_scene::blueprint::ObjectRef::Id("missing".into());
    registry::set(&mut scene.objects[0], &tween).unwrap();
    let mut world = World::default();
    assert!(scene.spawn(&mut world).is_err());
    assert_eq!(world.query::<Transform>().count(), 0);
}

#[test]
fn timeline_boundaries_seek_cameras_and_checkpoint_are_deterministic() {
    use bozzard_scene::{
        Camera, Layer,
        middleware::{
            curve::Repeat,
            signals::{Kind, Signals},
            timeline::{CameraCut, Marker, Timeline, crossed_markers},
        },
    };
    assert_eq!(
        crossed_markers([0., 0.5, 1.], 0., 2., 1., Repeat::Loop, true).unwrap(),
        [0, 1, 0, 2, 1, 0, 2]
    );
    assert_eq!(
        crossed_markers([0., 0.5, 1.], 0., 2., 1., Repeat::PingPong, true).unwrap(),
        [0, 1, 2, 1, 0]
    );
    assert!(crossed_markers([0.5], 0., 1e20, 1., Repeat::Loop, false).is_err());
    let (mut scene, _, _) = setup(false);
    let mut camera = scene.objects[0].clone();
    camera.id = "camera".into();
    camera.extras.clear();
    camera.camera = Some(Camera::Perspective {
        vertical_fov_degrees: 60.,
        near: 0.1,
        far: 100.,
    });
    let mut second = camera.clone();
    second.id = "cut".into();
    second.transform.translation[0] = 20.;
    scene.views.insert(Layer::ThreeD, camera.id.clone());
    scene.objects.extend([camera, second]);
    let motion = registry::get::<Tween>(&scene.objects[0]).unwrap().unwrap();
    scene.objects[0].extras.clear();
    registry::set(
        &mut scene.objects[0],
        &Timeline {
            motion: Tween {
                autoplay: true,
                ..motion
            },
            markers: Arc::new(vec![
                Marker {
                    time: 0.,
                    name: "Begin".into(),
                },
                Marker {
                    time: 1.,
                    name: "Cut".into(),
                },
            ]),
            cameras: Arc::new(vec![CameraCut {
                time: 1.,
                camera: "cut".into(),
                layer: Layer::ThreeD,
            }]),
        },
    )
    .unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.step_timelines(&mut world, 1.).unwrap();
    let names: Vec<_> = world
        .resource::<Signals>()
        .unwrap()
        .for_owner("cube", Kind::Timeline)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["Begin", "Cut"]);
    let cut_projection = instance
        .view(&world, Layer::ThreeD, 1.)
        .unwrap()
        .view_projection;
    let checkpoint = instance.save_game_json(&world).unwrap();
    instance
        .control_timeline(&mut world, "cube", Control::Seek(0.5))
        .unwrap();
    instance.step_timelines(&mut world, 0.).unwrap();
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("cube", Kind::Timeline)
            .count(),
        0
    );
    assert_ne!(
        instance
            .view(&world, Layer::ThreeD, 1.)
            .unwrap()
            .view_projection,
        cut_projection
    );
    instance.load_game_json(&mut world, &checkpoint).unwrap();
    assert_eq!(
        instance
            .view(&world, Layer::ThreeD, 1.)
            .unwrap()
            .view_projection,
        cut_projection
    );
    instance.step_timelines(&mut world, 0.).unwrap();
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("cube", Kind::Timeline)
            .count(),
        0
    );
    instance
        .control_timeline(&mut world, "cube", Control::Stop)
        .unwrap();
    instance.step_timelines(&mut world, 0.).unwrap();
    assert_ne!(
        instance
            .view(&world, Layer::ThreeD, 1.)
            .unwrap()
            .view_projection,
        cut_projection
    );
}
