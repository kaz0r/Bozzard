//! Authored motion tracks with shared curve evaluation and explicit playback controls.
use super::{
    curve::{Curve, Ease, Playhead, Repeat},
    registry::{Authored, get},
};
use crate::{
    Component, Field, FieldValue, Object, Scene, SceneInstance, Transform, Ui, World,
    blueprint::ObjectRef,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    #[default]
    Translation,
    Rotation,
    Scale,
    Color,
    Metallic,
    Roughness,
    LightIntensity,
    TextOpacity,
}
impl Property {
    pub const ALL: [Self; 8] = [
        Self::Translation,
        Self::Rotation,
        Self::Scale,
        Self::Color,
        Self::Metallic,
        Self::Roughness,
        Self::LightIntensity,
        Self::TextOpacity,
    ];
    pub fn channels(self) -> usize {
        if matches!(
            self,
            Self::Translation | Self::Rotation | Self::Scale | Self::Color
        ) {
            3
        } else {
            1
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Translation => "Translation",
            Self::Rotation => "Rotation (degrees)",
            Self::Scale => "Scale",
            Self::Color => "Color",
            Self::Metallic => "Metallic",
            Self::Roughness => "Roughness",
            Self::LightIntensity => "Light intensity",
            Self::TextOpacity => "Text opacity",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub target: ObjectRef,
    pub property: Property,
    pub channels: Vec<Curve>,
}
impl Track {
    pub fn new(property: Property) -> Self {
        Self {
            target: ObjectRef::SelfObject,
            property,
            channels: vec![
                Curve::constant(
                    if matches!(
                        property,
                        Property::Scale | Property::Color | Property::TextOpacity
                    ) {
                        1.
                    } else {
                        0.
                    }
                );
                property.channels()
            ],
        }
    }
    pub fn validate(&self, duration: f32) -> Result<()> {
        ensure!(
            self.channels.len() == self.property.channels(),
            "wrong channel count for {}",
            self.property.name()
        );
        ensure!(
            !matches!(self.target, ObjectRef::None),
            "motion track needs a target"
        );
        for curve in &self.channels {
            curve.validate()?;
            ensure!(
                curve.duration() <= duration,
                "motion key exceeds the clip duration"
            );
        }
        Ok(())
    }
    pub fn target<'a>(&'a self, owner: &'a str) -> &'a str {
        match &self.target {
            ObjectRef::Id(id) => id,
            _ => owner,
        }
    }
    pub fn validate_scene(&self, owner: &Object, scene: &Scene) -> Result<()> {
        let target = scene
            .objects
            .iter()
            .find(|o| o.id == self.target(&owner.id))
            .context("motion target does not exist")?;
        ensure!(
            match self.property {
                Property::Translation | Property::Rotation | Property::Scale => true,
                Property::Color => target.drawable.is_some() || target.text_rendering.is_some(),
                Property::Metallic | Property::Roughness => target.drawable.is_some(),
                Property::LightIntensity => target.light.is_some(),
                Property::TextOpacity => target.text_rendering.is_some(),
            },
            "{} needs a matching component on '{}'",
            self.property.name(),
            target.id
        );
        Ok(())
    }
    pub fn sample(&self, time: f32) -> [f32; 3] {
        let mut result = [0.; 3];
        for (out, curve) in result.iter_mut().zip(&self.channels) {
            *out = curve.sample(time);
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tween {
    pub enabled: bool,
    pub autoplay: bool,
    pub duration: f32,
    pub speed: f32,
    pub repeat: Repeat,
    pub ease: Ease,
    pub tracks: Arc<Vec<Track>>,
}
impl Default for Tween {
    fn default() -> Self {
        Self {
            enabled: true,
            autoplay: false,
            duration: 1.,
            speed: 1.,
            repeat: Repeat::Once,
            ease: Ease::Linear,
            tracks: Arc::default(),
        }
    }
}
impl Component for Tween {
    const NAME: &'static str = "tween";
    const LABEL: &'static str = "Tween";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Author motion tracks and keys in the Motion editor. Blueprints control play, pause, stop and seek.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::bool("autoplay", "Play on start"),
            Field::range("duration", "Duration (s)", 0.05, 0.001, 86400.),
            Field::range("speed", "Speed", 0.05, 0., 100.),
            Field::options("repeat", "Repeat", &["Once", "Loop", "Ping-pong"]),
            Field::options(
                "ease",
                "Ease",
                &[
                    "Linear",
                    "Smooth",
                    "Smoother",
                    "In Quad",
                    "Out Quad",
                    "In Out Quad",
                    "In Cubic",
                    "Out Cubic",
                    "In Out Cubic",
                    "In Sine",
                    "Out Sine",
                    "In Out Sine",
                ],
            ),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "autoplay" => FieldValue::Bool(self.autoplay),
            "duration" => FieldValue::Number(self.duration),
            "speed" => FieldValue::Number(self.speed),
            "repeat" => FieldValue::Index(self.repeat as usize),
            "ease" => FieldValue::Index(self.ease as usize),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "autoplay" => self.autoplay = value.bool()?,
            "duration" => self.duration = value.number()?,
            "speed" => self.speed = value.number()?,
            "repeat" => {
                self.repeat = *[Repeat::Once, Repeat::Loop, Repeat::PingPong]
                    .get(value.index()?)
                    .context("invalid repeat mode")?
            }
            "ease" => {
                self.ease = *[
                    Ease::Linear,
                    Ease::Smooth,
                    Ease::Smoother,
                    Ease::InQuad,
                    Ease::OutQuad,
                    Ease::InOutQuad,
                    Ease::InCubic,
                    Ease::OutCubic,
                    Ease::InOutCubic,
                    Ease::InSine,
                    Ease::OutSine,
                    Ease::InOutSine,
                ]
                .get(value.index()?)
                .context("invalid easing")?
            }
            _ => anyhow::bail!("unknown tween field"),
        };
        Ok(())
    }
}
impl Authored for Tween {
    fn write_targets(&self, owner: &str) -> Vec<String> {
        self.tracks
            .iter()
            .map(|t| t.target(owner).to_owned())
            .collect()
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.duration.is_finite() && (0.001..=86400.).contains(&self.duration),
            "tween duration must be within 0.001–86400 seconds"
        );
        ensure!(
            self.speed.is_finite() && (0.0..=100.).contains(&self.speed),
            "invalid tween speed"
        );
        ensure!(self.tracks.len() <= 128, "at most 128 motion tracks");
        for track in self.tracks.iter() {
            track.validate(self.duration)?;
        }
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        for track in self.tracks.iter() {
            track.validate_scene(owner, scene)?;
        }
        Ok(())
    }
    fn remap_objects(&mut self, mapping: &BTreeMap<String, String>) {
        for track in Arc::make_mut(&mut self.tracks) {
            if let ObjectRef::Id(id) = &mut track.target
                && let Some(new) = mapping.get(id)
            {
                *id = new.clone();
            }
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub clock: Playhead,
    pub initialized: bool,
    pub last_position: Option<f32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub players: BTreeMap<String, State>,
    #[serde(skip)]
    pub finished: BTreeSet<String>,
}
#[derive(Clone, Copy, Debug)]
pub enum Control {
    Play { restart: bool },
    Pause,
    Stop,
    Seek(f32),
}
impl SceneInstance {
    pub fn control_tween(&self, world: &mut World, owner: &str, control: Control) -> Result<()> {
        let entity = self.entity(owner).context("tween target is missing")?;
        let tween = world.get::<Tween>(entity).context("target has no Tween")?;
        let duration = tween.duration;
        if let Control::Seek(time) = control {
            ensure!(
                time.is_finite() && (0.0..=duration).contains(&time),
                "seek time outside tween duration"
            );
        }
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let state = world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .entry(owner.into())
            .or_default();
        state.initialized = true;
        match control {
            Control::Play { restart } => {
                state.clock.play(restart);
                if restart {
                    state.last_position = None;
                }
            }
            Control::Pause => state.clock.playing = false,
            Control::Stop => {
                state.clock = Playhead::default();
                state.last_position = None;
            }
            Control::Seek(time) => {
                state.clock.seek(f64::from(time))?;
                state.last_position = None;
            }
        }
        Ok(())
    }
    pub fn tween_progress(&self, world: &World, owner: &str) -> Result<f32> {
        let entity = self.entity(owner).context("tween target is missing")?;
        let tween = world.get::<Tween>(entity).context("target has no Tween")?;
        Ok(world
            .resource::<Runtime>()
            .and_then(|r| r.players.get(owner))
            .map_or(0., |s| {
                s.clock.position(tween.duration, tween.repeat) / tween.duration
            }))
    }
    pub fn step_tweens(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid tween timestep");
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        runtime.finished.clear();
        let result = (|| -> Result<()> {
            for (owner, entity) in &self.entities {
                let Some(tween) = world.get::<Tween>(*entity).cloned() else {
                    continue;
                };
                let state = runtime.players.entry(owner.clone()).or_default();
                if !state.initialized {
                    state.clock.playing = tween.autoplay;
                    state.initialized = true;
                    if !tween.autoplay {
                        state.last_position = Some(0.);
                    }
                }
                if !tween.enabled {
                    continue;
                }
                let completed = state.clock.completed;
                state
                    .clock
                    .advance(dt, tween.speed, tween.duration, tween.repeat)?;
                let position = state.clock.position(tween.duration, tween.repeat);
                if state.last_position != Some(position)
                    && (state.clock.playing
                        || state.clock.completed
                        || state.last_position.is_none())
                {
                    let time = tween.ease.sample(position / tween.duration) * tween.duration;
                    self.apply_motion(world, owner, &tween.tracks, time)?;
                    state.last_position = Some(position);
                }
                if !completed && state.clock.completed {
                    runtime.finished.insert(owner.clone());
                }
            }
            runtime.players.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Tween>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        result
    }
    pub(crate) fn apply_motion(
        &self,
        world: &mut World,
        owner: &str,
        tracks: &[Track],
        time: f32,
    ) -> Result<()> {
        let mut writes = Vec::with_capacity(tracks.len());
        for track in tracks {
            let entity = self
                .entity(track.target(owner))
                .context("motion target was removed")?;
            let value = track.sample(time);
            ensure!(
                value.iter().all(|v| v.is_finite()),
                "curve evaluation overflow"
            );
            match track.property {
                Property::Scale => ensure!(
                    value.iter().all(|v| v.abs() >= 0.0001),
                    "motion scale cannot be zero"
                ),
                Property::Color => ensure!(
                    value.iter().all(|v| (0.0..=1.).contains(v)),
                    "motion color must be within 0–1"
                ),
                Property::Metallic | Property::Roughness | Property::TextOpacity => ensure!(
                    (0.0..=1.).contains(&value[0]),
                    "motion material value must be within 0–1"
                ),
                Property::LightIntensity => ensure!(
                    (0.0..=100000.).contains(&value[0]),
                    "motion light intensity outside range"
                ),
                _ => {}
            }
            writes.push((entity, track, value));
        }
        for (entity, track, value) in writes {
            match track.property {
                Property::Translation | Property::Rotation | Property::Scale => {
                    let mut transform = *world
                        .get::<Transform>(entity)
                        .context("motion transform missing")?;
                    match track.property {
                        Property::Translation => transform.translation = value,
                        Property::Rotation => transform.rotation_degrees = value,
                        _ => transform.scale = value,
                    }
                    transform.validate()?;
                    world.insert(entity, transform)?;
                }
                Property::Color => {
                    if let Some(mut text) = world.get_mut::<crate::TextRendering>(entity) {
                        text.color[..3].copy_from_slice(&value);
                    }
                    if let Some(mut material) = world.get_mut::<crate::Material>(entity) {
                        material.color = value;
                    } else if let Some(mut drawable) = world.get_mut::<crate::Drawable>(entity) {
                        drawable.color = value;
                    }
                }
                Property::Metallic | Property::Roughness => {
                    if let Some(mut material) = world.get_mut::<crate::Material>(entity) {
                        if track.property == Property::Metallic {
                            material.metallic = Some(value[0]);
                        } else {
                            material.roughness = Some(value[0]);
                        }
                    } else {
                        let mut drawable = world
                            .get_mut::<crate::Drawable>(entity)
                            .context("motion mesh missing")?;
                        if track.property == Property::Metallic {
                            drawable.metallic = Some(value[0]);
                        } else {
                            drawable.roughness = Some(value[0]);
                        }
                    }
                }
                Property::LightIntensity => {
                    world
                        .get_mut::<crate::Light>(entity)
                        .context("motion light missing")?
                        .intensity = value[0]
                }
                Property::TextOpacity => {
                    world
                        .get_mut::<crate::TextRendering>(entity)
                        .context("motion text missing")?
                        .color[3] = value[0]
                }
            }
        }
        Ok(())
    }
}
/// Read a typed authored clip for editor timeline tools.
pub fn authored(object: &Object) -> Result<Option<Tween>> {
    get(object)
}
