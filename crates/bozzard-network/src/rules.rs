//! Authored Rhai rules for the bounded reference protocol. Transport owns peer
//! identity, acknowledgements and authority; scripts own gameplay state changes.
use crate::{
    flap::{Bird, Pipe},
    *,
};
use bozzard_scene::ScriptModule;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct Rules {
    player: ScriptModule,
    world: ScriptModule,
}
impl Rules {
    pub fn new(player: ScriptModule, world: ScriptModule) -> Result<Arc<Self>> {
        for (name, args) in [
            ("network_spawn", 1),
            ("network_predict", 3),
            ("network_input", 1),
        ] {
            player.require_function(name, args)?;
        }
        for (name, args) in [
            ("network_pipes", 0),
            ("network_step", 2),
            ("network_resolve", 3),
            ("network_finished", 1),
            ("network_countdown", 0),
        ] {
            world.require_function(name, args)?;
        }
        let rules = Arc::new(Self { player, world });
        for slot in 0..MAX_PLAYERS as u8 {
            rules.spawn(slot)?;
        }
        rules.pipes()?;
        rules.countdown()?;
        Ok(rules)
    }
    pub fn fingerprint(&self) -> String {
        format!(
            "{:016x}-{:016x}",
            self.player.fingerprint(),
            self.world.fingerprint()
        )
    }
    pub fn input(&self, key: &str) -> Result<bool> {
        self.player.call("network_input", vec![json!(key)])
    }
    pub fn spawn(&self, slot: u8) -> Result<Bird> {
        let bird: Bird = self.player.call("network_spawn", vec![json!(slot)])?;
        validate_bird(&bird)?;
        ensure!(
            bird.slot == slot && bird.input_ack == 0,
            "script changed player identity"
        );
        Ok(bird)
    }
    pub fn countdown(&self) -> Result<u16> {
        let ticks: u16 = self.world.call("network_countdown", vec![])?;
        ensure!(
            (1..=3600).contains(&ticks),
            "script countdown must be 1..3600 ticks"
        );
        Ok(ticks)
    }
    pub fn pipes(&self) -> Result<[Pipe; 3]> {
        let pipes = self.world.call("network_pipes", vec![])?;
        validate_pipes(&pipes)?;
        Ok(pipes)
    }
    pub fn predict(&self, bird: &mut Bird, pressed: bool) -> Result<()> {
        let next: Bird = self.player.call(
            "network_predict",
            vec![json!(bird), json!(pressed), json!(DT)],
        )?;
        validate_bird(&next)?;
        ensure!(
            next.slot == bird.slot
                && next.input_ack == bird.input_ack
                && next.alive == bird.alive
                && next.score == bird.score,
            "prediction cannot change identity, score or elimination"
        );
        *bird = next;
        Ok(())
    }
    pub fn step(&self, pipes: &mut [Pipe; 3]) -> Result<()> {
        let next = self
            .world
            .call("network_step", vec![json!(pipes), json!(DT)])?;
        validate_pipes(&next)?;
        *pipes = next;
        Ok(())
    }
    pub fn resolve(&self, bird: &mut Bird, before: &[Pipe; 3], after: &[Pipe; 3]) -> Result<()> {
        let next: Bird = self.world.call(
            "network_resolve",
            vec![json!(bird), json!(before), json!(after)],
        )?;
        validate_bird(&next)?;
        ensure!(
            next.slot == bird.slot && next.input_ack == bird.input_ack,
            "script changed player identity"
        );
        *bird = next;
        Ok(())
    }
    pub fn finished(&self, birds: &[Bird]) -> Result<bool> {
        self.world.call("network_finished", vec![json!(birds)])
    }
}
pub(crate) fn validate_bird(b: &Bird) -> Result<()> {
    ensure!(
        b.slot < MAX_PLAYERS as u8
            && b.x.is_finite()
            && b.x.abs() < 1000.
            && b.y.is_finite()
            && b.y.abs() < 1000.
            && b.velocity.is_finite()
            && b.velocity.abs() < 1000.,
        "invalid player state"
    );
    Ok(())
}
pub(crate) fn validate_pipes(pipes: &[Pipe; 3]) -> Result<()> {
    ensure!(
        pipes.iter().all(|p| p.x.is_finite()
            && p.x.abs() < 1000.
            && p.gap.is_finite()
            && p.gap.abs() < 1000.),
        "invalid obstacle state"
    );
    Ok(())
}
