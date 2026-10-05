//! Offline animation reuse. Bake once into a target rig; playback has no retargeting cost.
use super::data::{Channel, Clip, Pose, Property, Rig};
use crate::middleware::curve::{Curve, Key};
use anyhow::{Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoneMapping {
    pub source: usize,
    pub target: usize,
    /// Translate roots/hips; preserve the target's authored limb lengths elsewhere.
    pub translation: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetargetMap {
    pub bones: Vec<BoneMapping>,
    pub translation_scale: f32,
}
fn canonical(name: &str) -> String {
    name.rsplit(':')
        .next()
        .unwrap_or(name)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
impl RetargetMap {
    /// Exact normalized bone names are a starting point, not a guessed humanoid topology.
    /// Ambiguous names are omitted so an author can supply an explicit mapping.
    pub fn by_name(source: &Rig, target: &Rig) -> Self {
        let mut lookup: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, bone) in source.nodes.iter().enumerate() {
            lookup.entry(canonical(&bone.name)).or_default().push(index);
        }
        let mut target_counts = BTreeMap::new();
        for bone in &target.nodes {
            *target_counts.entry(canonical(&bone.name)).or_insert(0) += 1;
        }
        let bones = target
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, bone)| {
                let name = canonical(&bone.name);
                let candidates = lookup.get(&name)?;
                (candidates.len() == 1 && target_counts[&name] == 1).then(|| {
                    let source_index = candidates[0];
                    BoneMapping {
                        source: source_index,
                        target: index,
                        translation: source.nodes[source_index].parent.is_none()
                            || name == "hips"
                            || name == "root"
                            || name == "pelvis",
                    }
                })
            })
            .collect();
        Self {
            bones,
            translation_scale: 1.,
        }
    }
    pub fn validate(&self, source: &Rig, target: &Rig) -> Result<()> {
        ensure!(
            !self.bones.is_empty() && self.bones.len() <= target.nodes.len(),
            "retargeting needs at least one bone mapping"
        );
        ensure!(
            self.translation_scale.is_finite() && (0.001..=1000.).contains(&self.translation_scale),
            "retarget translation scale must be within 0.001–1000"
        );
        let mut targets = BTreeSet::new();
        let mut sources = BTreeSet::new();
        for bone in &self.bones {
            ensure!(
                bone.source < source.nodes.len()
                    && bone.target < target.nodes.len()
                    && targets.insert(bone.target)
                    && sources.insert(bone.source),
                "retarget mappings must be unique existing source and target bones"
            );
        }
        Ok(())
    }

    /// Uniform resampling makes mixed source interpolation predictable. Events retain their times.
    pub fn bake_clip(
        &self,
        source: &Rig,
        target: &Rig,
        clip: usize,
        name: String,
        rate: f32,
    ) -> Result<Clip> {
        self.bake_clip_with(source, target, clip, name, rate, || false)
    }
    pub fn bake_clip_with(
        &self,
        source: &Rig,
        target: &Rig,
        clip: usize,
        name: String,
        rate: f32,
        cancelled: impl Fn() -> bool,
    ) -> Result<Clip> {
        source.validate()?;
        target.validate()?;
        self.validate(source, target)?;
        ensure!(
            rate.is_finite() && (1.0..=120.).contains(&rate),
            "retarget sample rate must be 1–120 Hz"
        );
        let source_clip = clip;
        let clip = source
            .clips
            .get(source_clip)
            .ok_or_else(|| anyhow::anyhow!("retarget source clip is missing"))?;
        ensure!(
            !name.trim().is_empty() && name.len() <= 256,
            "retarget clip needs a name"
        );
        let frames = (clip.duration * rate).ceil() as usize + 1;
        let key_count = frames
            .checked_mul(self.bones.len())
            .and_then(|n| n.checked_mul(10));
        ensure!(
            frames <= crate::middleware::curve::MAX_KEYS
                && key_count.is_some_and(|n| n <= 1_000_000),
            "retargeted clip exceeds animation key limits; lower the sample rate"
        );
        let mut context = RetargetContext::new(self, source, target)?;
        let mut tracks = BakedTracks::new(self.bones.len(), frames);
        let mut sampled = Vec::new();
        for frame in 0..frames {
            ensure!(!cancelled(), "animation retargeting cancelled");
            let time = clip.duration * frame as f32 / (frames - 1) as f32;
            source.sample_into(source_clip, time, &mut sampled)?;
            context.apply(&sampled)?;
            tracks.push(&self.bones, &context.pose, time);
        }
        let mut result = Clip {
            name,
            duration: clip.duration,
            channels: tracks.into_channels(&self.bones),
            events: clip.events.clone(),
        };
        result.compact(&target.nodes)?;
        Ok(result)
    }
}
struct BakedTracks {
    curves: Vec<[Vec<Key>; 10]>,
    previous: Vec<Quat>,
}
impl BakedTracks {
    fn new(bones: usize, frames: usize) -> Self {
        Self {
            curves: (0..bones)
                .map(|_| std::array::from_fn(|_| Vec::with_capacity(frames)))
                .collect(),
            previous: vec![Quat::IDENTITY; bones],
        }
    }
    fn push(&mut self, mappings: &[BoneMapping], pose: &[Pose], time: f32) {
        for (index, mapping) in mappings.iter().enumerate() {
            let value = pose[mapping.target];
            let mut rotation = Quat::from_array(value.rotation);
            if !self.curves[index][0].is_empty() && self.previous[index].dot(rotation) < 0. {
                rotation = -rotation;
            }
            self.previous[index] = rotation;
            let components = value
                .translation
                .into_iter()
                .chain(rotation.to_array())
                .chain(value.scale);
            for (curve, component) in self.curves[index].iter_mut().zip(components) {
                curve.push(Key::new(time, component));
            }
        }
    }
    fn into_channels(self, mappings: &[BoneMapping]) -> Vec<Channel> {
        let mut channels = Vec::with_capacity(mappings.len() * 3);
        for (mapping, curves) in mappings.iter().zip(self.curves) {
            let mut iterator = curves.into_iter();
            for (property, axes) in [
                (Property::Translation, 3),
                (Property::Rotation, 4),
                (Property::Scale, 3),
            ] {
                channels.push(Channel {
                    node: mapping.target as u32,
                    property,
                    curves: iterator
                        .by_ref()
                        .take(axes)
                        .map(|keys| Curve {
                            interpolation: crate::middleware::curve::Interpolation::Linear,
                            keys,
                        })
                        .collect(),
                });
            }
        }
        channels
    }
}
/// Scratch storage and rest-space rotations are prepared once per bake, then reused per frame.
struct RetargetContext<'a> {
    map: &'a RetargetMap,
    source: &'a Rig,
    target: &'a Rig,
    source_rest: Vec<Quat>,
    target_rest: Vec<Quat>,
    lookup: Vec<Option<usize>>,
    pose: Vec<Pose>,
    rotations: Vec<Quat>,
    animated: Vec<Mat4>,
}
impl<'a> RetargetContext<'a> {
    fn new(map: &'a RetargetMap, source: &'a Rig, target: &'a Rig) -> Result<Self> {
        let mut lookup = vec![None; target.nodes.len()];
        for (index, mapping) in map.bones.iter().enumerate() {
            lookup[mapping.target] = Some(index);
        }
        Ok(Self {
            map,
            source,
            target,
            source_rest: globals(source, &source.rest_pose())?
                .iter()
                .map(rotation)
                .collect(),
            target_rest: globals(target, &target.rest_pose())?
                .iter()
                .map(rotation)
                .collect(),
            lookup,
            pose: target.rest_pose(),
            rotations: vec![Quat::IDENTITY; target.nodes.len()],
            animated: Vec::with_capacity(source.nodes.len()),
        })
    }
    fn apply(&mut self, sampled: &[Pose]) -> Result<()> {
        self.source.globals_into(sampled, &mut self.animated)?;
        for (index, bone) in self.target.nodes.iter().enumerate() {
            let parent = bone
                .parent
                .map_or(Quat::IDENTITY, |p| self.rotations[p as usize]);
            self.pose[index] = bone.rest;
            if let Some(mapping_index) = self.lookup[index] {
                let mapping = &self.map.bones[mapping_index];
                let desired = rotation(&self.animated[mapping.source])
                    * self.source_rest[mapping.source].conjugate()
                    * self.target_rest[index];
                self.pose[index].rotation = (parent.conjugate() * desired).normalize().to_array();
                if mapping.translation {
                    let source_parent = self.source.nodes[mapping.source]
                        .parent
                        .map_or(Quat::IDENTITY, |p| self.source_rest[p as usize]);
                    let target_parent = bone
                        .parent
                        .map_or(Quat::IDENTITY, |p| self.target_rest[p as usize]);
                    let delta = Vec3::from_array(sampled[mapping.source].translation)
                        - Vec3::from_array(self.source.nodes[mapping.source].rest.translation);
                    self.pose[index].translation = (Vec3::from_array(bone.rest.translation)
                        + target_parent.conjugate()
                            * (source_parent * delta)
                            * self.map.translation_scale)
                        .to_array();
                }
            }
            self.rotations[index] = parent * Quat::from_array(self.pose[index].rotation);
            self.pose[index].validate()?;
        }
        Ok(())
    }
}

fn rotation(matrix: &Mat4) -> Quat {
    matrix.to_scale_rotation_translation().1.normalize()
}
fn globals(rig: &Rig, pose: &[Pose]) -> Result<Vec<Mat4>> {
    let mut output = Vec::with_capacity(pose.len());
    rig.globals_into(pose, &mut output)?;
    Ok(output)
}
