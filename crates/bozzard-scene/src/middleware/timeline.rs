//! Cinematic motion, camera cuts and named markers on a deterministic playback clock.
use super::{
    curve::{Playhead, Repeat},
    registry::{Authored, PreviewPolicy},
    signals::{Kind, Signal, Signals},
    tween::{Control, Tween},
};
use crate::{Component, Field, FieldValue, Layer, Object, Scene, SceneInstance, Ui, World};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub time: f32,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraCut {
    pub time: f32,
    pub camera: String,
    pub layer: Layer,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timeline {
    pub motion: Tween,
    pub markers: Arc<Vec<Marker>>,
    pub cameras: Arc<Vec<CameraCut>>,
}
impl Component for Timeline {
    const NAME: &'static str = "timeline";
    const LABEL: &'static str = "Timeline";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Motion tracks, camera cuts and named Blueprint events. Seeking previews motion and cameras without firing markers. Stop releases the camera.";
    fn fields() -> &'static [Field] {
        Tween::fields()
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        self.motion.field(key)
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        self.motion.set_field(key, value)
    }
}
impl Authored for Timeline {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Omit;

    fn write_targets(&self, owner: &str) -> Vec<String> {
        self.motion.write_targets(owner)
    }
    fn validate(&self) -> Result<()> {
        self.motion.validate()?;
        ensure!(
            self.markers.len() <= 1024 && self.cameras.len() <= 1024,
            "timeline supports at most 1024 markers and camera cuts"
        );
        for marker in self.markers.iter() {
            ensure!(
                marker.time.is_finite() && (0.0..=self.motion.duration).contains(&marker.time),
                "marker outside timeline"
            );
            ensure!(
                !marker.name.trim().is_empty() && marker.name.len() <= 256,
                "marker name must contain 1–256 bytes"
            );
        }
        ensure!(
            self.markers.windows(2).all(|w| w[0].time <= w[1].time),
            "timeline markers must be ordered by time"
        );
        for cut in self.cameras.iter() {
            ensure!(
                cut.time.is_finite()
                    && (0.0..=self.motion.duration).contains(&cut.time)
                    && !cut.camera.is_empty(),
                "invalid camera cut"
            );
        }
        ensure!(
            self.cameras.windows(2).all(|w| w[0].time <= w[1].time),
            "camera cuts must be ordered by time"
        );
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        self.motion.validate_scene(owner, scene, ids)?;
        for cut in self.cameras.iter() {
            ensure!(
                scene.views.contains_key(&cut.layer),
                "camera cut requires an authored view"
            );
            ensure!(
                scene
                    .objects
                    .iter()
                    .any(|o| o.id == cut.camera && o.camera.is_some()),
                "timeline camera is missing"
            );
        }
        Ok(())
    }
    fn remap_objects(&mut self, mapping: &BTreeMap<String, String>) {
        self.motion.remap_objects(mapping);
        for cut in Arc::make_mut(&mut self.cameras) {
            if let Some(id) = mapping.get(&cut.camera) {
                cut.camera = id.clone();
            }
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub clock: Playhead,
    pub initialized: bool,
    pub active: bool,
    pub last_position: Option<f32>,
    pub include_start: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub players: BTreeMap<String, State>,
    pub cameras: BTreeMap<Layer, String>,
}
/// Enumerate crossed marker indices in playback order, bounded before allocating or iterating
/// cycles. Half-open (previous, next] intervals avoid duplicate boundary events across ticks.
pub fn crossed_markers(
    times: impl IntoIterator<Item = impl std::borrow::Borrow<f32>>,
    previous: f64,
    next: f64,
    duration: f32,
    repeat: Repeat,
    include_start: bool,
) -> Result<Vec<usize>> {
    ensure!(
        previous.is_finite()
            && next.is_finite()
            && previous >= 0.
            && next >= previous
            && duration.is_finite()
            && duration > 0.,
        "invalid marker interval"
    );
    let mut hits = Vec::new();
    let d = f64::from(duration);
    let start = if include_start && previous == 0. {
        -f64::EPSILON
    } else {
        previous
    };
    for (index, time) in times.into_iter().enumerate() {
        let time = f64::from(*std::borrow::Borrow::borrow(&time));
        ensure!((0.0..=d).contains(&time), "marker outside clip");
        let period = match repeat {
            Repeat::Once => 0.,
            Repeat::Loop => d,
            Repeat::PingPong => 2. * d,
        };
        let phases = [time, 2. * d - time];
        let count = if repeat == Repeat::PingPong && time > 0. && time < d {
            2
        } else {
            1
        };
        for &phase in &phases[..count] {
            if period == 0. {
                if phase > start && phase <= next {
                    hits.push((phase, index));
                }
            } else {
                let first = ((start - phase) / period).floor().max(-1.) + 1.;
                let last = ((next - phase) / period).floor();
                let count = (last - first + 1.).max(0.);
                ensure!(
                    count <= (Signals::LIMIT - hits.len()) as f64,
                    "timeline event budget exceeded; reduce timestep, speed or marker density"
                );
                for offset in 0..count as usize {
                    hits.push((phase + (first + offset as f64) * period, index));
                }
            }
            ensure!(
                hits.len() <= Signals::LIMIT,
                "timeline event budget exceeded"
            );
        }
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(hits.into_iter().map(|(_, index)| index).collect())
}
impl SceneInstance {
    pub fn control_timeline(&self, world: &mut World, owner: &str, control: Control) -> Result<()> {
        let entity = self.entity(owner).context("timeline target is missing")?;
        let timeline = world
            .get::<Timeline>(entity)
            .context("target has no Timeline")?;
        if let Control::Seek(time) = control {
            ensure!(
                time.is_finite() && (0.0..=timeline.motion.duration).contains(&time),
                "seek outside timeline"
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
                if restart || state.clock.completed || !state.active {
                    state.include_start = true;
                    state.last_position = None;
                }
                state.clock.play(restart);
                state.active = true;
            }
            Control::Pause => state.clock.playing = false,
            Control::Stop => {
                *state = State {
                    initialized: true,
                    ..Default::default()
                };
            }
            Control::Seek(time) => {
                state.clock.seek(f64::from(time))?;
                state.last_position = None;
                state.include_start = false;
                state.active = true;
            }
        }
        Ok(())
    }
    pub fn step_timelines(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid timeline timestep");
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let mut signals = world.remove_resource::<Signals>().unwrap_or_default();
        signals.begin(Kind::Timeline);
        runtime.cameras.clear();
        let result = (|| -> Result<()> {
            for (owner, &entity) in self.component_entities::<Timeline>(world) {
                let Some(timeline) = world.get::<Timeline>(entity).cloned() else {
                    continue;
                };
                let state = runtime.players.entry(owner.clone()).or_default();
                if !state.initialized {
                    state.initialized = true;
                    state.active = timeline.motion.autoplay;
                    state.clock.playing = state.active;
                    state.include_start = state.active;
                }
                if !timeline.motion.enabled || !state.active {
                    continue;
                }
                let previous = state.clock.elapsed;
                let was_playing = state.clock.playing;
                let mut clock = state.clock;
                clock.advance(
                    dt,
                    timeline.motion.speed,
                    timeline.motion.duration,
                    timeline.motion.repeat,
                )?;
                let hits = if was_playing && (clock.elapsed > previous || state.include_start) {
                    crossed_markers(
                        timeline.markers.iter().map(|m| m.time),
                        previous,
                        clock.elapsed,
                        timeline.motion.duration,
                        timeline.motion.repeat,
                        state.include_start,
                    )?
                } else {
                    Vec::new()
                };
                let position = clock.position(timeline.motion.duration, timeline.motion.repeat);
                if state.last_position != Some(position) {
                    self.apply_motion(
                        world,
                        owner,
                        &timeline.motion.tracks,
                        timeline
                            .motion
                            .ease
                            .sample(position / timeline.motion.duration)
                            * timeline.motion.duration,
                    )?;
                }
                for index in hits {
                    let marker = &timeline.markers[index];
                    signals.emit(
                        owner,
                        Signal {
                            kind: Kind::Timeline,
                            name: marker.name.clone(),
                            other: None,
                            value: marker.time,
                        },
                    )?;
                }
                state.clock = clock;
                state.include_start = false;
                state.last_position = Some(position);
                for cut in timeline
                    .cameras
                    .iter()
                    .take_while(|cut| cut.time <= position)
                {
                    ensure!(
                        self.entity(&cut.camera)
                            .is_some_and(|e| world.get::<crate::Camera>(e).is_some()),
                        "timeline camera was removed"
                    );
                    runtime.cameras.insert(cut.layer, cut.camera.clone());
                }
            }
            runtime.players.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Timeline>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        world.insert_resource(signals);
        result
    }
}
