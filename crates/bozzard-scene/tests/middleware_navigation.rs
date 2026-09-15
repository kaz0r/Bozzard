use bozzard_ecs::World;
use bozzard_scene::{
    Scene, Transform,
    middleware::{
        navigation::{
            BakeSettings, Behavior, Condition, Control, NavAgent, NavSurface, Runtime, State,
            Transition,
        },
        registry,
        signals::{Kind, Signals},
    },
};
use glam::Vec3;
use std::sync::Arc;
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"navigation","views":{},"objects":[
    {"id":"floor","name":"Floor","transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"collider":{"center":[0,0,0],"size":[12,1,12],"enabled":true}},
    {"id":"wall","name":"Wall","transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"collider":{"center":[0,0,0],"size":[0.5,2,2],"enabled":true}},
    {"id":"nav","name":"Navigation","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
    {"id":"agent","name":"Agent","transform":{"translation":[-3,0,0],"rotation_degrees":[0,-90,0],"scale":[1,1,1]}},
    {"id":"goal","name":"Goal","transform":{"translation":[3,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}
    ]}"#).unwrap()
}
fn setup() -> (Scene, World, bozzard_scene::SceneInstance) {
    let mut scene = scene();
    let settings = BakeSettings {
        min: [-5., -1., -5.],
        max: [5., 4., 5.],
        cell: 0.5,
        radius: 0.2,
        height: 1.2,
        climb: 0.3,
        ..Default::default()
    };
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let mut progress = 0;
    let data = instance
        .bake_navigation(&world, &settings, |done, _| {
            progress = done;
            Ok(())
        })
        .unwrap();
    assert_eq!(progress, 400);
    registry::set(
        &mut scene.objects[2],
        &NavSurface {
            settings,
            baked: Some(Arc::new(data)),
        },
    )
    .unwrap();
    registry::set(
        &mut scene.objects[3],
        &NavAgent {
            surface: "nav".into(),
            radius: 0.2,
            height: 1.2,
            eye_height: 1.,
            perception_target: Some("goal".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    (scene, world, instance)
}
#[test]
fn baked_triangles_and_astar_route_around_walls_with_clearance() {
    let (scene, _, _) = setup();
    let surface = registry::get::<NavSurface>(&scene.objects[2])
        .unwrap()
        .unwrap();
    let nav = surface.baked.unwrap();
    assert_eq!(
        nav.triangles().count(),
        nav.cells.iter().flatten().count() * 2
    );
    let from = Vec3::new(-3., 0., 0.);
    let to = Vec3::new(3., 0., 0.);
    let path = nav.path(from, to).unwrap();
    assert!(path.iter().any(|p| p.z.abs() > 1.4));
    assert!(path.iter().all(|p| p.y.abs() < 1e-5));
    assert_eq!(path, nav.path(from, to).unwrap());
    assert!(nav.path(from, Vec3::new(100., 0., 0.)).is_none());
    let mut malformed = (*nav).clone();
    malformed.cells.pop();
    assert!(malformed.validate().is_err());
    let huge = BakeSettings {
        cell: 0.05,
        ..Default::default()
    };
    assert!(huge.dimensions().is_err());
}
#[test]
fn steering_arrives_without_crossing_walls_and_checkpoint_resumes_the_route() {
    let (_, mut world, mut instance) = setup();
    instance
        .control_navigation(&mut world, "agent", Control::Destination([3., 0., 0.]))
        .unwrap();
    for _ in 0..120 {
        instance.step_navigation(&mut world, 1. / 60.).unwrap();
    }
    let save = instance.save_game_json(&world).unwrap();
    for _ in 0..60 {
        instance.step_navigation(&mut world, 1. / 60.).unwrap();
    }
    let expected = world
        .get::<Transform>(instance.entity("agent").unwrap())
        .unwrap()
        .translation;
    instance.load_game_json(&mut world, &save).unwrap();
    for _ in 0..60 {
        instance.step_navigation(&mut world, 1. / 60.).unwrap();
    }
    assert_eq!(
        world
            .get::<Transform>(instance.entity("agent").unwrap())
            .unwrap()
            .translation,
        expected
    );
    let mut arrived = false;
    for _ in 0..900 {
        instance.step_navigation(&mut world, 1. / 60.).unwrap();
        let p = world
            .get::<Transform>(instance.entity("agent").unwrap())
            .unwrap()
            .translation;
        assert!(
            !(p[0].abs() < 0.4 && p[2].abs() < 1.1),
            "agent crossed wall at {p:?}"
        );
        arrived |= world
            .resource::<Signals>()
            .unwrap()
            .for_owner("agent", Kind::Navigation)
            .any(|s| s.name == "Arrived");
    }
    assert!(
        arrived,
        "agent did not arrive: {:?}",
        world.resource::<Runtime>().unwrap()
    );
    let p = Vec3::from(
        world
            .get::<Transform>(instance.entity("agent").unwrap())
            .unwrap()
            .translation,
    );
    assert!(p.distance(Vec3::new(3., 0., 0.)) < 0.7, "{p:?}");
}
#[test]
fn vision_checks_occlusion_and_fov_before_state_machine_pursuit() {
    let (mut scene, _, _) = setup();
    let mut agent = registry::get::<NavAgent>(&scene.objects[3])
        .unwrap()
        .unwrap();
    agent.states = Arc::new(vec![
        State::default(),
        State {
            name: "Chase".into(),
            behavior: Behavior::Follow,
            target: Some("goal".into()),
            ..Default::default()
        },
    ]);
    agent.transitions = Arc::new(vec![Transition {
        from: "Idle".into(),
        to: "Chase".into(),
        condition: Condition::SeeTarget,
        ..Default::default()
    }]);
    registry::set(&mut scene.objects[3], &agent).unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    instance.step_navigation(&mut world, 0.).unwrap();
    assert!(!world.resource::<Runtime>().unwrap().agents["agent"].sees_target);
    world
        .get_mut::<Transform>(instance.entity("goal").unwrap())
        .unwrap()
        .translation = [-5., 0., 0.];
    instance.step_navigation(&mut world, 0.).unwrap();
    assert!(
        !world.resource::<Runtime>().unwrap().agents["agent"].sees_target,
        "behind the field of view"
    );
    world
        .get_mut::<Transform>(instance.entity("goal").unwrap())
        .unwrap()
        .translation = [0., 0., 3.];
    instance.step_navigation(&mut world, 0.).unwrap();
    let run = &world.resource::<Runtime>().unwrap().agents["agent"];
    assert!(run.sees_target);
    assert_eq!(agent.states[run.state].name, "Chase");
    assert!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("agent", Kind::Navigation)
            .any(|s| s.name == "TargetSeen")
    );
}

#[test]
fn disconnected_islands_and_nested_agent_ownership_are_rejected() {
    let (mut scene, _, _) = setup();
    let mut nav = registry::get::<NavSurface>(&scene.objects[2])
        .unwrap()
        .unwrap();
    let data = Arc::make_mut(nav.baked.as_mut().unwrap());
    let middle = data.dimensions[0] / 2;
    for row in 0..data.dimensions[1] {
        data.cells[row * data.dimensions[0] + middle] = None;
    }
    assert!(
        data.path(Vec3::new(-3., 0., 0.), Vec3::new(3., 0., 0.))
            .is_none()
    );
    let mut child = scene.objects[3].clone();
    child.id = "child-agent".into();
    child.parent = Some("agent".into());
    scene.objects.push(child);
    assert!(
        scene
            .validate()
            .unwrap_err()
            .to_string()
            .contains("parented")
    );
    let scene = Scene::from_json(r#"{"version":1,"name":"No navigation","views":{},"objects":[]}"#)
        .unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    instance.step_navigation(&mut world, 3600.).unwrap();
}
