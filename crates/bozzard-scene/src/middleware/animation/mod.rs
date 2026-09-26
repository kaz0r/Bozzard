//! Authored animation state machines, one-dimensional blend trees, events and root motion.
pub mod data;
#[derive(Clone, Debug)]
pub struct Palette {
    pub signature: u64,
    pub matrices: std::sync::Arc<Vec<[f32; 16]>>,
}
use super::{
    curve::{Playhead, Repeat},
    registry::{Authored, PreviewPolicy},
    signals::{Kind, Signal, Signals},
    timeline::crossed_markers,
};
use crate::{
    AssetKind, Component, Field, FieldValue, Object, Scene, SceneInstance, Transform, Ui, World,
};
use anyhow::{Context, Result, ensure};
use data::{Pose, Rig};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSample {
    pub threshold: f32,
    pub clip: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Motion {
    Clip {
        clip: usize,
    },
    Blend1d {
        parameter: String,
        samples: Vec<BlendSample>,
    },
}
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
    fn weights(&self, motion: &Motion, parameters: &BTreeMap<String, f32>) -> (usize, usize, f32) {
        match motion {
            Motion::Clip { clip } => (*clip, *clip, 0.),
            Motion::Blend1d { parameter, samples } => {
                let value = parameters[parameter];
                let end = samples.partition_point(|s| s.threshold <= value);
                if end == 0 {
                    (samples[0].clip, samples[0].clip, 0.)
                } else if end == samples.len() {
                    let clip = samples[end - 1].clip;
                    (clip, clip, 0.)
                } else {
                    let (a, b) = (&samples[end - 1], &samples[end]);
                    (
                        a.clip,
                        b.clip,
                        (value - a.threshold) / (b.threshold - a.threshold),
                    )
                }
            }
        }
    }
    fn sample(&self, weights: (usize, usize, f32), phase: f32) -> Result<Vec<Pose>> {
        let (a, b, w) = weights;
        let mut pose = self.rig.sample(a, phase * self.rig.clips[a].duration)?;
        if a != b {
            let other = self.rig.sample(b, phase * self.rig.clips[b].duration)?;
            for (p, q) in pose.iter_mut().zip(other) {
                *p = p.blend(q, w);
            }
        }
        Ok(pose)
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
            match &state.motion {
                Motion::Clip { clip } => {
                    ensure!(*clip < self.rig.clips.len(), "state clip does not exist")
                }
                Motion::Blend1d { parameter, samples } => {
                    ensure!(
                        self.parameters.contains_key(parameter)
                            && !samples.is_empty()
                            && samples.len() <= 64,
                        "blend tree requires a parameter and 1–64 samples"
                    );
                    ensure!(
                        samples
                            .iter()
                            .all(|s| s.threshold.is_finite() && s.clip < self.rig.clips.len())
                            && samples.windows(2).all(|w| w[0].threshold < w[1].threshold),
                        "blend thresholds must strictly increase and clips must exist"
                    );
                }
            }
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
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
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
        Ok(())
    }
    fn write_targets(&self, owner: &str) -> Vec<String> {
        vec![owner.into()]
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 128
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    pub from: Vec<Pose>,
    pub elapsed: f32,
    pub duration: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Player {
    pub dirty: bool,
    pub signature: u64,
    pub state: usize,
    pub initialized: bool,
    pub include_start: bool,
    pub clock: Playhead,
    pub parameters: BTreeMap<String, f32>,
    pub fade: Option<Fade>,
    pub pose: Arc<Vec<Pose>>,
    pub palette: Arc<Vec<[f32; 16]>>,
}
impl Player {
    fn initialize(&mut self, animator: &Animator) {
        if !self.initialized {
            self.state = animator
                .states
                .iter()
                .position(|s| s.name == animator.initial)
                .unwrap_or(0);
            self.parameters = animator.parameters.clone();
            self.clock.playing = animator.autoplay;
            self.include_start = animator.autoplay;
            self.initialized = true;
            self.signature = animator.rig.signature();
            self.dirty = true;
        }
    }
    fn transition(&mut self, target: usize, duration: f32) {
        self.fade = (duration > 0. && !self.pose.is_empty()).then(|| Fade {
            from: self.pose.as_ref().clone(),
            elapsed: 0.,
            duration,
        });
        self.state = target;
        self.clock = Playhead::default();
        self.clock.play(true);
        self.include_start = true;
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub players: BTreeMap<String, Player>,
}
#[derive(Clone, Debug)]
pub enum Control {
    Play { state: String, fade: f32 },
    Pause,
    Stop,
    Seek(f32),
    Parameter { name: String, value: f32 },
}
impl SceneInstance {
    pub fn control_animation(
        &self,
        world: &mut World,
        owner: &str,
        control: Control,
    ) -> Result<()> {
        let entity = self.entity(owner).context("animation target is missing")?;
        let animator = world
            .get::<Animator>(entity)
            .context("target has no Animator")?
            .clone();
        let target = match &control {
            Control::Play { state, fade } => {
                ensure!(
                    fade.is_finite() && (0.0..=60.).contains(fade),
                    "invalid animation fade"
                );
                Some(
                    animator
                        .states
                        .iter()
                        .position(|s| &s.name == state)
                        .context("animation state does not exist")?,
                )
            }
            Control::Seek(value) => {
                ensure!(
                    value.is_finite() && (0.0..=1.).contains(value),
                    "animation seek is normalized 0–1"
                );
                None
            }
            Control::Parameter { name, value } => {
                ensure!(
                    animator.parameters.contains_key(name) && value.is_finite(),
                    "animation parameter is missing or non-finite"
                );
                None
            }
            _ => None,
        };
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let player = world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .entry(owner.into())
            .or_default();
        player.initialize(&animator);
        player.dirty = true;
        match control {
            Control::Play { fade, .. } => player.transition(target.unwrap(), fade),
            Control::Pause => player.clock.playing = false,
            Control::Stop => {
                player.clock = Playhead::default();
                player.fade = None;
                player.include_start = false;
            }
            Control::Seek(value) => {
                player.clock.seek(f64::from(value))?;
                player.fade = None;
                player.include_start = false;
            }
            Control::Parameter { name, value } => {
                player.parameters.insert(name, value);
            }
        }
        Ok(())
    }
    pub fn step_animations(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid animation timestep");
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let mut signals = world.remove_resource::<Signals>().unwrap_or_default();
        signals.begin(Kind::Animation);
        let result = (|| -> Result<()> {
            for (owner, &entity) in self.component_entities::<Animator>(world) {
                let Some(animator) = world.get::<Animator>(entity).cloned() else {
                    continue;
                };
                let player = runtime.players.entry(owner.clone()).or_default();
                player.initialize(&animator);
                if animator.states.is_empty() {
                    if player.palette.is_empty() && !animator.rig.bindings.is_empty() {
                        player.pose = Arc::new(animator.rig.rest_pose());
                        player.palette = Arc::new(animator.rig.palette(&player.pose)?);
                    }
                    continue;
                }
                if !animator.enabled {
                    continue;
                }
                let current = &animator.states[player.state];
                if player.clock.playing
                    && let Some(transition) = animator.transitions.iter().find(|t| {
                        (t.from == "*" || t.from == current.name)
                            && t.to != current.name
                            && t.exit_time
                                .is_none_or(|v| player.clock.position(1., current.repeat) >= v)
                            && t.comparison
                                .matches(player.parameters[&t.parameter], t.threshold)
                    })
                {
                    let target = animator
                        .states
                        .iter()
                        .position(|s| s.name == transition.to)
                        .unwrap();
                    player.transition(target, transition.fade);
                }
                let state = &animator.states[player.state];
                let weights = animator.weights(&state.motion, &player.parameters);
                let (a, b, w) = weights;
                let duration =
                    animator.rig.clips[a].duration * (1. - w) + animator.rig.clips[b].duration * w;
                let previous = player.clock;
                let mut clock = previous;
                clock.advance(dt, animator.speed / duration, 1., state.repeat)?;
                if clock.elapsed == previous.elapsed
                    && !player.dirty
                    && player.fade.is_none()
                    && !player.include_start
                    && !player.pose.is_empty()
                {
                    continue;
                }
                let dominant = if w < 0.5 { a } else { b };
                let clip = &animator.rig.clips[dominant];
                let hits = if previous.playing
                    && (clock.elapsed > previous.elapsed || player.include_start)
                {
                    crossed_markers(
                        clip.events.iter().map(|e| e.time / clip.duration),
                        previous.elapsed,
                        clock.elapsed,
                        1.,
                        state.repeat,
                        player.include_start,
                    )?
                } else {
                    Vec::new()
                };
                let mut pose = animator.sample(weights, clock.position(1., state.repeat))?;
                if let Some(fade) = &mut player.fade {
                    if previous.playing {
                        fade.elapsed = (fade.elapsed + dt).min(fade.duration);
                    }
                    for (p, from) in pose.iter_mut().zip(&fade.from) {
                        *p = from.blend(*p, fade.elapsed / fade.duration);
                    }
                    if fade.elapsed >= fade.duration {
                        player.fade = None;
                    }
                }
                if let Some(root) = &animator.root_motion {
                    if clock.elapsed != previous.elapsed {
                        let before = root_position(
                            &animator,
                            weights,
                            previous.elapsed,
                            state.repeat,
                            root.node,
                        )?;
                        let after = root_position(
                            &animator,
                            weights,
                            clock.elapsed,
                            state.repeat,
                            root.node,
                        )?;
                        let mut delta = after.0 - before.0;
                        for axis in 0..3 {
                            if !root.translation[axis] {
                                delta[axis] = 0.;
                            }
                        }
                        let mut transform = *world
                            .get::<Transform>(entity)
                            .context("root motion transform is missing")?;
                        let rotation = Quat::from_euler(
                            EulerRot::XYZ,
                            transform.rotation_degrees[0].to_radians(),
                            transform.rotation_degrees[1].to_radians(),
                            transform.rotation_degrees[2].to_radians(),
                        );
                        transform.translation = (Vec3::from_array(transform.translation)
                            + rotation * (delta * Vec3::from_array(transform.scale)))
                        .to_array();
                        if root.yaw {
                            transform.rotation_degrees[1] += (after.1 - before.1).to_degrees();
                        }
                        transform.validate()?;
                        world.insert(entity, transform)?;
                    }
                    let joint = &mut pose[root.node];
                    let rest = animator.rig.nodes[root.node].rest;
                    for axis in 0..3 {
                        if root.translation[axis] {
                            joint.translation[axis] = rest.translation[axis];
                        }
                    }
                    if root.yaw {
                        let (yaw, _, _) = Quat::from_array(joint.rotation).to_euler(EulerRot::YXZ);
                        let (rest_yaw, _, _) =
                            Quat::from_array(rest.rotation).to_euler(EulerRot::YXZ);
                        joint.rotation = (Quat::from_rotation_y(rest_yaw - yaw)
                            * Quat::from_array(joint.rotation))
                        .normalize()
                        .to_array();
                    }
                }
                let palette = animator.rig.palette(&pose)?;
                for hit in hits {
                    let event = &clip.events[hit];
                    signals.emit(
                        owner,
                        Signal {
                            kind: Kind::Animation,
                            name: event.name.clone(),
                            other: None,
                            value: event.time,
                        },
                    )?;
                }
                player.clock = clock;
                player.dirty = false;
                player.include_start = false;
                player.pose = Arc::new(pose);
                player.palette = Arc::new(palette);
            }
            runtime.players.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Animator>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        world.insert_resource(signals);
        result
    }
}
fn root_position(
    animator: &Animator,
    weights: (usize, usize, f32),
    elapsed: f64,
    repeat: Repeat,
    node: usize,
) -> Result<(Vec3, f32)> {
    let clock = Playhead {
        elapsed,
        ..Default::default()
    };
    let joint = |phase: f32| -> Result<Pose> {
        let (a, b, w) = weights;
        let first = animator
            .rig
            .sample_joint(a, phase * animator.rig.clips[a].duration, node)?;
        if a == b {
            Ok(first)
        } else {
            Ok(first.blend(
                animator
                    .rig
                    .sample_joint(b, phase * animator.rig.clips[b].duration, node)?,
                w,
            ))
        }
    };
    let yaw_at = |phase: f32| -> Result<f32> {
        let (a, b, w) = weights;
        let first = animator
            .rig
            .root_yaw(a, phase * animator.rig.clips[a].duration, node)?;
        if a == b {
            Ok(first)
        } else {
            Ok(first * (1. - w)
                + animator
                    .rig
                    .root_yaw(b, phase * animator.rig.clips[b].duration, node)?
                    * w)
        }
    };
    let pose = joint(clock.position(1., repeat))?;
    let mut position = Vec3::from_array(pose.translation);
    let mut yaw = yaw_at(clock.position(1., repeat))?;
    if repeat == Repeat::Loop {
        let start = joint(0.)?;
        let end = joint(1.)?;
        let cycles = elapsed.floor() as f32;
        position +=
            (Vec3::from_array(end.translation) - Vec3::from_array(start.translation)) * cycles;
        yaw += (yaw_at(1.)? - yaw_at(0.)?) * cycles;
    }
    ensure!(
        position.is_finite() && yaw.is_finite(),
        "root motion overflow"
    );
    Ok((position, yaw))
}
