//! Reproducible CPU animation workload; run with --release, then compare on the same machine.
use anyhow::Result;
use bozzard_ecs::World;
use bozzard_scene::{
    Scene,
    middleware::{
        animation::{
            AnimationLayer, Animator, BlendPoint, BlendSample, BoneMask, IkConstraint, IkTarget,
            LayerBlend, Motion, StateDefinition,
            data::{Binding, Channel, Clip, Joint, Pose, Property, Rig},
        },
        curve::{Curve, Repeat},
        registry,
    },
};
use glam::{Mat4, Vec3};
use std::{hint::black_box, sync::Arc, time::Instant};

fn animator(joints: usize) -> Animator {
    let nodes: Vec<_> = (0..joints)
        .map(|index| Joint {
            name: format!("Joint {index}"),
            parent: (index > 0).then(|| (index - 1) as u32),
            rest: Pose {
                translation: [0., 0.025, 0.],
                ..Default::default()
            },
        })
        .collect();
    let bindings = (0..joints)
        .map(|index| Binding {
            node: index as u32,
            inverse_bind: Mat4::from_translation(Vec3::new(0., -0.025 * (index + 1) as f32, 0.))
                .to_cols_array(),
        })
        .collect();
    let clips = [0.02, 0.05]
        .into_iter()
        .enumerate()
        .map(|(index, distance)| Clip {
            name: format!("Clip {index}"),
            duration: 1.,
            channels: (0..joints)
                .map(|node| Channel {
                    node: node as u32,
                    property: Property::Translation,
                    curves: vec![
                        Curve::linear(0., distance, 1.),
                        Curve::constant(0.025),
                        Curve::constant(0.),
                    ],
                })
                .collect(),
            events: vec![],
        })
        .collect();
    let mut animator = Animator::from_rig(
        String::new(),
        Arc::new(Rig {
            nodes,
            bindings,
            clips,
        }),
    );
    animator.parameters.insert("Speed".into(), 0.4);
    animator.states = Arc::new(vec![StateDefinition {
        name: "Locomotion".into(),
        repeat: Repeat::Loop,
        motion: Motion::Blend1d {
            parameter: "Speed".into(),
            samples: vec![
                BlendSample {
                    threshold: 0.,
                    clip: 0,
                },
                BlendSample {
                    threshold: 1.,
                    clip: 1,
                },
            ],
        },
    }]);
    animator.initial = "Locomotion".into();
    animator
}

fn advanced(template: &mut Animator) {
    template
        .parameters
        .extend([("X".into(), 0.25), ("Y".into(), 0.25)]);
    Arc::make_mut(&mut template.states)[0].motion = Motion::Blend2d {
        parameters: ["X".into(), "Y".into()],
        samples: vec![
            BlendPoint {
                position: [0., 0.],
                clip: 0,
            },
            BlendPoint {
                position: [1., 0.],
                clip: 1,
            },
            BlendPoint {
                position: [0., 1.],
                clip: 0,
            },
        ],
    };
    template.layers = Arc::new(vec![AnimationLayer {
        name: "Action".into(),
        motion: Motion::Clip { clip: 1 },
        mask: BoneMask {
            root: Some(32),
            ..Default::default()
        },
        blend: LayerBlend::Additive,
        weight: 0.4,
        ..Default::default()
    }]);
    template.ik = Arc::new(
        [54, 60]
            .map(|root| IkConstraint {
                name: format!("Limb {root}"),
                root,
                middle: root + 1,
                tip: root + 2,
                target: IkTarget::Point {
                    position: [0.05, 1.5, 0.],
                },
                smoothing: 0.,
                ..Default::default()
            })
            .to_vec(),
    );
}
fn measure(actors: usize, paused: bool, extended: bool) -> Result<serde_json::Value> {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"benchmark","views":{},"objects":[]}"#)?;
    let mut template = animator(64);
    template.autoplay = !paused;
    if extended {
        advanced(&mut template);
    }
    for index in 0..actors {
        let mut object = bozzard_scene::Object {
            id: format!("actor-{index}"),
            name: "Actor".into(),
            ..Default::default()
        };
        registry::set(&mut object, &template)?;
        scene.objects.push(object);
    }
    let mut world = World::default();
    let instance = scene.spawn(&mut world)?;
    for _ in 0..120 {
        instance.step_animations(&mut world, 1. / 60.)?;
    }
    let mut timings = Vec::with_capacity(600);
    for _ in 0..600 {
        let started = Instant::now();
        instance.step_animations(black_box(&mut world), 1. / 60.)?;
        timings.push(started.elapsed().as_secs_f64() * 1_000_000.);
    }
    timings.sort_by(f64::total_cmp);
    Ok(serde_json::json!({
        "actors": actors, "joints_per_actor": 64, "paused": paused,
        "workload": if extended { "2d_layer_2ik" } else { "1d_blend" },
        "median_us": timings[300], "p95_us": timings[570], "samples": timings.len()
    }))
}

fn main() -> Result<()> {
    let mut measurements = Vec::new();
    for actors in [1, 32, 128] {
        for paused in [false, true] {
            measurements.push(measure(actors, paused, false)?);
        }
        measurements.push(measure(actors, false, true)?);
    }
    println!("{}", serde_json::to_string_pretty(&measurements)?);
    Ok(())
}
