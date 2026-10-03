use anyhow::Result;
use bozzard_scene::middleware::{
    animation::{
        data::{Binding, Channel, Clip, Joint, Pose, Property, Rig},
        retarget::RetargetMap,
    },
    curve::{Curve, Interpolation, Key},
};
use glam::{Mat4, Quat, Vec3};

fn rig() -> Rig {
    Rig {
        nodes: (0..2)
            .map(|i| Joint {
                name: if i == 0 { "Root" } else { "Child" }.into(),
                parent: (i == 1).then_some(0),
                rest: Pose::default(),
            })
            .collect(),
        bindings: vec![Binding {
            node: 0,
            inverse_bind: Mat4::IDENTITY.to_cols_array(),
        }],
        clips: vec![Clip {
            name: "Motion".into(),
            duration: 1.,
            channels: vec![],
            events: vec![],
        }],
    }
}
fn channel(node: u32, property: Property, values: &[f32], interpolation: Interpolation) -> Channel {
    Channel {
        node,
        property,
        curves: values
            .iter()
            .map(|&value| Curve {
                interpolation,
                keys: vec![Key::new(0., value), Key::new(1., value)],
            })
            .collect(),
    }
}

#[test]
fn compaction_preserves_animated_quaternions_steps_and_cubic_tangents() -> Result<()> {
    let mut original = rig();
    let mut rotation = channel(
        0,
        Property::Rotation,
        &[0., 0., 0., 1.],
        Interpolation::Linear,
    );
    let end = Quat::from_rotation_y(2.).to_array();
    for (axis, curve) in rotation.curves.iter_mut().enumerate() {
        curve.keys[1].value = end[axis];
    }
    let mut cubic = channel(
        1,
        Property::Translation,
        &[0., 0., 0.],
        Interpolation::Cubic,
    );
    cubic.curves[0].keys[0].outgoing = 2.;
    cubic.curves[0].keys[1].incoming = -2.;
    original.clips[0].channels = vec![
        channel(0, Property::Translation, &[0., 0., 0.], Interpolation::Step),
        rotation,
        channel(0, Property::Scale, &[2., 2., 2.], Interpolation::Step),
        cubic,
        channel(
            1,
            Property::Rotation,
            &[0., 0., 0., -1.],
            Interpolation::Linear,
        ),
    ];
    original.validate()?;
    let mut compacted = original.clone();
    compacted.clips[0].compact(&compacted.nodes)?;
    compacted.validate()?;
    assert_eq!(compacted.clips[0].channels.len(), 4);
    assert_eq!(compacted.clips[0].channels[1].curves[0].keys.len(), 1);
    assert_eq!(compacted.clips[0].channels[0].curves[0].keys.len(), 2);
    assert_eq!(compacted.clips[0].channels[2].curves[0].keys.len(), 2);
    for frame in 0..=100 {
        let a = original.sample(0, frame as f32 / 100.)?;
        let b = compacted.sample(0, frame as f32 / 100.)?;
        for (a, b) in a.iter().zip(b) {
            assert!(
                Vec3::from_array(a.translation).distance(Vec3::from_array(b.translation)) < 1e-5
            );
            assert!(Vec3::from_array(a.scale).distance(Vec3::from_array(b.scale)) < 1e-5);
            assert!(
                Quat::from_array(a.rotation)
                    .dot(Quat::from_array(b.rotation))
                    .abs()
                    > 0.99999
            );
        }
    }
    Ok(())
}

#[test]
fn compaction_rejects_invalid_channels_before_mutating_the_clip() {
    let mut rig = rig();
    rig.clips[0].channels = vec![channel(
        0,
        Property::Translation,
        &[0., 0., 0.],
        Interpolation::Linear,
    )];
    rig.clips[0].channels[0].curves[1].keys.clear();
    let before = rig.clips[0].clone();
    assert!(rig.clips[0].compact(&rig.nodes).is_err());
    assert_eq!(rig.clips[0], before);
}

#[test]
fn retargeting_checks_limits_before_work_and_cancels_without_publishing() -> Result<()> {
    let mut source = rig();
    let target = rig();
    let map = RetargetMap::by_name(&source, &target);
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() >= 3
    };
    let before = target.clone();
    assert!(
        map.bake_clip_with(&source, &target, 0, "Reused".into(), 30., cancelled)
            .is_err()
    );
    assert_eq!(calls.get(), 3);
    assert_eq!(target, before);
    source.clips[0].duration = 100.;
    calls.set(0);
    assert!(
        map.bake_clip_with(&source, &target, 0, "Too large".into(), 120., cancelled)
            .is_err()
    );
    assert_eq!(calls.get(), 0);
    Ok(())
}
