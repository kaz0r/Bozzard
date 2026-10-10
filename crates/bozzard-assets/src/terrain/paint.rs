//! Built-in material layers baked into one ordinary mesh surface.
use super::Terrain;
use crate::{ImageData, MeshPart, PbrMaterial, Sampler, SurfaceShading, Wrap, job::Progress};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainLayer {
    /// Linear RGB tint, matching PBR material factors.
    pub color: [f32; 3],
    /// Local terrain units per procedural material repeat.
    pub tiling: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainPaint {
    /// Fixed order: Grass, Dirt, Rock.
    pub layers: [TerrainLayer; 3],
    /// Row-major vertex weights, normalized exactly to 255.
    pub weights: Vec<[u8; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainPaintBrush {
    pub layer: usize,
    pub center: [f32; 2],
    pub radius: f32,
    /// Interpolation towards the selected layer, in 0..1 before soft falloff.
    pub strength: f32,
}

/// Retains sub-byte material coverage for the duration of a continuous stroke.
/// Create a new session after any external terrain or palette edit.
#[derive(Debug)]
pub struct TerrainPaintStroke {
    expected: Terrain,
    weights: Vec<[f64; 3]>,
}

impl TerrainPaintStroke {
    pub fn new(terrain: &Terrain) -> Result<Self> {
        terrain.validate()?;
        let weights = terrain.paint.as_ref().map_or_else(
            || vec![[255., 0., 0.]; terrain.heights.len()],
            |paint| paint.weights.iter().map(|w| w.map(f64::from)).collect(),
        );
        Ok(Self {
            expected: terrain.clone(),
            weights,
        })
    }

    /// `strength` is the interpolation amount for this stamp. For a rate per
    /// second, callers can use `1 - exp(-rate * dt)` independently of frame rate.
    /// Returns whether published bytes changed; fractional progress is retained
    /// even when this returns false. Rejects stale sessions before any mutation.
    pub fn paint(&mut self, terrain: &mut Terrain, brush: TerrainPaintBrush) -> Result<bool> {
        self.paint_path(terrain, &[brush])
    }

    /// Apply up to 128 interpolated stamps, checking the source snapshot once.
    /// Invalid stamps or an external edit reject the entire path before mutation.
    pub fn paint_path(
        &mut self,
        terrain: &mut Terrain,
        brushes: &[TerrainPaintBrush],
    ) -> Result<bool> {
        ensure!(
            matches_snapshot(&self.expected, terrain),
            "terrain paint stroke is stale; start a new stroke"
        );
        ensure!(
            brushes.len() <= 128,
            "terrain paint path exceeds 128 stamps"
        );
        for &brush in brushes {
            validate_brush(brush)?;
        }
        let mut changed = false;
        for &brush in brushes {
            changed |= self.paint_validated(terrain, brush);
        }
        Ok(changed)
    }

    fn paint_validated(&mut self, terrain: &mut Terrain, brush: TerrainPaintBrush) -> bool {
        let mut changed = false;
        let log_retention = (-f64::from(brush.strength)).ln_1p();
        brush_vertices(terrain.resolution, terrain.size, brush, |i, falloff| {
            if terrain.paint.is_none() {
                terrain.paint = Some(TerrainPaint::new(self.weights.len()));
                self.expected.paint = terrain.paint.clone();
                changed = true;
            }
            let precise = &mut self.weights[i];
            let others = [(brush.layer + 1) % 3, (brush.layer + 2) % 3];
            // Apply the falloff to the rate, keeping soft edges independent of
            // how many input frames or interpolated stamps represent the stroke.
            let retention = (log_retention * f64::from(falloff)).exp();
            precise[others[0]] *= retention;
            precise[others[1]] *= retention;
            precise[brush.layer] = 255. - precise[others[0]] - precise[others[1]];
            let painted = quantize(*precise);
            let weights = &mut terrain.paint.as_mut().unwrap().weights[i];
            changed |= *weights != painted;
            *weights = painted;
            self.expected.paint.as_mut().unwrap().weights[i] = painted;
        });
        changed
    }
}

fn matches_snapshot(expected: &Terrain, terrain: &Terrain) -> bool {
    if expected.version != terrain.version
        || expected.resolution != terrain.resolution
        || expected.size.map(f32::to_bits) != terrain.size.map(f32::to_bits)
        || expected.heights.len() != terrain.heights.len()
        || !expected
            .heights
            .iter()
            .zip(&terrain.heights)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    {
        return false;
    }
    match (&expected.paint, &terrain.paint) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.weights == b.weights
                && a.layers.iter().zip(&b.layers).all(|(a, b)| {
                    a.color.map(f32::to_bits) == b.color.map(f32::to_bits)
                        && a.tiling.to_bits() == b.tiling.to_bits()
                })
        }
        _ => false,
    }
}

impl TerrainPaint {
    pub const LAYER_NAMES: [&'static str; 3] = ["Grass", "Dirt", "Rock"];

    /// Initialize a validated terrain's vertex population with the Grass layer.
    pub fn new(vertex_count: usize) -> Self {
        Self {
            layers: [
                TerrainLayer {
                    color: [0.13, 0.30, 0.065],
                    tiling: 2.,
                },
                TerrainLayer {
                    color: [0.28, 0.115, 0.042],
                    tiling: 2.,
                },
                TerrainLayer {
                    color: [0.24, 0.26, 0.28],
                    tiling: 3.,
                },
            ],
            weights: vec![[255, 0, 0]; vertex_count],
        }
    }

    pub(super) fn validate(&self, vertex_count: usize) -> Result<()> {
        ensure!(
            self.weights.len() == vertex_count,
            "terrain paint weight count does not match its resolution"
        );
        ensure!(
            self.weights
                .iter()
                .all(|weights| weights.iter().map(|v| u16::from(*v)).sum::<u16>() == 255),
            "terrain paint weights must sum to 255"
        );
        for layer in &self.layers {
            ensure!(
                layer
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
                "terrain material color must be finite and within 0..1"
            );
            ensure!(
                layer.tiling.is_finite() && (0.01..=100_000.).contains(&layer.tiling),
                "terrain material tiling must be finite and within 0.01..100000"
            );
        }
        Ok(())
    }

    pub(super) fn mesh_part(
        &self,
        terrain: &Terrain,
        vertices: &[[f32; 8]],
        index_count: usize,
        progress: &Progress,
    ) -> Result<MeshPart> {
        let image = Arc::new(self.bake(terrain, progress)?);
        let attributes = vertices
            .iter()
            .map(|vertex| {
                let normal = Vec3::from_slice(&vertex[3..6]);
                let tangent = (Vec3::X - normal * normal.x)
                    .try_normalize()
                    .unwrap_or(Vec3::Z);
                let mut attributes = [0.; 12];
                attributes[..4].copy_from_slice(&[tangent.x, tangent.y, tangent.z, -1.]);
                attributes
            })
            .collect();
        Ok(MeshPart {
            // A material edit must not invalidate persistent surface overrides.
            source_key: "terrain-painted-v1".into(),
            name: "Terrain".into(),
            material_name: Some("Painted terrain".into()),
            start: 0,
            count: index_count as u32,
            color: [1.; 4],
            image: Some(image),
            alpha_cutoff: None,
            shading: Some(SurfaceShading {
                vertex_start: 0,
                vertices: attributes,
                material: PbrMaterial {
                    metallic: 0.,
                    roughness: 0.9,
                    normal_scale: 1.,
                    occlusion_strength: 1.,
                    emissive_factor: [0.; 3],
                    double_sided: false,
                    base_color_sampler: Sampler {
                        wrap_u: Wrap::Clamp,
                        wrap_v: Wrap::Clamp,
                        ..Sampler::default()
                    },
                    metallic_roughness: None,
                    normal: None,
                    occlusion: None,
                    emissive: None,
                },
            }),
        })
    }

    fn bake(&self, terrain: &Terrain, progress: &Progress) -> Result<ImageData> {
        // Fixed upper bound: one MiB of pixels, independent of brush count.
        const SIDE: usize = 512;
        let [nx, nz] = terrain.resolution.map(usize::from);
        let columns: Vec<_> = (0..SIDE)
            .map(|x| {
                let u = x as f32 / (SIDE - 1) as f32;
                let grid = u * (nx - 1) as f32;
                let ix = (grid.floor() as usize).min(nx - 2);
                (ix, grid - ix as f32, (u - 0.5) * terrain.size[0])
            })
            .collect();
        let mut rgba = vec![0; SIDE * SIDE * 4];
        for z in 0..SIDE {
            progress.check()?;
            let v = z as f32 / (SIDE - 1) as f32;
            let grid = v * (nz - 1) as f32;
            let iz = (grid.floor() as usize).min(nz - 2);
            let fraction_z = grid - iz as f32;
            let local_z = (v - 0.5) * terrain.size[1];
            for (x, &(ix, fraction_x, local_x)) in columns.iter().enumerate() {
                let a = self.weights[iz * nx + ix];
                let b = self.weights[iz * nx + ix + 1];
                let c = self.weights[(iz + 1) * nx + ix];
                let d = self.weights[(iz + 1) * nx + ix + 1];
                let mut color = [0.; 3];
                for (layer, material) in self.layers.iter().enumerate() {
                    // Match the mesh diagonal rather than inventing bilinear faces.
                    let weight = if fraction_x + fraction_z <= 1. {
                        a[layer] as f32 * (1. - fraction_x - fraction_z)
                            + b[layer] as f32 * fraction_x
                            + c[layer] as f32 * fraction_z
                    } else {
                        d[layer] as f32 * (fraction_x + fraction_z - 1.)
                            + c[layer] as f32 * (1. - fraction_x)
                            + b[layer] as f32 * (1. - fraction_z)
                    } / 255.;
                    if weight <= 0. {
                        continue;
                    }
                    let detail =
                        pattern(layer, local_x / material.tiling, local_z / material.tiling);
                    for (channel, tint) in color.iter_mut().zip(material.color) {
                        *channel += tint * detail * weight;
                    }
                }
                let pixel = &mut rgba[(z * SIDE + x) * 4..][..4];
                for (channel, value) in pixel[..3].iter_mut().zip(color) {
                    let value = value.clamp(0., 1.);
                    let srgb = if value <= 0.003_130_8 {
                        12.92 * value
                    } else {
                        1.055 * value.powf(1. / 2.4) - 0.055
                    };
                    *channel = (srgb * 255.).round() as u8;
                }
                pixel[3] = 255;
            }
        }
        Ok(ImageData {
            width: SIDE as u32,
            height: SIDE as u32,
            rgba,
            compressed: None,
        })
    }
}

impl Terrain {
    pub fn paint(&mut self, brush: TerrainPaintBrush) -> Result<bool> {
        self.validate()?;
        validate_brush(brush)?;
        let mut changed = false;
        brush_vertices(self.resolution, self.size, brush, |i, falloff| {
            let amount = brush.strength * falloff;
            if amount <= 0. {
                return;
            }
            // Allocate only after finding a covered vertex. A first Grass stroke
            // changes a legacy terrain's appearance even if its weights match.
            if self.paint.is_none() {
                self.paint = Some(TerrainPaint::new(self.heights.len()));
                changed = true;
            }
            let weights = &mut self.paint.as_mut().unwrap().weights[i];
            let painted = blend(*weights, brush.layer, amount);
            changed |= painted != *weights;
            *weights = painted;
        });
        Ok(changed)
    }
}

fn validate_brush(brush: TerrainPaintBrush) -> Result<()> {
    ensure!(brush.layer < 3, "invalid terrain paint layer");
    ensure!(
        brush.center.iter().all(|v| v.is_finite())
            && brush.radius.is_finite()
            && (0.001..=100_000.).contains(&brush.radius),
        "invalid terrain paint brush position or radius"
    );
    ensure!(
        brush.strength.is_finite() && (0.0..=1.).contains(&brush.strength),
        "invalid terrain paint brush strength"
    );
    Ok(())
}

fn brush_vertices(
    resolution: [u16; 2],
    size: [f32; 2],
    brush: TerrainPaintBrush,
    mut visit: impl FnMut(usize, f32),
) {
    if brush.strength == 0. {
        return;
    }
    let [nx, nz] = resolution.map(usize::from);
    let steps = [size[0] / (nx - 1) as f32, size[1] / (nz - 1) as f32];
    let ranges: [_; 2] = std::array::from_fn(|axis| {
        let limit = if axis == 0 { nx } else { nz };
        let center = brush.center[axis] + size[axis] * 0.5;
        let start = ((center - brush.radius) / steps[axis]).ceil().max(0.) as usize;
        let end =
            (((center + brush.radius) / steps[axis]).floor() + 1.).clamp(0., limit as f32) as usize;
        start.min(limit)..end
    });
    for z in ranges[1].clone() {
        for x in ranges[0].clone() {
            let dx = (x as f32 * steps[0] - size[0] * 0.5 - brush.center[0]) / brush.radius;
            let dz = (z as f32 * steps[1] - size[1] * 0.5 - brush.center[1]) / brush.radius;
            let squared = dx * dx + dz * dz;
            if squared >= 1. {
                continue;
            }
            let distance = squared.sqrt();
            let falloff = (1. - distance).powi(2) * (1. + 2. * distance);
            if falloff > 0. {
                visit(z * nx + x, falloff);
            }
        }
    }
}

fn quantize(weights: [f64; 3]) -> [u8; 3] {
    let mut bytes = weights.map(|w| w.clamp(0., 255.).floor() as u16);
    let mut fractions = std::array::from_fn::<_, 3, _>(|i| weights[i] - f64::from(bytes[i]));
    for _ in bytes.iter().sum::<u16>()..255 {
        let mut largest = 0;
        for i in 1..3 {
            if fractions[i] > fractions[largest] {
                largest = i;
            }
        }
        bytes[largest] += 1;
        fractions[largest] = -1.;
    }
    bytes.map(|b| b as u8)
}

fn blend(weights: [u8; 3], layer: usize, amount: f32) -> [u8; 3] {
    let old = u16::from(weights[layer]);
    let selected = (old as f32 + (255 - old) as f32 * amount).round() as u16;
    if selected == old {
        return weights;
    }
    let remaining = 255 - selected;
    let others = [(layer + 1) % 3, (layer + 2) % 3];
    let first = (u16::from(weights[others[0]]) * remaining + (255 - old) / 2) / (255 - old);
    let mut painted = [0; 3];
    painted[layer] = selected as u8;
    painted[others[0]] = first as u8;
    painted[others[1]] = (remaining - first) as u8;
    painted
}

fn hash(x: i32, z: i32) -> f32 {
    let mut bits = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841);
    bits ^= bits >> 13;
    bits = bits.wrapping_mul(0x85eb_ca6b);
    bits ^= bits >> 16;
    (bits & 0xffff) as f32 / 65535.
}

fn noise(x: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let u = x - x.floor();
    let v = z - z.floor();
    let u = u * u * (3. - 2. * u);
    let v = v * v * (3. - 2. * v);
    let a = hash(ix, iz);
    let b = hash(ix.wrapping_add(1), iz);
    let c = hash(ix, iz.wrapping_add(1));
    let d = hash(ix.wrapping_add(1), iz.wrapping_add(1));
    (a + (b - a) * u) * (1. - v) + (c + (d - c) * u) * v
}

fn pattern(layer: usize, x: f32, z: f32) -> f32 {
    match layer {
        0 => 0.72 + 0.24 * noise(x * 4., z * 4.) + 0.16 * noise(x * 48., z * 10.),
        1 => 0.70 + 0.28 * noise(x * 3., z * 3.) + 0.18 * noise(x * 25., z * 25.),
        _ => {
            let broad = noise(x * 2., z * 2.);
            0.58 + 0.40 * broad + 0.14 * noise(x * 14., z * 14.)
        }
    }
}

#[cfg(test)]
mod tests;
