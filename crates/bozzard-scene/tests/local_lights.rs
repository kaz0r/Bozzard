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
    assert!(!light.light.shadows); // Old scene files keep the previous appearance.
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
fn directional_roundtrip_parent_rotation_and_runtime_capture() {
    let mut scene = scene();
    scene.objects[2].light.as_mut().unwrap().kind = LightKind::Directional;
    let scene = Scene::from_json(&scene.to_json().unwrap()).unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let light = instance.view(&world, Layer::ThreeD, 1.).unwrap().lights[0];
    assert_eq!(light.light.kind, LightKind::Directional);
    assert!((Vec3::from(light.direction) - Vec3::NEG_X).length() < 1e-5);
    let entity = instance.entity("light").unwrap();
    world.get_mut::<Light>(entity).unwrap().intensity = 7.;
    assert_eq!(
        instance.capture(&world).unwrap().objects[2]
            .light
            .unwrap()
            .intensity,
        7.
    );
    world.get_mut::<Light>(entity).unwrap().intensity = f32::INFINITY;
    assert!(instance.view(&world, Layer::ThreeD, 1.).is_err());
}

#[test]
fn rejects_invalid_light_data_and_excess_without_spawning() {
    for light in [
        Light {
            shadow_bias: f32::NAN,
            ..Default::default()
        },
        Light {
            shadow_normal_bias: -0.1,
            ..Default::default()
        },
        Light {
            shadow_bias: 1.01,
            ..Default::default()
        },
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

#[test]
fn independent_shadow_budgets_count_disabled_lights_and_validate_live_components() {
    use bozzard_scene::{MAX_SHADOWED_POINT_LIGHTS, MAX_SHADOWED_SPOT_LIGHTS};
    for (kind, limit, other_kind, other_limit) in [
        (
            LightKind::Spot,
            MAX_SHADOWED_SPOT_LIGHTS,
            LightKind::Point,
            MAX_SHADOWED_POINT_LIGHTS,
        ),
        (
            LightKind::Point,
            MAX_SHADOWED_POINT_LIGHTS,
            LightKind::Spot,
            MAX_SHADOWED_SPOT_LIGHTS,
        ),
    ] {
        shadow_budget(kind, limit, other_kind, other_limit);
    }
}
fn shadow_budget(kind: LightKind, limit: usize, other_kind: LightKind, other_limit: usize) {
    let mut s = scene();
    let light = s.objects[2].light.as_mut().unwrap();
    light.kind = kind;
    light.shadows = true;
    light.shadow_bias = 0.02;
    light.shadow_normal_bias = 0.04;
    let template = s.objects[2].clone();
    for i in 1..limit {
        let mut o = template.clone();
        o.id = format!("shadow-{i}");
        s.objects.push(o);
    }
    for i in 0..other_limit {
        let mut point = template.clone();
        point.id = format!("other-{i}");
        point.light.as_mut().unwrap().kind = other_kind;
        s.objects.push(point);
    }
    // Switching a previously shadowed light to directional retains its authored
    // settings without consuming either local shadow budget.
    for i in 0..9 {
        let mut directional = template.clone();
        directional.id = format!("directional-{i}");
        directional.light.as_mut().unwrap().kind = LightKind::Directional;
        s.objects.push(directional);
    }
    assert_eq!(s, Scene::from_json(&s.to_json().unwrap()).unwrap());
    let mut world = World::default();
    let instance = s.spawn(&mut world).unwrap();
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(
        view.lights
            .iter()
            .filter(|l| l.light.requests_shadow_map())
            .count(),
        limit + other_limit
    );
    assert_eq!(view.lights[0].light.shadow_bias, 0.02);
    let entity = instance.entity("other-0").unwrap();
    let point = world.get_mut::<Light>(entity).unwrap();
    point.kind = kind;
    point.enabled = false;
    assert!(instance.view(&world, Layer::ThreeD, 1.).is_err());
    assert!(instance.capture(&world).is_err());
    let point = s
        .objects
        .iter_mut()
        .find(|o| o.id == "other-0")
        .unwrap()
        .light
        .as_mut()
        .unwrap();
    point.kind = kind;
    point.enabled = false;
    let mut empty = World::default();
    assert!(s.spawn(&mut empty).is_err());
    assert!(empty.is_empty());
}
