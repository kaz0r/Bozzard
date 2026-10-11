//! Per-user player settings: window, VSync, quality preset and bus volumes.
//!
//! Settings are a versioned file of their own, never part of a scene or a save slot. Games
//! edit them through Blueprint nodes and script functions; **Apply** publishes the edited
//! values to the host, **Save** writes them. Hosts decide where the file lives.
use crate::blueprint::{NodeKind as K, Value};
use crate::middleware::audio::Bus;
use anyhow::{Context, Result, bail, ensure};
use bozzard_ecs::World;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

/// Current settings file schema.
pub const SETTINGS_VERSION: u32 = 1;
/// File name inside a game's settings directory.
pub const SETTINGS_FILE: &str = "settings.json";
/// Larger files are rejected rather than read.
pub const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
/// Accepted logical window width/height.
pub const WINDOW_SIZE_RANGE: std::ops::RangeInclusive<u32> = 320..=16384;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    /// A borderless window covering the current monitor.
    Borderless,
    /// Exclusive fullscreen at the monitor's largest video mode.
    Fullscreen,
}
impl WindowMode {
    pub const ALL: [Self; 3] = [Self::Windowed, Self::Borderless, Self::Fullscreen];
    pub fn name(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Borderless => "borderless",
            Self::Fullscreen => "fullscreen",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name().eq_ignore_ascii_case(name.trim()))
            .with_context(|| {
                format!("unknown window mode '{name}'; use windowed, borderless or fullscreen")
            })
    }
}

/// Caps on authored rendering cost. High renders the scene as authored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Low,
    Medium,
    #[default]
    High,
}
impl Quality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|quality| quality.name().eq_ignore_ascii_case(name.trim()))
            .with_context(|| format!("unknown quality preset '{name}'; use low, medium or high"))
    }
}

/// A user volume slider. Each scales the authored mix; none can raise it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeChannel {
    Master,
    Music,
    /// Also scales the Ambience bus.
    Sfx,
    Ui,
}
impl VolumeChannel {
    pub const ALL: [Self; 4] = [Self::Master, Self::Music, Self::Sfx, Self::Ui];
    pub fn name(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Music => "music",
            Self::Sfx => "sfx",
            Self::Ui => "ui",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|channel| channel.name().eq_ignore_ascii_case(name.trim()))
            .with_context(|| {
                format!("unknown volume channel '{name}'; use master, music, sfx or ui")
            })
    }
}

/// One user's settings for one game. Missing fields take their defaults; unknown fields and
/// newer versions are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerSettings {
    pub version: u32,
    pub window_mode: WindowMode,
    /// Windowed client size in logical points.
    pub window_size: [u32; 2],
    pub vsync: bool,
    pub quality: Quality,
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub ui_volume: f32,
}
impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            window_mode: WindowMode::Windowed,
            window_size: [1024, 640],
            vsync: true,
            quality: Quality::High,
            master_volume: 1.,
            music_volume: 1.,
            sfx_volume: 1.,
            ui_volume: 1.,
        }
    }
}

#[derive(Deserialize)]
struct Header {
    version: Option<u32>,
}

