use bozzard_scene::{FogSettings, Scene};

#[test]
fn fog_legacy_roundtrip_and_validation() {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"Fog","views":{},"objects":[]}"#).unwrap();
    assert_eq!(scene.fog, FogSettings::default());
    assert!(!scene.fog.enabled);
    scene.fog = FogSettings {
        enabled: true,
        color: [0.2, 0.3, 0.4],
        distance_density: 0.1,
        start_distance: 2.,
        height_density: 0.3,
        base_height: -5.,
        height_falloff: 0.2,
    };
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    for field in [
        "distance_density",
        "height_density",
        "height_falloff",
        "start_distance",
        "base_height",
    ] {
        let mut json = serde_json::to_value(&scene).unwrap();
        json["fog"][field] = 100001.into();
        assert!(Scene::from_json(&json.to_string()).is_err(), "{field}");
    }
    for value in [-1., f32::NAN, f32::INFINITY, 1001.] {
        for field in 0..3 {
            let mut invalid = scene.clone();
            *match field {
                0 => &mut invalid.fog.distance_density,
                1 => &mut invalid.fog.height_density,
                _ => &mut invalid.fog.height_falloff,
            } = value;
            assert!(invalid.to_json().is_err());
        }
    }
    for value in [-1., 1.1, f32::NAN, f32::INFINITY] {
        let mut invalid = scene.clone();
        invalid.fog.color[0] = value;
        assert!(invalid.validate().is_err());
    }
    for value in [f32::NAN, f32::INFINITY, -100001., 100001.] {
        let mut invalid = scene.clone();
        invalid.fog.base_height = value;
        assert!(invalid.validate().is_err());
        invalid.fog = scene.fog;
        invalid.fog.start_distance = value;
        assert!(invalid.validate().is_err());
    }
    let mut json = serde_json::to_value(&scene).unwrap();
    json["fog"]["typo"] = true.into();
    assert!(Scene::from_json(&json.to_string()).is_err());
}
