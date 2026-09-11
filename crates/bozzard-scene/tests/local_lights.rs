use bozzard_ecs::World;
use bozzard_scene::{Layer, Light, LightKind, MAX_LOCAL_LIGHTS, Scene, Transform};
use glam::Vec3;
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Lights","views":{"3d":"camera","2d":"camera"},"objects":[
    {"id":"camera","name":"Camera","transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},
    {"id":"parent","name":"Parent","transform":{"translation":[2,0,0],"rotation_degrees":[0,90,0],"scale":[2,3,4]}},
    {"id":"light","name":"Lamp","parent":"parent","transform":{"translation":[0,0,-1],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"light":{"kind":"spot"}}
    ]}"#).unwrap()
}
#[test]
fn lights_roundtrip_transform_live_ecs_and_layer_isolation() {
    let scene = scene();
    assert_eq!(scene, Scene::from_json(&scene.to_json().unwrap()).unwrap());
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    let light = view.lights[0];
    assert!((Vec3::from(light.position) - Vec3::new(-2., 0., 0.)).length() < 1e-5);
    assert!((Vec3::from(light.direction) - Vec3::NEG_X).length() < 1e-5);
    assert_eq!(light.light.range, 10.); // Parent scale affects position, not range.
    assert_eq!(light.light.kind, LightKind::Spot);
    assert!(
        instance
            .view(&world, Layer::TwoD, 1.)
            .unwrap()
            .lights
            .is_empty()
    );
    let entity = instance.entity("light").unwrap();
    world.get_mut::<Light>(entity).unwrap().enabled = false;
    assert!(
        instance
            .view(&world, Layer::ThreeD, 1.)
            .unwrap()
            .lights
            .is_empty()
    );
    world.get_mut::<Light>(entity).unwrap().enabled = true;
    world.get_mut::<Transform>(entity).unwrap().translation = [0., 0., 0.];
    assert_eq!(
        instance.view(&world, Layer::ThreeD, 1.).unwrap().lights[0].position,
        [2., 0., 0.]
    );
    world.get_mut::<Light>(entity).unwrap().intensity = 25.;
    assert_eq!(
        instance.capture(&world).unwrap().objects[2]
            .light
            .unwrap()
            .intensity,
        25.
    );
    world.get_mut::<Light>(entity).unwrap().range = f32::NAN;
    assert!(instance.view(&world, Layer::ThreeD, 1.).is_err());
    assert!(instance.capture(&world).is_err());
}
#[test]
fn rejects_invalid_light_data_and_excess_without_spawning() {
    for light in [
        Light {
            range: 0.,
            ..Default::default()
        },
        Light {
            intensity: -1.,
            ..Default::default()
        },
        Light {
            color: [2., 0., 0.],
            ..Default::default()
        },
        Light {
            outer_angle_degrees: 90.,
            ..Default::default()
        },
        Light {
            inner_angle_degrees: 31.,
            outer_angle_degrees: 30.,
            ..Default::default()
        },
        Light {
            inner_angle_degrees: f32::NAN,
            ..Default::default()
        },
    ] {
        let mut s = scene();
        s.objects[2].light = Some(light);
        assert!(s.validate().is_err());
    }
    let mut s = scene();
    let template = s.objects[2].clone();
    for i in 1..MAX_LOCAL_LIGHTS {
        let mut o = template.clone();
        o.id = format!("light-{i}");
        s.objects.push(o);
    }
    s.validate().unwrap();
    let mut extra = template;
    extra.id = "excess".into();
    extra.light.as_mut().unwrap().enabled = false;
    s.objects.push(extra);
    let mut world = World::default();
    assert!(s.spawn(&mut world).is_err());
    assert!(world.is_empty());
}
