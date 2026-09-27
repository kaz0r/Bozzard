use super::shared::{MAX_SAVED_PLAYERS, Player};
use super::state::{State, number, numeric, values};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SLOT_COUNT: usize = 6; // autosave and five manual slots
pub const MAX_SAVE_BYTES: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub version: u32,
    pub game: String,
    pub saved_at: u64,
    pub state: State,
    /// Steam identities retain their own backpacks and locations on rejoin.
    /// Defaults keep existing solo saves readable.
    #[serde(default)]
    pub owner: Option<u64>,
    #[serde(default)]
    pub players: BTreeMap<u64, Player>,
}
impl Save {
    pub fn new(state: State) -> Self {
        Self {
            version: 1,
            game: "stellar-ix".into(),
            saved_at: now(),
            state,
            owner: None,
            players: BTreeMap::new(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.game == "stellar-ix",
            "This save belongs to another game or version."
        );
        self.state.validate()?;
        ensure!(
            self.players.len() <= MAX_SAVED_PLAYERS,
            "too many saved player records"
        );
        for (peer, player) in &self.players {
            ensure!(*peer != 0, "invalid saved player identity");
            player.validate()?;
            ensure!(
                matches!(&values(&self.state.controller, "chunk_nodes")?[player.position.archive_index()], bozzard_scene::blueprint::Value::Text(t) if !t.is_empty()),
                "saved player location is unexplored"
            );
        }
        if let Some(owner) = self.owner {
            ensure!(
                self.players.get(&owner) == Some(&Player::capture(&self.state)?),
                "save owner does not match the local player"
            );
        }
        Ok(())
    }
    pub fn description(&self) -> Result<String> {
        let phase = number(&self.state.controller, "phase")? as u32;
        let session = values(&self.state.controller, "session")?;
        let time = numeric(&session[120])?;
        let cycle = (time as f64 / std::f64::consts::TAU * 0.1).floor() as u64 + 1;
        let moon = numeric(&session[7])? == 1.;
        let night = moon || (time * 0.1).sin() < 0.;
        let age = now().saturating_sub(self.saved_at);
        let ago = if age < 60 {
            "just now".into()
        } else if age < 3600 {
            format!("{}m ago", age / 60)
        } else if age < 86400 {
            format!("{}h ago", age / 3600)
        } else {
            format!("{}d ago", age / 86400)
        };
        Ok(format!(
            "Tier {} / Phase {} · Cycle {} / {}\n{} · {}",
            phase / 4 + 1,
            phase % 4 + 1,
            cycle,
            if night { "Night" } else { "Day" },
            if self.state.dev_world() {
                "Dev World"
            } else if moon {
                "Stella-Z2"
            } else {
                "Stellar-BX"
            },
            ago
        ))
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn directory() -> PathBuf {
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
    };
    root.unwrap_or_else(|| PathBuf::from("."))
        .join("stellar-ix/saves")
}
fn path(root: &Path, slot: usize) -> Result<PathBuf> {
    ensure!(slot < SLOT_COUNT, "invalid save slot");
    Ok(root.join(if slot == 0 {
        "autosave.json".into()
    } else {
        format!("slot-{slot}.json")
    }))
}
pub fn write(root: &Path, slot: usize, save: &Save) -> Result<()> {
    save.validate()?;
    let bytes = serde_json::to_vec(save)?;
    ensure!(
        bytes.len() as u64 <= MAX_SAVE_BYTES,
        "save exceeds size limit"
    );
    fs::create_dir_all(root).context("Cannot create the save directory")?;
    crate::save_atomic(&path(root, slot)?, |file| {
        use std::io::Write;
        file.write_all(&bytes)?;
        Ok(())
    })
}
pub fn read(root: &Path, slot: usize) -> Result<Save> {
    let file = fs::File::open(path(root, slot)?).context("Cannot open this save")?;
    let mut bytes = Vec::new();
    file.take(MAX_SAVE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_SAVE_BYTES,
        "save exceeds size limit"
    );
    let save: Save =
        serde_json::from_slice(&bytes).context("This save is damaged or incompatible")?;
    save.validate()?;
    Ok(save)
}
#[derive(Clone, Debug)]
pub struct Slot {
    pub description: String,
    pub loadable: bool,
}
pub fn catalog(root: &Path) -> Vec<Slot> {
    (0..SLOT_COUNT)
        .map(|slot| {
            if !path(root, slot).unwrap().exists() {
                return Slot {
                    description: "Empty slot\nNo saved world".into(),
                    loadable: false,
                };
            }
            match read(root, slot).and_then(|save| save.description()) {
                Ok(description) => Slot {
                    description,
                    loadable: true,
                },
                Err(error) => Slot {
                    description: format!("Unavailable: {error}"),
                    loadable: false,
                },
            }
        })
        .collect()
}
