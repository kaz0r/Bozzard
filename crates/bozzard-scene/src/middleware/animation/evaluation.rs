//! Prepared controller data and caller-owned pose buffers. None of this is checkpoint state.
use super::{
    AnimationLayer, Animator, StateDefinition,
    data::{Pose, Rig},
    motion::{Mix, Plan},
};
use anyhow::Result;
use glam::Mat4;
use std::sync::Arc;

#[derive(Debug)]
struct Sources {
    rig: Arc<Rig>,
    states: Arc<Vec<StateDefinition>>,
    layers: Arc<Vec<AnimationLayer>>,
}
#[derive(Debug, Default)]
pub(super) struct Cache {
    sources: Option<Sources>,
    pub states: Vec<Plan>,
    pub layers: Vec<Plan>,
    pub masks: Vec<Vec<f32>>,
    pub references: Vec<Vec<Pose>>,
    pub rest_globals: Vec<Mat4>,
    pub scratch: Vec<Pose>,
    pub overlay: Vec<Pose>,
    pub globals: Vec<Mat4>,
    pub goals: Vec<Option<super::ik::Goal>>,
    pub feet: Vec<(usize, super::ik::Goal)>,
}
impl Clone for Cache {
    fn clone(&self) -> Self {
        // Runtime snapshots do not duplicate prepared geometry or temporary pose storage.
        Self::default()
    }
}
impl Cache {
    pub fn prepare(&mut self, animator: &Animator) -> Result<bool> {
        if self.sources.as_ref().is_some_and(|s| {
            Arc::ptr_eq(&s.rig, &animator.rig)
                && Arc::ptr_eq(&s.states, &animator.states)
                && Arc::ptr_eq(&s.layers, &animator.layers)
        }) {
            return Ok(false);
        }
        self.states = animator
            .states
            .iter()
            .map(|s| Plan::compile(&s.motion))
            .collect::<Result<_>>()?;
        self.layers = animator
            .layers
            .iter()
            .map(|s| Plan::compile(&s.motion))
            .collect::<Result<_>>()?;
        self.masks = animator
            .layers
            .iter()
            .map(|s| s.mask.compile(&animator.rig))
            .collect::<Result<_>>()?;
        self.references = animator
            .layers
            .iter()
            .map(|layer| {
                layer.reference_clip.map_or_else(
                    || Ok(animator.rig.rest_pose()),
                    |clip| animator.rig.sample(clip, 0.),
                )
            })
            .collect::<Result<_>>()?;
        animator
            .rig
            .globals_into(&animator.rig.rest_pose(), &mut self.rest_globals)?;
        self.sources = Some(Sources {
            rig: animator.rig.clone(),
            states: animator.states.clone(),
            layers: animator.layers.clone(),
        });
        Ok(true)
    }
}
pub(super) fn sample_mix(
    rig: &Rig,
    mix: Mix,
    phase: f32,
    output: &mut Vec<Pose>,
    scratch: &mut Vec<Pose>,
) -> Result<()> {
    let first = mix.samples[0];
    rig.sample_into(first.clip, phase * rig.clips[first.clip].duration, output)?;
    let mut total = first.weight;
    for contribution in mix.iter().skip(1) {
        rig.sample_into(
            contribution.clip,
            phase * rig.clips[contribution.clip].duration,
            scratch,
        )?;
        total += contribution.weight;
        let weight = contribution.weight / total;
        for (pose, other) in output.iter_mut().zip(scratch.iter()) {
            *pose = pose.blend(*other, weight);
        }
    }
    Ok(())
}