impl PlayerSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == SETTINGS_VERSION,
            "player settings version {} is not {SETTINGS_VERSION}",
            self.version
        );
        for size in self.window_size {
            window_dimension(size as f32)?;
        }
        for channel in VolumeChannel::ALL {
            check_volume(self.volume(channel))?;
        }
        Ok(())
    }
    /// Parse and validate a settings file. A missing, zero or newer version is an error.
    pub fn from_json(text: &str) -> Result<Self> {
        let header: Header =
            serde_json::from_str(text).context("player settings are not a JSON object")?;
        let version = header.version.context("player settings have no version")?;
        ensure!(version > 0, "player settings version 0 is invalid");
        ensure!(
            version <= SETTINGS_VERSION,
            "player settings version {version} is newer than this game supports ({SETTINGS_VERSION})"
        );
        let settings: Self = serde_json::from_str(text).context("invalid player settings")?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
    /// The file's settings, or `None` when it does not exist.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        let mut text = String::new();
        file.take(MAX_SETTINGS_BYTES + 1)
            .read_to_string(&mut text)
            .with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            text.len() as u64 <= MAX_SETTINGS_BYTES,
            "player settings exceed {} KiB",
            MAX_SETTINGS_BYTES / 1024
        );
        Self::from_json(&text)
            .map(Some)
            .with_context(|| format!("in {}", path.display()))
    }
    /// Write through a synced temporary file and rename, creating the directory.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let json = self.to_json()?;
        let directory = path
            .parent()
            .context("player settings path has no directory")?;
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        let temp = directory.join(format!(
            ".settings-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result.with_context(|| format!("writing {}", path.display()))
    }
    pub fn volume(&self, channel: VolumeChannel) -> f32 {
        match channel {
            VolumeChannel::Master => self.master_volume,
            VolumeChannel::Music => self.music_volume,
            VolumeChannel::Sfx => self.sfx_volume,
            VolumeChannel::Ui => self.ui_volume,
        }
    }
    /// The user's multiplier for one authored bus; master applies separately.
    pub fn bus_volume(&self, bus: Bus) -> f32 {
        match bus {
            Bus::Sfx | Bus::Ambience => self.sfx_volume,
            Bus::Music => self.music_volume,
            Bus::Ui => self.ui_volume,
        }
    }
    /// Apply a value change or reset. Apply and Save do not change the values.
    pub fn change(&mut self, request: &Request) -> Result<()> {
        match *request {
            Request::WindowMode(mode) => self.window_mode = mode,
            Request::WindowSize(size) => {
                for value in size {
                    window_dimension(value as f32)?;
                }
                self.window_size = size;
            }
            Request::Vsync(enabled) => self.vsync = enabled,
            Request::Quality(quality) => self.quality = quality,
            Request::Volume(channel, volume) => {
                check_volume(volume)?;
                *match channel {
                    VolumeChannel::Master => &mut self.master_volume,
                    VolumeChannel::Music => &mut self.music_volume,
                    VolumeChannel::Sfx => &mut self.sfx_volume,
                    VolumeChannel::Ui => &mut self.ui_volume,
                } = volume;
            }
            Request::Reset => *self = Self::default(),
            Request::Apply | Request::Save => {}
        }
        Ok(())
    }
}

/// A window width or height: an integer within [`WINDOW_SIZE_RANGE`].
pub fn window_dimension(value: f32) -> Result<u32> {
    ensure!(
        value.is_finite()
            && value.fract() == 0.
            && (*WINDOW_SIZE_RANGE.start() as f32..=*WINDOW_SIZE_RANGE.end() as f32)
                .contains(&value),
        "window size must be an integer in {}–{}",
        WINDOW_SIZE_RANGE.start(),
        WINDOW_SIZE_RANGE.end()
    );
    Ok(value as u32)
}
fn check_volume(volume: f32) -> Result<()> {
    ensure!(
        volume.is_finite() && (0.0..=1.).contains(&volume),
        "volume setting must be within 0–1"
    );
    Ok(())
}

/// One operation shared by Blueprint nodes, scripts and hosts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    WindowMode(WindowMode),
    WindowSize([u32; 2]),
    Vsync(bool),
    Quality(Quality),
    Volume(VolumeChannel, f32),
    /// Publish the edited values to the host.
    Apply,
    /// Write the edited values; does not apply them.
    Save,
    /// Return the edited values to defaults; Apply and Save remain separate.
    Reset,
}

