use anyhow::{Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PARTICLES: usize = 16_384;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleKind {
    #[default]
    Smoke,
    Ash,
    Sparks,
}
impl ParticleKind {
    pub const ALL: [Self; 3] = [Self::Smoke, Self::Ash, Self::Sparks];
    pub fn name(self) -> &'static str {
        match self {
            Self::Smoke => "Smoke",
            Self::Ash => "Ash",
            Self::Sparks => "Sparks & trails",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleEmitter {
    pub enabled: bool,
    pub kind: ParticleKind,
    pub rate: f32,
    pub lifetime: f32,
    pub radius: f32,
    pub speed: f32,
    pub spread: f32,
    pub start_size: f32,
    pub end_size: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub wind: [f32; 3],
    pub turbulence: f32,
    pub gravity: f32,
    pub drag: f32,
    pub softness: f32,
    pub trail_length: f32,
    pub max_particles: u32,
    pub seed: u32,
}
impl Default for ParticleEmitter {
    fn default() -> Self {
        Self::preset(ParticleKind::Smoke)
    }
}
impl ParticleEmitter {
    pub fn preset(kind: ParticleKind) -> Self {
        let mut value = Self {
            enabled: true,
            kind,
            rate: 14.,
            lifetime: 5.,
            radius: 0.25,
            speed: 0.7,
            spread: 0.3,
            start_size: 0.45,
            end_size: 2.2,
            color: [0.45, 0.49, 0.55],
            opacity: 0.2,
            wind: [0.15, 0., 0.05],
            turbulence: 0.5,
            gravity: 0.15,
            drag: 0.25,
            softness: 0.5,
            trail_length: 0.,
            max_particles: 512,
            seed: 1,
        };
        match kind {
            ParticleKind::Smoke => {}
            ParticleKind::Ash => {
                value.rate = 10.;
                value.lifetime = 7.;
                value.start_size = 0.035;
                value.end_size = 0.015;
                value.speed = 1.;
                value.spread = 0.6;
                value.color = [0.5, 0.45, 0.38];
                value.opacity = 0.75;
                value.gravity = -0.25;
                value.turbulence = 0.7;
                value.softness = 0.15;
            }
            ParticleKind::Sparks => {
                value.rate = 18.;
                value.lifetime = 2.5;
                value.start_size = 0.045;
                value.end_size = 0.008;
                value.speed = 2.3;
                value.spread = 0.8;
                value.color = [1., 0.36, 0.055];
                value.opacity = 1.;
                value.gravity = -0.9;
                value.drag = 0.15;
                value.turbulence = 0.4;
                value.softness = 0.1;
                value.trail_length = 0.12;
            }
        }
        value
    }
    pub fn validate(&self) -> Result<()> {
        let range = |v: f32, min: f32, max: f32, name: &str| -> Result<()> {
            ensure!(
                v.is_finite() && (min..=max).contains(&v),
                "particle {name} must be finite and within {min}..{max}"
            );
            Ok(())
        };
        for (v, min, max, name) in [
            (self.rate, 0., 500., "rate"),
            (self.lifetime, 0.1, 30., "lifetime"),
            (self.radius, 0., 20., "radius"),
            (self.speed, 0., 50., "speed"),
            (self.spread, 0., 20., "spread"),
            (self.start_size, 0.001, 20., "start size"),
            (self.end_size, 0.001, 40., "end size"),
            (self.opacity, 0., 1., "opacity"),
            (self.turbulence, 0., 10., "turbulence"),
            (self.gravity, -30., 30., "gravity"),
            (self.drag, 0., 10., "drag"),
            (self.softness, 0.001, 10., "soft intersections"),
            (self.trail_length, 0., 1., "trail length"),
        ] {
            range(v, min, max, name)?;
        }
        for v in self.color {
            range(v, 0., 1., "color")?;
        }
        for v in self.wind {
            range(v, -100., 100., "wind")?;
        }
        ensure!(
            (1..=2048).contains(&self.max_particles),
            "particle budget must be within 1..2048"
        );
        Ok(())
    }
}
/// Frame data only. Runtime particles never become authored scene objects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub id: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub kind: ParticleKind,
    pub softness: f32,
    pub trail_length: f32,
    pub seed: f32,
}
#[derive(Clone)]
struct Live {
    id: u64,
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
    rotation: f32,
    spin: f32,
    scale: f32,
    seed: f32,
    settings: ParticleEmitter,
}
#[derive(Clone, Default)]
struct State {
    particles: Vec<Live>,
    fraction: f32,
    serial: u32,
}
#[derive(Clone, Default)]
pub(crate) struct ParticleSystem {
    emitters: BTreeMap<String, State>,
}
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn random(seed: u32, index: u32) -> f32 {
    hash(seed.wrapping_add(index.wrapping_mul(0x9e3779b9))) as f32 / u32::MAX as f32
}
fn name_seed(id: &str) -> u32 {
    id.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    })
}
fn curl(p: Vec3, t: f32) -> Vec3 {
    // Curl of a trigonometric vector potential: divergence-free, smooth and deterministic.
    Vec3::new(
        (p.y * 1.3 + t * 0.7).cos() - (p.z * 0.9 + t).cos(),
        (p.z * 1.1 + t * 0.8).cos() - (p.x * 1.2 + t * 0.6).cos(),
        (p.x * 0.8 + t).cos() - (p.y * 1.4 + t * 0.9).cos(),
    )
}
impl ParticleSystem {
    pub fn step(
        &mut self,
        emitters: &[(String, Mat4, ParticleEmitter)],
        dt: f32,
        time: f32,
    ) -> Result<()> {
        ensure!(
            dt.is_finite() && (0.0..=1.).contains(&dt),
            "invalid particle timestep"
        );
        for (_, model, settings) in emitters {
            settings.validate()?;
            ensure!(model.is_finite(), "invalid particle emitter transform");
        }
        self.emitters
            .retain(|id, _| emitters.iter().any(|(key, _, _)| key == id));
        let mut total = self
            .emitters
            .values()
            .map(|state| state.particles.len())
            .sum::<usize>();
        for (id, model, settings) in emitters {
            let state = self.emitters.entry(id.clone()).or_default();
            let before = state.particles.len();
            for p in &mut state.particles {
                let steps = (dt * 60.).ceil().max(1.) as u32;
                let step = dt / steps as f32;
                for sub in 0..steps {
                    p.velocity.y += p.settings.gravity * step;
                    p.velocity += curl(
                        p.position * 0.8,
                        time - dt + step * sub as f32 + p.seed * 13.,
                    ) * p.settings.turbulence
                        * step;
                    p.velocity *= (-p.settings.drag * step).exp();
                    p.position += (p.velocity + Vec3::from(p.settings.wind)) * step;
                }
                p.age += dt;
                p.rotation += p.spin * dt;
            }
            state.particles.retain(|p| p.age < p.lifetime);
            total -= before - state.particles.len();
            if !settings.enabled || dt == 0. {
                continue;
            }
            state.fraction += settings.rate * dt;
            let wanted = state.fraction.floor() as usize;
            state.fraction -= wanted as f32;
            let count = wanted
                .min((settings.max_particles as usize).saturating_sub(state.particles.len()))
                .min(MAX_PARTICLES.saturating_sub(total));
            for _ in 0..count {
                state.serial = state.serial.wrapping_add(1);
                let seed = hash(name_seed(id) ^ settings.seed ^ state.serial);
                let r = |i| random(seed, i);
                let offset =
                    Vec3::new(r(0) * 2. - 1., r(1) * 0.25, r(2) * 2. - 1.) * settings.radius;
                let direction = Vec3::new(
                    (r(3) * 2. - 1.) * settings.spread,
                    settings.speed * (0.7 + 0.6 * r(4)),
                    (r(5) * 2. - 1.) * settings.spread,
                );
                let scale = model
                    .x_axis
                    .truncate()
                    .length()
                    .max(model.y_axis.truncate().length())
                    .max(model.z_axis.truncate().length());
                let velocity = model.transform_vector3(direction) / scale.max(0.0001);
                state.particles.push(Live {
                    id: (u64::from(name_seed(id)) << 32) | u64::from(state.serial),
                    position: model.transform_point3(offset),
                    velocity,
                    age: 0.,
                    lifetime: settings.lifetime * (0.75 + 0.5 * r(6)),
                    rotation: r(7) * std::f32::consts::TAU,
                    spin: (r(8) * 2. - 1.)
                        * if settings.kind == ParticleKind::Ash {
                            5.
                        } else {
                            0.5
                        },
                    scale: scale * (0.75 + 0.5 * r(9)),
                    seed: r(10),
                    settings: *settings,
                });
            }
            total += count;
        }
        Ok(())
    }
    pub fn frame(&self) -> Vec<Particle> {
        self.emitters
            .values()
            .flat_map(|state| state.particles.iter())
            .map(|p| {
                let t = (p.age / p.lifetime).clamp(0., 1.);
                let fade_in = (t / 0.12).clamp(0., 1.);
                let fade_out = ((1. - t) / 0.35).clamp(0., 1.);
                let opacity = p.settings.opacity
                    * fade_in
                    * fade_out
                    * if p.settings.kind == ParticleKind::Sparks {
                        (1. - t).powf(0.6)
                    } else {
                        1.
                    };
                Particle {
                    id: p.id,
                    position: p.position,
                    velocity: p.velocity + Vec3::from(p.settings.wind),
                    size: (p.settings.start_size
                        + (p.settings.end_size - p.settings.start_size) * t)
                        * p.scale,
                    rotation: p.rotation,
                    color: p.settings.color,
                    opacity,
                    kind: p.settings.kind,
                    softness: p.settings.softness,
                    trail_length: p.settings.trail_length,
                    seed: p.seed,
                }
            })
            .collect()
    }
}
impl crate::SceneInstance {
    pub fn step_particles(&mut self, world: &bozzard_ecs::World, dt: f32) -> Result<()> {
        if !self
            .entities
            .values()
            .any(|entity| world.get::<ParticleEmitter>(*entity).is_some())
        {
            self.particle_state.emitters.clear();
            return Ok(());
        }
        let matrices = self.global_transforms(world)?;
        let emitters: Vec<_> = self
            .entities
            .iter()
            .filter_map(|(id, entity)| {
                world
                    .get::<ParticleEmitter>(*entity)
                    .map(|settings| (id.clone(), matrices[id], *settings))
            })
            .collect();
        self.particle_state.step(&emitters, dt, self.display_time)
    }
}
