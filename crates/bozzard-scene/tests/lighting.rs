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
    scene.lighting.sun_color = [2.; 3];
    assert!(scene.validate().is_err());
}
