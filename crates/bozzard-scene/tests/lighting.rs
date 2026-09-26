use bozzard_scene::{Lighting, Scene};

fn legacy() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Lights","views":{},"objects":[]}"#).unwrap()
}
#[test]
fn lighting_defaults_roundtrip_and_validation() {
    let mut scene = legacy();
    assert_eq!(scene.lighting, Lighting::default());
    scene.lighting.sun_direction = [-1., 0.2, 3.];
    scene.lighting.sun_intensity = 12.;
    scene.lighting.ambient_color = [0.1, 0.5, 0.9];
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    for direction in [[0.; 3], [f32::NAN, 0., 1.], [f32::MAX; 3]] {
        let mut invalid = scene.clone();
        invalid.lighting.sun_direction = direction;
        assert!(invalid.validate().is_err());
    }
    for intensity in [-1., f32::NAN, f32::INFINITY, 100001.] {
        let mut invalid = scene.clone();
        invalid.lighting.sun_intensity = intensity;
        assert!(invalid.validate().is_err());
        invalid = scene.clone();
        invalid.lighting.ambient_intensity = intensity;
        assert!(invalid.validate().is_err());
    }
    for resolution in [0, 255, 1000, 8192] {
        let mut invalid = scene.clone();
        invalid.lighting.shadow_resolution = resolution;
        assert!(invalid.validate().is_err());
    }
    for bias in [-0.01, f32::NAN, 1.01] {
        let mut invalid = scene.clone();
        invalid.lighting.shadow_bias = bias;
        assert!(invalid.validate().is_err());
        invalid = scene.clone();
        invalid.lighting.shadow_normal_bias = bias;
        assert!(invalid.validate().is_err());
    }
    scene.lighting.shadows = false;
    scene.lighting.shadow_resolution = 4096;
    scene.lighting.shadow_bias = 0.02;
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    scene.lighting.sun_color = [2.; 3];
    assert!(scene.validate().is_err());
}

#[test]
fn display_defaults_and_validation_roundtrip() {
    let mut scene = legacy();
    assert!(scene.display.tone_mapping);
    scene.display.exposure_ev = 2.5;
    scene.display.tone_mapping = false;
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    for ev in [f32::NAN, f32::INFINITY, -16.01, 16.01] {
        scene.display.exposure_ev = ev;
        assert!(scene.validate().is_err());
    }
}

#[test]
fn environment_validation_and_scene_roundtrip() {
    let mut scene = legacy();
    assert_eq!(scene.environment.star_intensity, 0.);
    scene.environment.zenith = [1., 0.2, 0.1];
    scene.environment.intensity = 2.;
    scene.environment.star_intensity = 1.6;
    scene.environment.background = false;
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    for intensity in [-0.1, 1001., f32::NAN, f32::INFINITY] {
        let mut invalid = scene.clone();
        invalid.environment.intensity = intensity;
        assert!(invalid.validate().is_err());
        invalid = scene.clone();
        invalid.environment.star_intensity = intensity;
        assert!(invalid.validate().is_err());
    }
    for value in [-0.1, 1.1, f32::NAN] {
        let mut invalid = scene.clone();
        invalid.environment.ground[0] = value;
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn bloom_roundtrip_defaults_and_invalid_values() {
    use bozzard_scene::BloomSettings;
    let mut scene = legacy();
    assert!(!scene.display.bloom.enabled);
    scene.display.bloom = BloomSettings {
        enabled: true,
        intensity: 0.4,
        threshold: 2.,
        scatter: 0.6,
        anamorphic: 0.,
    };
    assert_eq!(scene, Scene::from_json(&scene.to_json().unwrap()).unwrap());
    for invalid in [
        BloomSettings {
            intensity: -0.1,
            ..Default::default()
        },
        BloomSettings {
            intensity: 10.01,
            ..Default::default()
        },
        BloomSettings {
            threshold: f32::NAN,
            ..Default::default()
        },
        BloomSettings {
            threshold: 60001.,
            ..Default::default()
        },
        BloomSettings {
            scatter: 1.01,
            ..Default::default()
        },
    ] {
        scene.display.bloom = invalid;
        assert!(scene.validate().is_err());
    }
}

fn scripted_lighting(source: &str) -> (bozzard_scene::SceneInstance, bozzard_ecs::World) {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"Runtime lighting","views":{"3d":"camera"},
        "assets":{"script":{"kind":"script","path":"lighting.rs"}},
        "objects":[{"id":"camera","name":"Camera",
            "transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100},
            "script_manager":{"scripts":[{"enabled":true,"script":"script"}]}}]}"#,
    )
    .unwrap();
    let mut world = bozzard_ecs::World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script("script".into(), source.into())
        .unwrap();
    (instance, world)
}

#[test]
fn runtime_lighting_overrides_preserve_authored_settings_and_restore_daylight() {
    use bozzard_scene::{GameplayInput, Layer};
    let (mut instance, mut world) = scripted_lighting(
        r#"
        fn on_start(me) {
            set_sun_light([0.3, 0.5, 0.8], 0.14);
            set_ambient_light([0.3, 0.5, 0.8], 0.035);
            set_star_intensity(1.6);
            set_environment([0.01, 0.02, 0.04], [0.01, 0.02, 0.04], [0.01, 0.02, 0.04], 0.5);
        }
        fn on_update(me, dt) {
            if input_pressed("D") {
                set_sun_light([1.0, 0.94, 0.82], 2.6);
                set_ambient_light([0.86, 0.94, 1.0], 0.18);
                set_environment([0.14, 0.37, 0.68], [0.65, 0.77, 0.86], [0.24, 0.30, 0.24], 0.5);
                set_star_intensity(0.0);
            }
        }
    "#,
    );
    let authored = instance.document().clone();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let night = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(night.lighting.sun_intensity, 0.14);
    assert_eq!(night.lighting.ambient_intensity, 0.035);
    assert_eq!(
        night.environment.star_intensity, 1.6,
        "environment color edit preserves stars"
    );
    assert_eq!(night.environment.ground, [0.01, 0.02, 0.04]);
    assert_eq!(
        night.lighting.sun_direction,
        authored.lighting.sun_direction
    );
    assert_eq!(
        night.lighting.shadow_resolution,
        authored.lighting.shadow_resolution
    );
    instance
        .step_scripts(
            &mut world,
            1. / 60.,
            GameplayInput {
                keys: bozzard_scene::keys::bit("D"),
                ..Default::default()
            },
        )
        .unwrap();
    let day = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(day.environment.star_intensity, 0.);
    assert_eq!(day.lighting.sun_intensity, 2.6);
    assert_eq!(day.environment.ground, [0.24, 0.30, 0.24]);
    assert_eq!(
        instance.document(),
        &authored,
        "Play lighting must not edit the authored document"
    );
}

#[test]
fn invalid_runtime_lighting_is_rejected_before_application() {
    use bozzard_scene::{GameplayInput, Layer};
    for call in [
        "set_sun_light([2.0, 0.5, 0.5], 1.0)",
        "set_ambient_light([0.5, 0.5, 0.5], -1.0)",
        "set_environment([0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [-0.1, 0.5, 0.5], 1.0)",
        "set_star_intensity(-0.1)",
        "set_star_intensity(1001.0)",
    ] {
        let (mut instance, mut world) =
            scripted_lighting(&format!("fn on_start(me) {{ {call}; }}"));
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err(),
            "{call}"
        );
        let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
        assert_eq!(view.lighting, instance.document().lighting);
        assert_eq!(view.environment, instance.document().environment);
    }
}
