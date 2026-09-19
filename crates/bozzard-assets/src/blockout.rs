//! Small closed brush primitives, centered in X/Z with their planting plane at local Y=0.
use crate::{MeshData, job::Progress};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrushPrimitive {
    #[default]
    Box,
    Ramp,
    Stairs {
        steps: u16,
    },
    Cylinder {
        sides: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blockout {
    pub version: u32,
    pub primitive: BrushPrimitive,
}
impl Default for Blockout {
    fn default() -> Self {
        Self {
            version: 1,
            primitive: BrushPrimitive::Box,
        }
    }
}
impl Blockout {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported blockout version");
        match self.primitive {
            BrushPrimitive::Stairs { steps } => {
                ensure!((1..=128).contains(&steps), "Stairs need 1..128 steps")
            }
            BrushPrimitive::Cylinder { sides } => {
                ensure!((3..=64).contains(&sides), "Cylinder needs 3..64 sides")
            }
            _ => {}
        }
        Ok(())
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 16 * 1024, "blockout source exceeds 16 KiB");
        let brush: Self = serde_json::from_slice(bytes)?;
        brush.validate()?;
        Ok(brush)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
    /// Stable shape identity. Instance dimensions belong to the object's transform.
    pub fn name(&self) -> String {
        match self.primitive {
            BrushPrimitive::Box => "box".into(),
            BrushPrimitive::Ramp => "ramp".into(),
            BrushPrimitive::Stairs { steps } => format!("stairs-{steps}"),
            BrushPrimitive::Cylinder { sides } => format!("cylinder-{sides}"),
        }
    }
    pub fn mesh(&self, progress: &Progress) -> Result<MeshData> {
        self.validate()?;
        progress.check()?;
        let mut builder = Builder {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
        match self.primitive {
            BrushPrimitive::Box | BrushPrimitive::Stairs { .. } => {
                let steps = match self.primitive {
                    BrushPrimitive::Stairs { steps } => steps,
                    _ => 1,
                };
                for step in 0..steps {
                    progress.check()?;
                    let previous = f32::from(step) / f32::from(steps);
                    let height = f32::from(step + 1) / f32::from(steps);
                    let z0 = previous - 0.5;
                    let z1 = height - 0.5;
                    // Only exterior faces: omit hidden faces between adjacent steps.
                    builder.quad([
                        [-0.5, height, z0],
                        [-0.5, height, z1],
                        [0.5, height, z1],
                        [0.5, height, z0],
                    ]);
                    builder.quad([
                        [-0.5, previous, z0],
                        [-0.5, height, z0],
                        [0.5, height, z0],
                        [0.5, previous, z0],
                    ]);
                    builder.quad([
                        [-0.5, 0., z0],
                        [-0.5, 0., z1],
                        [-0.5, height, z1],
                        [-0.5, height, z0],
                    ]);
                    builder.quad([
                        [0.5, 0., z0],
                        [0.5, height, z0],
                        [0.5, height, z1],
                        [0.5, 0., z1],
                    ]);
                }
                builder.quad([
                    [-0.5, 0., -0.5],
                    [0.5, 0., -0.5],
                    [0.5, 0., 0.5],
                    [-0.5, 0., 0.5],
                ]);
                builder.quad([
                    [-0.5, 0., 0.5],
                    [0.5, 0., 0.5],
                    [0.5, 1., 0.5],
                    [-0.5, 1., 0.5],
                ]);
            }
            BrushPrimitive::Ramp => {
                builder.quad([
                    [-0.5, 0., -0.5],
                    [0.5, 0., -0.5],
                    [0.5, 0., 0.5],
                    [-0.5, 0., 0.5],
                ]);
                builder.quad([
                    [-0.5, 0., -0.5],
                    [-0.5, 1., 0.5],
                    [0.5, 1., 0.5],
                    [0.5, 0., -0.5],
                ]);
                builder.quad([
                    [-0.5, 0., 0.5],
                    [0.5, 0., 0.5],
                    [0.5, 1., 0.5],
                    [-0.5, 1., 0.5],
                ]);
                builder.triangle(
                    [[-0.5, 0., -0.5], [-0.5, 0., 0.5], [-0.5, 1., 0.5]],
                    [[0., 0.], [1., 0.], [1., 1.]],
                );
                builder.triangle(
                    [[0.5, 0., -0.5], [0.5, 1., 0.5], [0.5, 0., 0.5]],
                    [[0., 0.], [1., 1.], [1., 0.]],
                );
            }
            BrushPrimitive::Cylinder { sides } => {
                for side in 0..sides {
                    let points: [_; 2] = std::array::from_fn(|i| {
                        let angle = std::f32::consts::TAU * f32::from((side + i as u16) % sides)
                            / f32::from(sides);
                        Vec3::new(0.5 * angle.cos(), 0., 0.5 * angle.sin())
                    });
                    let [a, b] = points;
                    builder.quad([a, a + Vec3::Y, b + Vec3::Y, b].map(|p| p.to_array()));
                    let uv = [[0.5, 0.5], [a.x + 0.5, a.z + 0.5], [b.x + 0.5, b.z + 0.5]];
                    builder.triangle([Vec3::ZERO, a, b].map(|p| p.to_array()), uv);
                    builder.triangle(
                        [Vec3::Y, b + Vec3::Y, a + Vec3::Y].map(|p| p.to_array()),
                        [uv[0], uv[2], uv[1]],
                    );
                }
            }
        }
        Ok(MeshData {
            skin: None,
            vertices: builder.vertices,
            indices: builder.indices,
            parts: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

struct Builder {
    vertices: Vec<[f32; 8]>,
    indices: Vec<u32>,
}
impl Builder {
    fn triangle(&mut self, points: [[f32; 3]; 3], uv: [[f32; 2]; 3]) {
        let [a, b, c] = points.map(Vec3::from_array);
        let normal = (b - a).cross(c - a).normalize();
        for (p, uv) in points.into_iter().zip(uv) {
            self.indices.push(self.vertices.len() as u32);
            self.vertices
                .push([p[0], p[1], p[2], normal.x, normal.y, normal.z, uv[0], uv[1]]);
        }
    }
    fn quad(&mut self, p: [[f32; 3]; 4]) {
        self.triangle([p[0], p[1], p[2]], [[0., 0.], [0., 1.], [1., 1.]]);
        self.triangle([p[0], p[2], p[3]], [[0., 0.], [1., 1.], [1., 0.]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_brushes_have_outward_faces_expected_volume_and_bounded_geometry() -> Result<()> {
        for (primitive, triangles, volume) in [
            (BrushPrimitive::Box, 12, 1.),
            (BrushPrimitive::Ramp, 8, 0.5),
            (BrushPrimitive::Stairs { steps: 4 }, 36, 0.625),
            (
                BrushPrimitive::Cylinder { sides: 16 },
                64,
                2. * (std::f32::consts::TAU / 16.).sin(),
            ),
        ] {
            let brush = Blockout {
                version: 1,
                primitive,
            };
            let mesh = brush.mesh(&Progress::default())?;
            assert_eq!(mesh.indices.len(), triangles * 3);
            let mut signed_volume = 0.;
            for tri in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [tri[0], tri[1], tri[2]]
                    .map(|i| Vec3::from_slice(&mesh.vertices[i as usize][..3]));
                signed_volume += a.dot(b.cross(c)) / 6.;
                let n = Vec3::from_slice(&mesh.vertices[tri[0] as usize][3..6]);
                assert!(n.is_finite() && (n.length() - 1.).abs() < 1e-5);
                assert!(n.dot((b - a).cross(c - a)) > 0.);
            }
            assert!(
                (signed_volume - volume).abs() < 1e-5,
                "{primitive:?}: {signed_volume} != {volume}"
            );
            assert_eq!(Blockout::from_json(&brush.to_json()?)?, brush);
        }
        assert!(
            Blockout {
                version: 1,
                primitive: BrushPrimitive::Stairs { steps: 0 }
            }
            .mesh(&Progress::default())
            .is_err()
        );
        assert!(
            Blockout {
                version: 1,
                primitive: BrushPrimitive::Cylinder { sides: 65 }
            }
            .validate()
            .is_err()
        );
        Ok(())
    }
}
