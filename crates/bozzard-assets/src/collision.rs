//! Bake source geometry once; the headless scene stores triangles rather than importing graphics.
use super::*;
use bozzard_scene::{Drawable, Mesh, MeshCollider, TriangleMesh};

impl AssetStore {
    // ponytail: explicit cooking is synchronous and capped at 100k triangles; use a loading job if it becomes slow.
    pub fn cook_mesh_collider(&self, drawable: &Drawable) -> Result<MeshCollider> {
        let mut triangles = Vec::new();
        let mut quad = |p: [Vec3; 4]| {
            triangles.push([p[0], p[1], p[3]].map(|p| p.to_array()));
            triangles.push([p[0], p[3], p[2]].map(|p| p.to_array()));
        };
        match &drawable.mesh {
            Mesh::Quad => quad(std::array::from_fn(|i| {
                Vec3::new((i & 1) as f32 - 0.5, (i >> 1) as f32 - 0.5, 0.)
            })),
            Mesh::Cube => {
                for axis in 0..3 {
                    for side in [0., 1.] {
                        quad(std::array::from_fn(|i| {
                            let mut p = Vec3::splat(-0.5);
                            p[axis] += side;
                            p[(axis + 1) % 3] += (i & 1) as f32;
                            p[(axis + 2) % 3] += (i >> 1) as f32;
                            p
                        }));
                    }
                }
            }
            Mesh::Asset(asset) | Mesh::Surface { asset, .. } => {
                let AssetData::Mesh(mesh) = self
                    .handle(asset)
                    .and_then(|h| self.get(h))
                    .and_then(|e| e.data())
                    .context("Load the source mesh before adding/rebuilding its collider")?
                else {
                    bail!("Mesh Collider source is not a mesh");
                };
                let mut append = |indices: &[u32], matrix: Mat4| -> Result<()> {
                    ensure!(
                        indices.len().is_multiple_of(3)
                            && triangles.len() + indices.len() / 3 <= 100_000,
                        "Mesh Collider limit: 100000 triangles; use separate surfaces or a simplified mesh"
                    );
                    for indices in indices.chunks_exact(3) {
                        let mut triangle = [[0.; 3]; 3];
                        for (p, index) in triangle.iter_mut().zip(indices) {
                            let vertex = mesh
                                .vertices
                                .get(*index as usize)
                                .context("invalid collision mesh index")?;
                            *p = matrix
                                .transform_point3(Vec3::from_slice(&vertex[..3]))
                                .to_array();
                        }
                        triangles.push(triangle);
                    }
                    Ok(())
                };
                if let Mesh::Surface { .. } = &drawable.mesh {
                    let (part, bounds) = self.mesh_surface(&drawable.mesh).context("Source surface changed or is missing; reassign the mesh before rebuilding its collider")?;
                    let center = bounds[0] * 0.5 + bounds[1] * 0.5;
                    append(
                        &mesh.indices[part.start as usize..(part.start + part.count) as usize],
                        Mat4::from_translation(-center),
                    )?;
                } else if mesh.parts.is_empty() {
                    append(&mesh.indices, Mat4::IDENTITY)?;
                } else {
                    for (index, part) in mesh.parts.iter().enumerate() {
                        let bounds = mesh.part_bounds(index).context("empty model surface")?;
                        let matrix = drawable
                            .material_overrides
                            .iter()
                            .find(|v| v.surface as usize == index && v.source == part.source_key)
                            .map(|v| v.matrix(bounds[0] * 0.5 + bounds[1] * 0.5))
                            .unwrap_or(Mat4::IDENTITY);
                        append(
                            &mesh.indices[part.start as usize..(part.start + part.count) as usize],
                            matrix,
                        )?;
                    }
                }
            }
        }
        Ok(MeshCollider {
            enabled: true,
            mesh: TriangleMesh::new(triangles)?,
        })
    }
}
