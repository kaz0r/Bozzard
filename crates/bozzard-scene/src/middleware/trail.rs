//! Trail Renderer: a ribbon that follows an object's path. The simulation lays a point
//! whenever the object has moved far enough and drops points older than the lifetime;
//! each rendered frame adds the object's current position as the head, so the ribbon
//! stays attached between ticks. Purely visual: it never affects gameplay state.
use super::registry::{Authored, PreviewPolicy};
use crate::{Component, Field, FieldValue, Ui, VectorRole};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailBlend {
    /// Ordinary transparency: the trail covers what is behind it.
    #[default]
    Alpha,
    /// Light added to the scene, for glows, sparks and magic.
    Additive,
}
pub const TRAIL_BLENDS: &[&str] = &["Alpha", "Additive"];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Trail {
    pub enabled: bool,
    /// Seconds a point stays on the trail.
    pub lifetime: f32,
    /// World distance the emitter travels before another point is laid.
    pub min_distance: f32,
    /// Width at the object, in world units.
    pub start_width: f32,
    /// Width where the trail ends.
    pub end_width: f32,
    /// Linear RGB at the object and at the end.
    pub start_color: [f32; 3],
    pub end_color: [f32; 3],
    pub start_opacity: f32,
    pub end_opacity: f32,
    /// Multiplies the colour; above 1 reaches the bloom threshold and glows.
    pub intensity: f32,
    pub blend: TrailBlend,
    /// Emission point in the object's local space.
    pub offset: [f32; 3],
}
impl Default for Trail {
    fn default() -> Self {
        Self {
            enabled: true,
            lifetime: 0.6,
            min_distance: 0.1,
            start_width: 0.3,
            end_width: 0.0,
            start_color: [1.0, 0.85, 0.5],
            end_color: [1.0, 0.35, 0.1],
            start_opacity: 1.0,
            end_opacity: 0.0,
            intensity: 1.0,
            blend: TrailBlend::Alpha,
            offset: [0.0; 3],
        }
    }
}
/// Points kept per trail. A fast object with a long lifetime lays its oldest points down
/// further apart rather than growing without bound.
pub const MAX_TRAIL_POINTS: usize = 256;

impl Component for Trail {
    const NAME: &'static str = "trail";
    const LABEL: &'static str = "Trail Renderer";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "A ribbon following this object's path in Play. Width, colour and opacity blend from the object (start) to the oldest point (end). Unlit; use Additive and Intensity above 1 for a glow.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::range("lifetime", "Lifetime (s)", 0.01, 0.01, 30.0),
            F::range("min_distance", "Point spacing", 0.005, 0.001, 100.0)
                .help("World distance moved before another point is laid. Smaller is smoother."),
            F::range("start_width", "Start width", 0.01, 0.0, 100.0),
            F::range("end_width", "End width", 0.01, 0.0, 100.0),
            F::vector("start_color", "Start color", VectorRole::Color, 0.01),
            F::vector("end_color", "End color", VectorRole::Color, 0.01),
            F::range("start_opacity", "Start opacity", 0.01, 0.0, 1.0),
            F::range("end_opacity", "End opacity", 0.01, 0.0, 1.0),
            F::range("intensity", "Intensity", 0.05, 0.0, 64.0),
            F::options("blend", "Blend", TRAIL_BLENDS),
            F::vector("offset", "Offset", VectorRole::Offset, 0.05)
                .help("Where the trail leaves the object, in its local space."),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "lifetime" => FieldValue::Number(self.lifetime),
            "min_distance" => FieldValue::Number(self.min_distance),
            "start_width" => FieldValue::Number(self.start_width),
            "end_width" => FieldValue::Number(self.end_width),
            "start_color" => FieldValue::Vector(self.start_color),
            "end_color" => FieldValue::Vector(self.end_color),
            "start_opacity" => FieldValue::Number(self.start_opacity),
            "end_opacity" => FieldValue::Number(self.end_opacity),
            "intensity" => FieldValue::Number(self.intensity),
            "blend" => FieldValue::Index(self.blend as usize),
            "offset" => FieldValue::Vector(self.offset),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "lifetime" => self.lifetime = value.number()?,
            "min_distance" => self.min_distance = value.number()?,
            "start_width" => self.start_width = value.number()?,
            "end_width" => self.end_width = value.number()?,
            "start_color" => self.start_color = value.vector()?,
            "end_color" => self.end_color = value.vector()?,
            "start_opacity" => self.start_opacity = value.number()?,
            "end_opacity" => self.end_opacity = value.number()?,
            "intensity" => self.intensity = value.number()?,
            "blend" => {
                self.blend = *[TrailBlend::Alpha, TrailBlend::Additive]
                    .get(value.index()?)
                    .context("no such trail blend")?
            }
            "offset" => self.offset = value.vector()?,
            other => anyhow::bail!("Trail Renderer has no field '{other}'"),
        }
        Ok(())
    }
}
impl Authored for Trail {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;
    fn validate(&self) -> Result<()> {
        let within = |value: f32, lo: f32, hi: f32| value.is_finite() && (lo..=hi).contains(&value);
        ensure!(
            within(self.lifetime, 0.01, 30.0) && within(self.min_distance, 0.001, 100.0),
            "trail lifetime must be 0.01–30 s and point spacing 0.001–100"
        );
        ensure!(
            within(self.start_width, 0.0, 100.0) && within(self.end_width, 0.0, 100.0),
            "trail widths must be 0–100"
        );
        ensure!(
            self.start_color
                .iter()
                .chain(&self.end_color)
                .all(|c| within(*c, 0.0, 1.0)),
            "trail colours must be linear RGB in 0–1; use Intensity to brighten"
        );
        ensure!(
            within(self.start_opacity, 0.0, 1.0)
                && within(self.end_opacity, 0.0, 1.0)
                && within(self.intensity, 0.0, 64.0),
            "trail opacity must be 0–1 and intensity 0–64"
        );
        ensure!(
            self.offset.iter().all(|v| v.is_finite()),
            "trail offset must be finite"
        );
        Ok(())
    }
}

