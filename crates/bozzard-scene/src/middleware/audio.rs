//! Deterministic audio transport and spatial mixing. Native device handles never enter the ECS.
use super::registry::Authored;
use crate::{
    AssetKind, Component, Field, FieldValue, Layer, Object, Scene, SceneInstance, Ui, World,
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bus {
    #[default]
    Sfx,
    Music,
    Ui,
    Ambience,
}
impl Bus {
    pub const ALL: [Self; 4] = [Self::Sfx, Self::Music, Self::Ui, Self::Ambience];
    pub fn name(self) -> &'static str {
        match self {
            Self::Sfx => "Sfx",
            Self::Music => "Music",
            Self::Ui => "Ui",
            Self::Ambience => "Ambience",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|b| b.name().eq_ignore_ascii_case(name))
            .context("unknown audio bus")
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioSource {
    pub enabled: bool,
    pub asset: String,
    pub autoplay: bool,
    pub looping: bool,
    pub streaming: bool,
    pub spatial: bool,
    pub pause_with_game: bool,
    pub bus: Bus,
    pub volume: f32,
    pub pitch: f32,
    pub pan: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
    /// Baked from the audio asset so headless playback emits the same completion event.
    pub duration: f64,
}
impl Default for AudioSource {
    fn default() -> Self {
        Self {
            enabled: true,
            asset: String::new(),
            autoplay: false,
            looping: false,
            streaming: false,
            spatial: true,
            pause_with_game: true,
            bus: Bus::Sfx,
            volume: 1.,
            pitch: 1.,
            pan: 0.,
            min_distance: 1.,
            max_distance: 40.,
            rolloff: 1.,
            duration: 1.,
        }
    }
}
impl Component for AudioSource {
    const NAME: &'static str = "audio_source";
    const LABEL: &'static str = "Audio Source";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "WAV, OGG/Vorbis, MP3 or FLAC. Stream long music; cache short effects. Blueprint Play/Stop/Seek and volume/pitch/pan nodes control this source. Spatial sound follows the active camera or Audio Listener.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::asset("asset", "Clip", AssetKind::Audio),
            Field::bool("autoplay", "Play on start"),
            Field::bool("looping", "Loop"),
            Field::bool("streaming", "Stream from disk"),
            Field::bool("spatial", "3D spatial sound"),
            Field::bool("pause_with_game", "Pause with game"),
            Field::options("bus", "Bus", &["Sfx", "Music", "Ui", "Ambience"]),
            Field::range("volume", "Volume", 0.01, 0., 4.),
            Field::range("pitch", "Playback pitch", 0.01, 0.125, 4.),
            Field::range("pan", "Pan", 0.01, -1., 1.),
            Field::range("min_distance", "Full volume distance", 0.1, 0., 100000.),
            Field::range("max_distance", "Silent distance", 0.1, 0.001, 100000.),
            Field::range("rolloff", "Attenuation curve", 0.05, 0.1, 8.),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "asset" => FieldValue::Text(self.asset.clone()),
            "autoplay" => FieldValue::Bool(self.autoplay),
            "looping" => FieldValue::Bool(self.looping),
            "streaming" => FieldValue::Bool(self.streaming),
            "spatial" => FieldValue::Bool(self.spatial),
            "pause_with_game" => FieldValue::Bool(self.pause_with_game),
            "bus" => FieldValue::Index(self.bus as usize),
            "volume" => FieldValue::Number(self.volume),
            "pitch" => FieldValue::Number(self.pitch),
            "pan" => FieldValue::Number(self.pan),
            "min_distance" => FieldValue::Number(self.min_distance),
            "max_distance" => FieldValue::Number(self.max_distance),
            "rolloff" => FieldValue::Number(self.rolloff),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "asset" => self.asset = value.text()?.into(),
            "autoplay" => self.autoplay = value.bool()?,
            "looping" => self.looping = value.bool()?,
            "streaming" => self.streaming = value.bool()?,
            "spatial" => self.spatial = value.bool()?,
            "pause_with_game" => self.pause_with_game = value.bool()?,
            "bus" => self.bus = *Bus::ALL.get(value.index()?).context("invalid audio bus")?,
            "volume" => self.volume = value.number()?,
            "pitch" => self.pitch = value.number()?,
            "pan" => self.pan = value.number()?,
            "min_distance" => self.min_distance = value.number()?,
            "max_distance" => self.max_distance = value.number()?,
            "rolloff" => self.rolloff = value.number()?,
            _ => anyhow::bail!("unknown audio source field"),
        };
        Ok(())
    }
}
impl Authored for AudioSource {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.volume,
                self.pitch,
                self.pan,
                self.min_distance,
                self.max_distance,
                self.rolloff
            ]
            .iter()
            .all(|v| v.is_finite()),
            "non-finite audio source parameter"
        );
        ensure!(
            (0.0..=4.).contains(&self.volume)
                && (0.125..=4.).contains(&self.pitch)
                && (-1.0..=1.).contains(&self.pan),
            "invalid audio volume, pitch or pan"
        );
        ensure!(
            self.min_distance >= 0.
                && self.max_distance > self.min_distance
                && self.max_distance <= 100000.
                && (0.1..=8.).contains(&self.rolloff),
            "invalid spatial audio distances or attenuation"
        );
        ensure!(
            self.duration.is_finite() && self.duration > 0. && self.duration <= 86400.,
            "audio duration must be within one day"
        );
        Ok(())
    }
    fn validate_scene(&self, _owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 256,
            "scene exceeds 256 audio sources"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioMixer {
    pub master: f32,
    pub buses: [f32; 4],
    pub muted: bool,
}
impl Default for AudioMixer {
    fn default() -> Self {
        Self {
            master: 1.,
            buses: [1.; 4],
            muted: false,
        }
    }
}
impl Component for AudioMixer {
    const NAME: &'static str = "audio_mixer";
    const LABEL: &'static str = "Audio Mixer";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "One mixer per scene. Master volume applies to all buses; Blueprint Set Audio Bus Volume adjusts Sfx, Music, Ui or Ambience.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::range("master", "Master", 0.01, 0., 4.),
            Field::bool("muted", "Mute all"),
            Field::range("sfx", "Sfx", 0.01, 0., 4.),
            Field::range("music", "Music", 0.01, 0., 4.),
            Field::range("ui", "Ui", 0.01, 0., 4.),
            Field::range("ambience", "Ambience", 0.01, 0., 4.),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "master" => FieldValue::Number(self.master),
            "muted" => FieldValue::Bool(self.muted),
            _ => FieldValue::Number(self.buses[Bus::parse(key).ok()? as usize]),
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "master" => self.master = value.number()?,
            "muted" => self.muted = value.bool()?,
            _ => self.buses[Bus::parse(key)? as usize] = value.number()?,
        };
        Ok(())
    }
}
impl Authored for AudioMixer {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.buses
                .iter()
                .chain([&self.master])
                .all(|v| v.is_finite() && (0.0..=4.).contains(v)),
            "invalid mixer volume"
        );
        Ok(())
    }
    fn validate_scene(&self, _owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 1,
            "scene has more than one Audio Mixer"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioListener {
    pub enabled: bool,
}
impl Default for AudioListener {
    fn default() -> Self {
        Self { enabled: true }
    }
}
impl Component for AudioListener {
    const NAME: &'static str = "audio_listener";
    const LABEL: &'static str = "Audio Listener";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Overrides the camera as the 3D sound listener. Local +X points right, local -Z points forward.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[Field::bool("enabled", "Enabled")];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        (key == "enabled").then_some(FieldValue::Bool(self.enabled))
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        ensure!(key == "enabled", "unknown listener field");
        self.enabled = value.bool()?;
        Ok(())
    }
}
impl Authored for AudioListener {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
    fn validate_scene(&self, _owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        let mut active = 0;
        for object in &scene.objects {
            if super::registry::get::<Self>(object)?.is_some_and(|l| l.enabled) {
                active += 1;
            }
        }
        ensure!(
            active <= 1,
            "scene has more than one enabled Audio Listener"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    Stopped,
    Playing,
    Paused,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Voice {
    pub initialized: bool,
    pub transport: Transport,
    pub position: f64,
    pub epoch: u64,
    pub volume: f32,
    pub pitch: f32,
    pub pan: f32,
}
impl Voice {
    fn initialize(&mut self, source: &AudioSource) {
        if !self.initialized {
            self.initialized = true;
            self.transport = if source.autoplay && !source.asset.is_empty() {
                Transport::Playing
            } else {
                Transport::Stopped
            };
            self.volume = source.volume;
            self.pitch = source.pitch;
            self.pan = source.pan;
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub voices: BTreeMap<String, Voice>,
    pub buses: [Option<f32>; 4],
    #[serde(skip)]
    pub finished: BTreeSet<String>,
}
#[derive(Clone, Copy, Debug)]
pub enum Control {
    Play { restart: bool },
    Pause,
    Stop,
    Seek(f64),
    Volume(f32),
    Pitch(f32),
    Pan(f32),
}
#[derive(Clone, Debug)]
pub struct Playback {
    pub id: u64,
    pub owner: String,
    pub asset: String,
    pub bus: Bus,
    pub transport: Transport,
    pub position: f64,
    pub epoch: u64,
    pub looping: bool,
    pub streaming: bool,
    pub volume: f32,
    pub pitch: f32,
    pub pan: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub sources: Vec<Playback>,
    pub master: f32,
    pub buses: [f32; 4],
}
/// Distance attenuation and listener-relative pan are shared with headless tests.
pub fn spatial_mix(source: &AudioSource, position: Vec3, listener: Mat4) -> (f32, f32) {
    if !source.spatial {
        return (1., source.pan);
    }
    let relative = position - listener.w_axis.truncate();
    let distance = relative.length();
    let attenuation = ((source.max_distance - distance)
        / (source.max_distance - source.min_distance))
        .clamp(0., 1.)
        .powf(source.rolloff);
    let right = listener.x_axis.truncate().normalize_or_zero();
    let pan = (relative.normalize_or_zero().dot(right) + source.pan).clamp(-1., 1.);
    (attenuation, pan)
}
impl SceneInstance {
    pub fn control_audio(&self, world: &mut World, owner: &str, control: Control) -> Result<()> {
        let entity = self.entity(owner).context("audio target is missing")?;
        let source = world
            .get::<AudioSource>(entity)
            .context("target has no Audio Source")?
            .clone();
        match control {
            Control::Seek(time) => ensure!(
                time.is_finite() && (0.0..=source.duration).contains(&time),
                "audio seek outside clip"
            ),
            Control::Volume(v) => ensure!(
                v.is_finite() && (0.0..=4.).contains(&v),
                "audio volume outside 0–4"
            ),
            Control::Pitch(v) => ensure!(
                v.is_finite() && (0.125..=4.).contains(&v),
                "audio pitch outside 0.125–4"
            ),
            Control::Pan(v) => ensure!(
                v.is_finite() && (-1.0..=1.).contains(&v),
                "audio pan outside -1–1"
            ),
            Control::Play { .. } => ensure!(!source.asset.is_empty(), "audio source has no clip"),
            _ => {}
        }
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let voice = world
            .resource_mut::<Runtime>()
            .unwrap()
            .voices
            .entry(owner.into())
            .or_default();
        voice.initialize(&source);
        match control {
            Control::Play { restart } => {
                if restart || voice.transport == Transport::Stopped {
                    voice.position = 0.;
                    voice.epoch = voice.epoch.wrapping_add(1);
                }
                voice.transport = Transport::Playing;
            }
            Control::Pause => {
                if voice.transport == Transport::Playing {
                    voice.transport = Transport::Paused;
                }
            }
            Control::Stop => {
                voice.transport = Transport::Stopped;
                voice.position = 0.;
                voice.epoch = voice.epoch.wrapping_add(1);
            }
            Control::Seek(time) => {
                voice.position = time;
                voice.epoch = voice.epoch.wrapping_add(1);
            }
            Control::Volume(v) => voice.volume = v,
            Control::Pitch(v) => voice.pitch = v,
            Control::Pan(v) => voice.pan = v,
        }
        Ok(())
    }
    pub fn set_audio_bus_volume(&self, world: &mut World, bus: &str, volume: f32) -> Result<()> {
        let bus = Bus::parse(bus)?;
        ensure!(
            volume.is_finite() && (0.0..=4.).contains(&volume),
            "audio bus volume outside 0–4"
        );
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        world.resource_mut::<Runtime>().unwrap().buses[bus as usize] = Some(volume);
        Ok(())
    }
    /// Called even while gameplay is paused, so UI/music may opt out of the pause.
    pub fn step_audio(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid audio timestep");
        let running = crate::game_flow::simulation_running(world);
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        runtime.finished.clear();
        for (owner, &entity) in &self.entities {
            let Some(source) = world.get::<AudioSource>(entity) else {
                continue;
            };
            let voice = runtime.voices.entry(owner.clone()).or_default();
            voice.initialize(source);
            if source.enabled
                && voice.transport == Transport::Playing
                && (running || !source.pause_with_game)
            {
                voice.position += f64::from(dt) * f64::from(voice.pitch);
                if voice.position >= source.duration {
                    if source.looping {
                        voice.position = voice.position.rem_euclid(source.duration);
                    } else {
                        voice.position = source.duration;
                        voice.transport = Transport::Stopped;
                        runtime.finished.insert(owner.clone());
                    }
                }
            }
        }
        runtime.voices.retain(|id, _| {
            self.entity(id)
                .is_some_and(|e| world.get::<AudioSource>(e).is_some())
        });
        world.insert_resource(runtime);
        Ok(())
    }
    pub fn audio_frame(&self, world: &World, layer: Layer) -> Result<Frame> {
        let matrices = self.global_transforms(world)?;
        let running = crate::game_flow::simulation_running(world);
        let listener = self
            .entities
            .iter()
            .find(|(_, e)| world.get::<AudioListener>(**e).is_some_and(|l| l.enabled))
            .map(|(id, _)| id)
            .or_else(|| {
                world
                    .resource::<super::timeline::Runtime>()
                    .and_then(|r| r.cameras.get(&layer))
            })
            .or_else(|| self.document.views.get(&layer));
        let listener = listener
            .and_then(|id| matrices.get(id))
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        let mixer = self
            .entities
            .values()
            .find_map(|e| world.get::<AudioMixer>(*e))
            .cloned()
            .unwrap_or_default();
        let runtime = world.resource::<Runtime>();
        let mut frame = Frame {
            master: if mixer.muted { 0. } else { mixer.master },
            buses: std::array::from_fn(|i| {
                runtime.and_then(|r| r.buses[i]).unwrap_or(mixer.buses[i])
            }),
            sources: Vec::new(),
        };
        for (owner, &entity) in &self.entities {
            let Some(source) = world.get::<AudioSource>(entity) else {
                continue;
            };
            if source.asset.is_empty() {
                continue;
            }
            let mut initial = Voice::default();
            initial.initialize(source);
            let voice = runtime
                .and_then(|r| r.voices.get(owner))
                .unwrap_or(&initial);
            let (attenuation, spatial_pan) =
                spatial_mix(source, matrices[owner].w_axis.truncate(), listener);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            entity.hash(&mut hasher);
            frame.sources.push(Playback {
                id: hasher.finish().max(1),
                owner: owner.clone(),
                asset: source.asset.clone(),
                bus: source.bus,
                transport: if voice.transport == Transport::Playing
                    && (!source.enabled || !running && source.pause_with_game)
                {
                    Transport::Paused
                } else {
                    voice.transport
                },
                position: voice.position,
                epoch: voice.epoch,
                looping: source.looping,
                streaming: source.streaming,
                volume: voice.volume * attenuation,
                pitch: voice.pitch,
                pan: if source.spatial {
                    (spatial_pan - source.pan + voice.pan).clamp(-1., 1.)
                } else {
                    voice.pan
                },
            });
        }
        Ok(frame)
    }
}
