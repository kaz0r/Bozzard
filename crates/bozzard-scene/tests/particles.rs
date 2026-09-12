use bozzard_ecs::World;
use bozzard_scene::{Layer, ParticleEmitter, ParticleKind, Scene};
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Particles","views":{"3d":"camera","2d":"camera2"},"objects":[{"id":"camera","name":"Camera","transform":{"translation":[0,1,5],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},{"id":"camera2","name":"2D","transform":{"translation":[0,0,5],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}},{"id":"emitter","name":"Emitter","transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"particle_emitter":{}}]}"#).unwrap()
}
#[test]
fn particles_are_deterministic_bounded_transient_and_follow_wind() {
    for kind in ParticleKind::ALL {
        let mut scene = scene();
        scene.objects[2].particle_emitter = Some(ParticleEmitter::preset(kind));
        let mut a = World::default();
        let mut b = World::default();
        let mut ia = scene.spawn(&mut a).unwrap();
        let mut ib = scene.spawn(&mut b).unwrap();
        for _ in 0..240 {
            for (instance, world) in [(&mut ia, &a), (&mut ib, &b)] {
                instance.advance_display(1. / 60.).unwrap();
                instance.step_particles(world, 1. / 60.).unwrap();
            }
        }
        let particles = ia.view(&a, Layer::ThreeD, 1.).unwrap().particles;
        assert!(!particles.is_empty() && particles.len() <= 512);
        assert_eq!(particles, ib.view(&b, Layer::ThreeD, 1.).unwrap().particles);
        assert!(
            particles.iter().all(|p| p.position.is_finite()
                && p.size > 0.
                && p.opacity >= 0.
                && p.opacity <= 1.)
        );
        assert!(ia.view(&a, Layer::TwoD, 1.).unwrap().particles.is_empty());
        assert_eq!(ia.capture(&a).unwrap(), scene);
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
        let entity = ia.entity("emitter").unwrap();
        let mut emitter = *a.get::<ParticleEmitter>(entity).unwrap();
        emitter.enabled = false;
        a.insert(entity, emitter).unwrap();
        for _ in 0..2400 {
            ia.advance_display(1. / 60.).unwrap();
            ia.step_particles(&a, 1. / 60.).unwrap();
        }
        assert!(ia.view(&a, Layer::ThreeD, 1.).unwrap().particles.is_empty());
    }
    let mut scene = scene();
    let emitter = scene.objects[2].particle_emitter.as_mut().unwrap();
    emitter.wind = [3., 0., 0.];
    emitter.turbulence = 0.;
    emitter.spread = 0.;
    emitter.max_particles = 10;
    emitter.rate = 500.;
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    for _ in 0..120 {
        instance.advance_display(1. / 60.).unwrap();
        instance.step_particles(&world, 1. / 60.).unwrap();
    }
    let frame = instance.view(&world, Layer::ThreeD, 1.).unwrap().particles;
    assert_eq!(frame.len(), 10);
    assert!(frame.iter().all(|p| p.position.x > 4.));
    world
        .remove::<ParticleEmitter>(instance.entity("emitter").unwrap())
        .unwrap();
    instance.step_particles(&world, 1. / 60.).unwrap();
    assert!(
        instance
            .view(&world, Layer::ThreeD, 1.)
            .unwrap()
            .particles
            .is_empty()
    );
}
#[test]
fn emitter_rejects_invalid_parameters() {
    for kind in ParticleKind::ALL {
        ParticleEmitter::preset(kind).validate().unwrap();
    }
    let mut value = ParticleEmitter {
        opacity: f32::NAN,
        ..Default::default()
    };
    assert!(value.validate().is_err());
    value = ParticleEmitter::default();
    value.lifetime = 0.;
    assert!(value.validate().is_err());
    value = ParticleEmitter::default();
    value.rate = 501.;
    assert!(value.validate().is_err());
    value = ParticleEmitter::default();
    value.max_particles = 2049;
    assert!(value.validate().is_err());
    let mut document = scene();
    document.objects[2].particle_emitter = Some(value);
    assert!(document.validate().is_err());
}