/// One point of a ribbon, ready to draw: world position, full width and linear RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailVertex {
    pub position: Vec3,
    pub width: f32,
    pub color: [f32; 4],
}
/// A trail as the renderer draws it, oldest point first.
#[derive(Clone, Debug, PartialEq)]
pub struct TrailRibbon {
    pub points: Vec<TrailVertex>,
    pub additive: bool,
}

#[derive(Clone, Debug)]
struct History {
    settings: Trail,
    /// World position and the trail clock when it was laid, oldest first.
    points: VecDeque<(Vec3, f32)>,
}
/// Every object's recorded path. Lives on the scene instance like the particle system.
#[derive(Clone, Debug, Default)]
pub(crate) struct TrailSystem {
    trails: BTreeMap<String, History>,
    time: f32,
}
impl TrailSystem {
    /// Advance the clock, then lay and expire points for each enabled trail at its
    /// object's world matrix. Objects that lost or disabled their trail drop it.
    pub(crate) fn step(&mut self, trails: &[(String, Mat4, Trail)], dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0.0, "trail timestep must be finite");
        self.time += dt;
        let time = self.time;
        self.trails
            .retain(|id, _| trails.iter().any(|(owner, _, t)| owner == id && t.enabled));
        for (id, matrix, settings) in trails {
            if !settings.enabled {
                continue;
            }
            settings.validate()?;
            let position = matrix.transform_point3(Vec3::from(settings.offset));
            ensure!(position.is_finite(), "trail on '{id}' left finite space");
            let history = self.trails.entry(id.clone()).or_insert_with(|| History {
                settings: *settings,
                points: VecDeque::new(),
            });
            history.settings = *settings;
            while history
                .points
                .front()
                .is_some_and(|&(_, born)| time - born > settings.lifetime)
            {
                history.points.pop_front();
            }
            if history
                .points
                .back()
                .is_none_or(|&(last, _)| last.distance(position) >= settings.min_distance)
            {
                if history.points.len() == MAX_TRAIL_POINTS {
                    history.points.pop_front();
                }
                history.points.push_back((position, time));
            }
        }
        Ok(())
    }
    pub(crate) fn remove_objects(&mut self, ids: &std::collections::BTreeSet<String>) {
        self.trails.retain(|id, _| !ids.contains(id));
    }
    /// Ribbons for one rendered frame. `head` gives an object's current (interpolated)
    /// matrix, which becomes the newest point so the ribbon meets the object.
    pub(crate) fn frame(&self, head: impl Fn(&str) -> Option<Mat4>) -> Vec<TrailRibbon> {
        self.trails
            .iter()
            .filter_map(|(id, history)| {
                let settings = &history.settings;
                let mut points: Vec<(Vec3, f32)> = history.points.iter().copied().collect();
                if let Some(matrix) = head(id) {
                    let position = matrix.transform_point3(Vec3::from(settings.offset));
                    if position.is_finite()
                        && points
                            .last()
                            .is_none_or(|&(last, _)| last.distance(position) > 1e-4)
                    {
                        points.push((position, self.time));
                    }
                }
                (points.len() >= 2).then(|| TrailRibbon {
                    additive: settings.blend == TrailBlend::Additive,
                    points: points
                        .into_iter()
                        .map(|(position, born)| {
                            // 0 at the object, 1 where the point expires.
                            let t = ((self.time - born) / settings.lifetime).clamp(0.0, 1.0);
                            let mix = |a: f32, b: f32| a + (b - a) * t;
                            let opacity = mix(settings.start_opacity, settings.end_opacity);
                            TrailVertex {
                                position,
                                width: mix(settings.start_width, settings.end_width),
                                color: [
                                    mix(settings.start_color[0], settings.end_color[0])
                                        * settings.intensity,
                                    mix(settings.start_color[1], settings.end_color[1])
                                        * settings.intensity,
                                    mix(settings.start_color[2], settings.end_color[2])
                                        * settings.intensity,
                                    opacity,
                                ],
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> Mat4 {
        Mat4::from_translation(Vec3::new(x, 0.0, 0.0))
    }

    #[test]
    fn points_are_laid_by_distance_and_expire_by_age() {
        let settings = Trail {
            lifetime: 1.0,
            min_distance: 0.5,
            ..Default::default()
        };
        let mut system = TrailSystem::default();
        // Moving 0.25 per tick lays a point every other tick.
        for tick in 0..8 {
            system
                .step(&[("a".into(), at(tick as f32 * 0.25), settings)], 0.1)
                .unwrap();
        }
        let xs: Vec<_> = system.trails["a"].points.iter().map(|p| p.0.x).collect();
        assert_eq!(xs, [0.0, 0.5, 1.0, 1.5]);
        // Standing still for over a second expires the path; one point marks where the
        // object stands, which is not a ribbon until it moves again.
        for _ in 0..12 {
            system
                .step(&[("a".into(), at(1.75), settings)], 0.1)
                .unwrap();
        }
        let points: Vec<_> = system.trails["a"].points.iter().map(|p| p.0.x).collect();
        assert_eq!(points, [1.75]);
        assert!(system.frame(|_| Some(at(1.75))).is_empty());
        // A removed component drops its history on the next step.
        system.step(&[], 0.1).unwrap();
        assert!(system.trails.is_empty());
    }

    #[test]
    fn frames_blend_from_the_object_to_the_tail_and_attach_the_head() {
        let settings = Trail {
            lifetime: 1.0,
            min_distance: 0.1,
            start_width: 1.0,
            end_width: 0.0,
            start_color: [1.0, 0.0, 0.0],
            end_color: [0.0, 0.0, 1.0],
            start_opacity: 1.0,
            end_opacity: 0.0,
            intensity: 2.0,
            blend: TrailBlend::Additive,
            ..Default::default()
        };
        let mut system = TrailSystem::default();
        system
            .step(&[("a".into(), at(0.0), settings)], 0.0)
            .unwrap();
        system
            .step(&[("a".into(), at(1.0), settings)], 0.5)
            .unwrap();
        let ribbons = system.frame(|id| (id == "a").then(|| at(1.5)));
        assert_eq!(ribbons.len(), 1);
        let ribbon = &ribbons[0];
        assert!(ribbon.additive);
        assert_eq!(
            ribbon
                .points
                .iter()
                .map(|p| p.position.x)
                .collect::<Vec<_>>(),
            [0.0, 1.0, 1.5]
        );
        // The oldest point is half way through its life; the head is brand new.
        let tail = ribbon.points[0];
        assert!((tail.width - 0.5).abs() < 1e-6 && (tail.color[3] - 0.5).abs() < 1e-6);
        assert_eq!(tail.color[..3], [1.0, 0.0, 1.0]);
        let head = ribbon.points[2];
        assert_eq!((head.width, head.color), (1.0, [2.0, 0.0, 0.0, 1.0]));
        // One point and no head is not a ribbon yet.
        let mut lone = TrailSystem::default();
        lone.step(&[("a".into(), at(0.0), settings)], 0.1).unwrap();
        assert!(lone.frame(|_| None).is_empty());
    }

    #[test]
    fn long_fast_trails_keep_a_bounded_history() {
        let settings = Trail {
            lifetime: 30.0,
            min_distance: 0.01,
            ..Default::default()
        };
        let mut system = TrailSystem::default();
        for tick in 0..1000 {
            system
                .step(&[("a".into(), at(tick as f32), settings)], 1.0 / 60.0)
                .unwrap();
        }
        assert_eq!(system.trails["a"].points.len(), MAX_TRAIL_POINTS);
        assert_eq!(system.trails["a"].points.back().unwrap().0.x, 999.0);
    }

    #[test]
    fn out_of_range_settings_are_rejected() {
        for broken in [
            Trail {
                lifetime: 0.0,
                ..Default::default()
            },
            Trail {
                start_color: [2.0, 0.0, 0.0],
                ..Default::default()
            },
            Trail {
                end_width: f32::NAN,
                ..Default::default()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }
}
