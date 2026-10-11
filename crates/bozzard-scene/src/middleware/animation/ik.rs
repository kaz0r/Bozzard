//! Analytic two-bone IK and grounded feet. Solver data stays independent of the renderer.
use super::data::{Pose, Rig};
use crate::CollisionSnapshot;
use anyhow::{Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IkTarget {
    Point {
        position: [f32; 3],
    },
    Object {
        object: String,
        offset: [f32; 3],
    },
    Ground {
        sole_height: f32,
        ray_up: f32,
        ray_down: f32,
        layers: u32,
        /// Lifted feet release their contact instead of being glued to the ground.
        release_height: f32,
        plant: bool,
        align_normal: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IkConstraint {
    pub name: String,
    pub root: usize,
    pub middle: usize,
    pub tip: usize,
    /// Model-space bend direction. Keeps knees/elbows stable even at a straight rest pose.
    pub pole: [f32; 3],
    pub target: IkTarget,
    pub weight: f32,
    pub weight_parameter: Option<String>,
    pub smoothing: f32,
}
/// Bounded body-height adjustment lets both legs reach steps below the animated pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FootPlacement {
    pub pelvis: usize,
    pub max_up: f32,
    pub max_down: f32,
    pub smoothing: f32,
}
impl Default for FootPlacement {
    fn default() -> Self {
        Self {
            pelvis: 0,
            max_up: 0.25,
            max_down: 0.4,
            smoothing: 14.,
        }
    }
}
impl FootPlacement {
    pub(crate) fn validate(&self, rig: &Rig) -> Result<()> {
        ensure!(
            self.pelvis < rig.nodes.len(),
            "foot placement pelvis is missing"
        );
        ensure!(
            [self.max_up, self.max_down]
                .iter()
                .all(|v| v.is_finite() && (0.0..=2.).contains(v))
                && self.smoothing.is_finite()
                && (0.0..=100.).contains(&self.smoothing),
            "invalid foot placement limits"
        );
        Ok(())
    }
}
impl Default for IkConstraint {
    fn default() -> Self {
        Self {
            name: "Foot placement".into(),
            root: 0,
            middle: 1,
            tip: 2,
            pole: [0., 0., -1.],
            target: IkTarget::Ground {
                sole_height: 0.1,
                ray_up: 0.6,
                ray_down: 0.8,
                layers: u32::MAX,
                release_height: 0.2,
                plant: true,
                align_normal: true,
            },
            weight: 1.,
            weight_parameter: None,
            smoothing: 20.,
        }
    }
}
impl IkConstraint {
    pub(crate) fn validate(&self, rig: &Rig, parameters: &BTreeMap<String, f32>) -> Result<()> {
        ensure!(super::valid_name(&self.name), "IK constraint needs a name");
        ensure!(
            self.root < rig.nodes.len()
                && self.middle < rig.nodes.len()
                && self.tip < rig.nodes.len()
                && rig.nodes[self.middle].parent == Some(self.root as u32)
                && rig.nodes[self.tip].parent == Some(self.middle as u32),
            "IK needs a direct three-bone chain (upper limb, lower limb, end)"
        );
        let pole = Vec3::from_array(self.pole);
        ensure!(
            pole.is_finite() && pole.length_squared() > 1e-8 && pole.length_squared().is_finite(),
            "IK bend direction must be finite and nonzero"
        );
        ensure!(
            self.weight.is_finite()
                && (0.0..=1.).contains(&self.weight)
                && self.smoothing.is_finite()
                && (0.0..=100.).contains(&self.smoothing),
            "IK weight must be 0–1 and smoothing 0–100"
        );
        ensure!(
            self.weight_parameter
                .as_ref()
                .is_none_or(|p| parameters.contains_key(p)),
            "IK weight parameter is missing"
        );
        match &self.target {
            IkTarget::Point { position } => ensure!(
                position.iter().all(|n| n.is_finite()),
                "IK point must be finite"
            ),
            IkTarget::Object { object, offset } => ensure!(
                !object.is_empty() && offset.iter().all(|n| n.is_finite()),
                "IK object needs a target and finite offset"
            ),
            IkTarget::Ground {
                sole_height,
                ray_up,
                ray_down,
                release_height,
                ..
            } => {
                ensure!(
                    [*sole_height, *ray_up, *ray_down, *release_height]
                        .iter()
                        .all(|n| n.is_finite() && *n >= 0. && *n <= 10.)
                        && *release_height > 0.
                        && *ray_up + *ray_down > 0.,
                    "foot probe distances must be 0–10 and release height must be positive"
                );
            }
        }
        let mut globals = Vec::new();
        rig.globals_into(&rig.rest_pose(), &mut globals)?;
        ensure!(
            globals[self.root]
                .w_axis
                .truncate()
                .distance_squared(globals[self.middle].w_axis.truncate())
                > 1e-8
                && globals[self.middle]
                    .w_axis
                    .truncate()
                    .distance_squared(globals[self.tip].w_axis.truncate())
                    > 1e-8,
            "IK chain cannot have zero-length limbs"
        );
        Ok(())
    }
    pub(crate) fn weight(&self, parameters: &BTreeMap<String, f32>) -> f32 {
        self.weight
            * self
                .weight_parameter
                .as_ref()
                .map_or(1., |p| parameters[p].clamp(0., 1.))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IkState {
    pub target: Option<[f32; 3]>,
    pub normal: [f32; 3],
    pub support: Option<String>,
    pub support_local: [f32; 3],
    pub support_normal: [f32; 3],
    pub planted: bool,
}
pub(crate) struct Context<'a> {
    pub owner: &'a str,
    pub model: Mat4,
    pub objects: &'a crate::transforms::Matrices<'a>,
    pub collisions: &'a CollisionSnapshot,
    pub rest_globals: &'a [Mat4],
    pub dt: f32,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Goal {
    pub position: Vec3,
    pub normal: Option<Vec3>,
    pub weight: f32,
}
fn ground_goal(
    constraint: &IkConstraint,
    state: &mut IkState,
    foot: Vec3,
    weight: f32,
    context: &Context<'_>,
    budget: &mut usize,
) -> Result<Option<Goal>> {
    let IkTarget::Ground {
        sole_height,
        ray_up,
        ray_down,
        layers,
        release_height,
        plant,
        align_normal,
    } = constraint.target
    else {
        unreachable!()
    };
    let lift = (context.model.inverse().transform_point3(foot).y
        - context.rest_globals[constraint.tip].w_axis.y)
        .max(0.);
    let weight = weight * (1. - lift / release_height).clamp(0., 1.);
    if weight <= 0. {
        state.planted = false;
        state.target = None;
        return Ok(None);
    }
    let origin = foot + Vec3::Y * ray_up;
    let Some(hit) = context.collisions.raycast_budget(
        origin,
        -Vec3::Y,
        ray_up + ray_down,
        Some(context.owner),
        layers,
        budget,
    )?
    else {
        *state = IkState::default();
        return Ok(None);
    };
    // Ignore near-vertical walls: a sole needs an upward-facing support surface.
    if hit.normal.y < 0.2 {
        *state = IkState::default();
        return Ok(None);
    }
    let mut target = hit.position;
    let mut normal = hit.normal;
    let should_plant = plant && weight > 0.8;
    if should_plant && state.planted {
        if let Some(matrix) = state
            .support
            .as_ref()
            .and_then(|id| context.objects.get(id))
        {
            let locked = matrix.transform_point3(Vec3::from_array(state.support_local));
            // Teleports/long strides release a stale plant instead of stretching a leg indefinitely.
            if locked.distance_squared(target) < (ray_up + ray_down).powi(2) {
                target = locked;
                normal = matrix
                    .inverse()
                    .transpose()
                    .transform_vector3(Vec3::from_array(state.support_normal))
                    .normalize_or_zero();
            } else {
                state.planted = false;
            }
        } else {
            state.planted = false;
        }
    }
    if should_plant
        && !state.planted
        && let Some(matrix) = context.objects.get(&hit.object)
    {
        state.support = Some(hit.object.clone());
        state.support_local = matrix.inverse().transform_point3(target).to_array();
        state.support_normal = matrix
            .transpose()
            .transform_vector3(normal)
            .normalize_or_zero()
            .to_array();
    }
    state.planted = should_plant;
    target += normal * sole_height;
    Ok(Some(Goal {
        position: target,
        normal: align_normal.then_some(normal),
        weight,
    }))
}
fn resolve_goal(
    constraint: &IkConstraint,
    state: &mut IkState,
    globals: &[Mat4],
    weight: f32,
    context: &Context<'_>,
    budget: &mut usize,
) -> Result<Option<Goal>> {
    match &constraint.target {
        IkTarget::Point { position } => Ok(Some(Goal {
            position: Vec3::from_array(*position),
            normal: None,
            weight,
        })),
        IkTarget::Object { object, offset } => Ok(context.objects.get(object).map(|matrix| Goal {
            position: matrix.transform_point3(Vec3::from_array(*offset)),
            normal: None,
            weight,
        })),
        IkTarget::Ground { .. } => ground_goal(
            constraint,
            state,
            context
                .model
                .transform_point3(globals[constraint.tip].w_axis.truncate()),
            weight,
            context,
            budget,
        ),
    }
}
pub(super) fn prepare(
    constraint: &IkConstraint,
    state: &mut IkState,
    globals: &[Mat4],
    weight: f32,
    context: &Context<'_>,
    budget: &mut usize,
) -> Result<Option<Goal>> {
    if weight <= 0. {
        *state = IkState::default();
        return Ok(None);
    }
    let Some(goal) = resolve_goal(constraint, state, globals, weight, context, budget)? else {
        return Ok(None);
    };
    let target = state.target.map_or(goal.position, |previous| {
        if constraint.smoothing == 0. {
            goal.position
        } else {
            Vec3::from_array(previous).lerp(
                goal.position,
                1. - (-constraint.smoothing * context.dt).exp(),
            )
        }
    });
    state.target = Some(target.to_array());
    let normal = goal.normal.map(|normal| {
        let previous = Vec3::from_array(state.normal);
        let next = if previous.length_squared() > 0. && constraint.smoothing > 0. {
            previous
                .lerp(normal, 1. - (-constraint.smoothing * context.dt).exp())
                .normalize_or_zero()
        } else {
            normal
        };
        state.normal = next.to_array();
        next
    });
    Ok(Some(Goal {
        position: target,
        normal,
        weight: goal.weight,
    }))
}
pub(super) fn apply(
    rig: &Rig,
    pose: &mut [Pose],
    globals: &mut Vec<Mat4>,
    constraint: &IkConstraint,
    goal: Goal,
    context: &Context<'_>,
) -> Result<()> {
    rig.globals_into(pose, globals)?;
    solve(
        rig,
        pose,
        globals,
        constraint,
        context.model.inverse().transform_point3(goal.position),
        goal.weight,
    )?;
    if let Some(normal) = goal.normal {
        let normal = context
            .model
            .transpose()
            .transform_vector3(normal)
            .normalize_or_zero();
        rig.globals_into(pose, globals)?;
        let current = globals[constraint.tip].to_scale_rotation_translation().1;
        let rest = context.rest_globals[constraint.tip]
            .to_scale_rotation_translation()
            .1;
        let up = current * (rest.conjugate() * Vec3::Y);
        rotate(
            rig,
            pose,
            globals,
            constraint.tip,
            Quat::from_rotation_arc(up, normal),
            goal.weight,
        );
    }
    Ok(())
}

pub(super) fn adjust_pelvis(
    settings: &FootPlacement,
    rig: &Rig,
    pose: &mut [Pose],
    globals: &[Mat4],
    goals: &[(usize, Goal)],
    offset: &mut f32,
    context: &Context<'_>,
) {
    let desired = goals
        .iter()
        .map(|(tip, goal)| {
            (goal.position.y
                - context
                    .model
                    .transform_point3(globals[*tip].w_axis.truncate())
                    .y)
                * goal.weight
        })
        .reduce(f32::min)
        .unwrap_or(0.)
        .clamp(-settings.max_down, settings.max_up);
    let factor = if settings.smoothing == 0. || context.dt == 0. {
        1.
    } else {
        1. - (-settings.smoothing * context.dt).exp()
    };
    *offset += (desired - *offset) * factor;
    let parent = rig.nodes[settings.pelvis]
        .parent
        .map_or(context.model, |p| context.model * globals[p as usize]);
    pose[settings.pelvis].translation = (Vec3::from_array(pose[settings.pelvis].translation)
        + parent.inverse().transform_vector3(Vec3::Y * *offset))
    .to_array();
}

fn solve(
    rig: &Rig,
    pose: &mut [Pose],
    globals: &mut Vec<Mat4>,
    chain: &IkConstraint,
    target: Vec3,
    weight: f32,
) -> Result<()> {
    let a = globals[chain.root].w_axis.truncate();
    let b = globals[chain.middle].w_axis.truncate();
    let c = globals[chain.tip].w_axis.truncate();
    let (upper, lower) = (a.distance(b), b.distance(c));
    let direction = (target - a).normalize_or_zero();
    if direction.length_squared() == 0. || upper < 1e-6 || lower < 1e-6 {
        return Ok(());
    }
    let distance = a.distance(target).clamp(
        (upper - lower).abs() + 1e-5,
        (upper + lower - 1e-5).max(1e-5),
    );
    let pole = Vec3::from_array(chain.pole);
    let mut bend = (pole - direction * pole.dot(direction)).normalize_or_zero();
    if bend.length_squared() == 0. {
        bend = direction.any_orthonormal_vector();
    }
    let along = (upper * upper + distance * distance - lower * lower) / (2. * distance);
    let height = (upper * upper - along * along).max(0.).sqrt();
    let desired_middle = a + direction * along + bend * height;
    rotate(
        rig,
        pose,
        globals,
        chain.root,
        Quat::from_rotation_arc((b - a).normalize(), (desired_middle - a).normalize()),
        weight,
    );
    rig.globals_into(pose, globals)?;
    let b = globals[chain.middle].w_axis.truncate();
    let c = globals[chain.tip].w_axis.truncate();
    let desired = (target - b).normalize_or_zero();
    if desired.length_squared() > 0. {
        rotate(
            rig,
            pose,
            globals,
            chain.middle,
            Quat::from_rotation_arc((c - b).normalize(), desired),
            weight,
        );
    }
    Ok(())
}
fn rotate(rig: &Rig, pose: &mut [Pose], globals: &[Mat4], bone: usize, delta: Quat, weight: f32) {
    let parent = rig.nodes[bone].parent.map_or(Quat::IDENTITY, |p| {
        globals[p as usize].to_scale_rotation_translation().1
    });
    let current = Quat::from_array(pose[bone].rotation);
    let desired = parent.conjugate() * delta * parent * current;
    pose[bone].rotation = current
        .slerp(desired.normalize(), weight)
        .normalize()
        .to_array();
}
