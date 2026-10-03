//! Skeletal controllers, blend spaces, layers, IK, and root-motion alignment.
pub mod data;
mod evaluation;
pub mod ik;
pub mod layers;
pub mod motion;
pub mod retarget;
mod root;
mod runtime;
pub mod warp;
pub use ik::{FootPlacement, IkConstraint, IkTarget};
pub use layers::{AnimationLayer, BoneMask, LayerBlend};
pub use motion::{BlendPoint, BlendSample, Motion};
pub use runtime::{Control, Fade, LayerPlayer, Player, Runtime};
pub use warp::{MotionWarp, WarpGoal, WarpTarget};
#[derive(Clone, Debug)]
pub struct Palette {
    pub signature: u64,
    pub matrices: std::sync::Arc<Vec<[f32; 16]>>,
}
use super::{
    curve::Repeat,
    registry::{Authored, PreviewPolicy},
};
use crate::{AssetKind, Component, Field, FieldValue, Object, Scene, Ui, World};
use anyhow::{Result, ensure};
use data::Rig;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDefinition {
    pub name: String,
    pub motion: Motion,
    pub repeat: Repeat,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Above,
    Below,
    Equal,
    NotEqual,
}
impl Comparison {
    fn matches(self, value: f32, threshold: f32) -> bool {
        match self {
            Self::Above => value > threshold,
            Self::Below => value < threshold,
            Self::Equal => value == threshold,
            Self::NotEqual => value != threshold,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// A named source state, or "*" for any state.
    pub from: String,
    pub to: String,
    pub parameter: String,
    pub comparison: Comparison,
    pub threshold: f32,
    pub fade: f32,
    /// Optional minimum normalized progress, before the condition may fire.
    pub exit_time: Option<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootMotion {
    pub node: usize,
    pub translation: [bool; 3],
    pub yaw: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Animator {
    pub enabled: bool,
    pub autoplay: bool,
    pub speed: f32,
    pub asset: String,
    pub initial: String,
    pub rig: Arc<Rig>,
    pub states: Arc<Vec<StateDefinition>>,
    pub transitions: Arc<Vec<Transition>>,
    pub parameters: BTreeMap<String, f32>,
    pub root_motion: Option<RootMotion>,
    pub layers: Arc<Vec<AnimationLayer>>,
    pub ik: Arc<Vec<IkConstraint>>,
    pub foot_placement: Option<FootPlacement>,
    pub warps: Arc<Vec<MotionWarp>>,
}
impl Default for Animator {
    fn default() -> Self {
        Self {
            enabled: true,
            autoplay: true,
            speed: 1.,
            asset: String::new(),
            initial: String::new(),
            rig: Arc::default(),
            states: Arc::default(),
            transitions: Arc::default(),
            parameters: BTreeMap::new(),
            root_motion: None,
            layers: Arc::default(),
            ik: Arc::default(),
            foot_placement: None,
            warps: Arc::default(),
        }
    }
}
impl Animator {
    pub fn from_rig(asset: String, rig: Arc<Rig>) -> Self {
        let states: Vec<_> = rig
            .clips
            .iter()
            .enumerate()
            .map(|(clip, c)| StateDefinition {
                name: c.name.clone(),
                motion: Motion::Clip { clip },
                repeat: Repeat::Loop,
            })
            .collect();
        Self {
            asset,
            initial: states.first().map_or_else(String::new, |s| s.name.clone()),
            states: Arc::new(states),
            rig,
            ..Default::default()
        }
    }
}

impl Component for Animator {
    const NAME: &'static str = "animator";
    const LABEL: &'static str = "Animator";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Cook a model rig to edit clips, events and a state machine. Blueprint parameters drive blend trees and transitions. Rig data is stored in the scene for headless playback.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::bool("autoplay", "Play on start"),
            Field::range("speed", "Speed", 0.05, 0., 100.),
            Field::asset("asset", "Model", AssetKind::Mesh),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "autoplay" => FieldValue::Bool(self.autoplay),
            "speed" => FieldValue::Number(self.speed),
            "asset" => FieldValue::Text(self.asset.clone()),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "autoplay" => self.autoplay = value.bool()?,
            "speed" => self.speed = value.number()?,
            "asset" => self.asset = value.text()?.into(),
            _ => anyhow::bail!("unknown animator field"),
        };
        Ok(())
    }
}
impl Authored for Animator {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;

    fn accept_prepared(prepared: &mut World, live: &mut World) {
        if let Some(runtime) = prepared.remove_resource::<Runtime>() {
            if let Some(current) = live.resource_mut::<Runtime>() {
                current.players.extend(runtime.players);
            } else {
                live.insert_resource(runtime);
            }
        }
    }
    fn initialize_runtime(&self, world: &mut World, owner: &str) -> Result<()> {
        let pose = Arc::new(self.rig.rest_pose());
        let palette = Arc::new(self.rig.palette(&pose)?);
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let mut player = Player {
            pose,
            palette,
            signature: self.rig.signature(),
            ..Default::default()
        };
        player.initialize(self);
        world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .insert(owner.into(), player);
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        self.rig.validate()?;
        ensure!(
            self.speed.is_finite() && (0.0..=100.).contains(&self.speed),
            "invalid animation speed"
        );
        ensure!(
            self.states.len() <= 64
                && self.transitions.len() <= 128
                && self.parameters.len() <= 128,
            "animation state machine exceeds limits"
        );
        for (key, value) in &self.parameters {
            ensure!(
                valid_name(key) && value.is_finite(),
                "invalid animation parameter"
            );
        }
        let mut names = BTreeSet::new();
        for state in self.states.iter() {
            ensure!(
                valid_name(&state.name) && state.name != "*" && names.insert(state.name.as_str()),
                "animation state names must be unique"
            );
            state.motion.validate(&self.rig, &self.parameters)?;
        }
        ensure!(
            self.states.is_empty() && self.initial.is_empty()
                || names.contains(self.initial.as_str()),
            "initial animation state is missing"
        );
        for t in self.transitions.iter() {
            ensure!(
                (t.from == "*" || names.contains(t.from.as_str()))
                    && names.contains(t.to.as_str())
                    && self.parameters.contains_key(&t.parameter),
                "transition state or parameter is missing"
            );
            ensure!(
                t.threshold.is_finite()
                    && t.fade.is_finite()
                    && (0.0..=60.).contains(&t.fade)
                    && t.exit_time
                        .is_none_or(|v| v.is_finite() && (0.0..=1.).contains(&v)),
                "invalid transition fade or exit time"
            );
        }
        if let Some(root) = &self.root_motion {
            ensure!(
                root.node < self.rig.nodes.len(),
                "root motion joint is missing"
            );
        }
        ensure!(
            self.layers.len() <= 16 && self.ik.len() <= 16 && self.warps.len() <= 64,
            "animator supports at most 16 layers, 16 IK chains and 64 motion-warp windows"
        );
        let mut layer_names = BTreeSet::new();
        for layer in self.layers.iter() {
            layer.validate(&self.rig, &self.parameters)?;
            ensure!(
                layer_names.insert(&layer.name),
                "animation layer names must be unique"
            );
        }
        let mut ik_names = BTreeSet::new();
        if let Some(settings) = &self.foot_placement {
            settings.validate(&self.rig)?;
        }
        for chain in self.ik.iter() {
            chain.validate(&self.rig, &self.parameters)?;
            ensure!(ik_names.insert(&chain.name), "IK names must be unique");
        }
        let mut warp_names = BTreeSet::new();
        for (index, window) in self.warps.iter().enumerate() {
            window.validate(self)?;
            ensure!(
                warp_names.insert(&window.name),
                "motion-warp names must be unique"
            );
            ensure!(
                !self.warps[..index]
                    .iter()
                    .any(|other| other.state == window.state
                        && other.start < window.end
                        && window.start < other.end),
                "motion-warp windows in one state cannot overlap"
            );
        }
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        if !self.asset.is_empty() {
            ensure!(
                scene
                    .assets
                    .get(&self.asset)
                    .is_some_and(|a| a.kind == AssetKind::Mesh),
                "animator model is missing"
            );
            ensure!(owner.drawable.as_ref().is_some_and(|d| matches!(&d.mesh, crate::Mesh::Asset(id) | crate::Mesh::Surface{asset:id,..} if id == &self.asset)), "Animator must use the object's mesh asset");
        }
        for target in self.object_targets() {
            ensure!(
                ids.contains(target),
                "animation target '{target}' is missing"
            );
        }
        Ok(())
    }
    fn remap_objects(&mut self, mapping: &BTreeMap<String, String>) {
        for chain in Arc::make_mut(&mut self.ik) {
            if let IkTarget::Object { object, .. } = &mut chain.target
                && let Some(target) = mapping.get(object)
            {
                *object = target.clone();
            }
        }
        for window in Arc::make_mut(&mut self.warps) {
            if let WarpTarget::Object { object, .. } = &mut window.target
                && let Some(target) = mapping.get(object)
            {
                *object = target.clone();
            }
        }
    }
    fn write_targets(&self, owner: &str) -> Vec<String> {
        vec![owner.into()]
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 128
}
impl Animator {
    fn object_targets(&self) -> impl Iterator<Item = &str> {
        self.ik
            .iter()
            .filter_map(|chain| match &chain.target {
                IkTarget::Object { object, .. } => Some(object.as_str()),
                _ => None,
            })
            .chain(self.warps.iter().filter_map(|window| match &window.target {
                WarpTarget::Object { object, .. } => Some(object.as_str()),
                _ => None,
            }))
    }
}
