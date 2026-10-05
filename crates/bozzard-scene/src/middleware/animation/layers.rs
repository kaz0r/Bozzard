//! Per-bone override and additive layers with persistent clocks and prepared masks.
use super::{
    data::{Pose, Rig},
    motion::Motion,
};
use anyhow::{Result, ensure};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerBlend {
    #[default]
    Override,
    Additive,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoneMask {
    /// This bone and its descendants participate; None selects the whole skeleton.
    pub root: Option<usize>,
    /// Optional explicit weights override the subtree selection for individual bones.
    pub weights: BTreeMap<usize, f32>,
}
impl BoneMask {
    pub(crate) fn compile(&self, rig: &Rig) -> Result<Vec<f32>> {
        ensure!(
            self.root.is_none_or(|i| i < rig.nodes.len()),
            "layer mask root is missing"
        );
        ensure!(
            self.weights.len() <= rig.nodes.len(),
            "layer mask exceeds skeleton size"
        );
        let mut result = vec![0.; rig.nodes.len()];
        for (index, joint) in rig.nodes.iter().enumerate() {
            result[index] = if self.root.is_none() || self.root == Some(index) {
                1.
            } else {
                joint.parent.map_or(0., |p| result[p as usize])
            };
        }
        // Overrides affect only their named bone; subtree membership is determined above.
        for (&index, &weight) in &self.weights {
            ensure!(
                index < result.len() && weight.is_finite() && (0.0..=1.).contains(&weight),
                "layer mask needs existing bones and weights within 0–1"
            );
            result[index] = weight;
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnimationLayer {
    pub name: String,
    pub motion: Motion,
    pub blend: LayerBlend,
    pub mask: BoneMask,
    pub weight: f32,
    pub weight_parameter: Option<String>,
    /// Time in seconds to fade a parameter-driven layer in or out. Zero changes immediately.
    pub fade: f32,
    /// Synchronized layers sample the locomotion phase; independent layers own a clock.
    pub synchronized: bool,
    pub repeat: super::Repeat,
    pub speed: f32,
    /// Additive deltas are measured against this clip's first pose, or the rest pose.
    pub reference_clip: Option<usize>,
}
impl Default for AnimationLayer {
    fn default() -> Self {
        Self {
            name: "Upper body".into(),
            motion: Motion::Clip { clip: 0 },
            blend: LayerBlend::Override,
            mask: BoneMask::default(),
            weight: 1.,
            weight_parameter: None,
            fade: 0.,
            synchronized: false,
            repeat: super::Repeat::Loop,
            speed: 1.,
            reference_clip: None,
        }
    }
}
impl AnimationLayer {
    pub(crate) fn validate(&self, rig: &Rig, parameters: &BTreeMap<String, f32>) -> Result<()> {
        ensure!(
            super::valid_name(&self.name),
            "animation layer name is empty or too long"
        );
        ensure!(
            self.weight.is_finite()
                && (0.0..=1.).contains(&self.weight)
                && self.speed.is_finite()
                && (0.0..=100.).contains(&self.speed)
                && self.fade.is_finite()
                && (0.0..=60.).contains(&self.fade),
            "layer weight must be 0–1 and speed 0–100"
        );
        ensure!(
            self.weight_parameter
                .as_ref()
                .is_none_or(|p| parameters.contains_key(p)),
            "animation layer weight parameter is missing"
        );
        ensure!(
            self.reference_clip.is_none_or(|c| c < rig.clips.len()),
            "additive reference clip is missing"
        );
        self.motion.validate(rig, parameters)?;
        let events_fit = |clip: usize| {
            rig.clips[clip]
                .events
                .iter()
                .all(|event| self.name.len() + 1 + event.name.len() <= 256)
        };
        let valid_events = match &self.motion {
            Motion::Clip { clip } => events_fit(*clip),
            Motion::Blend1d { samples, .. } => samples.iter().all(|s| events_fit(s.clip)),
            Motion::Blend2d { samples, .. } => samples.iter().all(|s| events_fit(s.clip)),
        };
        ensure!(
            valid_events,
            "layer/event names together must fit within 256 bytes"
        );
        self.mask.compile(rig)?;
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
pub(crate) fn apply(
    pose: &mut [Pose],
    overlay: &[Pose],
    reference: &[Pose],
    mask: &[f32],
    blend: LayerBlend,
    weight: f32,
) {
    for (((base, overlay), reference), mask) in
        pose.iter_mut().zip(overlay).zip(reference).zip(mask)
    {
        let weight = weight * mask;
        if weight <= 0. {
            continue;
        }
        match blend {
            LayerBlend::Override => *base = base.blend(*overlay, weight),
            LayerBlend::Additive => {
                base.translation = (Vec3::from_array(base.translation)
                    + (Vec3::from_array(overlay.translation)
                        - Vec3::from_array(reference.translation))
                        * weight)
                    .to_array();
                let delta = Quat::from_array(reference.rotation).conjugate()
                    * Quat::from_array(overlay.rotation);
                base.rotation = (Quat::from_array(base.rotation)
                    * Quat::IDENTITY.slerp(delta, weight))
                .normalize()
                .to_array();
                base.scale = (Vec3::from_array(base.scale)
                    * Vec3::ONE.lerp(
                        Vec3::from_array(overlay.scale) / Vec3::from_array(reference.scale),
                        weight,
                    ))
                .to_array();
            }
        }
    }
}
