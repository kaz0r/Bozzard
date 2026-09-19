//! Offline, attribute-aware LOD generation. Each material is simplified independently.
use crate::{MeshData, MeshPart, SurfaceShading, job::Progress};
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy, Debug)]
pub struct SimplifySettings {
    /// Requested fraction of the original triangles, in (0, 1].
    pub ratio: f32,
    /// Maximum combined geometric/attribute error, relative to each surface's extent.
    pub max_error: f32,
    /// Keep open boundaries fixed, including boundaries between materials.
    pub lock_borders: bool,
}
impl Default for SimplifySettings {
    fn default() -> Self {
        Self {
            ratio: 0.5,
            max_error: 0.01,
            lock_borders: true,
        }
    }
}
impl SimplifySettings {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.ratio.is_finite() && self.ratio > 0. && self.ratio <= 1.,
            "LOD ratio must be in (0, 1]"
        );
        ensure!(
            self.max_error.is_finite() && (0. ..=1.).contains(&self.max_error),
            "LOD error must be in [0, 1]"
        );
        Ok(())
    }
}
pub struct Simplification {
    pub mesh: MeshData,
    pub source_triangles: usize,
    pub triangles: usize,
    /// Largest combined error reported by the simplifier across all surfaces.
    pub error: f32,
}

/// Preserves material boundaries, normal/UV discontinuities and all supported PBR maps.
/// The requested ratio is a target: the error/border constraints take precedence.
/// No input or source file is mutated. Skinned models require a separate deformation-aware LOD path.
pub fn simplify_mesh(
    mesh: &MeshData,
    settings: SimplifySettings,
    progress: &Progress,
) -> Result<Simplification> {
    settings.validate()?;
    validate_mesh(mesh)?;
    let fallback = MeshPart {
        source_key: "mesh".into(),
        name: "Mesh".into(),
        material_name: None,
        start: 0,
        count: mesh.indices.len() as u32,
        color: [1.; 4],
        image: None,
        alpha_cutoff: None,
        shading: None,
    };
    let parts = if mesh.parts.is_empty() {
        std::slice::from_ref(&fallback)
    } else {
        &mesh.parts
    };
    let mut output = MeshData {
        skin: None,
        vertices: Vec::new(),
        indices: Vec::new(),
        parts: Vec::new(),
        warnings: mesh.warnings.clone(),
    };
    let mut max_error = 0_f32;
    for (part_index, part) in parts.iter().enumerate() {
        progress.stage(format!(
            "Simplifying surface {}/{}",
            part_index + 1,
            parts.len()
        ))?;
        let input = &mesh.indices[part.start as usize..(part.start + part.count) as usize];
        // Compact before simplification. Welding compares every attribute, so coincident
        // vertices on a UV seam or hard edge cannot silently become one vertex.
        let mut vertices = Vec::new();
        let mut local = std::collections::HashMap::new();
        let indices: Vec<_> = input
            .iter()
            .map(|&index| {
                *local.entry(index).or_insert_with(|| {
                    let mut vertex = [0_f32; 20];
                    vertex[..8].copy_from_slice(&mesh.vertices[index as usize]);
                    if let Some(s) = &part.shading {
                        vertex[8..].copy_from_slice(&s.vertices[(index - s.vertex_start) as usize]);
                    }
                    let next = vertices.len() as u32;
                    vertices.push(vertex);
                    next
                })
            })
            .collect();
        let (count, remap) = meshopt::generate_vertex_remap(&vertices, Some(&indices));
        let vertices = meshopt::remap_vertex_buffer(&vertices, count, &remap);
        let indices = meshopt::remap_index_buffer(Some(&indices), count, &remap);
        let adapter = meshopt::VertexDataAdapter::new(
            meshopt::typed_to_bytes(&vertices),
            std::mem::size_of::<[f32; 20]>(),
            0,
        )?;
        // Normal XYZ, base UV, tangent XYZW and four texture-coordinate pairs.
        let weights = [
            1., 1., 1., 10., 10., 1., 1., 1., 1., 10., 10., 10., 10., 10., 10., 10., 10.,
        ];
        let attribute_count = if part.shading.is_some() { 17 } else { 5 };
        let attributes: Vec<f32> = vertices
            .iter()
            .flat_map(|v| v[3..3 + attribute_count].iter().copied())
            .collect();
        let target = ((indices.len() / 3) as f32 * settings.ratio).ceil().max(1.) as usize * 3;
        let mut error = 0.;
        let mut reduced = meshopt::simplify_with_attributes_and_locks(
            &indices,
            &adapter,
            &attributes,
            &weights[..attribute_count],
            attribute_count * 4,
            &vec![false; vertices.len()],
            target,
            settings.max_error,
            if settings.lock_borders {
                meshopt::SimplifyOptions::LockBorder
            } else {
                meshopt::SimplifyOptions::None
            },
            Some(&mut error),
        );
        progress.check()?;
        ensure!(
            !reduced.is_empty(),
            "simplification removed every triangle in surface {}",
            part.name
        );
        max_error = max_error.max(error);
        // BLEND triangle order is meaningful. Keep the simplifier's order for
        // potentially translucent material surfaces; opaque surfaces can optimize cache use.
        let transparent = part.alpha_cutoff.is_none()
            && (part.color[3] < 1.
                || part
                    .image
                    .as_ref()
                    .is_some_and(|i| i.rgba.chunks_exact(4).any(|p| p[3] != 255)));
        if !transparent {
            meshopt::optimize_vertex_cache_in_place(&mut reduced, vertices.len());
        }
        let compact = meshopt::optimize_vertex_fetch(&mut reduced, &vertices);
        ensure!(
            output.vertices.len() + compact.len() <= crate::MAX_VERTICES,
            "simplified surface vertices exceed the mesh limit"
        );
        let base = output.vertices.len() as u32;
        let start = output.indices.len() as u32;
        output.vertices.extend(
            compact
                .iter()
                .map(|v| <[f32; 8]>::try_from(&v[..8]).unwrap()),
        );
        output.indices.extend(reduced.iter().map(|i| base + i));
        output.parts.push(MeshPart {
            start,
            count: reduced.len() as u32,
            shading: part.shading.as_ref().map(|s| SurfaceShading {
                vertex_start: base,
                vertices: compact.iter().map(|v| v[8..].try_into().unwrap()).collect(),
                material: s.material.clone(),
            }),
            ..part.clone()
        });
    }
    if mesh.parts.is_empty() {
        output.parts.clear();
    }
    Ok(Simplification {
        source_triangles: mesh.indices.len() / 3,
        triangles: output.indices.len() / 3,
        error: max_error,
        mesh: output.with_surface_keys(),
    })
}

