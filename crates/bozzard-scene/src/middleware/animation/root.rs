//! Root extraction and piecewise alignment. Split ticks at warp boundaries before continuing.
use super::{Animator, Player, Repeat, RootMotion, data::Pose, motion::Mix, warp};
use crate::{Transform, middleware::curve::Playhead};
use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};

pub(super) struct Step<'a> {
    pub before: Playhead,
    pub after: Playhead,
    pub mix: Mix,
    pub local: Transform,
    pub model: Mat4,
    pub objects: &'a crate::transforms::Matrices<'a>,
}
struct Path<'a> {
    animator: &'a Animator,
    root: &'a RootMotion,
    mix: Mix,
    repeat: Repeat,
    basis: Mat4,
}
impl Path<'_> {
    fn sample(&self, elapsed: f64) -> Result<(Vec3, f32)> {
        let phase = Playhead {
            elapsed,
            ..Default::default()
        }
        .position(1., self.repeat);
        let mut position = Vec3::ZERO;
        let mut yaw = 0.;
        for contribution in self.mix.iter() {
            let clip = contribution.clip;
            let duration = self.animator.rig.clips[clip].duration;
            let sample = |phase| {
                self.animator
                    .rig
                    .sample_joint(clip, phase * duration, self.root.node)
            };
            let yaw_at = |phase| {
                self.animator.rig.root_yaw_in_basis(
                    clip,
                    phase * duration,
                    self.root.node,
                    self.basis.to_scale_rotation_translation().1,
                )
            };
            let mut p = Vec3::from_array(sample(phase)?.translation);
            let mut y = yaw_at(phase)?;
            if self.repeat == Repeat::Loop {
                let cycles = elapsed.floor() as f32;
                p += (Vec3::from_array(sample(1.)?.translation)
                    - Vec3::from_array(sample(0.)?.translation))
                    * cycles;
                y += (yaw_at(1.)? - yaw_at(0.)?) * cycles;
            }
            position += self.basis.transform_vector3(p) * contribution.weight;
            yaw += y * contribution.weight;
        }
        ensure!(
            position.is_finite() && yaw.is_finite(),
            "root motion overflow"
        );
        Ok((position, yaw))
    }
    fn delta(&self, before: Vec3, after: Vec3) -> Vec3 {
        let mut delta = after - before;
        for axis in 0..3 {
            if !self.root.translation[axis] {
                delta[axis] = 0.;
            }
        }
        delta
    }
}
pub(super) fn apply(
    animator: &Animator,
    player: &mut Player,
    step: Step<'_>,
) -> Result<Option<Transform>> {
    let Some(root) = &animator.root_motion else {
        return Ok(None);
    };
    let state = &animator.states[player.state];
    let basis = animator.rig.nodes[root.node]
        .parent
        .map_or(Mat4::IDENTITY, |p| player.cache.rest_globals[p as usize]);
    let path = Path {
        animator,
        root,
        mix: step.mix,
        repeat: state.repeat,
        basis,
    };
    let mut transform = step.local;
    if step.before.elapsed != step.after.elapsed {
        let (delta, yaw) = displacement(&path, player, &state.name, &step)?;
        let parent = step.model * step.local.matrix().inverse();
        transform.translation = (Vec3::from_array(transform.translation)
            + parent.inverse().transform_vector3(delta))
        .to_array();
        if yaw != 0. {
            let parent_rotation = parent.to_scale_rotation_translation().1;
            let rotation = step.local.matrix().to_scale_rotation_translation().1;
            let next = parent_rotation.conjugate()
                * Quat::from_rotation_y(yaw)
                * parent_rotation
                * rotation;
            let (y, x, z) = next.to_euler(EulerRot::YXZ);
            transform.rotation_degrees = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
        }
        transform.validate()?;
    }
    strip(
        &mut std::sync::Arc::make_mut(&mut player.pose)[root.node],
        animator.rig.nodes[root.node].rest,
        root,
        basis,
    );
    Ok((transform != step.local).then_some(transform))
}
fn displacement(
    path: &Path<'_>,
    player: &Player,
    state: &str,
    step: &Step<'_>,
) -> Result<(Vec3, f32)> {
    let mut time = step.before.elapsed;
    let end = step.after.elapsed;
    let mut position = step.model.w_axis.truncate();
    let origin = position;
    let mut orientation = step.model;
    let initial_yaw = orientation
        .to_scale_rotation_translation()
        .1
        .to_euler(EulerRot::YXZ)
        .0;
    let mut yaw = initial_yaw;
    let mut sampled = path.sample(time)?;
    let reference_yaw = if path.root.yaw {
        path.sample(0.)?.1
    } else {
        0.
    };
    let cycle_yaw = if path.root.yaw && path.repeat == Repeat::Loop {
        path.sample(1.)?.1 - reference_yaw
    } else {
        0.
    };
    let mut segments = 0;
    while time < end {
        segments += 1;
        ensure!(segments <= 4096, "root motion tick exceeds segment budget");
        let boundary = if path.root.yaw && path.repeat == Repeat::Loop {
            (time.floor() + 1.).min(end)
        } else {
            end
        };
        let next = path
            .animator
            .warps
            .iter()
            .filter(|w| w.state == state)
            .flat_map(|w| [f64::from(w.start), f64::from(w.end)])
            .filter(|boundary| *boundary > time && *boundary < end)
            .fold(boundary, f64::min);
        let after = path.sample(next)?;
        // Positions are already in clip space. Remove yaw consumed earlier in this cycle
        // before rotating their deltas by the actor; otherwise smaller ticks bend the path twice.
        let consumed_yaw = if path.root.yaw {
            sampled.1 - reference_yaw - time.floor() as f32 * cycle_yaw
        } else {
            0.
        };
        let motion_basis = orientation * Mat4::from_quat(Quat::from_rotation_y(-consumed_yaw));
        let mut delta = motion_basis.transform_vector3(path.delta(sampled.0, after.0));
        let mut turn = if path.root.yaw {
            after.1 - sampled.1
        } else {
            0.
        };
        if let Some(window) = path
            .animator
            .warps
            .iter()
            .find(|w| w.state == state && time >= f64::from(w.start) && time < f64::from(w.end))
            && let Some(goal) = window.goal(&player.warp_targets, step.objects)
        {
            let finish = path.sample(f64::from(window.end))?;
            let correction = warp::correction(
                window,
                goal,
                warp::WindowStep {
                    before: time as f32,
                    after: next as f32,
                    position,
                    yaw,
                    remaining: motion_basis.transform_vector3(path.delta(sampled.0, finish.0)),
                    remaining_yaw: if path.root.yaw {
                        finish.1 - sampled.1
                    } else {
                        0.
                    },
                },
            );
            delta += correction.0;
            turn += correction.1;
        }
        position += delta;
        yaw += turn;
        orientation = Mat4::from_quat(Quat::from_rotation_y(turn)) * orientation;
        sampled = after;
        time = next;
    }
    Ok((position - origin, yaw - initial_yaw))
}
fn strip(pose: &mut Pose, rest: Pose, root: &RootMotion, basis: Mat4) {
    let mut delta = basis
        .transform_vector3(Vec3::from_array(pose.translation) - Vec3::from_array(rest.translation));
    for axis in 0..3 {
        if root.translation[axis] {
            delta[axis] = 0.;
        }
    }
    pose.translation =
        (Vec3::from_array(rest.translation) + basis.inverse().transform_vector3(delta)).to_array();
    if root.yaw {
        let parent = basis.to_scale_rotation_translation().1;
        let global = parent * Quat::from_array(pose.rotation);
        let yaw = global.to_euler(EulerRot::YXZ).0;
        let rest_yaw = (parent * Quat::from_array(rest.rotation))
            .to_euler(EulerRot::YXZ)
            .0;
        pose.rotation = (parent.conjugate() * Quat::from_rotation_y(rest_yaw - yaw) * global)
            .normalize()
            .to_array();
    }
}
