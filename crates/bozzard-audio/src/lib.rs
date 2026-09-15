//! Native audio output. Scene transport stays deterministic and independent of an audio device.
use anyhow::{Context, Result, ensure};
use bozzard_scene::{
    AssetKind, Scene,
    middleware::audio::{Frame, Playback, Transport},
};
use kira::{
    AudioManager, AudioManagerSettings, Capacities, Decibels, Tween,
    backend::{Backend, DefaultBackend},
    sound::{
        FromFileError,
        static_sound::{StaticSoundData, StaticSoundHandle},
        streaming::{StreamingSoundData, StreamingSoundHandle},
    },
    track::{TrackBuilder, TrackHandle},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
const CLIP_BYTES: usize = 8 * 1024 * 1024;
fn ramp() -> Tween {
    Tween {
        duration: Duration::from_millis(10),
        ..Default::default()
    }
}
fn gain(volume: f32) -> Decibels {
    if volume <= 0. {
        Decibels::SILENCE
    } else {
        Decibels(20. * volume.log10())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: SystemTime,
}
fn stamp(path: &Path) -> Result<Stamp> {
    let meta = path
        .metadata()
        .with_context(|| format!("read audio {}", path.display()))?;
    ensure!(
        meta.is_file() && meta.len() > 0 && meta.len() <= 1024 * 1024 * 1024,
        "audio file must be between 1 byte and 1 GiB"
    );
    Ok(Stamp {
        bytes: meta.len(),
        modified: meta.modified()?,
    })
}
struct Cached {
    stamp: Stamp,
    data: StaticSoundData,
    used: u64,
}
enum Handle {
    Static(StaticSoundHandle),
    Stream(StreamingSoundHandle<FromFileError>),
}
macro_rules! command { ($self:expr, $method:ident($($arg:expr),*)) => { match $self { Handle::Static(h) => h.$method($($arg),*), Handle::Stream(h) => h.$method($($arg),*) } }; }
struct Voice {
    handle: Handle,
    path: PathBuf,
    last: Playback,
}

/// Reusable mixer with four buses and a bounded cache. The backend is replaceable for sample tests.
pub struct AudioOutput<B: Backend> {
    _manager: AudioManager<B>,
    tracks: Vec<TrackHandle>,
    voices: BTreeMap<u64, Voice>,
    cache: BTreeMap<PathBuf, Cached>,
    cache_bytes: usize,
    serial: u64,
    bus_gains: [f32; 4],
}
impl<B: Backend> AudioOutput<B>
where
    B::Error: std::fmt::Debug,
{
    pub fn new(settings: B::Settings) -> Result<Self> {
        let mut manager = AudioManager::<B>::new(AudioManagerSettings {
            backend_settings: settings,
            capacities: Capacities {
                sub_track_capacity: 4,
                send_track_capacity: 0,
                clock_capacity: 0,
                modulator_capacity: 0,
                listener_capacity: 0,
            },
            main_track_builder: Default::default(),
            internal_buffer_size: 128,
        })
        .map_err(|e| anyhow::anyhow!("open audio device: {e:?}"))?;
        let tracks = (0..4)
            .map(|_| {
                manager.add_sub_track(
                    TrackBuilder::new()
                        .sound_capacity(512)
                        .sub_track_capacity(0),
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self {
            _manager: manager,
            tracks,
            voices: BTreeMap::new(),
            cache: BTreeMap::new(),
            cache_bytes: 0,
            serial: 0,
            bus_gains: [1.; 4],
        })
    }
    pub fn cached_bytes(&self) -> usize {
        self.cache_bytes
    }
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }
    pub fn stop_all(&mut self) {
        for (_, mut voice) in std::mem::take(&mut self.voices) {
            command!(&mut voice.handle, stop(ramp()));
        }
    }
    /// Each source is synchronized only when its transport or parameters change; position is
    /// authoritative on creation/seek, allowing the device clock to run smoothly between frames.
    pub fn sync(&mut self, frame: &Frame, scene: &Scene, root: &Path) -> Result<()> {
        ensure!(
            frame.sources.len() <= 256,
            "audio frame exceeds 256 sources"
        );
        ensure!(
            frame.master.is_finite()
                && (0.0..=4.).contains(&frame.master)
                && frame
                    .buses
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=4.).contains(v)),
            "invalid audio mix"
        );
        self.serial = self.serial.wrapping_add(1);
        for (i, volume) in frame.buses.iter().enumerate() {
            let volume = volume * frame.master;
            if self.bus_gains[i] != volume {
                self.tracks[i].set_volume(gain(volume), ramp());
                self.bus_gains[i] = volume;
            }
        }
        let ids: BTreeSet<_> = frame
            .sources
            .iter()
            .filter(|s| s.transport != Transport::Stopped)
            .map(|s| s.id)
            .collect();
        self.voices.retain(|id, voice| {
            if ids.contains(id) {
                true
            } else {
                command!(&mut voice.handle, stop(ramp()));
                false
            }
        });
        for source in &frame.sources {
            if source.transport == Transport::Stopped {
                continue;
            }
            ensure!(
                source.position.is_finite()
                    && source.position >= 0.
                    && source.volume.is_finite()
                    && (0.0..=4.).contains(&source.volume)
                    && source.pitch.is_finite()
                    && (0.125..=4.).contains(&source.pitch)
                    && source.pan.is_finite()
                    && (-1.0..=1.).contains(&source.pan),
                "invalid audio source playback"
            );
            let asset = scene
                .assets
                .get(&source.asset)
                .context("audio asset is missing")?;
            ensure!(
                asset.kind == AssetKind::Audio,
                "audio source references a non-audio asset"
            );
            let path = root.join(&asset.path);
            if self.voices.get(&source.id).is_some_and(|v| {
                v.path != path
                    || v.last.bus != source.bus
                    || v.last.streaming != source.streaming
                    || v.last.epoch != source.epoch
                        && command!(&v.handle, state()) == kira::sound::PlaybackState::Stopped
            }) {
                let mut old = self.voices.remove(&source.id).unwrap();
                command!(&mut old.handle, stop(ramp()));
            }
            if !self.voices.contains_key(&source.id) {
                let mut handle = self
                    .start(&path, source)
                    .with_context(|| format!("play audio {}", source.owner))?;
                if source.transport == Transport::Paused {
                    command!(
                        &mut handle,
                        pause(Tween {
                            duration: Duration::ZERO,
                            ..Default::default()
                        })
                    );
                }
                self.voices.insert(
                    source.id,
                    Voice {
                        handle,
                        path,
                        last: source.clone(),
                    },
                );
                continue;
            }
            let voice = self.voices.get_mut(&source.id).unwrap();
            if voice.last.epoch != source.epoch {
                command!(&mut voice.handle, seek_to(source.position));
            }
            if voice.last.volume != source.volume {
                command!(&mut voice.handle, set_volume(gain(source.volume), ramp()));
            }
            if voice.last.pitch != source.pitch {
                command!(
                    &mut voice.handle,
                    set_playback_rate(f64::from(source.pitch), ramp())
                );
            }
            if voice.last.pan != source.pan {
                command!(&mut voice.handle, set_panning(source.pan, ramp()));
            }
            if voice.last.looping != source.looping {
                command!(
                    &mut voice.handle,
                    set_loop_region(source.looping.then_some(kira::sound::Region::from(0.0..)))
                );
            }
            if voice.last.transport != source.transport {
                match source.transport {
                    Transport::Paused => command!(&mut voice.handle, pause(ramp())),
                    Transport::Playing => command!(&mut voice.handle, resume(ramp())),
                    Transport::Stopped => {}
                }
            }
            if let Handle::Stream(h) = &mut voice.handle
                && let Some(error) = h.pop_error()
            {
                anyhow::bail!("decode audio {}: {error}", source.owner);
            }
            voice.last.position = source.position;
            voice.last.epoch = source.epoch;
            voice.last.volume = source.volume;
            voice.last.pitch = source.pitch;
            voice.last.pan = source.pan;
            voice.last.looping = source.looping;
            voice.last.transport = source.transport;
        }
        Ok(())
    }
    fn start(&mut self, path: &Path, source: &Playback) -> Result<Handle> {
        let metadata = stamp(path)?;
        if self.cache.get(path).is_some_and(|entry| {
            entry.stamp != metadata && std::sync::Arc::strong_count(&entry.data.frames) == 1
        }) {
            let entry = self.cache.remove(path).unwrap();
            self.cache_bytes -= entry.data.frames.len() * std::mem::size_of::<kira::Frame>();
        }
        let mut stream = None;
        if !source.streaming && !self.cache.contains_key(path) {
            let data = StreamingSoundData::from_file(path)?;
            let bytes = data
                .num_frames()
                .checked_mul(std::mem::size_of::<kira::Frame>())
                .context("audio clip size overflow")?;
            if bytes <= CLIP_BYTES {
                while self.cache_bytes + bytes > CACHE_BYTES {
                    // Active static voices share these frames. Never evict their accounting.
                    let oldest = self
                        .cache
                        .iter()
                        .filter(|(_, e)| std::sync::Arc::strong_count(&e.data.frames) == 1)
                        .min_by_key(|(_, e)| e.used)
                        .map(|(p, _)| p.clone());
                    let Some(oldest) = oldest else {
                        break;
                    };
                    let entry = self.cache.remove(&oldest).unwrap();
                    self.cache_bytes -=
                        entry.data.frames.len() * std::mem::size_of::<kira::Frame>();
                }
                if self.cache_bytes + bytes <= CACHE_BYTES {
                    // Metadata is checked before decoding, then verified after decoding as well.
                    let clip = StaticSoundData::from_file(path)?;
                    let actual = clip.frames.len() * std::mem::size_of::<kira::Frame>();
                    ensure!(
                        actual <= CLIP_BYTES
                            && self.cache_bytes + actual <= CACHE_BYTES
                            && stamp(path)? == metadata,
                        "audio changed during decoding or exceeded cache bound"
                    );
                    self.cache_bytes += actual;
                    self.cache.insert(
                        path.to_owned(),
                        Cached {
                            stamp: metadata,
                            data: clip,
                            used: self.serial,
                        },
                    );
                }
            }
            stream = Some(data);
        }
        let region = source.looping.then_some(kira::sound::Region::from(0.0..));
        if !source.streaming
            && let Some(entry) = self
                .cache
                .get_mut(path)
                .filter(|entry| entry.stamp == metadata)
        {
            entry.used = self.serial;
            let data = entry
                .data
                .start_position(source.position)
                .loop_region(region)
                .volume(gain(source.volume))
                .playback_rate(f64::from(source.pitch))
                .panning(source.pan)
                .fade_in_tween(ramp());
            return Ok(Handle::Static(self.tracks[source.bus as usize].play(data)?));
        }
        let data = match stream {
            Some(data) => data,
            None => StreamingSoundData::from_file(path)?,
        };
        let data = data
            .start_position(source.position)
            .loop_region(region)
            .volume(gain(source.volume))
            .playback_rate(f64::from(source.pitch))
            .panning(source.pan)
            .fade_in_tween(ramp());
        Ok(Handle::Stream(self.tracks[source.bus as usize].play(data)?))
    }
}
/// Lazy device initialization keeps silent scenes usable on machines with no audio device.
#[derive(Default)]
pub struct NativeAudio {
    output: Option<AudioOutput<DefaultBackend>>,
    failed: bool,
}
impl NativeAudio {
    /// Drop only this engine's voices and cache when an asset reload changes decoded samples.
    pub fn invalidate_assets(&mut self) {
        self.output = None;
        self.failed = false;
    }
    pub fn stop(&mut self) {
        if let Some(output) = &mut self.output {
            output.stop_all();
        }
        self.failed = false;
    }
    pub fn sync(&mut self, frame: &Frame, scene: &Scene, root: &Path) -> Result<()> {
        if self.failed {
            return Ok(());
        }
        if self.output.is_none()
            && frame
                .sources
                .iter()
                .any(|s| s.transport == Transport::Playing)
        {
            match AudioOutput::new(Default::default()) {
                Ok(output) => self.output = Some(output),
                Err(error) => {
                    self.failed = true;
                    return Err(error);
                }
            }
        }
        if let Some(output) = &mut self.output
            && let Err(error) = output.sync(frame, scene, root)
        {
            output.stop_all();
            self.failed = true;
            return Err(error);
        }
        Ok(())
    }
}
