//! Stellar-IX's native session services. Rhai owns gameplay and presentation;
//! native code owns local files and (separately) authenticated session authority.
pub mod authority;
pub mod guest;
pub mod host;
pub mod link;
mod live;
pub mod network;
pub mod replication;
pub mod saves;
pub mod shared;
pub mod state;
pub mod transport;
use anyhow::{Context, Result, ensure};
use bozzard_app::{App, World, job::Job};
use bozzard_scene::{
    BlueprintRuntime, Scene, SceneInstance,
    blueprint::{Blackboard, BlackboardValue as B, Value},
    middleware::ui::Control,
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Authority {
    #[default]
    Solo,
    Host,
    Guest,
}
pub struct Session {
    pub directory: PathBuf,
    pub authority: Authority,
    pub local_peer: Option<bozzard_network::Peer>,
    pub players: std::collections::BTreeMap<bozzard_network::Peer, shared::Player>,
    /// Native load boundary. Network hosts use this to issue a fresh world epoch.
    pub(crate) world_revision: u64,
    job: Option<std::sync::Mutex<Job<Completed>>>,
    pending: Option<saves::Save>,
    generation: f32,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            directory: saves::directory(),
            authority: Authority::Solo,
            local_peer: None,
            players: Default::default(),
            world_revision: 0,
            job: None,
            pending: None,
            generation: 0.,
        }
    }
}
enum Completed {
    Saved(Vec<saves::Slot>),
    Loaded(Box<saves::Save>),
    Catalog(Vec<saves::Slot>),
}
pub fn is_factory(scene: &Scene) -> bool {
    scene.assets.contains_key("earth-factory") && scene.objects.iter().any(|o| o.id == "controller")
}
pub fn install(app: &mut App, scene: &Scene) {
    if !is_factory(scene) {
        return;
    }
    app.world.insert_resource(Session::default());
    app.add_named_system("Factory co-op host", |world, _, tick| {
        host::step(world, tick.delta);
    });
    app.add_named_system("Factory co-op guest", |world, _, _| {
        guest::step(world);
    });
    app.add_named_system("Factory session", |world, _, _| {
        let Some(mut session) = world.remove_resource::<Session>() else {
            return;
        };
        if let Err(error) = session.update(world) {
            let _ = notice(world, &format!("Save / Load: {error:#}"));
            let _ = set_session(world, 116, 0.);
            if session_value(world, 121).unwrap_or_default() >= 1200. {
                let _ = set_session(world, 121, 1140.);
            }
        }
        world.insert_resource(session);
    });
}
pub fn session_value(world: &World, index: usize) -> Result<f32> {
    let board = world
        .resource::<BlueprintRuntime>()
        .context("missing gameplay state")?
        .object_blackboard("controller")
        .context("missing controller")?;
    state::numeric(
        state::values(board, "session")?
            .get(index)
            .context("missing session value")?,
    )
}
pub fn set_session(world: &mut World, index: usize, value: f32) -> Result<()> {
    let runtime = world
        .resource_mut::<BlueprintRuntime>()
        .context("missing gameplay state")?;
    let mut session = runtime
        .object_blackboard("controller")
        .context("missing controller")?["session"]
        .clone();
    session.values_mut()[index] = Value::Number(value);
    runtime.patch_blackboards(
        &Blackboard::new(),
        &[("controller".into(), [("session".into(), session)].into())].into(),
    )
}
fn control(world: &mut World, id: &str, control: Control) -> Result<()> {
    let instance = world
        .remove_resource::<SceneInstance>()
        .context("missing scene")?;
    let result = if instance.entity(id).is_some() {
        instance.control_ui(world, id, control)
    } else {
        Ok(())
    };
    world.insert_resource(instance);
    result
}
fn notice(world: &mut World, message: &str) -> Result<()> {
    let text: String = message.chars().take(700).collect();
    control(world, "saves-status", Control::Text(text.clone()))?;
    world
        .resource_mut::<BlueprintRuntime>()
        .context("missing gameplay state")?
        .patch_blackboards(
            &[("message".into(), B::Scalar(Value::Text(text)))].into(),
            &Default::default(),
        )
}
fn catalog(world: &mut World, slots: Vec<saves::Slot>) -> Result<()> {
    let loading = session_value(world, 123)? == 2.;
    let authority = session_value(world, 122)? != 2.;
    for (slot, entry) in slots.into_iter().enumerate() {
        control(
            world,
            &format!("save-info-{slot}"),
            Control::Text(entry.description),
        )?;
        control(
            world,
            &format!("save-slot-{slot}"),
            Control::Enabled(authority && if loading { entry.loadable } else { slot > 0 }),
        )?;
    }
    Ok(())
}
impl Session {
    pub(crate) fn bind_local_peer(&mut self, peer: bozzard_network::Peer) {
        if let Some(previous) = self.local_peer.filter(|previous| *previous != peer) {
            self.players.remove(&previous);
        }
        self.local_peer = Some(peer);
    }
    fn update(&mut self, world: &mut World) -> Result<()> {
        if world
            .resource::<BlueprintRuntime>()
            .and_then(|r| r.object_blackboard("controller"))
            .is_none()
        {
            return Ok(());
        }
        let authority = match self.authority {
            Authority::Solo => 0.,
            Authority::Host => 1.,
            Authority::Guest => 2.,
        };
        if session_value(world, 122)? != authority {
            set_session(world, 122, authority)?;
        }
        let generation = session_value(world, 126)?;
        if generation != self.generation {
            self.pending = None;
            self.players.clear();
            if let Some(job) = self.job.take() {
                job.into_inner().unwrap().cancel();
            }
            self.generation = generation;
            set_session(world, 125, 0.)?;
        }
        if let Some(result) = self
            .job
            .as_mut()
            .and_then(|job| job.get_mut().unwrap().poll())
        {
            self.job = None;
            set_session(world, 125, 0.)?;
            match result {
                Ok(Completed::Saved(slots)) => {
                    set_session(world, 121, 0.)?;
                    notice(world, "Game saved.")?;
                    catalog(world, slots)?;
                }
                Ok(Completed::Catalog(slots)) => {
                    catalog(world, slots)?;
                }
                Ok(Completed::Loaded(save)) => {
                    ensure!(
                        self.authority != Authority::Guest,
                        "Only the host can load the world."
                    );
                    // Validate the entire patch against this build before Rhai removes models.
                    let mut check = world.resource::<BlueprintRuntime>().unwrap().clone();
                    check.patch_blackboards(
                        &save.state.scene,
                        &[("controller".into(), save.state.controller.clone())].into(),
                    )?;
                    self.pending = Some(*save);
                    set_session(world, 116, 3.)?;
                    notice(world, "Restoring world…")?;
                }
                Err(error) => {
                    notice(world, &format!("Save / Load failed: {error:#}"))?;
                    if session_value(world, 121)? >= 1200. {
                        set_session(world, 121, 1140.)?;
                    }
                }
            }
        }
        let command = session_value(world, 116)? as u32;
        if command == 0 || command == 3 || command == 5 {
            return Ok(());
        }
        if command == 4 {
            ensure!(
                self.authority != Authority::Guest,
                "Only the host can load the world."
            );
            let save = self
                .pending
                .take()
                .context("no validated save to restore")?;
            let mut state = save.state;
            let mut players = save.players;
            // Preserve the source identity when opening a multiplayer save in
            // solo mode, so hosting it later transfers (rather than duplicates)
            // the local backpack into the authenticated Steam host record.
            if self.local_peer.is_none() {
                self.local_peer = save.owner;
            }
            if let Some(peer) = self.local_peer {
                if let Some(previous) = save.owner.filter(|previous| *previous != peer) {
                    players.remove(&previous);
                }
                players.insert(peer, shared::Player::capture(&state)?);
            }
            let values = state.controller.get_mut("session").unwrap().values_mut();
            values[116] = Value::Number(5.);
            values[122] = Value::Number(authority);
            values[126] = Value::Number(generation);
            world
                .resource_mut::<BlueprintRuntime>()
                .unwrap()
                .patch_blackboards(
                    &state.scene,
                    &[("controller".into(), state.controller)].into(),
                )?;
            self.players = players;
            self.world_revision = self
                .world_revision
                .checked_add(1)
                .context("world revision exhausted")?;
            return Ok(());
        }
        set_session(world, 116, 0.)?;
        ensure!(
            self.authority != Authority::Guest || command == 6,
            "Only the host can save or load the world."
        );
        ensure!(
            self.job.is_none() && self.pending.is_none(),
            "A save operation is already running."
        );
        let slot = session_value(world, 117)?;
        ensure!(
            slot.fract() == 0. && (0. ..saves::SLOT_COUNT as f32).contains(&slot),
            "invalid save slot"
        );
        let root = self.directory.clone();
        let slot = slot as usize;
        self.job = Some(std::sync::Mutex::new(match command {
            1 => {
                let mut save = saves::Save::new(state::State::capture(
                    world.resource::<BlueprintRuntime>().unwrap(),
                )?);
                if let Some(peer) = self.local_peer {
                    self.players
                        .insert(peer, shared::Player::capture(&save.state)?);
                    save.owner = Some(peer);
                }
                save.players = self.players.clone();
                notice(
                    world,
                    if slot == 0 {
                        "Auto-saving…"
                    } else {
                        "Saving…"
                    },
                )?;
                Job::start("Save factory", move |progress| {
                    progress.check()?;
                    saves::write(&root, slot, &save)?;
                    Ok(Completed::Saved(saves::catalog(&root)))
                })?
            }
            2 => {
                notice(world, "Reading save…")?;
                Job::start("Load factory", move |_| {
                    Ok(Completed::Loaded(Box::new(saves::read(&root, slot)?)))
                })?
            }
            6 => Job::start("List saves", move |_| {
                Ok(Completed::Catalog(saves::catalog(&root)))
            })?,
            _ => anyhow::bail!("unknown persistence command"),
        }));
        set_session(world, 125, 1.)?;
        Ok(())
    }
}
