//! Playback state belongs to checkpoints, while authored defaults remain in the scene document.
use super::{animation, audio, navigation, registry, sprite, timeline, tween, ui};
use crate::{Scene, World};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Save {
    pub ui: Option<ui::Runtime>,
    pub ui_preferences: Option<ui::Preferences>,
    pub sprites: Option<sprite::Runtime>,
    pub navigation: Option<navigation::Runtime>,
    pub audio: Option<audio::Runtime>,
    pub tweens: Option<tween::Runtime>,
    pub timelines: Option<timeline::Runtime>,
    pub animations: Option<animation::Runtime>,
}
impl Save {
    pub fn capture(world: &World) -> Self {
        Self {
            ui: world.resource::<ui::Runtime>().cloned(),
            ui_preferences: world.resource::<ui::Preferences>().cloned(),
            sprites: world.resource::<sprite::Runtime>().cloned(),
            navigation: world.resource::<navigation::Runtime>().cloned(),
            audio: world.resource::<audio::Runtime>().cloned(),
            tweens: world.resource::<tween::Runtime>().cloned(),
            timelines: world.resource::<timeline::Runtime>().cloned(),
            animations: world.resource::<animation::Runtime>().cloned(),
        }
    }
    pub fn validate(&self, scene: &Scene) -> Result<()> {
        if let Some(runtime) = &self.ui {
            runtime.validate(scene)?;
        }
        if let Some(preferences) = &self.ui_preferences {
            preferences.validate()?;
        }
        if let Some(runtime) = &self.sprites {
            ensure!(
                runtime.players.len() <= scene.objects.len(),
                "too many saved sprite players"
            );
            for (owner, run) in &runtime.players {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved sprite owner missing"))?;
                let source = registry::get::<sprite::Sprite>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved Sprite missing"))?;
                ensure!(
                    run.clip.is_none_or(|i| i < source.clips.len())
                        && run.frame < source.atlas.frames()
                        && run.clock.elapsed.is_finite()
                        && run.clock.elapsed >= 0.,
                    "invalid saved sprite animation"
                );
            }
        }
        if let Some(runtime) = &self.navigation {
            ensure!(
                runtime.agents.len() <= 256 && runtime.next < 256,
                "invalid saved navigation scheduler"
            );
            for (owner, run) in &runtime.agents {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved agent owner missing"))?;
                let agent = registry::get::<navigation::NavAgent>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved Navigation Agent missing"))?;
                ensure!(
                    run.state < agent.states.len()
                        && run.elapsed.is_finite()
                        && (0.0..=86400.).contains(&run.elapsed)
                        && run.repath.is_finite()
                        && (0.0..=60.).contains(&run.repath)
                        && run.path.len() <= 16384
                        && run.cursor <= run.path.len()
                        && run
                            .path
                            .iter()
                            .chain(run.destination.iter())
                            .chain(run.goal.iter())
                            .chain([&run.velocity])
                            .flatten()
                            .all(|n| n.is_finite())
                        && run
                            .target
                            .as_ref()
                            .is_none_or(|id| scene.objects.iter().any(|o| &o.id == id))
                        && run.waypoint < 128,
                    "invalid saved navigation agent"
                );
            }
        }
        if let Some(runtime) = &self.audio {
            ensure!(
                runtime.voices.len() <= 256
                    && runtime
                        .buses
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0.0..=4.).contains(v)),
                "invalid saved audio mixer"
            );
            for (owner, voice) in &runtime.voices {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved audio owner missing"))?;
                let source = registry::get::<audio::AudioSource>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved Audio Source missing"))?;
                ensure!(
                    voice.position.is_finite()
                        && (0.0..=source.duration).contains(&voice.position)
                        && voice.volume.is_finite()
                        && (0.0..=4.).contains(&voice.volume)
                        && voice.pitch.is_finite()
                        && (0.125..=4.).contains(&voice.pitch)
                        && voice.pan.is_finite()
                        && (-1.0..=1.).contains(&voice.pan),
                    "invalid saved audio voice"
                );
            }
        }
        if let Some(runtime) = &self.animations {
            ensure!(
                runtime.players.len() <= scene.objects.len(),
                "too many saved animators"
            );
            for (owner, player) in &runtime.players {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved animator owner is missing"))?;
                let animator = registry::get::<animation::Animator>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved Animator is missing"))?;
                ensure!(
                    player.signature == animator.rig.signature(),
                    "saved animation signature mismatch"
                );
                ensure!(
                    (animator.states.is_empty() && player.state == 0
                        || player.state < animator.states.len())
                        && player.clock.elapsed.is_finite()
                        && player.clock.elapsed >= 0.,
                    "invalid saved animation state or clock"
                );
                ensure!(
                    player.parameters.keys().eq(animator.parameters.keys())
                        && player.parameters.values().all(|v| v.is_finite()),
                    "saved animation parameters do not match authored parameters"
                );
                if !player.pose.is_empty() {
                    let palette = animator.rig.palette(&player.pose)?;
                    ensure!(
                        palette.len() == player.palette.len()
                            && palette
                                .iter()
                                .flatten()
                                .zip(player.palette.iter().flatten())
                                .all(|(a, b)| b.is_finite()
                                    && (a - b).abs() <= 1e-4 * a.abs().max(1.)),
                        "saved skin palette does not match pose"
                    );
                } else {
                    ensure!(
                        player.palette.is_empty(),
                        "saved animation palette has no pose"
                    );
                }
                if let Some(fade) = &player.fade {
                    ensure!(
                        fade.elapsed.is_finite()
                            && fade.duration.is_finite()
                            && fade.duration > 0.
                            && fade.duration <= 60.
                            && (0.0..=fade.duration).contains(&fade.elapsed),
                        "invalid saved animation transition"
                    );
                    animator.rig.palette(&fade.from)?;
                }
            }
        }
        if let Some(runtime) = &self.timelines {
            ensure!(
                runtime.players.len() <= scene.objects.len(),
                "too many saved timelines"
            );
            for (owner, state) in &runtime.players {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved timeline owner is missing"))?;
                let timeline = registry::get::<timeline::Timeline>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved timeline component is missing"))?;
                ensure!(
                    state.clock.elapsed.is_finite()
                        && state.clock.elapsed >= 0.
                        && state.last_position.is_none_or(
                            |p| p.is_finite() && (0.0..=timeline.motion.duration).contains(&p)
                        ),
                    "invalid saved timeline clock"
                );
            }
            for (layer, camera) in &runtime.cameras {
                ensure!(
                    scene.views.contains_key(layer)
                        && scene
                            .objects
                            .iter()
                            .any(|o| &o.id == camera && o.camera.is_some()),
                    "invalid saved cinematic camera"
                );
            }
        }
        if let Some(runtime) = &self.tweens {
            ensure!(
                runtime.players.len() <= scene.objects.len(),
                "too many saved tweens"
            );
            for (owner, state) in &runtime.players {
                let object = scene
                    .objects
                    .iter()
                    .find(|o| &o.id == owner)
                    .ok_or_else(|| anyhow::anyhow!("saved tween owner is missing"))?;
                let tween = registry::get::<tween::Tween>(object)?
                    .ok_or_else(|| anyhow::anyhow!("saved tween component is missing"))?;
                ensure!(
                    state.clock.elapsed.is_finite()
                        && state.clock.elapsed >= 0.
                        && state
                            .last_position
                            .is_none_or(|p| p.is_finite() && (0.0..=tween.duration).contains(&p)),
                    "invalid saved tween clock"
                );
            }
        }
        Ok(())
    }
    pub fn restore(self, world: &mut World) {
        if let Some(runtime) = self.ui {
            world.insert_resource(runtime);
        }
        if let Some(preferences) = self.ui_preferences {
            world.insert_resource(preferences);
        }
        if let Some(runtime) = self.sprites {
            runtime.restore_with_visuals(world);
        }
        if let Some(runtime) = self.navigation {
            world.insert_resource(runtime);
        }
        if let Some(runtime) = self.audio {
            world.insert_resource(runtime);
        }
        if let Some(runtime) = self.animations {
            world.insert_resource(runtime);
        }
        if let Some(runtime) = self.timelines {
            world.insert_resource(runtime);
        }
        if let Some(runtime) = self.tweens {
            world.insert_resource(runtime);
        }
    }
}
pub fn clear(world: &mut World) {
    world.remove_resource::<ui::Runtime>();
    // Accessibility preferences persist across levels; saved games restore their own preferences.
    world.remove_resource::<sprite::Runtime>();
    world.remove_resource::<navigation::Runtime>();
    world.remove_resource::<audio::Runtime>();
    world.remove_resource::<tween::Runtime>();
    world.remove_resource::<timeline::Runtime>();
    world.remove_resource::<animation::Runtime>();
    world.remove_resource::<super::signals::Signals>();
}

