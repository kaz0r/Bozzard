//! Baked ground navigation, deterministic A* and authored agent state machines.
mod mesh;
mod runtime;
use super::{
    registry::{self, Authored, PreviewPolicy},
    signals::{Kind, Signal, Signals},
};
use crate::{
    Component, Field, FieldValue, Object, Scene, SceneInstance, Transform, Ui, VectorRole, World,
};
use anyhow::{Context, Result, ensure};
use glam::Vec3;
pub use mesh::{BakeSettings, Cell, NavData, geometry_signature};
pub use runtime::{AgentState, Control, Runtime};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NavSurface {
    pub settings: BakeSettings,
    pub baked: Option<Arc<NavData>>,
}
impl Component for NavSurface {
    const NAME: &'static str = "nav_surface";
    const LABEL: &'static str = "Navigation Surface";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Bake ground navigation from static colliders. Bounds are world coordinates. Cell clearance includes the agent radius; one height per column. Use separate bounded surfaces for stacked floors. Rebake after changing colliders.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::vector("min", "Bounds minimum", VectorRole::Position, 0.1),
            Field::vector("max", "Bounds maximum", VectorRole::Position, 0.1),
            Field::range("cell", "Cell size", 0.05, 0.05, 100.),
            Field::range("radius", "Agent radius", 0.01, 0.01, 10.),
            Field::range("height", "Agent height", 0.05, 0.1, 100.),
            Field::range("climb", "Step height", 0.01, 0., 10.),
            Field::range("slope", "Maximum slope °", 1., 0., 80.),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "min" => FieldValue::Vector(self.settings.min),
            "max" => FieldValue::Vector(self.settings.max),
            "cell" => FieldValue::Number(self.settings.cell),
            "radius" => FieldValue::Number(self.settings.radius),
            "height" => FieldValue::Number(self.settings.height),
            "climb" => FieldValue::Number(self.settings.climb),
            "slope" => FieldValue::Number(self.settings.slope_degrees),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "min" => self.settings.min = value.vector()?,
            "max" => self.settings.max = value.vector()?,
            "cell" => self.settings.cell = value.number()?,
            "radius" => self.settings.radius = value.number()?,
            "height" => self.settings.height = value.number()?,
            "climb" => self.settings.climb = value.number()?,
            "slope" => self.settings.slope_degrees = value.number()?,
            _ => anyhow::bail!("unknown navigation field"),
        };
        self.baked = None;
        Ok(())
    }
}
impl Authored for NavSurface {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;