pub(crate) fn validate_mesh(mesh: &MeshData) -> Result<()> {
    ensure!(
        mesh.skin.is_none(),
        "automatic LOD generation currently requires a static mesh"
    );
    validate_geometry(mesh)
}

pub(crate) fn validate_geometry(mesh: &MeshData) -> Result<()> {
    ensure!(
        !mesh.vertices.is_empty() && mesh.vertices.len() <= crate::MAX_VERTICES,
        "empty or oversized vertex stream"
    );
    ensure!(
        !mesh.indices.is_empty()
            && mesh.indices.len() <= 3_000_000
            && mesh.indices.len().is_multiple_of(3),
        "invalid triangle index count"
    );
    ensure!(
        mesh.vertices.iter().flatten().all(|v| v.is_finite())
            && mesh
                .indices
                .iter()
                .all(|&i| (i as usize) < mesh.vertices.len()),
        "invalid mesh vertex or index"
    );
    ensure!(mesh.parts.len() <= crate::MAX_PARTS, "too many surfaces");
    let mut cursor = 0;
    for part in &mesh.parts {
        let end = part
            .start
            .checked_add(part.count)
            .context("surface range overflow")? as usize;
        ensure!(
            part.start as usize == cursor
                && part.count > 0
                && part.count.is_multiple_of(3)
                && end <= mesh.indices.len(),
            "surfaces must partition the index stream in order"
        );
        if let Some(s) = &part.shading {
            let vertex_end = (s.vertex_start as usize)
                .checked_add(s.vertices.len())
                .context("shading range overflow")?;
            ensure!(
                vertex_end <= mesh.vertices.len()
                    && s.vertices.iter().flatten().all(|v| v.is_finite()),
                "invalid shading vertices"
            );
            ensure!(
                mesh.indices[cursor..end]
                    .iter()
                    .all(|&i| i >= s.vertex_start && (i as usize) < vertex_end),
                "surface index outside shading range"
            );
        }
        cursor = end;
    }
    ensure!(
        mesh.parts.is_empty() || cursor == mesh.indices.len(),
        "surfaces do not cover all triangles"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid() -> MeshData {
        grid_size(32)
    }
    fn grid_size(size: u32) -> MeshData {
        let mut mesh = MeshData {
            skin: None,
            vertices: Vec::new(),
            indices: Vec::new(),
            parts: Vec::new(),
            warnings: Vec::new(),
        };
        for y in 0..=size {
            for x in 0..=size {
                let [x, y] = [x as f32 / size as f32, y as f32 / size as f32];
                mesh.vertices.push([x, y, 0., 0., 0., 1., x, y]);
            }
        }
        for y in 0..size {
            for x in 0..size {
                let i = y * (size + 1) + x;
                mesh.indices
                    .extend([i, i + 1, i + size + 1, i + 1, i + size + 2, i + size + 1]);
            }
        }
        mesh
    }
    #[test]
    #[ignore = "release-mode offline mesh-generation benchmark; run explicitly"]
    fn simplification_scale_benchmark() {
        if cfg!(debug_assertions) {
            eprintln!("Use --release for comparable benchmark timings");
        }
        let mut mesh = grid_size(256);
        for v in &mut mesh.vertices {
            let [x, y] = [v[0], v[1]];
            v[2] = 0.04 * (x * 12.).sin() * (y * 8.).cos();
            let normal = glam::Vec3::new(
                -0.48 * (x * 12.).cos() * (y * 8.).cos(),
                0.32 * (x * 12.).sin() * (y * 8.).sin(),
                1.,
            )
            .normalize();
            v[3..6].copy_from_slice(&normal.to_array());
        }
        for ratio in [0.5, 0.25, 0.125] {
            let settings = SimplifySettings {
                ratio,
                max_error: 0.01,
                lock_borders: true,
            };
            let mut samples = Vec::new();
            let mut triangles = 0;
            let mut vertices = 0;
            for run in 0..8 {
                let start = std::time::Instant::now();
                let result = simplify_mesh(&mesh, settings, &Progress::default()).unwrap();
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                assert!(
                    result.error <= settings.max_error
                        && result.triangles < result.source_triangles
                );
                if run > 0 {
                    samples.push(elapsed);
                }
                triangles = result.triangles;
                vertices = result.mesh.vertices.len();
            }
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "mesh_lod_benchmark curved_grid source_triangles={} triangles={} ratio={} vertices={} geometry_bytes={} median_ms={:.3}",
                mesh.indices.len() / 3,
                triangles,
                ratio,
                vertices,
                vertices * 32 + triangles * 12,
                samples[samples.len() / 2]
            );
        }
    }
    #[test]
    fn simplifies_real_geometry_and_preserves_all_locked_boundary_vertices() {
        let mesh = grid();
        let result = simplify_mesh(
            &mesh,
            SimplifySettings {
                ratio: 0.2,
                ..Default::default()
            },
            &Progress::default(),
        )
        .unwrap();
        assert_eq!(result.source_triangles, 2048);
        assert!(
            result.triangles <= 410 && result.triangles > 0,
            "{}",
            result.triangles
        );
        assert!(result.mesh.vertices.len() < mesh.vertices.len() / 2);
        assert!(result.error <= 0.01);
        for vertex in mesh
            .vertices
            .iter()
            .filter(|v| v[0] == 0. || v[0] == 1. || v[1] == 0. || v[1] == 1.)
        {
            assert!(
                result.mesh.vertices.contains(vertex),
                "lost boundary {vertex:?}"
            );
        }
        for v in &result.mesh.vertices {
            assert!(mesh.vertices.contains(v));
        }
        let again = simplify_mesh(
            &mesh,
            SimplifySettings {
                ratio: 0.2,
                ..Default::default()
            },
            &Progress::default(),
        )
        .unwrap();
        assert_eq!(again.mesh.indices, result.mesh.indices);
        assert_eq!(again.mesh.vertices, result.mesh.vertices);
    }
    #[test]
    fn surfaces_and_attributes_are_kept_independent_and_malformed_ranges_fail() {
        let mut mesh = grid();
        let second = mesh.vertices.len() as u32;
        let mut back = mesh.vertices.clone();
        for v in &mut back {
            v[0] += 2.;
            v[5] = -1.;
            v[6] = 1. - v[6];
        }
        mesh.vertices.extend(back);
        let split = mesh.indices.len() as u32;
        mesh.indices
            .extend(mesh.indices.clone().into_iter().map(|i| i + second));
        for index in 0..2 {
            mesh.parts.push(MeshPart {
                source_key: format!("part-{index}"),
                name: format!("part-{index}"),
                material_name: Some(format!("material-{index}")),
                start: index * split,
                count: split,
                color: [index as f32, 0.5, 1., 1.],
                image: None,
                alpha_cutoff: None,
                shading: None,
            });
        }
        let result =
            simplify_mesh(&mesh, SimplifySettings::default(), &Progress::default()).unwrap();
        assert_eq!(result.mesh.parts.len(), 2);
        for (before, after) in mesh.parts.iter().zip(&result.mesh.parts) {
            assert_eq!(before.color, after.color);
            assert_eq!(before.material_name, after.material_name);
            assert!(after.count < before.count);
            assert_ne!(before.source_key, after.source_key);
        }
        validate_mesh(&result.mesh).unwrap();
        mesh.parts[1].count = u32::MAX;
        assert!(simplify_mesh(&mesh, SimplifySettings::default(), &Progress::default()).is_err());
        mesh.parts.clear();
        mesh.indices[0] = u32::MAX;
        assert!(simplify_mesh(&mesh, SimplifySettings::default(), &Progress::default()).is_err());
        for ratio in [0., -1., f32::NAN, 2.] {
            assert!(
                SimplifySettings {
                    ratio,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