/// A world resource: the values games edit, the values the host uses, and the file.
#[derive(Clone, Debug, Default)]
pub struct SettingsStore {
    edited: PlayerSettings,
    applied: PlayerSettings,
    revision: u64,
    path: Option<PathBuf>,
    saved: Option<PlayerSettings>,
    rejected: Option<String>,
}
impl SettingsStore {
    /// Default settings saved to `path`, or kept in memory without one. Nothing is read.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    /// What getters report and Apply publishes.
    pub fn edited(&self) -> &PlayerSettings {
        &self.edited
    }
    /// What the host should currently use.
    pub fn applied(&self) -> &PlayerSettings {
        &self.applied
    }
    /// Increments whenever the applied settings are replaced.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The last saved settings, including in-memory saves without a path.
    pub fn saved(&self) -> Option<&PlayerSettings> {
        self.saved.as_ref()
    }
    /// Why the file was ignored at load, until a save replaces it.
    pub fn rejected(&self) -> Option<&str> {
        self.rejected.as_deref()
    }
    /// Read the user's file as both edited and applied. A missing file is `Ok(false)`.
    /// An invalid or newer file keeps the current values and is returned as an error;
    /// a later save first moves it aside instead of overwriting it.
    pub fn load(&mut self) -> Result<bool> {
        let Some(path) = &self.path else {
            return Ok(false);
        };
        match PlayerSettings::load(path) {
            Ok(Some(settings)) => {
                self.edited = settings;
                self.publish(settings);
                self.saved = Some(settings);
                self.rejected = None;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => {
                self.rejected = Some(format!("{error:#}"));
                Err(error.context("ignoring player settings; using defaults"))
            }
        }
    }
    pub fn request(&mut self, request: Request) -> Result<()> {
        match request {
            Request::Apply => self.publish(self.edited),
            Request::Save => self.save()?,
            _ => self.edited.change(&request)?,
        }
        Ok(())
    }
    fn publish(&mut self, settings: PlayerSettings) {
        self.applied = settings;
        self.revision = self.revision.wrapping_add(1);
    }
    fn save(&mut self) -> Result<()> {
        if let Some(path) = &self.path {
            if self.rejected.is_some() && path.exists() {
                let aside = rejected_path(path);
                std::fs::rename(path, &aside).with_context(|| {
                    format!(
                        "keeping rejected player settings as {} before saving",
                        aside.display()
                    )
                })?;
            }
            self.edited.save(path)?;
        } else {
            self.edited.validate()?;
        }
        self.saved = Some(self.edited);
        self.rejected = None;
        Ok(())
    }
}

/// Where a rejected file is kept when a game saves over it: `settings.json.rejected`.
pub fn rejected_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".rejected");
    path.with_file_name(name)
}

/// The edited settings, or defaults when no store exists yet.
pub fn current(world: &World) -> PlayerSettings {
    world
        .resource::<SettingsStore>()
        .map_or_else(PlayerSettings::default, |store| *store.edited())
}
/// The applied settings, or defaults when no store exists yet.
pub fn applied(world: &World) -> PlayerSettings {
    world
        .resource::<SettingsStore>()
        .map_or_else(PlayerSettings::default, |store| *store.applied())
}
/// Perform a request, creating an in-memory store when the host configured none.
pub fn request(world: &mut World, request: Request) -> Result<()> {
    if world.resource::<SettingsStore>().is_none() {
        world.insert_resource(SettingsStore::default());
    }
    world
        .resource_mut::<SettingsStore>()
        .expect("settings store")
        .request(request)
}