/// Release only a departing scene's state; shared mixer settings and other levels survive.
pub(crate) fn remove_objects(world: &mut World, ids: &std::collections::BTreeSet<String>) {
    if let Some(runtime) = world.resource_mut::<ui::Runtime>() {
        runtime.widgets.retain(|id, _| !ids.contains(id));
        for focus in [&mut runtime.focus, &mut runtime.active] {
            if focus.as_ref().is_some_and(|id| ids.contains(id)) {
                *focus = None;
            }
        }
    }
    if let Some(runtime) = world.resource_mut::<sprite::Runtime>() {
        runtime.remove_objects(ids);
    }
    if let Some(runtime) = world.resource_mut::<navigation::Runtime>() {
        runtime.agents.retain(|id, _| !ids.contains(id));
        for agent in runtime.agents.values_mut() {
            if agent.target.as_ref().is_some_and(|id| ids.contains(id)) {
                agent.target = None;
                agent.path.clear();
                agent.cursor = 0;
                agent.sees_target = false;
            }
        }
    }
    if let Some(runtime) = world.resource_mut::<audio::Runtime>() {
        runtime.voices.retain(|id, _| !ids.contains(id));
        runtime.finished.retain(|id| !ids.contains(id));
    }
    if let Some(runtime) = world.resource_mut::<animation::Runtime>() {
        runtime.players.retain(|id, _| !ids.contains(id));
    }
    if let Some(runtime) = world.resource_mut::<timeline::Runtime>() {
        runtime.players.retain(|id, _| !ids.contains(id));
        runtime.cameras.retain(|_, id| !ids.contains(id));
    }
    if let Some(runtime) = world.resource_mut::<tween::Runtime>() {
        runtime.players.retain(|id, _| !ids.contains(id));
        runtime.finished.retain(|id| !ids.contains(id));
    }
    if let Some(signals) = world.resource_mut::<super::signals::Signals>() {
        signals.remove_objects(ids);
    }
}
