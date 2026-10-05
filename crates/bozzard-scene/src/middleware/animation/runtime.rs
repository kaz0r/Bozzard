//! Serializable playback state; preparation and pose composition are kept in focused helpers.
use super::{
    Animator, Repeat,
    data::Pose,
    evaluation::{Cache, sample_mix},
    ik, layers,
    motion::Mix,
    root,
    warp::WarpGoal,
};
use crate::{
    SceneInstance, Transform, World,
    middleware::{
        curve::Playhead,
        signals::{Kind, Signal, Signals},
        timeline::crossed_markers,
    },
};
use anyhow::{Context as _, Result, ensure};
use glam::Mat4;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    pub from: Vec<Pose>,
    pub elapsed: f32,
    pub duration: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayerPlayer {
    pub clock: Playhead,
    pub active: bool,
    pub include_start: bool,
    pub weight: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Player {
    pub dirty: bool,
    pub signature: u64,
    pub state: usize,
    pub initialized: bool,
    pub include_start: bool,
    pub clock: Playhead,
    pub parameters: BTreeMap<String, f32>,
    pub fade: Option<Fade>,
    pub pose: Arc<Vec<Pose>>,
    /// Unlayered outgoing pose avoids applying an additive layer twice during a state fade.
    pub base_pose: Arc<Vec<Pose>>,
    pub palette: Arc<Vec<[f32; 16]>>,
    pub layers: Vec<LayerPlayer>,
    pub ik: Vec<ik::IkState>,
    pub pelvis_offset: f32,
    pub warp_targets: BTreeMap<String, WarpGoal>,
    #[serde(skip)]
    pub(super) cache: Cache,
}
impl Player {
    pub(super) fn initialize(&mut self, animator: &Animator) {
        if !self.initialized {
            self.state = animator
                .states
                .iter()
                .position(|s| s.name == animator.initial)
                .unwrap_or(0);
            self.parameters = animator.parameters.clone();
            self.clock.playing = animator.autoplay;
            self.include_start = animator.autoplay;
            self.initialized = true;
            self.signature = animator.rig.signature();
            self.dirty = true;
        }
        self.layers
            .resize(animator.layers.len(), LayerPlayer::default());
        self.ik.resize(animator.ik.len(), ik::IkState::default());
    }
    fn transition(&mut self, target: usize, duration: f32) {
        self.fade = (duration > 0. && !self.pose.is_empty()).then(|| Fade {
            from: if self.base_pose.is_empty() {
                self.pose.as_ref()
            } else {
                self.base_pose.as_ref()
            }
            .clone(),
            elapsed: 0.,
            duration,
        });
        self.state = target;
        self.clock = Playhead::default();
        self.clock.play(true);
        self.include_start = true;
        self.ik.fill(ik::IkState::default());
        self.pelvis_offset = 0.;
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub players: BTreeMap<String, Player>,
}
#[derive(Clone, Debug)]
pub enum Control {
    Play { state: String, fade: f32 },
    Pause,
    Stop,
    Seek(f32),
    Parameter { name: String, value: f32 },
    RestartLayer { name: String },
    WarpTarget { name: String, goal: WarpGoal },
    ClearWarpTarget { name: String },
}
impl SceneInstance {
    pub fn control_animation(
        &self,
        world: &mut World,
        owner: &str,
        control: Control,
    ) -> Result<()> {
        let entity = self.entity(owner).context("animation target is missing")?;
        let animator = world
            .get::<Animator>(entity)
            .context("target has no Animator")?;
        validate_control(animator, &control)?;
        let state = if let Control::Play { state, .. } = &control {
            animator.states.iter().position(|s| &s.name == state)
        } else {
            None
        };
        let layer_index = if let Control::RestartLayer { name } = &control {
            animator.layers.iter().position(|layer| &layer.name == name)
        } else {
            None
        };
        if let Control::Parameter { name, value } = &control
            && world
                .resource::<Runtime>()
                .and_then(|r| r.players.get(owner))
                .is_some_and(|p| p.parameters.get(name) == Some(value))
        {
            return Ok(());
        }
        // Initializing ordinarily happens on spawn; retain support for runtime component additions.
        let initial = (!world
            .resource::<Runtime>()
            .is_some_and(|r| r.players.contains_key(owner)))
        .then(|| animator.clone());
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let player = world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .entry(owner.into())
            .or_default();
        if let Some(initial) = &initial {
            player.initialize(initial);
        }
        player.dirty = true;
        match control {
            Control::Play { fade, .. } => player.transition(state.unwrap(), fade),
            Control::Pause => player.clock.playing = false,
            Control::Stop => {
                player.clock = Playhead::default();
                player.fade = None;
                player.include_start = false;
                player.layers.fill(LayerPlayer::default());
                player.ik.fill(ik::IkState::default());
            }
            Control::Seek(value) => {
                player.clock.seek(f64::from(value))?;
                player.fade = None;
                player.include_start = false;
                player.ik.fill(ik::IkState::default());
            }
            Control::Parameter { name, value } => {
                player.parameters.insert(name, value);
            }
            Control::RestartLayer { .. } => {
                player.layers[layer_index.unwrap()] = LayerPlayer::default()
            }
            Control::WarpTarget { name, goal } => {
                player.warp_targets.insert(name, goal);
            }
            Control::ClearWarpTarget { name } => {
                player.warp_targets.remove(&name);
            }
        }
        Ok(())
    }
    pub fn step_animations(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid animation timestep");
        let owners = self.component_entities::<Animator>(world);
        if owners.is_empty() {
            return Ok(());
        }
        let (needs_ground, needs_objects) = owners
            .iter()
            .filter_map(|(_, entity)| world.get::<Animator>(**entity).filter(|a| a.enabled))
            .fold((false, false), |(ground, objects), animator| {
                (
                    ground
                        || animator
                            .ik
                            .iter()
                            .any(|c| matches!(c.target, ik::IkTarget::Ground { .. })),
                    objects
                        || animator
                            .ik
                            .iter()
                            .any(|c| matches!(c.target, ik::IkTarget::Object { .. }))
                        || animator
                            .warps
                            .iter()
                            .any(|w| matches!(w.target, super::warp::WarpTarget::Object { .. })),
                )
            });
        // World-point goals need only their actor matrix. Object goals need transforms;
        // ground probes additionally need collision geometry and support transforms.
        let (collisions, objects) = if needs_ground {
            self.collision_geometry(world)?
        } else if needs_objects {
            (Default::default(), self.global_transforms(world)?)
        } else {
            (Default::default(), BTreeMap::new())
        };
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let mut signals = world.remove_resource::<Signals>().unwrap_or_default();
        signals.begin(Kind::Animation);
        let result = (|| -> Result<()> {
            for (owner, &entity) in owners {
                let animator = world
                    .get::<Animator>(entity)
                    .context("Animator was removed")?;
                let player = runtime.players.entry(owner.clone()).or_default();
                player.initialize(animator);
                if !animator.enabled {
                    continue;
                }
                let spatial = animator.root_motion.is_some() || !animator.ik.is_empty();
                let local = if spatial {
                    *world
                        .get::<Transform>(entity)
                        .context("animation owner has no transform")?
                } else {
                    Transform::default()
                };
                let model = if spatial {
                    self.global_transform(world, owner)?
                } else {
                    Mat4::IDENTITY
                };
                let context = Frame {
                    owner,
                    local,
                    model,
                    collisions: &collisions,
                    objects: &objects,
                    dt,
                };
                let transform = evaluate(animator, player, &mut signals, &context)
                    .with_context(|| format!("animating '{owner}'"))?;
                if let Some(transform) = transform {
                    world.insert(entity, transform)?;
                }
            }
            runtime.players.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Animator>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        world.insert_resource(signals);
        result
    }
    /// Model-space bones composed with the actor's live world transform, useful for attachments.
    pub fn animation_bone_transform(
        &self,
        world: &World,
        owner: &str,
        bone: usize,
    ) -> Result<Mat4> {
        let entity = self.entity(owner).context("animation owner is missing")?;
        let animator = world
            .get::<Animator>(entity)
            .context("target has no Animator")?;
        ensure!(bone < animator.rig.nodes.len(), "animation bone is missing");
        let player = world
            .resource::<Runtime>()
            .and_then(|r| r.players.get(owner))
            .context("animation pose is unavailable")?;
        let mut globals = Vec::new();
        animator.rig.globals_into(&player.pose, &mut globals)?;
        Ok(self.global_transform(world, owner)? * globals[bone])
    }
}
fn validate_control(animator: &Animator, control: &Control) -> Result<()> {
    match control {
        Control::Play { state, fade } => {
            ensure!(
                fade.is_finite() && (0.0..=60.).contains(fade),
                "invalid animation fade"
            );
            ensure!(
                animator.states.iter().any(|s| &s.name == state),
                "animation state does not exist"
            );
        }
        Control::Seek(value) => ensure!(
            value.is_finite() && (0.0..=1.).contains(value),
            "animation seek is normalized 0–1"
        ),
        Control::Parameter { name, value } => ensure!(
            animator.parameters.contains_key(name) && value.is_finite(),
            "animation parameter is missing or non-finite"
        ),
        Control::RestartLayer { name } => ensure!(
            animator.layers.iter().any(|l| &l.name == name),
            "animation layer is missing"
        ),
        Control::WarpTarget { name, goal } => {
            ensure!(
                animator.warps.iter().any(|w| &w.name == name),
                "motion-warp window is missing"
            );
            goal.validate()?;
        }
        Control::ClearWarpTarget { name } => ensure!(
            animator.warps.iter().any(|w| &w.name == name),
            "motion-warp window is missing"
        ),
        Control::Pause | Control::Stop => {}
    }
    Ok(())
}
struct Frame<'a> {
    owner: &'a str,
    local: Transform,
    model: Mat4,
    collisions: &'a crate::CollisionSnapshot,
    objects: &'a BTreeMap<String, Mat4>,
    dt: f32,
}
struct Tick {
    before: Playhead,
    after: Playhead,
    mix: Mix,
}
fn advance(animator: &Animator, player: &mut Player, dt: f32) -> Result<Tick> {
    let current = &animator.states[player.state];
    if player.clock.playing
        && let Some(transition) = animator.transitions.iter().find(|t| {
            (t.from == "*" || t.from == current.name)
                && t.to != current.name
                && t.exit_time
                    .is_none_or(|v| player.clock.position(1., current.repeat) >= v)
                && t.comparison
                    .matches(player.parameters[&t.parameter], t.threshold)
        })
    {
        let target = animator
            .states
            .iter()
            .position(|s| s.name == transition.to)
            .unwrap();
        player.transition(target, transition.fade);
    }
    let state = &animator.states[player.state];
    let mix = player.cache.states[player.state].weights(&state.motion, &player.parameters);
    let before = player.clock;
    let mut after = before;
    after.advance(
        dt,
        animator.speed / mix.duration(&animator.rig),
        1.,
        state.repeat,
    )?;
    Ok(Tick { before, after, mix })
}
fn evaluate(
    animator: &Animator,
    player: &mut Player,
    signals: &mut Signals,
    context: &Frame<'_>,
) -> Result<Option<Transform>> {
    player.dirty |= player.cache.prepare(animator)?;
    if animator.states.is_empty() {
        return Ok(None);
    }
    // Paused actors reuse their palette before blend lookup or clock work. Ground contacts
    // still update so a stationary actor follows a moving support.
    if !player.clock.playing && !needs_pose_update(animator, player) {
        return Ok(None);
    }
    let tick = advance(animator, player, context.dt)?;
    if tick.before.elapsed == tick.after.elapsed && !needs_pose_update(animator, player) {
        return Ok(None);
    }
    let state = &animator.states[player.state];
    let phase = tick.after.position(1., state.repeat);
    let pose = Arc::make_mut(&mut player.pose);
    sample_mix(
        &animator.rig,
        tick.mix,
        phase,
        pose,
        &mut player.cache.scratch,
    )?;
    fade_pose(pose, &mut player.fade, tick.before.playing, context.dt);
    if !animator.layers.is_empty() || !animator.ik.is_empty() || animator.root_motion.is_some() {
        Arc::make_mut(&mut player.base_pose).clone_from(player.pose.as_ref());
    }
    apply_layers(animator, player, &tick, signals, context)?;
    let transform = root::apply(
        animator,
        player,
        root::Step {
            before: tick.before,
            after: tick.after,
            mix: tick.mix,
            local: context.local,
            model: context.model,
            objects: context.objects,
        },
    )?;
    apply_ik(animator, player, transform, context)?;
    animator.rig.palette_into(
        &player.pose,
        &mut player.cache.globals,
        Arc::make_mut(&mut player.palette),
    )?;
    emit_events(
        animator,
        tick.mix,
        EventSpan {
            before: tick.before,
            after: tick.after,
            repeat: state.repeat,
            include_start: player.include_start,
            layer: "",
            owner: context.owner,
        },
        signals,
    )?;
    player.clock = tick.after;
    player.dirty = false;
    player.include_start = false;
    Ok(transform)
}
fn needs_pose_update(animator: &Animator, player: &Player) -> bool {
    player.dirty
        || player.fade.is_some()
        || player.include_start
        || player.pose.is_empty()
        || !animator.ik.is_empty()
}
fn fade_pose(pose: &mut [Pose], fade: &mut Option<Fade>, playing: bool, dt: f32) {
    let Some(current) = fade else { return };
    if playing {
        current.elapsed = (current.elapsed + dt).min(current.duration);
    }
    for (pose, from) in pose.iter_mut().zip(&current.from) {
        *pose = from.blend(*pose, current.elapsed / current.duration);
    }
    if current.elapsed >= current.duration {
        *fade = None;
    }
}
fn apply_layers(
    animator: &Animator,
    player: &mut Player,
    tick: &Tick,
    signals: &mut Signals,
    context: &Frame<'_>,
) -> Result<()> {
    for (index, layer) in animator.layers.iter().enumerate() {
        let run = &mut player.layers[index];
        let desired = layer.weight(&player.parameters);
        if layer.fade == 0. || (!tick.before.playing && context.dt == 0.) {
            run.weight = desired;
        } else if tick.before.playing {
            let change = context.dt / layer.fade;
            run.weight += (desired - run.weight).clamp(-change, change);
        }
        if run.weight <= 0. {
            run.active = false;
            continue;
        }
        if !run.active {
            run.clock = Playhead::default();
            run.include_start = true;
            run.active = true;
        }
        run.clock.playing = tick.before.playing;
        let before = run.clock;
        let mix = player.cache.layers[index].weights(&layer.motion, &player.parameters);
        let sample_phase = if layer.synchronized {
            run.clock = tick.after;
            tick.after
                .position(1., animator.states[player.state].repeat)
        } else {
            run.clock.advance(
                context.dt,
                layer.speed / mix.duration(&animator.rig),
                1.,
                layer.repeat,
            )?;
            run.clock.position(1., layer.repeat)
        };
        sample_mix(
            &animator.rig,
            mix,
            sample_phase,
            &mut player.cache.overlay,
            &mut player.cache.scratch,
        )?;
        layers::apply(
            Arc::make_mut(&mut player.pose).as_mut_slice(),
            &player.cache.overlay,
            &player.cache.references[index],
            &player.cache.masks[index],
            layer.blend,
            run.weight,
        );
        emit_events(
            animator,
            mix,
            EventSpan {
                before: if layer.synchronized {
                    tick.before
                } else {
                    before
                },
                after: run.clock,
                repeat: if layer.synchronized {
                    animator.states[player.state].repeat
                } else {
                    layer.repeat
                },
                include_start: run.include_start,
                layer: &layer.name,
                owner: context.owner,
            },
            signals,
        )?;
        run.include_start = false;
    }
    Ok(())
}
fn apply_ik(
    animator: &Animator,
    player: &mut Player,
    transform: Option<Transform>,
    frame: &Frame<'_>,
) -> Result<()> {
    if animator.ik.is_empty() {
        return Ok(());
    }
    let model = transform.map_or(frame.model, |next| {
        frame.model * frame.local.matrix().inverse() * next.matrix()
    });
    let context = ik::Context {
        owner: frame.owner,
        model,
        objects: frame.objects,
        collisions: frame.collisions,
        rest_globals: &player.cache.rest_globals,
        dt: frame.dt,
    };
    animator
        .rig
        .globals_into(&player.pose, &mut player.cache.globals)?;
    player.cache.goals.clear();
    player.cache.feet.clear();
    let mut budget = 1_000_000;
    for (index, constraint) in animator.ik.iter().enumerate() {
        let goal = ik::prepare(
            constraint,
            &mut player.ik[index],
            &player.cache.globals,
            constraint.weight(&player.parameters),
            &context,
            &mut budget,
        )?;
        if matches!(constraint.target, ik::IkTarget::Ground { .. })
            && let Some(goal) = goal
        {
            player.cache.feet.push((constraint.tip, goal));
        }
        player.cache.goals.push(goal);
    }
    if let Some(settings) = &animator.foot_placement {
        ik::adjust_pelvis(
            settings,
            &animator.rig,
            Arc::make_mut(&mut player.pose).as_mut_slice(),
            &player.cache.globals,
            &player.cache.feet,
            &mut player.pelvis_offset,
            &context,
        );
    }
    for (constraint, goal) in animator.ik.iter().zip(&player.cache.goals) {
        if let Some(goal) = goal {
            ik::apply(
                &animator.rig,
                Arc::make_mut(&mut player.pose).as_mut_slice(),
                &mut player.cache.globals,
                constraint,
                *goal,
                &context,
            )?;
        }
    }
    Ok(())
}
struct EventSpan<'a> {
    before: Playhead,
    after: Playhead,
    repeat: Repeat,
    include_start: bool,
    layer: &'a str,
    owner: &'a str,
}
fn emit_events(
    animator: &Animator,
    mix: Mix,
    span: EventSpan<'_>,
    signals: &mut Signals,
) -> Result<()> {
    let EventSpan {
        before,
        after,
        repeat,
        include_start,
        layer,
        owner,
    } = span;
    let clip = &animator.rig.clips[mix.dominant()];
    if clip.events.is_empty()
        || !before.playing
        || (before.elapsed == after.elapsed && !include_start)
    {
        return Ok(());
    }
    let hits = crossed_markers(
        clip.events.iter().map(|e| e.time / clip.duration),
        before.elapsed,
        after.elapsed,
        1.,
        repeat,
        include_start,
    )?;
    for hit in hits {
        let event = &clip.events[hit];
        signals.emit(
            owner,
            Signal {
                kind: Kind::Animation,
                name: if layer.is_empty() {
                    event.name.clone()
                } else {
                    format!("{layer}/{}", event.name)
                },
                other: None,
                value: event.time,
            },
        )?;
    }
    Ok(())
}
