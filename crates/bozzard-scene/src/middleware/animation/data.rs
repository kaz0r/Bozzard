//! Cooked glTF-compatible rigs and clips. No importer or graphics dependency is needed to sample.
use crate::middleware::{
    curve::{Curve, Interpolation},
    timeline::Marker,
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_NODES: usize = 1024;
pub const MAX_BINDINGS: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pose {
    pub translation: [f32; 3],
    /// Unit quaternion, XYZW.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        }
    }
}
impl Pose {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.translation
                .iter()
                .chain(&self.rotation)
                .chain(&self.scale)
                .all(|v| v.is_finite()),
            "non-finite joint pose"
        );
        ensure!(
            self.scale.iter().all(|v| v.abs() >= 0.0001)
                && (Quat::from_array(self.rotation).length_squared() - 1.).abs() < 0.002,
            "singular scale or non-unit joint rotation"
        );
        Ok(())
    }
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from_array(self.scale),
            Quat::from_array(self.rotation),
            Vec3::from_array(self.translation),
        )
    }
    pub fn blend(self, other: Self, weight: f32) -> Self {
        Self {
            translation: Vec3::from_array(self.translation)
                .lerp(Vec3::from_array(other.translation), weight)
                .to_array(),
            rotation: Quat::from_array(self.rotation)
                .slerp(Quat::from_array(other.rotation), weight)
                .normalize()
                .to_array(),
            scale: Vec3::from_array(self.scale)
                .lerp(Vec3::from_array(other.scale), weight)
                .to_array(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    pub name: String,
    /// Parents precede children in the cooked array; iteration is a topological traversal.
    pub parent: Option<u32>,
    pub rest: Pose,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub node: u32,
    /// Includes inverse mesh bind transform because imported vertices are in model space.
    pub inverse_bind: [f32; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub node: u32,
    pub property: Property,
    pub curves: Vec<Curve>,
}
impl Channel {
    fn validate(&self, nodes: usize, duration: f32) -> Result<()> {
        ensure!(
            (self.node as usize) < nodes
                && self.curves.len()
                    == if self.property == Property::Rotation {
                        4
                    } else {
                        3
                    },
            "invalid animation channel"
        );
        for curve in &self.curves {
            curve.validate()?;
            ensure!(curve.duration() <= duration, "animation key outside clip");
            ensure!(
                self.property != Property::Rotation
                    || (curve.interpolation == self.curves[0].interpolation
                        && curve.keys.len() == self.curves[0].keys.len()
                        && curve
                            .keys
                            .iter()
                            .zip(&self.curves[0].keys)
                            .all(|(a, b)| a.time == b.time)),
                "animation channel samples must share timestamps and interpolation"
            );
        }
        if self.property == Property::Rotation {
            for index in 0..self.curves[0].keys.len() {
                let q = Quat::from_array(std::array::from_fn(|axis| {
                    self.curves[axis].keys[index].value
                }));
                ensure!(
                    (q.length_squared() - 1.).abs() < 0.002,
                    "animation rotation key must be a unit quaternion"
                );
            }
        }
        Ok(())
    }
    fn apply(&self, time: f32, pose: &mut Pose) -> Result<()> {
        match self.property {
            Property::Translation => {
                pose.translation = std::array::from_fn(|i| self.curves[i].sample(time))
            }
            Property::Scale => pose.scale = std::array::from_fn(|i| self.curves[i].sample(time)),
            Property::Rotation => {
                let curve = &self.curves[0];
                let end = curve.keys.partition_point(|k| k.time <= time);
                let q = if curve.interpolation == Interpolation::Linear
                    && end > 0
                    && end < curve.keys.len()
                {
                    let a = Quat::from_array(std::array::from_fn(|i| {
                        self.curves[i].keys[end - 1].value
                    }));
                    let b =
                        Quat::from_array(std::array::from_fn(|i| self.curves[i].keys[end].value));
                    a.slerp(
                        b,
                        (time - curve.keys[end - 1].time)
                            / (curve.keys[end].time - curve.keys[end - 1].time),
                    )
                } else {
                    Quat::from_array(std::array::from_fn(|i| self.curves[i].sample(time)))
                };
                ensure!(
                    q.is_finite() && q.length_squared() > 1e-12,
                    "animation quaternion interpolation is singular"
                );
                pose.rotation = q.normalize().to_array();
            }
        }
        pose.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub events: Vec<Marker>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rig {
    pub nodes: Vec<Joint>,
    pub bindings: Vec<Binding>,
    pub clips: Vec<Clip>,
}
impl Rig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.nodes.len() <= MAX_NODES
                && self.bindings.len() <= MAX_BINDINGS
                && self.clips.len() <= 256,
            "rig exceeds node, binding or clip limit"
        );
        for (index, node) in self.nodes.iter().enumerate() {
            ensure!(
                node.name.len() <= 256 && node.parent.is_none_or(|p| (p as usize) < index),
                "rig parent order is invalid"
            );
            node.rest.validate()?;
        }
        for binding in &self.bindings {
            let matrix = Mat4::from_cols_array(&binding.inverse_bind);
            ensure!(
                (binding.node as usize) < self.nodes.len() && affine(matrix),
                "invalid inverse bind matrix"
            );
        }
        let mut names = BTreeSet::new();
        let mut keys = 0usize;
        for clip in &self.clips {
            ensure!(
                !clip.name.is_empty() && clip.name.len() <= 256 && names.insert(&clip.name),
                "clip names must be unique and nonempty"
            );
            ensure!(
                clip.duration.is_finite()
                    && (0.001..=86400.).contains(&clip.duration)
                    && clip.channels.len() <= self.nodes.len() * 3,
                "invalid animation clip duration or channel count"
            );
            let mut targets = BTreeSet::new();
            for channel in &clip.channels {
                channel.validate(self.nodes.len(), clip.duration)?;
                ensure!(
                    targets.insert((channel.node, channel.property)),
                    "duplicate animation channel"
                );
                keys += channel.curves.iter().map(|c| c.keys.len()).sum::<usize>();
                ensure!(keys <= 1_000_000, "rig exceeds one million scalar keys");
            }
            ensure!(
                clip.events.len() <= 1024 && clip.events.windows(2).all(|w| w[0].time <= w[1].time),
                "invalid animation event count or order"
            );
            for event in &clip.events {
                ensure!(
                    event.time.is_finite()
                        && (0.0..=clip.duration).contains(&event.time)
                        && !event.name.is_empty()
                        && event.name.len() <= 256,
                    "invalid animation event"
                );
            }
        }
        self.palette(&self.rest_pose())?;
        Ok(())
    }
    pub fn rest_pose(&self) -> Vec<Pose> {
        self.nodes.iter().map(|n| n.rest).collect()
    }
    pub fn sample(&self, clip: usize, time: f32) -> Result<Vec<Pose>> {
        ensure!(time.is_finite(), "invalid clip sample time");
        let clip = self
            .clips
            .get(clip)
            .context("animation clip does not exist")?;
        let mut pose = self.rest_pose();
        for channel in &clip.channels {
            channel.apply(time, &mut pose[channel.node as usize])?;
        }
        Ok(pose)
    }
    /// Root motion needs only one joint, avoiding temporary whole-skeleton poses.
    pub fn sample_joint(&self, clip: usize, time: f32, node: usize) -> Result<Pose> {
        let mut pose = self
            .nodes
            .get(node)
            .context("root motion joint missing")?
            .rest;
        let clip = self.clips.get(clip).context("root motion clip missing")?;
        for channel in clip.channels.iter().filter(|c| c.node as usize == node) {
            channel.apply(time, &mut pose)?;
        }
        Ok(pose)
    }
    /// Unwrap each shortest-arc key interval so crossing +/-pi does not teleport an actor.
    pub fn root_yaw(&self, clip: usize, time: f32, node: usize) -> Result<f32> {
        let yaw = |q: Quat| q.normalize().to_euler(glam::EulerRot::YXZ).0;
        let current = yaw(Quat::from_array(
            self.sample_joint(clip, time, node)?.rotation,
        ));
        let Some(channel) = self.clips[clip]
            .channels
            .iter()
            .find(|c| c.node as usize == node && c.property == Property::Rotation)
        else {
            return Ok(current);
        };
        let at = |index: usize| {
            yaw(Quat::from_array(std::array::from_fn(|axis| {
                channel.curves[axis].keys[index].value
            })))
        };
        let delta = |a: f32| {
            (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
        };
        let mut previous = at(0);
        let mut total = previous;
        for (index, _) in channel.curves[0]
            .keys
            .iter()
            .enumerate()
            .skip(1)
            .take_while(|(_, k)| k.time <= time)
        {
            let next = at(index);
            total += delta(next - previous);
            previous = next;
        }
        Ok(total + delta(current - previous))
    }
    pub fn palette(&self, pose: &[Pose]) -> Result<Vec<[f32; 16]>> {
        ensure!(pose.len() == self.nodes.len(), "pose does not match rig");
        let mut global = Vec::with_capacity(pose.len());
        for (joint, pose) in self.nodes.iter().zip(pose) {
            pose.validate()?;
            let matrix =
                joint.parent.map_or(Mat4::IDENTITY, |p| global[p as usize]) * pose.matrix();
            ensure!(
                affine(matrix),
                "animated joint hierarchy is singular or overflows"
            );
            global.push(matrix);
        }
        self.bindings
            .iter()
            .map(|b| {
                let matrix = global[b.node as usize] * Mat4::from_cols_array(&b.inverse_bind);
                ensure!(
                    affine(matrix),
                    "animated skin matrix is singular or overflows"
                );
                Ok(matrix.to_cols_array())
            })
            .collect()
    }
    /// Layout identity is independent of clips, so event/transition edits need no mesh upload.
    pub fn signature(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for binding in &self.bindings {
            for byte in binding
                .node
                .to_le_bytes()
                .into_iter()
                .chain(binding.inverse_bind.iter().flat_map(|v| v.to_le_bytes()))
            {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        hash
    }
}
fn affine(matrix: Mat4) -> bool {
    matrix.is_finite()
        && matrix.determinant().is_finite()
        && matrix.determinant().abs() > 1e-12
        && matrix.x_axis.w.abs() < 1e-5
        && matrix.y_axis.w.abs() < 1e-5
        && matrix.z_axis.w.abs() < 1e-5
        && (matrix.w_axis.w - 1.).abs() < 1e-5
}
