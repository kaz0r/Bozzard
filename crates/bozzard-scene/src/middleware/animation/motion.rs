//! Bounded blend spaces. Triangulation is prepared once, never on the animation tick.
use super::data::Rig;
use anyhow::{Result, ensure};
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSample {
    pub threshold: f32,
    pub clip: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendPoint {
    pub position: [f32; 2],
    pub clip: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Motion {
    Clip {
        clip: usize,
    },
    Blend1d {
        parameter: String,
        samples: Vec<BlendSample>,
    },
    Blend2d {
        parameters: [String; 2],
        samples: Vec<BlendPoint>,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Contribution {
    pub clip: usize,
    pub weight: f32,
}

/// At most three clips contribute to a triangle, or two at the clamped hull edge.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mix {
    pub samples: [Contribution; 3],
    pub count: usize,
}
impl Mix {
    fn single(clip: usize) -> Self {
        Self::new(&[(clip, 1.)])
    }
    fn new(samples: &[(usize, f32)]) -> Self {
        let mut result = Self {
            samples: [Contribution::default(); 3],
            count: 0,
        };
        for &(clip, weight) in samples {
            if weight > 0. {
                result.samples[result.count] = Contribution { clip, weight };
                result.count += 1;
            }
        }
        result
    }
    pub fn iter(&self) -> impl Iterator<Item = &Contribution> {
        self.samples[..self.count].iter()
    }
    pub fn duration(&self, rig: &Rig) -> f32 {
        self.iter()
            .map(|s| rig.clips[s.clip].duration * s.weight)
            .sum()
    }
    pub fn dominant(&self) -> usize {
        self.iter()
            .max_by(|a, b| a.weight.total_cmp(&b.weight).then(b.clip.cmp(&a.clip)))
            .expect("validated motion has a contribution")
            .clip
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    triangles: Vec<[usize; 3]>,
    boundary: Vec<[usize; 2]>,
}
impl Plan {
    pub fn compile(motion: &Motion) -> Result<Self> {
        let Motion::Blend2d { samples, .. } = motion else {
            return Ok(Self::default());
        };
        let points: Vec<_> = samples
            .iter()
            .map(|s| DVec2::new(s.position[0] as f64, s.position[1] as f64))
            .collect();
        let triangles = triangulate(&points)?;
        let boundary = edge_counts(&triangles)
            .into_iter()
            .filter_map(|(edge, count)| (count == 1).then_some(edge))
            .collect();
        Ok(Self {
            triangles,
            boundary,
        })
    }

    pub fn weights(&self, motion: &Motion, parameters: &BTreeMap<String, f32>) -> Mix {
        match motion {
            Motion::Clip { clip } => Mix::single(*clip),
            Motion::Blend1d { parameter, samples } => {
                let value = parameters[parameter];
                let end = samples.partition_point(|s| s.threshold <= value);
                if end == 0 {
                    return Mix::single(samples[0].clip);
                }
                if end == samples.len() {
                    return Mix::single(samples[end - 1].clip);
                }
                let (a, b) = (&samples[end - 1], &samples[end]);
                let weight = (value - a.threshold) / (b.threshold - a.threshold);
                Mix::new(&[(a.clip, 1. - weight), (b.clip, weight)])
            }
            Motion::Blend2d {
                parameters: axes,
                samples,
            } => {
                let point = DVec2::new(parameters[&axes[0]] as f64, parameters[&axes[1]] as f64);
                for triangle in &self.triangles {
                    let positions = triangle.map(|i| point_at(&samples[i]));
                    let weights = barycentric(point, positions);
                    if weights.iter().all(|w| *w >= -1e-9) {
                        let clamped = weights.map(|w| w.max(0.));
                        let sum: f64 = clamped.iter().sum();
                        return Mix::new(&std::array::from_fn::<_, 3, _>(|i| {
                            (samples[triangle[i]].clip, (clamped[i] / sum) as f32)
                        }));
                    }
                }
                // Outside the convex hull, project to its closest edge. No extrapolated poses.
                let (edge, weight) = self
                    .boundary
                    .iter()
                    .map(|edge| {
                        let (a, b) = (point_at(&samples[edge[0]]), point_at(&samples[edge[1]]));
                        let weight = ((point - a).dot(b - a) / a.distance_squared(b)).clamp(0., 1.);
                        (edge, weight, point.distance_squared(a.lerp(b, weight)))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
                    .map(|(e, w, _)| (e, w as f32))
                    .expect("validated blend space has a boundary");
                Mix::new(&[
                    (samples[edge[0]].clip, 1. - weight),
                    (samples[edge[1]].clip, weight),
                ])
            }
        }
    }
}

impl Motion {
    pub fn uses_parameter(&self, name: &str) -> bool {
        match self {
            Self::Clip { .. } => false,
            Self::Blend1d { parameter, .. } => parameter == name,
            Self::Blend2d { parameters, .. } => parameters.iter().any(|p| p == name),
        }
    }
    pub(crate) fn validate(&self, rig: &Rig, parameters: &BTreeMap<String, f32>) -> Result<()> {
        match self {
            Self::Clip { clip } => {
                ensure!(*clip < rig.clips.len(), "animation clip does not exist")
            }
            Self::Blend1d { parameter, samples } => {
                ensure!(
                    parameters.contains_key(parameter)
                        && !samples.is_empty()
                        && samples.len() <= 64,
                    "1D blend needs a parameter and 1–64 samples"
                );
                ensure!(
                    samples
                        .iter()
                        .all(|s| s.threshold.is_finite() && s.clip < rig.clips.len())
                        && samples.windows(2).all(|w| w[0].threshold < w[1].threshold),
                    "blend thresholds must strictly increase and clips must exist"
                );
            }
            Self::Blend2d {
                parameters: axes,
                samples,
            } => {
                ensure!(
                    axes[0] != axes[1]
                        && axes.iter().all(|p| parameters.contains_key(p))
                        && (3..=64).contains(&samples.len()),
                    "2D blend needs two different parameters and 3–64 samples"
                );
                for (index, sample) in samples.iter().enumerate() {
                    ensure!(
                        sample.clip < rig.clips.len()
                            && sample
                                .position
                                .iter()
                                .all(|v| v.is_finite() && v.abs() <= 10_000.),
                        "2D blend samples need existing clips and finite coordinates within ±10000"
                    );
                    ensure!(
                        !samples[..index]
                            .iter()
                            .any(|s| s.position == sample.position),
                        "2D blend sample positions must be unique"
                    );
                }
                Plan::compile(self)?;
            }
        }
        Ok(())
    }
}

fn point_at(sample: &BlendPoint) -> DVec2 {
    DVec2::new(sample.position[0] as f64, sample.position[1] as f64)
}
fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn barycentric(p: DVec2, [a, b, c]: [DVec2; 3]) -> [f64; 3] {
    let area = cross(b - a, c - a);
    let v = cross(p - a, c - a) / area;
    let w = cross(b - a, p - a) / area;
    [1. - v - w, v, w]
}
fn edge_counts(triangles: &[[usize; 3]]) -> BTreeMap<[usize; 2], usize> {
    let mut counts = BTreeMap::new();
    for &[a, b, c] in triangles {
        for mut edge in [[a, b], [b, c], [c, a]] {
            edge.sort_unstable();
            *counts.entry(edge).or_default() += 1;
        }
    }
    counts
}
fn in_circle(point: DVec2, [a, b, c]: [DVec2; 3]) -> bool {
    let (a, b, c) = (a - point, b - point, c - point);
    let determinant = a.length_squared() * cross(b, c) - b.length_squared() * cross(a, c)
        + c.length_squared() * cross(a, b);
    determinant * cross(b - a, c - a).signum() > 0.
}
fn triangulate(input: &[DVec2]) -> Result<Vec<[usize; 3]>> {
    let count = input.len();
    let min = input
        .iter()
        .copied()
        .fold(DVec2::splat(f64::INFINITY), DVec2::min);
    let max = input
        .iter()
        .copied()
        .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
    let center = (min + max) * 0.5;
    let span = (max - min).max_element().max(1.) * 32.;
    let mut points = input.to_vec();
    points.extend([
        center + DVec2::new(-span, -span),
        center + DVec2::new(span, -span),
        center + DVec2::new(0., span),
    ]);
    let mut triangles = vec![[count, count + 1, count + 2]];
    for index in 0..count {
        let mut removed = Vec::new();
        triangles.retain(|triangle| {
            if in_circle(points[index], triangle.map(|i| points[i])) {
                removed.push(*triangle);
                false
            } else {
                true
            }
        });
        for (edge, instances) in edge_counts(&removed) {
            if instances == 1
                && cross(
                    points[edge[1]] - points[edge[0]],
                    points[index] - points[edge[0]],
                )
                .abs()
                    > 1e-12
            {
                triangles.push([edge[0], edge[1], index]);
            }
        }
    }
    triangles.retain(|triangle| triangle.iter().all(|i| *i < count));
    ensure!(
        !triangles.is_empty(),
        "2D blend needs non-collinear sample positions"
    );
    Ok(triangles)
}
