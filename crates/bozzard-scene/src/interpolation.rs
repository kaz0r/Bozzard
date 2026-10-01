//! Presentation history belongs to one live world; it is never serialized.
use super::*;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub entity: Entity,
    pub previous: Transform,
    pub current: Transform,
    pub discontinuous: bool,
    active: bool,
}

pub(super) struct History {
    instance: u64,
    revision: u64,
    tick: u64,
    pub fraction: f32,
    pub samples: Vec<Option<Sample>>,
    moving: Vec<usize>,
    resets: HashSet<Entity>,
    cameras: [Option<Entity>; 2],
}

impl History {
    fn new(instance: &SceneInstance, world: &World) -> Result<Self> {
        let mut history = Self {
            instance: instance.instance_id,
            revision: u64::MAX,
            tick: world.change_tick(),
            fraction: 1.,
            samples: Vec::new(),
            moving: Vec::new(),
            resets: HashSet::new(),
            cameras: active_cameras(instance, world),
        };
        history.sync_membership(instance, world)?;
        Ok(history)
    }

    fn sync_membership(&mut self, instance: &SceneInstance, world: &World) -> Result<()> {
        if self.instance == instance.instance_id && self.revision == instance.hierarchy_revision {
            return Ok(());
        }
        let mut samples = vec![None; instance.document.objects.len()];
        if self.instance == instance.instance_id {
            for sample in self.samples.iter().flatten() {
                if let Some(&index) = instance.object_indices.get(&sample.entity) {
                    samples[index] = Some(*sample);
                }
            }
        } else {
            self.resets.clear();
            self.tick = world.change_tick();
        }
        self.moving.clear();
        for (index, object) in instance.document.objects.iter().enumerate() {
            if let Some(sample) = samples[index] {
                if sample.active {
                    self.moving.push(index);
                }
            } else {
                let entity = instance.entities[&object.id];
                let current = *world
                    .get::<Transform>(entity)
                    .context("scene object/transform was removed")?;
                samples[index] = Some(Sample {
                    entity,
                    previous: current,
                    current,
                    discontinuous: false,
                    active: false,
                });
            }
        }
        self.resets
            .retain(|e| instance.object_indices.contains_key(e));
        self.samples = samples;
        self.instance = instance.instance_id;
        self.revision = instance.hierarchy_revision;
        Ok(())
    }

    pub fn matches(&self, instance: &SceneInstance) -> bool {
        self.instance == instance.instance_id && self.revision == instance.hierarchy_revision
    }

    pub fn has_motion(&self) -> bool {
        !self.moving.is_empty()
    }

    /// A discontinuity in an ancestor snaps the entire subtree. Sharing this rule
    /// keeps full view extraction and single-camera UI projection in agreement.
    pub fn local_sample(
        &self,
        index: usize,
        entity: Entity,
        local: Transform,
        parent_snapped: bool,
    ) -> (bool, Option<Sample>) {
        let sample = self.samples[index].as_ref().filter(|s| s.entity == entity);
        let snapped =
            parent_snapped || sample.is_none_or(|s| s.discontinuous || s.current != local);
        (
            snapped,
            // Inactive samples already have previous == current. Avoid comparing
            // or copying two full poses for every static object on every frame.
            sample.filter(|s| !snapped && s.active).copied(),
        )
    }

    fn synchronize_external_writes(
        &mut self,
        instance: &SceneInstance,
        world: &World,
    ) -> Result<()> {
        for (entity, current) in world.changed_since::<Transform>(self.tick.saturating_sub(1)) {
            let Some(&index) = instance.object_indices.get(&entity) else {
                continue;
            };
            let sample = self.samples[index]
                .as_mut()
                .expect("live presentation sample");
            if sample.current == *current
                || world
                    .changed_tick::<Transform>(entity)
                    .is_none_or(|tick| tick > self.tick)
            {
                continue;
            }
            current.validate()?;
            // Host camera/input writes can be overwritten by the next tick's systems.
            // Save their new baseline before that tick to avoid rewinding.
            sample.previous = *current;
            sample.current = *current;
            sample.discontinuous = true;
            if !sample.active {
                sample.active = true;
                self.moving.push(index);
            }
        }
        Ok(())
    }

