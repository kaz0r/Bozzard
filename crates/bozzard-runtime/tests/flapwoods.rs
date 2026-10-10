//! The shipped Blueprint game: ready, flap, clear pipes, pause, die, retry.
use bozzard_runtime::SceneRuntime;
use bozzard_scene::{
    GameAction as A, GamePhase as P, GameplayInput, Scene, TextRendering, Transform,
};

fn load() -> SceneRuntime {
    SceneRuntime::new(
        &Scene::from_json(include_str!(
            "../../../examples/demo/scenes/flap-woods.json"
        ))
        .unwrap(),
    )
    .unwrap()
}
fn step(d: &mut SceneRuntime, flap: bool) {
    d.set_gameplay_input(GameplayInput {
        jump: flap,
        ..Default::default()
    });
    d.app.step();
    d.check_simulation().unwrap();
}
fn position(d: &SceneRuntime, id: &str) -> [f32; 3] {
    d.app
        .world
        .get::<Transform>(d.instance().entity(id).unwrap())
        .unwrap()
        .translation
}
fn score(d: &SceneRuntime) -> u32 {
    d.app
        .world
        .get::<TextRendering>(d.instance().entity("score").unwrap())
        .unwrap()
        .text
        .strip_prefix("Score: ")
        .unwrap()
        .parse()
        .unwrap()
}
fn pilot(d: &SceneRuntime) -> bool {
    let pipe = (1..=3)
        .min_by(|a, b| {
            let distance = |i| {
                let x = position(d, &format!("pipe-{i}"))[0];
                if x < -6.4 { f32::MAX } else { x }
            };
            distance(*a).total_cmp(&distance(*b))
        })
        .unwrap();
    let gap = position(d, &format!("pipe-{pipe}-bottom"))[1] + 7.8;
    position(d, "bird")[1] < gap - 0.4
}

#[test]
fn waits_for_start_flaps_then_dies_and_waits_for_explicit_retry() {
    let mut d = load();
    let initial = d.instance().capture(&d.app.world).unwrap();
    for _ in 0..120 {
        step(&mut d, true);
    }
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), initial);
    d.game_action(A::Start).unwrap();
    step(&mut d, true);
    let y = position(&d, "bird")[1];
    for _ in 0..20 {
        step(&mut d, false);
    }
    assert!(position(&d, "bird")[1] > y + 0.5);
    for _ in 0..180 {
        step(&mut d, false);
    }
    assert_eq!(d.game_session().unwrap().phase, P::GameOver);
    let dead = d.instance().capture(&d.app.world).unwrap();
    for _ in 0..120 {
        step(&mut d, true);
    }
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), dead);
    d.game_action(A::Restart).unwrap();
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), initial);
    assert_eq!(score(&d), 0);
}

#[test]
fn colliding_with_pipe_or_ceiling_ends_the_run_without_awarding_a_point() {
    for obstacle in ["pipe-1-bottom", "ceiling"] {
        let mut d = load();
        d.game_action(A::Start).unwrap();
        let x = if obstacle == "ceiling" { -5. } else { -7. };
        let e = d.instance().entity(obstacle).unwrap();
        d.app.world.get_mut::<Transform>(e).unwrap().translation = [x, 0.65, 0.];
        step(&mut d, false);
        assert_eq!(d.game_session().unwrap().phase, P::GameOver, "{obstacle}");
        assert_eq!(score(&d), 0);
    }
}

#[test]
fn cleared_pairs_score_once_recycle_and_pause_and_retry_reset_everything() {
    let mut d = load();
    d.game_action(A::Start).unwrap();
    let initial = d.instance().capture(&d.app.world).unwrap();
    let mut previous = 0;
    for tick in 0..1800 {
        let flap = pilot(&d);
        step(&mut d, flap);
        assert_eq!(
            d.game_session().unwrap().phase,
            P::Playing,
            "pilot died at {tick}; y={:?}",
            position(&d, "bird")
        );
        let points = score(&d);
        assert!(points == previous || points == previous + 1);
        if points > previous {
            assert!(
                (1..=3).any(|i| (position(&d, &format!("pipe-{i}"))[0] + 6.35).abs() < 0.12),
                "award only after a cleared pair"
            );
        }
        previous = points;
        if tick == 250 {
            assert_eq!(points, 1);
            d.game_action(A::Pause).unwrap();
            let paused = d.instance().capture(&d.app.world).unwrap();
            for _ in 0..120 {
                step(&mut d, true);
            }
            assert_eq!(d.instance().capture(&d.app.world).unwrap(), paused);
            d.game_action(A::Resume).unwrap();
        }
    }
    assert!(score(&d) >= 6, "all three pipes recycle and score again");
    d.game_action(A::Restart).unwrap();
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), initial);
}
