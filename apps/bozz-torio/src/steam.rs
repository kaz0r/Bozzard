use crate::sim::Game;
#[cfg(feature = "steam")]
use crate::sim::Item;

#[cfg(feature = "steam")]
pub struct SteamBridge {
    client: Option<steamworks::Client>,
    stats_ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _stats_callback: Option<steamworks::CallbackHandle>,
    status: String,
    awarded: std::collections::HashSet<&'static str>,
}

#[cfg(feature = "steam")]
impl SteamBridge {
    pub fn new(app_id: u32, offline: bool) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let stats_ready = Arc::new(AtomicBool::new(false));
        if offline {
            return Self {
                client: None,
                stats_ready,
                _stats_callback: None,
                status: "Offline mode".into(),
                awarded: Default::default(),
            };
        }
        match steamworks::Client::init_app(app_id) {
            Ok(client) => {
                let ready = Arc::clone(&stats_ready);
                let handle =
                    client.register_callback(move |event: steamworks::UserStatsReceived| {
                        ready.store(event.result.is_ok(), Ordering::Relaxed);
                    });
                client
                    .user_stats()
                    .request_user_stats(client.user().steam_id().raw());
                client
                    .friends()
                    .set_rich_presence("status", Some("Building a Bozz-torio factory"));
                Self {
                    client: Some(client),
                    stats_ready,
                    _stats_callback: Some(handle),
                    status: format!("Steam connected · app {app_id}"),
                    awarded: Default::default(),
                }
            }
            Err(error) => Self {
                client: None,
                stats_ready,
                _stats_callback: None,
                status: format!("Offline · {error}"),
                awarded: Default::default(),
            },
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn connected(&self) -> bool {
        self.client.is_some()
    }
    pub fn overlay(&self) {
        if let Some(client) = &self.client {
            client.friends().activate_game_overlay("Friends");
        }
    }
    pub fn pump(&mut self, game: &Game) {
        use std::sync::atomic::Ordering;
        let Some(client) = &self.client else { return };
        client.run_callbacks();
        if !self.stats_ready.load(Ordering::Relaxed) {
            return;
        }
        let conditions = [
            (
                "BOZZ_FIRST_INGOT",
                game.produced[Item::IronBar.index()] + game.produced[Item::CopperBar.index()] > 0,
            ),
            ("BOZZ_FIRST_DELIVERY", game.order_index > 0),
            ("BOZZ_FACTORY_BUILDER", game.placed >= 20),
            ("BOZZ_CIRCUIT_AGE", game.produced[Item::Circuit.index()] > 0),
        ];
        let mut changed = false;
        for (name, achieved) in conditions {
            if achieved
                && !self.awarded.contains(name)
                && client.user_stats().achievement(name).set().is_ok()
            {
                self.awarded.insert(name);
                changed = true;
            }
        }
        if changed {
            let _ = client.user_stats().store_stats();
        }
    }
}

#[cfg(not(feature = "steam"))]
pub struct SteamBridge;

#[cfg(not(feature = "steam"))]
impl SteamBridge {
    pub fn new(_app_id: u32, _offline: bool) -> Self {
        Self
    }
    pub fn status(&self) -> &str {
        "Steam support not compiled"
    }
    pub fn connected(&self) -> bool {
        false
    }
    pub fn overlay(&self) {}
    pub fn pump(&mut self, _game: &Game) {}
}
