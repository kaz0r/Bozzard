use bozzard_demo::SceneDemo;
use bozzard_scene::{GameplayInput, Layer, Scene, Transform};
use glam::Vec3;
use std::time::Duration;

fn fixture() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Frame partitions","views":{"3d":"camera"},"objects":[
        {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}},
        {"id":"mover","name":"Mover","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}}
    ]}"#).unwrap()
}

fn demo(threaded: bool, interpolated: bool) -> SceneDemo {
    let mut demo = SceneDemo::new(&fixture()).unwrap();
    demo.set_threaded_simulation(threaded).unwrap();
    demo.set_render_interpolation(interpolated).unwrap();
    let mover = demo.instance().entity("mover").unwrap();
    // Registered after the engine's observer: the completed snapshot must still include it.
    demo.app.add_system(move |_, commands, tick| {
        let delta = tick.delta.as_secs_f32() * 3.;
        commands
            .queue(move |world| world.get_mut::<Transform>(mover).unwrap().translation[0] += delta);
    });
    demo
}

#[test]
fn host_edits_become_the_baseline_before_a_system_writes_the_same_transform() {
    let mut demo = demo(true, true);
    let tick = demo.app.timestep();
    demo.advance_with_frame(tick, || ()).unwrap();
    let mover = demo.instance().entity("mover").unwrap();
    demo.app
        .world
        .get_mut::<Transform>(mover)
        .unwrap()
        .translation[0] = 30.;
    demo.set_render_interpolation(true).unwrap();
    demo.advance_with_frame(tick + tick / 2, || ()).unwrap();
    let view = demo.render_view(Layer::ThreeD, 1., None).unwrap();
    assert!((view.objects[0].0.transform_point3(Vec3::ZERO).x - 30.025).abs() < 0.0001);
    assert!(
        (demo.app.world.get::<Transform>(mover).unwrap().translation[0] - 30.05).abs() < 0.0001
    );
}

#[test]
fn refresh_partitions_and_workers_have_identical_gameplay_and_continuous_motion() {
    let mut reference = None;
    for hz in [60_u128, 120, 144] {
        for threaded in [false, true] {
            for interpolated in [false, true] {
                let mut demo = demo(threaded, interpolated);
                let tick = demo.app.timestep().as_nanos();
                let mut previous_time = 0;
                for frame in 0..(hz * 2) {
                    demo.set_render_interpolation(interpolated).unwrap();
                    let view = demo.render_view(Layer::ThreeD, 1., None).unwrap();
                    let shown = view.objects[0].0.transform_point3(Vec3::ZERO).x;
                    if interpolated && demo.app.ticks() > 0 {
                        let expected = (previous_time as f64 / 1e9 - tick as f64 / 1e9) * 3.;
                        assert!(
                            (f64::from(shown) - expected).abs() < 0.00002,
                            "hz={hz} frame={frame}: {shown} vs {expected}"
                        );
                    }
                    let time = (frame + 1) * tick * 60 / hz;
                    demo.advance_with_frame(
                        Duration::from_nanos((time - previous_time) as u64),
                        || (),
                    )
                    .unwrap();
                    previous_time = time;
                }
                assert_eq!(demo.app.ticks(), 120);
                let state = (
                    demo.instance().capture(&demo.app.world).unwrap(),
                    demo.instance().save_game_json(&demo.app.world).unwrap(),
                );
                if let Some(reference) = &reference {
                    assert_eq!(&state, reference);
                } else {
                    reference = Some(state);
                }
            }
        }
    }
}

#[test]
fn pause_step_resume_and_disabling_history_show_exact_poses_without_rewinding() {
    let mut demo = demo(true, true);
    let tick = demo.app.timestep();
    demo.advance_with_frame(tick * 3 + tick / 2, || ()).unwrap();
    assert_eq!(demo.app.ticks(), 3);
    let control = demo
        .app
        .world
        .resource_mut::<bozzard_diagnostics::ExecutionControl>()
        .unwrap();
    control.paused = true;
    demo.set_render_interpolation(true).unwrap();
    let exact = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 1.)
        .unwrap();
    assert_eq!(
        demo.render_view(Layer::ThreeD, 1., None).unwrap().objects,
        exact.objects
    );
    demo.advance_with_frame(Duration::from_secs(1), || ())
        .unwrap();
    assert_eq!(demo.app.ticks(), 3);
    let control = demo
        .app
        .world
        .resource_mut::<bozzard_diagnostics::ExecutionControl>()
        .unwrap();
    control.paused = false;
    control.pause_after_tick = true;
    demo.app.step();
    assert_eq!(demo.app.ticks(), 4);
    let exact = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 1.)
        .unwrap();
    assert_eq!(
        demo.render_view(Layer::ThreeD, 1., None).unwrap().objects,
        exact.objects
    );
    let control = demo
        .app
        .world
        .resource_mut::<bozzard_diagnostics::ExecutionControl>()
        .unwrap();
    control.paused = false;
    control.pause_after_tick = false;
    demo.set_render_interpolation(true).unwrap();
    assert_eq!(
        demo.render_view(Layer::ThreeD, 1., None).unwrap().objects,
        exact.objects
    );
    demo.set_threaded_simulation(false).unwrap();
    demo.set_render_interpolation(false).unwrap();
    assert_eq!(
        demo.render_view(Layer::ThreeD, 1., None).unwrap().objects,
        exact.objects
    );
}

#[test]
fn a_one_tick_input_pulse_is_preserved_and_the_latency_tradeoff_is_bounded() {
    for threaded in [false, true] {
        for interpolated in [false, true] {
            let mut demo = SceneDemo::new(&fixture()).unwrap();
            demo.set_threaded_simulation(threaded).unwrap();
            demo.set_render_interpolation(interpolated).unwrap();
            let mover = demo.instance().entity("mover").unwrap();
            demo.app.add_system(move |world, _, _| {
                if world
                    .resource::<GameplayInput>()
                    .is_some_and(|i| i.keys != 0)
                {
                    world.get_mut::<Transform>(mover).unwrap().translation[0] += 1.;
                }
            });
            let tick = demo.app.timestep();
            demo.advance_with_frame(tick, || ()).unwrap();
            demo.set_gameplay_input(GameplayInput {
                keys: bozzard_scene::keys::bit("D"),
                ..Default::default()
            });
            let mut first_visible = None;
            for frame in 0..3 {
                demo.set_render_interpolation(interpolated).unwrap();
                let shown = demo.render_view(Layer::ThreeD, 1., None).unwrap().objects[0]
                    .0
                    .transform_point3(Vec3::ZERO)
                    .x;
                if shown > 0. && first_visible.is_none() {
                    first_visible = Some(frame);
                }
                demo.advance_with_frame(tick, || ()).unwrap();
                demo.set_gameplay_input(GameplayInput::default());
            }
            assert_eq!(first_visible, Some(if interpolated { 2 } else { 1 }));
            assert_eq!(
                demo.app.world.get::<Transform>(mover).unwrap().translation[0],
                1.
            );
        }
    }
}