/// Settings query nodes: pure reads of the edited values.
pub(crate) fn is_query(kind: K) -> bool {
    matches!(
        kind,
        K::WindowModeSetting
            | K::WindowSizeSetting
            | K::VsyncSetting
            | K::QualitySetting
            | K::VolumeSetting
    )
}
pub(crate) fn query(world: &World, kind: K, port: usize, inputs: &[Value]) -> Result<Value> {
    let settings = current(world);
    Ok(match kind {
        K::WindowModeSetting => Value::Text(settings.window_mode.name().into()),
        K::WindowSizeSetting => Value::Number(settings.window_size[port.min(1)] as f32),
        K::VsyncSetting => Value::Bool(settings.vsync),
        K::QualitySetting => Value::Text(settings.quality.name().into()),
        K::VolumeSetting => {
            let channel = inputs.first().context("missing volume channel")?.text()?;
            Value::Number(settings.volume(VolumeChannel::parse(channel)?))
        }
        _ => bail!("{} is not a settings query", kind.title()),
    })
}
/// Settings action nodes.
pub(crate) fn is_action(kind: K) -> bool {
    matches!(
        kind,
        K::SetWindowModeSetting
            | K::SetWindowSizeSetting
            | K::SetVsyncSetting
            | K::SetQualitySetting
            | K::SetVolumeSetting
            | K::ApplySettings
            | K::SaveSettings
            | K::ResetSettings
    )
}
/// The request for an action node from its first and second data inputs.
pub(crate) fn action_request(kind: K, first: &Value, second: Option<&Value>) -> Result<Request> {
    let second = || second.context("missing second settings input");
    Ok(match kind {
        K::SetWindowModeSetting => Request::WindowMode(WindowMode::parse(first.text()?)?),
        K::SetWindowSizeSetting => Request::WindowSize([
            window_dimension(first.number()?)?,
            window_dimension(second()?.number()?)?,
        ]),
        K::SetVsyncSetting => Request::Vsync(first.boolean()?),
        K::SetQualitySetting => Request::Quality(Quality::parse(first.text()?)?),
        K::SetVolumeSetting => {
            Request::Volume(VolumeChannel::parse(first.text()?)?, second()?.number()?)
        }
        K::ApplySettings => Request::Apply,
        K::SaveSettings => Request::Save,
        K::ResetSettings => Request::Reset,
        _ => bail!("{} is not a settings action", kind.title()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "bozzard-settings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn defaults_are_valid_and_round_trip_through_json() {
        let defaults = PlayerSettings::default();
        defaults.validate().unwrap();
        assert_eq!(defaults.window_mode, WindowMode::Windowed);
        assert_eq!(defaults.window_size, [1024, 640]);
        assert!(defaults.vsync);
        assert_eq!(defaults.quality, Quality::High);
        let custom = PlayerSettings {
            window_mode: WindowMode::Borderless,
            window_size: [1920, 1080],
            vsync: false,
            quality: Quality::Low,
            master_volume: 0.5,
            music_volume: 0.25,
            sfx_volume: 0.,
            ui_volume: 1.,
            ..defaults
        };
        let json = custom.to_json().unwrap();
        assert!(json.contains("\"window_mode\": \"borderless\""), "{json}");
        assert_eq!(PlayerSettings::from_json(&json).unwrap(), custom);
        let partial = PlayerSettings::from_json(r#"{"version":1,"vsync":false}"#).unwrap();
        assert_eq!(
            partial,
            PlayerSettings {
                vsync: false,
                ..defaults
            }
        );
    }

    #[test]
    fn invalid_unknown_and_newer_files_are_rejected_with_reasons() {
        for (json, reason) in [
            ("[]", "not a JSON object"),
            ("{}", "no version"),
            (r#"{"version":0}"#, "version 0"),
            (
                r#"{"version":2,"future":true}"#,
                "newer than this game supports",
            ),
            (r#"{"version":1,"colour":"red"}"#, "invalid player settings"),
            (
                r#"{"version":1,"window_mode":"tiny"}"#,
                "invalid player settings",
            ),
            (r#"{"version":1,"window_size":[100,100]}"#, "window size"),
            (r#"{"version":1,"music_volume":1.5}"#, "volume setting"),
        ] {
            let error = format!("{:#}", PlayerSettings::from_json(json).unwrap_err());
            assert!(error.contains(reason), "{json}: {error}");
        }
        let mut settings = PlayerSettings::default();
        for request in [
            Request::WindowSize([319, 600]),
            Request::Volume(VolumeChannel::Ui, -0.1),
            Request::Volume(VolumeChannel::Master, f32::NAN),
        ] {
            assert!(settings.change(&request).is_err(), "{request:?}");
        }
        assert_eq!(settings, PlayerSettings::default());
        assert!(window_dimension(800.5).is_err());
        assert_eq!(window_dimension(800.).unwrap(), 800);
        assert!(WindowMode::parse("Full Screen").is_err());
        assert_eq!(
            WindowMode::parse(" Fullscreen ").unwrap(),
            WindowMode::Fullscreen
        );
        assert_eq!(Quality::parse("MEDIUM").unwrap(), Quality::Medium);
        assert_eq!(VolumeChannel::parse("SFX").unwrap(), VolumeChannel::Sfx);
    }

    #[test]
    fn store_separates_edits_from_applied_values_and_saves_to_its_file() {
        let directory = temp_dir("store");
        let path = directory.join("nested").join(SETTINGS_FILE);
        let mut store = SettingsStore::new(Some(path.clone()));
        assert!(!store.load().unwrap(), "a missing file keeps defaults");
        assert!(!path.exists(), "loading never writes");
        store.request(Request::Vsync(false)).unwrap();
        store
            .request(Request::Volume(VolumeChannel::Music, 0.4))
            .unwrap();
        assert!(!store.edited().vsync);
        assert!(store.applied().vsync, "edits wait for Apply");
        let revision = store.revision();
        store.request(Request::Apply).unwrap();
        assert_eq!(store.revision(), revision + 1);
        assert_eq!(store.applied(), store.edited());
        assert!(!path.exists(), "Apply does not save");
        store.request(Request::Save).unwrap();
        assert_eq!(PlayerSettings::load(&path).unwrap(), Some(*store.edited()));
        store.request(Request::Reset).unwrap();
        assert_eq!(*store.edited(), PlayerSettings::default());
        assert!(!store.applied().vsync, "Reset waits for Apply too");

        let mut reloaded = SettingsStore::new(Some(path.clone()));
        assert!(reloaded.load().unwrap());
        assert_eq!(reloaded.applied().music_volume, 0.4);
        assert_eq!(reloaded.edited(), reloaded.applied());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn rejected_files_fall_back_to_defaults_and_are_kept_aside_on_save() {
        let directory = temp_dir("rejected");
        let path = directory.join(SETTINGS_FILE);
        std::fs::create_dir_all(&directory).unwrap();
        let newer = r#"{"version":7,"hdr":true}"#;
        std::fs::write(&path, newer).unwrap();
        let mut store = SettingsStore::new(Some(path.clone()));
        let error = format!("{:#}", store.load().unwrap_err());
        assert!(error.contains("using defaults"), "{error}");
        assert!(error.contains("version 7 is newer"), "{error}");
        assert_eq!(*store.applied(), PlayerSettings::default());
        assert!(store.rejected().is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer, "untouched");
        store.request(Request::Quality(Quality::Medium)).unwrap();
        store.request(Request::Save).unwrap();
        assert_eq!(
            std::fs::read_to_string(rejected_path(&path)).unwrap(),
            newer,
            "the newer file survives an older game's save"
        );
        assert_eq!(
            PlayerSettings::load(&path).unwrap().unwrap().quality,
            Quality::Medium
        );
        assert!(store.rejected().is_none());

        std::fs::write(&path, vec![b' '; MAX_SETTINGS_BYTES as usize + 1]).unwrap();
        let error = format!("{:#}", PlayerSettings::load(&path).unwrap_err());
        assert!(error.contains("exceed"), "{error}");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn requests_without_a_host_store_use_memory() {
        let mut world = World::default();
        assert_eq!(current(&world), PlayerSettings::default());
        request(&mut world, Request::WindowMode(WindowMode::Fullscreen)).unwrap();
        request(&mut world, Request::Save).unwrap();
        let store = world.resource::<SettingsStore>().unwrap();
        assert_eq!(store.path(), None);
        assert_eq!(store.saved().unwrap().window_mode, WindowMode::Fullscreen);
        assert_eq!(applied(&world).window_mode, WindowMode::Windowed);
    }

    #[test]
    fn every_settings_node_is_a_query_or_an_action_with_matching_pins() {
        let settings: Vec<_> = K::specs()
            .iter()
            .filter(|spec| spec.title.contains("Setting"))
            .collect();
        assert_eq!(settings.len(), 13);
        for spec in settings {
            assert_ne!(is_query(spec.kind), is_action(spec.kind), "{}", spec.title);
            assert_eq!(is_action(spec.kind), spec.kind.action(), "{}", spec.title);
        }
    }
}
