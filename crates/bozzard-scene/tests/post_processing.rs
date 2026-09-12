use bozzard_scene::*;
use glam::Vec3;
fn legacy() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"legacy","views":{},"objects":[],"display":{"exposure_ev":0,"tone_mapping":true,"bloom":{"enabled":true,"intensity":0.4,"threshold":0.5,"scatter":0.7}}}"#).unwrap()
}
#[test]
fn legacy_defaults_and_presets_round_trip() {
    let mut scene = legacy();
    assert_eq!(scene.display.tone_mapper, ToneMapper::Reinhard);
    assert_eq!(scene.display.bloom.anamorphic, 0.);
    assert_eq!(scene.display.color_grading, ColorGrading::default());
    assert!(!scene.display.ambient_occlusion.enabled);
    assert!(!scene.display.heat_distortion.enabled);
    assert!(scene.post_process_volumes.is_empty());
    for preset in DisplayPreset::ALL {
        scene.display = DisplaySettings::preset(preset);
        scene.post_process_volumes = vec![PostProcessVolume {
            display: scene.display,
            ..Default::default()
        }];
        scene.validate().unwrap();
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
}
#[test]
fn all_controls_reject_non_finite_values_and_unknown_fields() {
    let fields: &[fn(&mut DisplaySettings) -> &mut f32] = &[
        |d| &mut d.exposure_ev,
        |d| &mut d.bloom.anamorphic,
        |d| &mut d.color_grading.temperature,
        |d| &mut d.color_grading.tint,
        |d| &mut d.color_grading.saturation,
        |d| &mut d.color_grading.contrast,
        |d| &mut d.color_grading.lift[0],
        |d| &mut d.color_grading.gamma[1],
        |d| &mut d.color_grading.gain[2],
        |d| &mut d.ambient_occlusion.intensity,
        |d| &mut d.ambient_occlusion.radius,
        |d| &mut d.ambient_occlusion.bias,
        |d| &mut d.heat_distortion.strength,
        |d| &mut d.heat_distortion.threshold,
        |d| &mut d.heat_distortion.speed,
        |d| &mut d.heat_distortion.rise,
        |d| &mut d.grain.intensity,
        |d| &mut d.grain.size,
        |d| &mut d.vignette.intensity,
        |d| &mut d.vignette.roundness,
        |d| &mut d.vignette.feather,
    ];
    for field in fields {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut settings = DisplaySettings::default();
            *field(&mut settings) = invalid;
            assert!(settings.validate().is_err());
        }
    }
    let mut json = serde_json::to_value(legacy()).unwrap();
    json["display"]["heat_distortion"]["strenth"] = 2.into();
    assert!(Scene::from_json(&json.to_string()).is_err());
}
#[test]
fn volumes_blend_smoothly_and_have_stable_priority() {
    let mut scene = legacy();
    scene.display = DisplaySettings::default();
    let target = DisplaySettings {
        exposure_ev: 2.,
        ..DisplaySettings::preset(DisplayPreset::Bonfire)
    };
    let volume = PostProcessVolume {
        half_size: [1.; 3],
        blend_distance: 2.,
        display: target,
        ..Default::default()
    };
    scene.post_process_volumes.push(volume);
    assert_eq!(scene.display_at(Vec3::ZERO), target);
    assert_eq!(scene.display_at(Vec3::new(4., 0., 0.)), scene.display);
    let midpoint = scene.display_at(Vec3::new(2., 0., 0.));
    assert_eq!(midpoint.exposure_ev, 1.);
    assert!((midpoint.bloom.intensity - target.bloom.intensity * 0.5).abs() < 1e-6);
    assert!(
        (midpoint.heat_distortion.strength - target.heat_distortion.strength * 0.5).abs() < 1e-6
    );
    let a = scene.display_at(Vec3::new(2.999, 0., 0.));
    assert!(a.bloom.intensity < 0.00001);
    scene.post_process_volumes.push(PostProcessVolume {
        priority: -1,
        display: DisplaySettings {
            exposure_ev: -4.,
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(scene.display_at(Vec3::ZERO).exposure_ev, 2.);
    scene.post_process_volumes[1].priority = 1;
    assert_eq!(scene.display_at(Vec3::ZERO).exposure_ev, -4.);
    scene.post_process_volumes[1].enabled = false;
    assert_eq!(scene.display_at(Vec3::ZERO).exposure_ev, 2.);
}
#[test]
fn malformed_and_excessive_volumes_are_rejected() {
    let mut scene = legacy();
    scene.post_process_volumes = vec![PostProcessVolume::default(); 33];
    assert!(scene.validate().is_err());
    scene.post_process_volumes.truncate(1);
    scene.post_process_volumes[0].half_size[1] = 0.;
    assert!(scene.validate().is_err());
    scene.post_process_volumes[0] = PostProcessVolume::default();
    scene.post_process_volumes[0].center[0] = f32::NAN;
    assert!(scene.validate().is_err());
}

#[test]
fn volumetric_defaults_validation_and_volume_blending() {
    use bozzard_scene::VolumetricFog;
    let mut scene = legacy();
    assert!(!scene.display.volumetric_fog.enabled);
    let fields: &[fn(&mut VolumetricFog) -> &mut f32] = &[
        |v| &mut v.density,
        |v| &mut v.albedo[0],
        |v| &mut v.anisotropy,
        |v| &mut v.base_height,
        |v| &mut v.height_falloff,
        |v| &mut v.start_distance,
        |v| &mut v.max_distance,
        |v| &mut v.noise_amount,
        |v| &mut v.noise_scale,
        |v| &mut v.wind[1],
        |v| &mut v.light_intensity,
        |v| &mut v.ambient,
    ];
    for field in fields {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut value = VolumetricFog::default();
            *field(&mut value) = invalid;
            assert!(value.validate().is_err());
        }
    }
    for steps in [0, 15, 97, u32::MAX] {
        assert!(
            VolumetricFog {
                steps,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        VolumetricFog {
            start_distance: 50.,
            max_distance: 20.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    let target = DisplaySettings {
        volumetric_fog: VolumetricFog {
            enabled: true,
            density: 0.2,
            wind: [1., 2., 3.],
            ..Default::default()
        },
        ..Default::default()
    };
    scene.post_process_volumes = vec![PostProcessVolume {
        half_size: [1.; 3],
        blend_distance: 2.,
        display: target,
        ..Default::default()
    }];
    assert_eq!(
        scene.display_at(Vec3::ZERO).volumetric_fog,
        target.volumetric_fog
    );
    assert!(
        (scene
            .display_at(Vec3::new(2., 0., 0.))
            .volumetric_fog
            .density
            - 0.1)
            .abs()
            < 1e-6
    );
    assert!(
        !scene
            .display_at(Vec3::new(4., 0., 0.))
            .volumetric_fog
            .enabled
    );
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
}