    fn capture(&mut self, instance: &SceneInstance, world: &World) -> Result<()> {
        self.sync_membership(instance, world)?;
        let cameras = active_cameras(instance, world);
        for (previous, current) in self.cameras.iter().zip(cameras) {
            if *previous != current
                && let Some(entity) = current
            {
                self.resets.insert(entity);
            }
        }
        self.cameras = cameras;
        for index in self.moving.drain(..) {
            let sample = self.samples[index]
                .as_mut()
                .expect("live presentation sample");
            sample.previous = sample.current;
            sample.discontinuous = false;
            sample.active = false;
        }
        // Include the preceding change tick: a host may write a camera or transform
        // between fixed ticks. Such writes snap instead of replaying stale history.
        for (entity, current) in world.changed_since::<Transform>(self.tick.saturating_sub(1)) {
            let Some(&index) = instance.object_indices.get(&entity) else {
                continue;
            };
            let sample = self.samples[index]
                .as_mut()
                .expect("live presentation sample");
            if sample.current == *current {
                continue;
            }
            current.validate()?;
            let discontinuous = self.resets.contains(&entity)
                || world
                    .changed_tick::<Transform>(entity)
                    .is_some_and(|tick| tick <= self.tick)
                || sample
                    .current
                    .scale
                    .iter()
                    .zip(current.scale)
                    .any(|(a, b)| a.signum() != b.signum());
            sample.previous = if discontinuous {
                *current
            } else {
                sample.current
            };
            sample.current = *current;
            sample.discontinuous = discontinuous;
            sample.active = true;
            self.moving.push(index);
        }
        for entity in self.resets.drain() {
            let Some(&index) = instance.object_indices.get(&entity) else {
                continue;
            };
            let sample = self.samples[index]
                .as_mut()
                .expect("live presentation sample");
            sample.previous = sample.current;
            sample.discontinuous = true;
            if !sample.active {
                sample.active = true;
                self.moving.push(index);
            }
        }
        self.tick = world.change_tick();
        Ok(())
    }
}

fn active_cameras(instance: &SceneInstance, world: &World) -> [Option<Entity>; 2] {
    [Layer::TwoD, Layer::ThreeD].map(|layer| {
        world
            .resource::<middleware::timeline::Runtime>()
            .and_then(|r| r.cameras.get(&layer))
            .filter(|id| {
                instance
                    .entity(id)
                    .is_some_and(|e| world.get::<Camera>(e).is_some())
            })
            .or_else(|| instance.document.views.get(&layer))
            .and_then(|id| instance.entity(id))
    })
}

impl SceneInstance {
    pub fn render_interpolation_enabled(world: &World) -> bool {
        world.resource::<History>().is_some()
    }
    /// Keep projected UI labels on the same camera pose as this native frame.
    pub fn set_render_interpolation_fraction(world: &mut World, fraction: f32) -> Result<()> {
        ensure!(
            fraction.is_finite() && (0. ..=1.).contains(&fraction),
            "interpolation fraction must be finite and within 0..1"
        );
        if let Some(history) = world.resource_mut::<History>() {
            history.fraction = fraction;
        }
        Ok(())
    }
    /// Native hosts enable history before advancing their first presentation tick.
    /// Disabling releases its memory; authored and headless worlds need no history.
    pub fn set_render_interpolation(&self, world: &mut World, enabled: bool) -> Result<()> {
        if !enabled {
            world.remove_resource::<History>();
        } else {
            let mut history = match world.remove_resource::<History>() {
                Some(history) => history,
                None => History::new(self, world)?,
            };
            let result = history
                .sync_membership(self, world)
                .and_then(|()| history.synchronize_external_writes(self, world));
            world.insert_resource(history);
            result?;
        }
        Ok(())
    }

    /// Call once after a completed fixed tick, including its deferred commands.
    /// Only changed transforms and the preceding tick's moving samples are updated.
    pub fn capture_render_transforms(&self, world: &mut World) -> Result<()> {
        let Some(mut history) = world.remove_resource::<History>() else {
            return Ok(());
        };
        let result = history.capture(self, world);
        world.insert_resource(history);
        result
    }

    /// Snap an object and its descendants after teleporting or seeking a pose.
    /// Ordinary Set Position remains continuous; gameplay can explicitly mark a jump.
    pub fn reset_render_interpolation(&self, world: &mut World, id: &str) -> Result<()> {
        let entity = self
            .entity(id)
            .context("interpolation target does not exist")?;
        if let Some(history) = world.resource_mut::<History>() {
            history.resets.insert(entity);
            if history.matches(self)
                && let Some(sample) = history.samples[self.object_indices[&entity]].as_mut()
            {
                sample.discontinuous = true;
            }
        }
        Ok(())
    }
}

pub(super) fn matrix(previous: Transform, current: Transform, alpha: f32) -> Mat4 {
    if alpha == 0. {
        return previous.matrix();
    }
    let rotation = |transform: Transform| {
        let [x, y, z] = transform.rotation_degrees.map(f32::to_radians);
        Quat::from_euler(EulerRot::YXZ, y, x, z)
    };
    let current_rotation = rotation(current);
    let rotation = if previous.rotation_degrees == current.rotation_degrees {
        current_rotation
    } else {
        rotation(previous).slerp(current_rotation, alpha)
    };
    // Weighted endpoints avoid overflow in (end - start) for large valid coordinates.
    let mix = |a: [f32; 3], b: [f32; 3]| Vec3::from(a) * (1. - alpha) + Vec3::from(b) * alpha;
    Mat4::from_scale_rotation_translation(
        mix(previous.scale, current.scale),
        rotation,
        mix(previous.translation, current.translation),
    )
}