    fn validate(&self) -> Result<()> {
        self.settings.dimensions()?;
        if let Some(data) = &self.baked {
            data.validate()?;
            ensure!(
                data.settings == self.settings,
                "stale navigation bake settings"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Behavior {
    #[default]
    Idle,
    MoveTo,
    Follow,
    Flee,
    Patrol,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub name: String,
    pub behavior: Behavior,
    pub target: Option<String>,
    pub destination: [f32; 3],
    pub patrol: Vec<[f32; 3]>,
    pub speed: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            name: "Idle".into(),
            behavior: Behavior::Idle,
            target: None,
            destination: [0.; 3],
            patrol: vec![],
            speed: 1.,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    SeeTarget,
    LostTarget,
    Arrived,
    After,
    Blocked,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub condition: Condition,
    pub seconds: f32,
}
impl Default for Transition {
    fn default() -> Self {
        Self {
            from: "Idle".into(),
            to: "Idle".into(),
            condition: Condition::SeeTarget,
            seconds: 1.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NavAgent {
    pub enabled: bool,
    pub surface: String,
    pub speed: f32,
    pub acceleration: f32,
    pub radius: f32,
    pub height: f32,
    pub stopping_distance: f32,
    pub separation: f32,
    pub sight_range: f32,
    pub sight_degrees: f32,
    pub eye_height: f32,
    pub perception_target: Option<String>,
    pub repath_seconds: f32,
    pub initial: String,
    pub states: Arc<Vec<State>>,
    pub transitions: Arc<Vec<Transition>>,
}
impl Default for NavAgent {
    fn default() -> Self {
        Self {
            enabled: true,
            surface: String::new(),
            speed: 3.,
            acceleration: 12.,
            radius: 0.25,
            height: 1.8,
            stopping_distance: 0.15,
            separation: 1.,
            sight_range: 12.,
            sight_degrees: 120.,
            eye_height: 1.4,
            perception_target: None,
            repath_seconds: 0.5,
            initial: "Idle".into(),
            states: Arc::new(vec![State::default()]),
            transitions: Arc::new(vec![]),
        }
    }
}
impl Component for NavAgent {
    const NAME: &'static str = "nav_agent";
    const LABEL: &'static str = "Navigation Agent";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "The object origin is the agent's feet. Navigation owns its position: do not combine with Gravity, Player Controller or animation root motion. State actions remain data, and Blueprint nodes can set destinations/states and receive agent events.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::object("surface", "Navigation surface"),
            Field::range("speed", "Speed", 0.1, 0., 100.),
            Field::range("acceleration", "Acceleration", 0.1, 0.1, 1000.),
            Field::range("radius", "Radius", 0.01, 0.01, 10.),
            Field::range("height", "Height", 0.05, 0.1, 100.),
            Field::range("stopping_distance", "Arrival distance", 0.01, 0.01, 10.),
            Field::range("separation", "Avoid agents", 0.1, 0., 10.),
            Field::range("sight_range", "Sight range", 0.1, 0., 1000.),
            Field::range("sight_degrees", "Field of view °", 1., 1., 360.),
            Field::range("eye_height", "Eye height", 0.05, 0., 100.),
            Field::range("repath_seconds", "Repath interval", 0.05, 0.05, 60.),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "surface" => FieldValue::Object(self.surface.clone()),
            "speed" => FieldValue::Number(self.speed),
            "acceleration" => FieldValue::Number(self.acceleration),
            "radius" => FieldValue::Number(self.radius),
            "height" => FieldValue::Number(self.height),
            "stopping_distance" => FieldValue::Number(self.stopping_distance),
            "separation" => FieldValue::Number(self.separation),
            "sight_range" => FieldValue::Number(self.sight_range),
            "sight_degrees" => FieldValue::Number(self.sight_degrees),
            "eye_height" => FieldValue::Number(self.eye_height),
            "repath_seconds" => FieldValue::Number(self.repath_seconds),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = v.bool()?,
            "surface" => self.surface = v.object()?.into(),
            "speed" => self.speed = v.number()?,
            "acceleration" => self.acceleration = v.number()?,
            "radius" => self.radius = v.number()?,
            "height" => self.height = v.number()?,
            "stopping_distance" => self.stopping_distance = v.number()?,
            "separation" => self.separation = v.number()?,
            "sight_range" => self.sight_range = v.number()?,
            "sight_degrees" => self.sight_degrees = v.number()?,
            "eye_height" => self.eye_height = v.number()?,
            "repath_seconds" => self.repath_seconds = v.number()?,
            _ => anyhow::bail!("unknown agent field"),
        };
        Ok(())
    }
}
impl Authored for NavAgent {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Omit;

    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.speed,
                self.acceleration,
                self.radius,
                self.height,
                self.stopping_distance,
                self.separation,
                self.sight_range,
                self.sight_degrees,
                self.eye_height,
                self.repath_seconds
            ]
            .iter()
            .all(|v| v.is_finite()),
            "non-finite agent setting"
        );
        ensure!(
            (0.0..=100.).contains(&self.speed)
                && (0.1..=1000.).contains(&self.acceleration)
                && (0.01..=10.).contains(&self.radius)
                && (0.1..=100.).contains(&self.height)
                && (0.01..=10.).contains(&self.stopping_distance)
                && (0.0..=10.).contains(&self.separation)
                && (0.0..=1000.).contains(&self.sight_range)
                && (1.0..=360.).contains(&self.sight_degrees)
                && (0.0..=self.height).contains(&self.eye_height)
                && (0.05..=60.).contains(&self.repath_seconds),
            "invalid agent setting"
        );
        ensure!(
            !self.states.is_empty() && self.states.len() <= 64 && self.transitions.len() <= 256,
            "agent needs 1–64 states and at most 256 transitions"
        );
        let mut names = BTreeSet::new();
        for s in self.states.iter() {
            ensure!(
                !s.name.is_empty()
                    && s.name.len() <= 128
                    && names.insert(s.name.as_str())
                    && Vec3::from(s.destination).is_finite()
                    && s.patrol.len() <= 128
                    && s.patrol.iter().all(|p| Vec3::from(*p).is_finite())
                    && s.speed.is_finite()
                    && (0.0..=4.).contains(&s.speed),
                "invalid agent state"
            );
            ensure!(
                s.behavior != Behavior::Patrol || !s.patrol.is_empty(),
                "patrol state needs waypoints"
            );
        }
        ensure!(
            names.contains(self.initial.as_str()),
            "agent initial state missing"
        );
        for t in self.transitions.iter() {
            ensure!(
                (t.from == "*" || names.contains(t.from.as_str()))
                    && names.contains(t.to.as_str())
                    && t.seconds.is_finite()
                    && (0.0..=86400.).contains(&t.seconds),
                "invalid agent transition"
            );
        }
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            owner.gravity.is_none() && owner.player_controller.is_none(),
            "navigation agent cannot also be a rigidbody/player controller"
        );
        let mut parent = owner.parent.as_deref();
        let mut visited = BTreeSet::new();
        while let Some(id) = parent {
            ensure!(
                visited.insert(id),
                "scene transform hierarchy contains a cycle"
            );
            let ancestor = scene.objects.iter().find(|o| o.id == id);
            ensure!(
                ancestor.is_none_or(|o| !o.extras.contains_key(Self::NAME)),
                "navigation agents cannot be parented under another agent"
            );
            parent = ancestor.and_then(|o| o.parent.as_deref());
        }
        if let Some(animator) = registry::get::<super::animation::Animator>(owner)? {
            ensure!(
                animator.root_motion.is_none(),
                "navigation and animation root motion cannot both own movement"
            );
        }
        if !self.surface.is_empty() {
            ensure!(
                scene
                    .objects
                    .iter()
                    .any(|o| o.id == self.surface && o.extras.contains_key(NavSurface::NAME)),
                "agent navigation surface missing"
            );
        }
        for target in self
            .perception_target
            .iter()
            .chain(self.states.iter().filter_map(|s| s.target.as_ref()))
        {
            ensure!(
                ids.contains(target.as_str()) && target != &owner.id,
                "agent target missing or points to itself"
            );
        }
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 256,
            "scene exceeds 256 navigation agents"
        );
        Ok(())
    }
    fn remap_objects(&mut self, m: &BTreeMap<String, String>) {
        if let Some(id) = m.get(&self.surface) {
            self.surface = id.clone();
        }
        for target in self.perception_target.iter_mut().chain(
            Arc::make_mut(&mut self.states)
                .iter_mut()
                .filter_map(|s| s.target.as_mut()),
        ) {
            if let Some(id) = m.get(target) {
                *target = id.clone();
            }
        }
    }
    fn write_targets(&self, owner: &str) -> Vec<String> {
        vec![owner.into()]
    }
}
