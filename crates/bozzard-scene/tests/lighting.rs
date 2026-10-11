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
    // Zero (every caster) is the default and stays out of saved scenes.
    assert!(!scene.to_json().unwrap().contains("shadow_distance"));
    for distance in [-1., f32::NAN, f32::INFINITY, 100001.] {
        let mut invalid = scene.clone();
        invalid.lighting.shadow_distance = distance;
        assert!(invalid.validate().is_err());
    }
    scene.lighting.shadow_distance = 45.;
    assert!(scene.to_json().unwrap().contains("shadow_distance"));
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
fn scripts_move_the_sun_and_set_its_shadows_without_editing_the_document() {
    use bozzard_scene::{GameplayInput, Layer};
    let (mut instance, mut world) = scripted_lighting(
        r#"
        fn on_update(me, dt) {
            let t = elapsed_time();
            set_sun_direction([cos(t), sin(t), 0.25]);
            set_shadow_distance(48.0);
            if input_pressed("S") { set_sun_shadows(false); }
        }
    "#,
    );
    let authored = instance.document().clone();
    instance
        .step_scripts(&mut world, 0.5, GameplayInput::default())
        .unwrap();
    let morning = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert!(morning.lighting.shadows);
    assert_eq!(morning.lighting.shadow_distance, 48.);
    assert_eq!(
        morning.lighting.sun_direction,
        [0.5_f32.cos(), 0.5_f32.sin(), 0.25]
    );
    assert_eq!(
        morning.lighting.sun_intensity, authored.lighting.sun_intensity,
        "moving the sun keeps its color and intensity"
    );
    instance
        .step_scripts(
            &mut world,
            0.5,
            GameplayInput {
                keys: bozzard_scene::keys::bit("S"),
                ..Default::default()
            },
        )
        .unwrap();
    let noon = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(
        noon.lighting.sun_direction,
        [1_f32.cos(), 1_f32.sin(), 0.25]
    );
    assert!(!noon.lighting.shadows);
    assert_eq!(instance.document(), &authored);
}

#[test]
fn scripts_set_a_perspective_camera_field_of_view() {
    use bozzard_scene::{Camera, GameplayInput};
    let scene = Scene::from_json(
        r#"{"version":1,"name":"Lens","views":{"3d":"eye"},
        "assets":{"script":{"kind":"script","path":"lens.rs"}},
        "objects":[{"id":"eye","name":"Eye",
            "transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100},
            "script_manager":{"scripts":[{"enabled":true,"script":"script"}]}}]}"#,
    )
    .unwrap();
    let mut world = bozzard_ecs::World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script(
            "script".into(),
            r#"fn on_update(me, dt) { set_camera_fov(me, 82.5); }"#.into(),
        )
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        *world
            .get::<Camera>(instance.entity("eye").unwrap())
            .unwrap(),
        Camera::Perspective {
            vertical_fov_degrees: 82.5,
            near: 0.1,
            far: 100.
        }
    );
    for call in ["set_camera_fov(me, 0.5)", "set_camera_fov(me, 179.0)"] {
        instance
            .register_script(
                "script".into(),
                format!("fn on_update(me, dt) {{ {call}; }}"),
            )
            .unwrap();
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err(),
            "{call}"
        );
    }
    // Orthographic cameras keep set_camera_size.
    let (mut instance, mut world) =
        scripted_lighting(r#"fn on_update(me, dt) { set_camera_fov(me, 70.0); }"#);
    let error = instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("perspective"), "{error:#}");
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
        "set_fog([0.5, 1.2, 0.5], 0.01)",
        "set_fog([0.5, 0.5, 0.5], -0.01)",
        "set_fog([0.5, 0.5, 0.5], 1001.0)",
        "set_sun_direction([0.0, 0.0, 0.0])",
        "set_sun_direction([1.0, 2.0])",
        "set_shadow_distance(-1.0)",
        "set_shadow_distance(100001.0)",
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
        assert_eq!(view.fog, instance.document().fog);
    }
}

#[test]
fn runtime_fog_overrides_distance_fog_without_editing_the_document() {
    use bozzard_scene::{GameplayInput, Layer};
    let (mut instance, mut world) = scripted_lighting(
        r#"
        fn on_start(me) { set_fog([0.02, 0.03, 0.08], 0.03); }
        fn on_update(me, dt) {
            if input_pressed("N") { set_fog([0.6, 0.7, 0.9], 0.0); }
        }
    "#,
    );
    let authored = instance.document().clone();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let night = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert!(night.fog.enabled);
    assert_eq!(night.fog.color, [0.02, 0.03, 0.08]);
    assert_eq!(night.fog.distance_density, 0.03);
    // Height fog and start distance stay authored.
    assert_eq!(night.fog.start_distance, authored.fog.start_distance);
    assert_eq!(night.fog.height_density, authored.fog.height_density);
    instance
        .step_scripts(
            &mut world,
            1. / 60.,
            GameplayInput {
                keys: bozzard_scene::keys::bit("N"),
                ..Default::default()
            },
        )
        .unwrap();
    let off = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert!(!off.fog.enabled, "density 0 disables distance fog");
    assert_eq!(off.fog.color, [0.6, 0.7, 0.9]);
    assert_eq!(instance.document(), &authored);
}
