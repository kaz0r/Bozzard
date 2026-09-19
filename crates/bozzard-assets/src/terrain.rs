//! Bounded editable heightfields. Runtime rendering, picking and physics use ordinary meshes.
use crate::{MeshData, job::Progress};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub version: u32,
    /// Vertex counts in X and Z, including both boundaries.
    pub resolution: [u16; 2],
    /// Total width/depth, centered on the object's local origin.
    pub size: [f32; 2],
    /// Row-major local Y, with X varying fastest.
    pub heights: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushMode {
    Raise,
    Lower,
    Flatten,
    Smooth,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainBrush {
    pub mode: BrushMode,
    pub center: [f32; 2],
    pub radius: f32,
    /// Height delta for Raise/Lower; interpolation weight (0..1) for Flatten/Smooth.
    pub strength: f32,
    pub target_height: f32,
}

impl Terrain {
    pub fn flat(resolution: [u16; 2], size: [f32; 2]) -> Result<Self> {
        ensure!(
            resolution.iter().all(|n| (2..=129).contains(n)),
            "terrain resolution must be 2..129 vertices per axis"
        );
        let terrain = Self {
            version: 1,
            resolution,
            size,
            heights: vec![0.; usize::from(resolution[0]) * usize::from(resolution[1])],
        };
        terrain.validate()?;
        Ok(terrain)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported terrain version");
        ensure!(
            self.resolution.iter().all(|n| (2..=129).contains(n)),
            "terrain resolution must be 2..129 vertices per axis"
        );
        ensure!(
            self.size
                .iter()
                .all(|n| n.is_finite() && (0.01..=100_000.).contains(n)),
            "terrain size must be finite and within 0.01..100000"
        );
        ensure!(
            self.heights.len() == usize::from(self.resolution[0]) * usize::from(self.resolution[1]),
            "terrain height count does not match its resolution"
        );
        ensure!(
            self.heights
                .iter()
                .all(|n| n.is_finite() && n.abs() <= 10_000.),
            "terrain heights must be finite and within ±10000"
        );
        Ok(())
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 4 * 1024 * 1024,
            "terrain source exceeds 4 MiB"
        );
        let terrain: Self = serde_json::from_slice(bytes)?;
        terrain.validate()?;
        Ok(terrain)
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    /// Exact height and face normal of the same two triangles emitted by `mesh`.
    pub fn sample(&self, point: [f32; 2]) -> Option<(f32, Vec3)> {
        if !point.iter().all(|v| v.is_finite())
            || self.resolution.iter().any(|n| *n < 2)
            || self.heights.len()
                != usize::from(self.resolution[0]) * usize::from(self.resolution[1])
            || self.size.iter().any(|n| !n.is_finite() || *n <= 0.)
        {
            return None;
        }
        let [nx, nz] = self.resolution.map(usize::from);
        let [sx, sz] = [
            self.size[0] / (nx - 1) as f32,
            self.size[1] / (nz - 1) as f32,
        ];
        let x = (point[0] + self.size[0] * 0.5) / sx;
        let z = (point[1] + self.size[1] * 0.5) / sz;
        if x < 0. || z < 0. || x > (nx - 1) as f32 || z > (nz - 1) as f32 {
            return None;
        }
        let ix = (x.floor() as usize).min(nx - 2);
        let iz = (z.floor() as usize).min(nz - 2);
        let [u, v] = [x - ix as f32, z - iz as f32];
        let a = self.heights[iz * nx + ix];
        let b = self.heights[iz * nx + ix + 1];
        let c = self.heights[(iz + 1) * nx + ix];
        let d = self.heights[(iz + 1) * nx + ix + 1];
        let (height, dx, dz) = if u + v <= 1. {
            (a + (b - a) * u + (c - a) * v, b - a, c - a)
        } else {
            (d + (c - d) * (1. - u) + (b - d) * (1. - v), d - c, d - b)
        };
        let normal = Vec3::new(-dx / sx, 1., -dz / sz).normalize();
        (height.is_finite() && normal.is_finite()).then_some((height, normal))
    }

    pub fn brush(&mut self, brush: TerrainBrush) -> Result<bool> {
        self.validate()?;
        ensure!(
            brush.center.iter().all(|v| v.is_finite())
                && brush.radius.is_finite()
                && (0.001..=100_000.).contains(&brush.radius),
            "invalid terrain brush position or radius"
        );
        ensure!(
            brush.strength.is_finite()
                && (0.0..=1000.).contains(&brush.strength)
                && brush.target_height.is_finite()
                && brush.target_height.abs() <= 10_000.,
            "invalid terrain brush strength or target"
        );
        let [nx, nz] = self.resolution.map(usize::from);
        let [sx, sz] = [
            self.size[0] / (nx - 1) as f32,
            self.size[1] / (nz - 1) as f32,
        ];
        let ranges: [_; 2] = std::array::from_fn(|axis| {
            let step = if axis == 0 { sx } else { sz };
            let limit = if axis == 0 { nx } else { nz };
            let center = brush.center[axis] + self.size[axis] * 0.5;
            let start = ((center - brush.radius) / step).ceil().max(0.) as usize;
            let end =
                (((center + brush.radius) / step).floor() + 1.).clamp(0., limit as f32) as usize;
            start.min(limit)..end
        });
        // Smoothing reads one immutable neighborhood; other modes allocate nothing.
        let previous = (brush.mode == BrushMode::Smooth).then(|| self.heights.clone());
        let mut changed = false;
        for z in ranges[1].clone() {
            for x in ranges[0].clone() {
                let dx = (x as f32 * sx - self.size[0] * 0.5 - brush.center[0]) / brush.radius;
                let dz = (z as f32 * sz - self.size[1] * 0.5 - brush.center[1]) / brush.radius;
                let distance = (dx * dx + dz * dz).sqrt();
                if distance >= 1. {
                    continue;
                }
                let falloff = (1. - distance).powi(2) * (1. + 2. * distance);
                let i = z * nx + x;
                let old = self.heights[i];
                let value = match brush.mode {
                    BrushMode::Raise => old + brush.strength * falloff,
                    BrushMode::Lower => old - brush.strength * falloff,
                    BrushMode::Flatten => {
                        old + (brush.target_height - old) * brush.strength.min(1.) * falloff
                    }
                    BrushMode::Smooth => {
                        let heights = previous.as_ref().unwrap();
                        let mut sum = 0.;
                        let mut count = 0;
                        for zz in z.saturating_sub(1)..=(z + 1).min(nz - 1) {
                            for xx in x.saturating_sub(1)..=(x + 1).min(nx - 1) {
                                sum += heights[zz * nx + xx];
                                count += 1;
                            }
                        }
                        old + (sum / count as f32 - old) * brush.strength.min(1.) * falloff
                    }
                }
                .clamp(-10_000., 10_000.);
                changed |= value != old;
                self.heights[i] = value;
            }
        }
        Ok(changed)
    }

    pub fn mesh(&self, progress: &Progress) -> Result<MeshData> {
        self.validate()?;
        let [nx, nz] = self.resolution.map(usize::from);
        let mut vertices = Vec::with_capacity(nx * nz);
        let mut indices = Vec::with_capacity((nx - 1) * (nz - 1) * 6);
        for z in 0..nz {
            progress.check()?;
            for x in 0..nx {
                let u = x as f32 / (nx - 1) as f32;
                let v = z as f32 / (nz - 1) as f32;
                vertices.push([
                    (u - 0.5) * self.size[0],
                    self.heights[z * nx + x],
                    (v - 0.5) * self.size[1],
                    0.,
                    0.,
                    0.,
                    u,
                    v,
                ]);
                if x + 1 < nx && z + 1 < nz {
                    let a = (z * nx + x) as u32;
                    let b = a + 1;
                    let c = a + nx as u32;
                    let d = c + 1;
                    indices.extend([a, c, b, b, c, d]);
                }
            }
        }
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
                .map(|i| Vec3::from_slice(&vertices[i as usize][..3]));
            let normal = (b - a).cross(c - a);
            for &i in triangle {
                let vertex = &mut vertices[i as usize];
                for axis in 0..3 {
                    vertex[axis + 3] += normal[axis];
                }
            }
        }
        for vertex in &mut vertices {
            let normal = Vec3::from_slice(&vertex[3..6]).normalize_or_zero();
            vertex[3..6].copy_from_slice(&normal.to_array());
        }
        Ok(MeshData {
            skin: None,
            vertices,
            indices,
            parts: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sloped_mesh_sampling_normals_and_roundtrip_agree() -> Result<()> {
        let mut terrain = Terrain::flat([3, 3], [4., 4.])?;
        for z in 0..3 {
            for x in 0..3 {
                terrain.heights[z * 3 + x] = x as f32 + 2. * z as f32;
            }
        }
        let mesh = terrain.mesh(&Progress::default())?;
        assert_eq!(mesh.indices.len(), 24);
        assert_eq!(mesh.vertices.len(), 9);
        let expected = Vec3::new(-0.5, 1., -1.).normalize();
        for vertex in &mesh.vertices {
            assert!(Vec3::from_slice(&vertex[3..6]).abs_diff_eq(expected, 1e-6));
        }
        for p in [[-1.5, -1.5], [-0.5, -0.5], [2., 2.]] {
            let (height, normal) = terrain.sample(p).unwrap();
            assert!((height - ((p[0] + 2.) * 0.5 + p[1] + 2.)).abs() < 1e-6);
            assert!(normal.abs_diff_eq(expected, 1e-6));
        }
        assert!(terrain.sample([2.01, 0.]).is_none());
        assert_eq!(Terrain::from_json(&terrain.to_json()?)?, terrain);
        Ok(())
    }

    #[test]
    fn brushes_are_bounded_and_smoothing_is_symmetric() -> Result<()> {
        let mut terrain = Terrain::flat([5, 5], [4., 4.])?;
        let mut brush = TerrainBrush {
            mode: BrushMode::Raise,
            center: [0., 0.],
            radius: 1.,
            strength: 4.,
            target_height: 2.,
        };
        assert!(terrain.brush(brush)?);
        assert_eq!(terrain.heights[12], 4.);
        assert_eq!(terrain.heights.iter().filter(|h| **h != 0.).count(), 1);
        brush.mode = BrushMode::Smooth;
        brush.radius = 3.;
        brush.strength = 1.;
        terrain.brush(brush)?;
        assert!(terrain.heights[12] < 4.);
        assert_eq!(terrain.heights[11], terrain.heights[13]);
        assert_eq!(terrain.heights[7], terrain.heights[17]);
        brush.mode = BrushMode::Flatten;
        brush.radius = 1.;
        terrain.brush(brush)?;
        assert_eq!(terrain.heights[12], 2.);
        brush.mode = BrushMode::Lower;
        brush.strength = 1.;
        terrain.brush(brush)?;
        assert_eq!(terrain.heights[12], 1.);
        brush.center = [100., 100.];
        assert!(!terrain.brush(brush)?);
        brush.center = [f32::MAX, -f32::MAX];
        assert!(!terrain.brush(brush)?);
        assert!(Terrain::flat([130, 2], [1., 1.]).is_err());
        assert!(Terrain::flat([2, 2], [f32::NAN, 1.]).is_err());
        terrain.heights.pop();
        assert!(terrain.validate().is_err());
        assert!(terrain.sample([0., 0.]).is_none());
        Ok(())
    }
}
