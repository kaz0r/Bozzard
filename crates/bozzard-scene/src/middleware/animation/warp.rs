//! Root-motion alignment over explicit animation windows, including ticks crossing a boundary.
use super::{Animator, Repeat};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarpTarget {
    Point {
        position: [f32; 3],
        yaw_degrees: f32,
    },
    Object {
        object: String,
        offset: [f32; 3],
        yaw_degrees: f32,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionWarp {
    pub name: String,
    pub state: String,
    pub start: f32,
    pub end: f32,
    pub translation: [bool; 3],
    pub yaw: bool,
    pub target: WarpTarget,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarpGoal {
    pub position: [f32; 3],
    pub yaw_degrees: f32,
}
impl WarpGoal {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.position.iter().all(|v| v.is_finite()) && self.yaw_degrees.is_finite(),
            "motion-warp target must be finite"
        );
        Ok(())
    }
}
impl MotionWarp {
    pub(crate) fn validate(&self, animator: &Animator) -> Result<()> {
        ensure!(super::valid_name(&self.name), "motion warp needs a name");
        ensure!(
            animator.root_motion.is_some(),
            "motion warping requires root motion"
        );
        ensure!(
            animator
                .states
                .iter()
                .any(|s| s.name == self.state && s.repeat == Repeat::Once),
            "motion-warp state must exist and play once"
        );
        ensure!(
            self.start.is_finite()
                && self.end.is_finite()
                && self.start >= 0.
                && self.end <= 1.
                && self.start < self.end,
            "motion-warp window must satisfy 0 ≤ start < end ≤ 1"
        );
        match &self.target {
            WarpTarget::Point {
                position,
                yaw_degrees,
            } => WarpGoal {
                position: *position,
                yaw_degrees: *yaw_degrees,
            }
            .validate()?,
            WarpTarget::Object {
                object,
                offset,
                yaw_degrees,
            } => {
                ensure!(!object.is_empty(), "motion warp needs a target object");
                WarpGoal {
                    position: *offset,
                    yaw_degrees: *yaw_degrees,
                }
                .validate()?;
            }
        }
        Ok(())
    }
    pub(crate) fn goal(
        &self,
        overrides: &BTreeMap<String, WarpGoal>,
        objects: &crate::transforms::Matrices<'_>,
    ) -> Option<WarpGoal> {
        if let Some(goal) = overrides.get(&self.name) {
            return Some(*goal);
        }
        match &self.target {
            WarpTarget::Point {
                position,
                yaw_degrees,
            } => Some(WarpGoal {
                position: *position,
                yaw_degrees: *yaw_degrees,
            }),
            WarpTarget::Object {
                object,
                offset,
                yaw_degrees,
            } => objects.get(object).map(|matrix| WarpGoal {
                position: matrix
                    .transform_point3(Vec3::from_array(*offset))
                    .to_array(),
                yaw_degrees: matrix
                    .to_scale_rotation_translation()
                    .1
                    .to_euler(glam::EulerRot::YXZ)
                    .0
                    .to_degrees()
                    + yaw_degrees,
            }),
        }
    }
}
pub(crate) struct WindowStep {
    pub before: f32,
    pub after: f32,
    /// Actor position/yaw at the start of the overlapping portion of this window.
    pub position: Vec3,
    pub yaw: f32,
    /// Unwarped displacement and yaw still remaining until the end of the window.
    pub remaining: Vec3,
    pub remaining_yaw: f32,
}
pub(crate) fn correction(window: &MotionWarp, goal: WarpGoal, step: WindowStep) -> (Vec3, f32) {
    let before = step.before.max(window.start);
    let after = step.after.min(window.end);
    if after <= before || before >= window.end {
        return (Vec3::ZERO, 0.);
    }
    let fraction = ((after - before) / (window.end - before)).clamp(0., 1.);
    let mut offset = Vec3::from_array(goal.position) - (step.position + step.remaining);
    for axis in 0..3 {
        if !window.translation[axis] {
            offset[axis] = 0.;
        }
    }
    let yaw = if window.yaw {
        shortest_angle(goal.yaw_degrees.to_radians() - step.yaw - step.remaining_yaw) * fraction
    } else {
        0.
    };
    (offset * fraction, yaw)
}
pub(crate) fn shortest_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
