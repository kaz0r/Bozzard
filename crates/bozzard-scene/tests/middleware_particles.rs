use bozzard_ecs::World;
use bozzard_scene::{
    Camera, Layer, Object, ParticleEmitter, Scene, Transform,
    middleware::{
        curve::Curve,
        particle::{Curves, Modules},
        registry,
    },
};
use std::sync::Arc;
fn scene() -> Scene {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"Particles","views":{},"assets":{},"objects":[]}"#)
            .unwrap();
    scene.objects.push(Object {
        id: "camera".into(),
        name: "Camera".into(),
        camera: Some(Camera::Orthographic {
            vertical_size: 10.,
            near: 0.1,
            far: 100.,
        }),
        transform: Transform {
            translation: [0., 0., 10.],
            ..Default::default()
        },
        ..Default::default()
    });
    scene.views.insert(Layer::ThreeD, "camera".into());
    scene.objects.push(Object {
        id: "effect".into(),
        name: "Effect".into(),
        particle_emitter: Some(ParticleEmitter {
            rate: 60.,
            max_particles: 1,
            lifetime: 10.,
            ..Default::default()
        }),
        ..Default::default()
    });
    scene
}
#[test]
fn lifetime_curves_modify_spawned_particles_and_gpu_mode_preserves_lifecycle() {
    let base = scene();
    let mut authored = base.clone();
    let mut curves = Curves {
        size: Curve::constant(0.5),
        opacity: Curve::constant(0.5),
        speed: Curve::constant(0.),
        ..Default::default()
    };
    curves.color[0] = Curve::constant(0.);
    registry::set(
        &mut authored.objects[1],
        &Modules {
            curves: Arc::new(curves),
            ..Default::default()
        },
    )
    .unwrap();
    let mut a = World::default();
    let mut b = World::default();
    let mut reference = base.spawn(&mut a).unwrap();
    let mut modified = authored.spawn(&mut b).unwrap();
    for (instance, world) in [(&mut reference, &mut a), (&mut modified, &mut b)] {
        instance.advance_display(1. / 60.).unwrap();
        instance.step_particles(world, 1. / 60.).unwrap();
    }
    let first = modified.view(&b, Layer::ThreeD, 1.).unwrap().particles[0];
    for (instance, world) in [(&mut reference, &mut a), (&mut modified, &mut b)] {
        instance.advance_display(0.5).unwrap();
        instance.step_particles(world, 0.5).unwrap();
    }
    let original = reference.view(&a, Layer::ThreeD, 1.).unwrap().particles[0];
    let p = modified.view(&b, Layer::ThreeD, 1.).unwrap().particles[0];
    assert_eq!(p.position, first.position);
    assert_ne!(original.position, p.position);
    assert_eq!(p.size, original.size * 0.5);
    assert_eq!(p.opacity, original.opacity * 0.5);
    assert_eq!(p.color[0], 0.);
    modified.set_gpu_particles(true);
    modified.step_particles(&b, 1. / 60.).unwrap();
    modified.advance_display(0.5).unwrap();
    modified.step_particles(&b, 0.5).unwrap();
    let gpu = modified.view(&b, Layer::ThreeD, 1.).unwrap().particles[0];
    assert_eq!(gpu.simulation.unwrap().age, 0.5);
    assert_eq!(gpu.simulation.unwrap().speed, 0.);
    modified.restart_runtime_scene(&mut b).unwrap();
    assert!(modified.gpu_particles_enabled());
    assert!(
        modified
            .view(&b, Layer::ThreeD, 1.)
            .unwrap()
            .particles
            .is_empty()
    );
    let mut invalid = Modules::default();
    Arc::make_mut(&mut invalid.curves).size = Curve::linear(1., 0., 2.);
    assert!(registry::set(&mut authored.objects[1], &invalid).is_err());
}
