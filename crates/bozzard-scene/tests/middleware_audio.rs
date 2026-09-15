use bozzard_ecs::World;
use bozzard_scene::{
    GamePhase, GameSession, Layer, Scene,
    middleware::{
        audio::{AudioMixer, AudioSource, Control, Runtime, Transport, spatial_mix},
        registry,
    },
};
use glam::{Mat4, Vec3};
fn setup() -> (Scene, World, bozzard_scene::SceneInstance) {
    let mut scene = Scene::from_json(r#"{"version":1,"name":"audio","views":{},"objects":[{"id":"source","name":"Source","transform":{"translation":[5,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}],"assets":{"clip":{"kind":"audio","path":"clip.wav"}}}"#).unwrap();
    registry::set(
        &mut scene.objects[0],
        &AudioSource {
            asset: "clip".into(),
            autoplay: true,
            duration: 2.,
            min_distance: 0.,
            max_distance: 10.,
            ..Default::default()
        },
    )
    .unwrap();
    registry::set(
        &mut scene.objects[0],
        &AudioMixer {
            master: 0.8,
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    (scene, world, instance)
}
#[test]
fn spatial_transport_bus_and_checkpoint_stay_deterministic_without_a_device() {
    let (scene, mut world, mut instance) = setup();
    let source = registry::get::<AudioSource>(&scene.objects[0])
        .unwrap()
        .unwrap();
    assert_eq!(
        spatial_mix(&source, Vec3::new(5., 0., 0.), Mat4::IDENTITY),
        (0.5, 1.)
    );
    assert_eq!(
        spatial_mix(&source, Vec3::new(10., 0., 0.), Mat4::IDENTITY).0,
        0.
    );
    instance.step_audio(&mut world, 0.5).unwrap();
    instance
        .set_audio_bus_volume(&mut world, "Music", 0.2)
        .unwrap();
    let frame = instance.audio_frame(&world, Layer::ThreeD).unwrap();
    assert_eq!(
        (
            frame.master,
            frame.buses[1],
            frame.sources[0].volume,
            frame.sources[0].position
        ),
        (0.8, 0.2, 0.5, 0.5)
    );
    instance
        .control_audio(&mut world, "source", Control::Pitch(2.))
        .unwrap();
    let save = instance.save_game_json(&world).unwrap();
    instance.step_audio(&mut world, 1.).unwrap();
    assert!(
        world
            .resource::<Runtime>()
            .unwrap()
            .finished
            .contains("source")
    );
    instance.step_audio(&mut world, 0.).unwrap();
    assert!(world.resource::<Runtime>().unwrap().finished.is_empty());
    instance.load_game_json(&mut world, &save).unwrap();
    instance.step_audio(&mut world, 0.25).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().voices["source"].position,
        1.
    );
    instance
        .control_audio(&mut world, "source", Control::Pause)
        .unwrap();
    instance.step_audio(&mut world, 1.).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().voices["source"].position,
        1.
    );
    assert!(
        instance
            .control_audio(&mut world, "source", Control::Seek(f64::NAN))
            .is_err()
    );
    assert!(
        instance
            .set_audio_bus_volume(&mut world, "missing", 1.)
            .is_err()
    );
    assert!(
        instance
            .control_audio(&mut world, "source", Control::Pitch(0.))
            .is_err()
    );
}
#[test]
fn game_pause_respects_each_source_and_never_starts_a_stopped_voice() {
    let (_, mut world, instance) = setup();
    world.insert_resource(GameSession {
        phase: GamePhase::Paused,
        message: String::new(),
    });
    instance.step_audio(&mut world, 0.5).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().voices["source"].position,
        0.
    );
    assert_eq!(
        instance.audio_frame(&world, Layer::ThreeD).unwrap().sources[0].transport,
        Transport::Paused
    );
    world
        .get_mut::<AudioSource>(instance.entity("source").unwrap())
        .unwrap()
        .pause_with_game = false;
    instance.step_audio(&mut world, 0.5).unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().voices["source"].position,
        0.5
    );
    instance
        .control_audio(&mut world, "source", Control::Stop)
        .unwrap();
    world
        .get_mut::<AudioSource>(instance.entity("source").unwrap())
        .unwrap()
        .enabled = false;
    assert_eq!(
        instance.audio_frame(&world, Layer::ThreeD).unwrap().sources[0].transport,
        Transport::Stopped
    );
}

#[test]
fn audio_completion_delay_runs_during_pause_while_gameplay_delay_stays_frozen() {
    use bozzard_scene::{
        GameplayInput, Transform,
        blueprint::{Blueprint, BlueprintAttachment, Node, NodeKind as N, Socket, Value, Wire},
    };
    let (mut scene, _, _) = setup();
    scene.game_flow = Some(Default::default());
    let mut source = registry::get::<AudioSource>(&scene.objects[0])
        .unwrap()
        .unwrap();
    source.pause_with_game = false;
    registry::set(&mut scene.objects[0], &source).unwrap();
    for (event, action, value) in [
        (N::Start, N::SetPosition, [9., 0., 0.]),
        (N::AudioFinished, N::SetScale, [2.; 3]),
    ] {
        let mut delay = Node::new(2, N::Delay, [200., 0.]);
        delay.inputs[1] = Value::Number(1.);
        let mut effect = Node::new(3, action, [400., 0.]);
        effect.inputs[1] = Value::Vector(value);
        scene.objects[0].blueprints.push(BlueprintAttachment {
            enabled: true,
            graph: Blueprint {
                nodes: vec![Node::new(1, event, [0.; 2]), delay, effect],
                wires: vec![
                    Wire {
                        from: Socket { node: 1, port: 0 },
                        to: Socket { node: 2, port: 0 },
                    },
                    Wire {
                        from: Socket { node: 2, port: 0 },
                        to: Socket { node: 3, port: 0 },
                    },
                ],
                ..Default::default()
            },
        });
    }
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    world.insert_resource(GameSession {
        phase: GamePhase::Playing,
        message: String::new(),
    });
    instance
        .step_blueprints(&mut world, 0.01, GameplayInput::default())
        .unwrap();
    world.resource_mut::<GameSession>().unwrap().phase = GamePhase::Paused;
    instance.step_audio(&mut world, 2.).unwrap();
    instance
        .step_blueprints(&mut world, 0.01, GameplayInput::default())
        .unwrap();
    let save = instance.save_game_json(&world).unwrap();
    instance.load_game_json(&mut world, &save).unwrap();
    instance
        .step_blueprints(&mut world, 1.1, GameplayInput::default())
        .unwrap();
    let transform = world
        .get::<Transform>(instance.entity("source").unwrap())
        .unwrap();
    assert_eq!(transform.scale, [2.; 3]);
    assert_eq!(transform.translation, [5., 0., 0.]);
    world.resource_mut::<GameSession>().unwrap().phase = GamePhase::Playing;
    instance
        .step_blueprints(&mut world, 1.1, GameplayInput::default())
        .unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("source").unwrap())
            .unwrap()
            .translation,
        [9., 0., 0.]
    );
}
