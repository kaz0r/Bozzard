//! Lossless full-resolution cooking. First-use vertex order improves fetch locality;
//! complete attribute tuples weld duplicates without changing primitive order.
use crate::{MeshData, SurfaceShading, animation::Skin, job::Progress};
use anyhow::Result;

pub(crate) fn mesh(mesh: &MeshData, progress: &Progress) -> Result<MeshData> {
    progress.check()?;
    // Packing part ranges is lossless only when their authored order is the
    // complete original index order. Preserve arbitrary ranges verbatim: gaps,
    // overlap or reordering would otherwise change global picking triangle IDs
    // and could leave cloned part offsets pointing at a different surface.
    let mut cursor = 0;
    for part in &mesh.parts {
        let Some(end) = part.start.checked_add(part.count) else {
            return Ok(mesh.clone());
        };
        if part.start as usize != cursor
            || part.count == 0
            || !part.count.is_multiple_of(3)
            || end as usize > mesh.indices.len()
        {
            return Ok(mesh.clone());
        }
        cursor = end as usize;
    }
    if !mesh.parts.is_empty() && cursor != mesh.indices.len() {
        return Ok(mesh.clone());
    }
    let mut output = MeshData {
        skin: mesh.skin.as_ref().map(|skin| Skin {
            rig: skin.rig.clone(),
            vertices: Vec::new(),
        }),
        vertices: Vec::with_capacity(mesh.vertices.len()),
        indices: Vec::with_capacity(mesh.indices.len()),
        parts: Vec::with_capacity(mesh.parts.len()),
        warnings: mesh.warnings.clone(),
    };
    let ranges: Vec<_> = if mesh.parts.is_empty() {
        vec![(0, mesh.indices.len(), None)]
    } else {
        mesh.parts
            .iter()
            .map(|part| (part.start as usize, part.count as usize, Some(part)))
            .collect()
    };
    for (start, count, part) in ranges {
        progress.check()?;
        let index_start = output.indices.len() as u32;
        let base = output.vertices.len() as u32;
        let mut attributes = Vec::new();
        // Separate surfaces cannot weld across different PBR or skin streams.
        let mut remap = std::collections::HashMap::<[u32; 28], u32>::new();
        for (offset, &index) in mesh.indices[start..start + count].iter().enumerate() {
            if offset.is_multiple_of(4096) {
                progress.check()?;
            }
            let vertex = mesh.vertices[index as usize];
            let shading = part
                .and_then(|p| p.shading.as_ref())
                .map(|s| s.vertices[(index - s.vertex_start) as usize]);
            let skin = mesh.skin.as_ref().map(|s| s.vertices[index as usize]);
            let mut key = [0; 28];
            key[..8].copy_from_slice(&vertex.map(f32::to_bits));
            if let Some(shading) = shading {
                key[8..20].copy_from_slice(&shading.map(f32::to_bits));
            }
            if let Some(skin) = skin {
                key[20..].copy_from_slice(&skin);
            }
            let next = *remap.entry(key).or_insert_with(|| {
                let next = output.vertices.len() as u32;
                output.vertices.push(vertex);
                if let Some(shading) = shading {
                    attributes.push(shading);
                }
                if let Some(skin) = skin {
                    output.skin.as_mut().unwrap().vertices.push(skin);
                }
                next
            });
            output.indices.push(next);
        }
        if let Some(part) = part {
            let mut part = part.clone();
            part.start = index_start;
            if let Some(shading) = &part.shading {
                part.shading = Some(SurfaceShading {
                    vertex_start: base,
                    vertices: attributes,
                    material: shading.material.clone(),
                });
            }
            output.parts.push(part);
        }
        // A source with shared vertices across surfaces can expand when streams
        // split. Keep its original buffers rather than increasing memory use.
        if output.vertices.len() > mesh.vertices.len()
            || output.vertices.len() > crate::MAX_VERTICES
        {
            return Ok(mesh.clone());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeshPart, PbrMaterial};
    use std::sync::Arc;

    fn ordered_attributes(mesh: &MeshData) -> Vec<[u32; 28]> {
        let mut values = Vec::new();
        for part in &mesh.parts {
            for &index in &mesh.indices[part.start as usize..(part.start + part.count) as usize] {
                let mut value = [0; 28];
                value[..8].copy_from_slice(&mesh.vertices[index as usize].map(f32::to_bits));
                if let Some(s) = &part.shading {
                    value[8..20].copy_from_slice(
                        &s.vertices[(index - s.vertex_start) as usize].map(f32::to_bits),
                    );
                }
                if let Some(s) = &mesh.skin {
                    value[20..].copy_from_slice(&s.vertices[index as usize]);
                }
                values.push(value);
            }
        }
        values
    }
    #[test]
    fn lossless_welding_preserves_all_stream_bits_and_triangle_picking_order() {
        let a = [0., 0., 0., 0., 0., 1., 0., 0.];
        let b = [1., 0., 0., 0., 0., 1., 1., 0.];
        let c = [0., 1., 0., 0., 0., 1., 0., 1.];
        let mut shading = vec![[1.; 12]; 6];
        shading[4][8] = -0.; // Do not weld an emissive UV discontinuity.
        let input = MeshData {
            vertices: vec![a, b, c, a, b, c],
            indices: vec![3, 4, 5, 0, 1, 2],
            warnings: vec!["source warning".into()],
            skin: Some(Skin {
                rig: Arc::new(Default::default()),
                vertices: vec![[0; 8]; 6],
            }),
            parts: vec![MeshPart {
                source_key: "authored-source".into(),
                name: "alpha surface".into(),
                material_name: None,
                start: 0,
                count: 6,
                color: [1., 1., 1., 0.5],
                image: None,
                alpha_cutoff: None,
                shading: Some(SurfaceShading {
                    vertex_start: 0,
                    vertices: shading,
                    material: PbrMaterial {
                        metallic: 0.,
                        roughness: 1.,
                        normal_scale: 1.,
                        occlusion_strength: 1.,
                        emissive_factor: [0.; 3],
                        double_sided: false,
                        base_color_sampler: crate::Sampler::default(),
                        metallic_roughness: None,
                        normal: None,
                        occlusion: None,
                        emissive: None,
                    },
                }),
            }],
        };
        let output = mesh(&input, &Progress::default()).unwrap();
        assert_eq!(output.vertices.len(), 4);
        assert_eq!(output.indices, [0, 1, 2, 0, 3, 2]);
        assert_eq!(ordered_attributes(&input), ordered_attributes(&output));
        assert_eq!(input.part_bounds(0), output.part_bounds(0));
        assert_eq!(output.parts[0].source_key, input.parts[0].source_key);
        assert_eq!(output.parts[0].color, input.parts[0].color);
        assert_eq!(output.warnings, input.warnings);
        assert_eq!(input.vertices.len(), 6);
        let source_index = crate::picking::MeshIndex::build(&input, &Progress::default()).unwrap();
        let cooked_index = crate::picking::MeshIndex::build(&output, &Progress::default()).unwrap();
        // Overlapping alpha triangles keep their authored picking identities,
        // independently of the welded GPU vertex numbers.
        for triangle in [0, 1] {
            let accept = |candidate| candidate == triangle;
            let origin = glam::Vec3::new(0.25, 0.25, 1.);
            let direction = glam::Vec3::NEG_Z;
            let before = source_index.cast_filtered(&input, origin, direction, &accept);
            let after = cooked_index.cast_filtered(&output, origin, direction, &accept);
            assert_eq!(before, after);
            assert_eq!(after.unwrap().triangle, triangle);
        }
    }
    #[test]
    fn cross_surface_sharing_never_expands_buffers() {
        let vertices = vec![[0.; 8]; 3];
        let part = MeshPart {
            source_key: "source".into(),
            name: "surface".into(),
            material_name: None,
            start: 0,
            count: 3,
            color: [1.; 4],
            image: None,
            alpha_cutoff: None,
            shading: None,
        };
        let mut other = part.clone();
        other.start = 3;
        let input = MeshData {
            vertices,
            indices: vec![0, 1, 2, 0, 1, 2],
            parts: vec![part, other],
            skin: None,
            warnings: Vec::new(),
        };
        let output = mesh(&input, &Progress::default()).unwrap();
        assert!(output.vertices.len() <= input.vertices.len());
        assert_eq!(ordered_attributes(&input), ordered_attributes(&output));
    }

    #[test]
    fn noncanonical_part_spans_preserve_source_indices_ranges_and_picking_ids() {
        let vertices: Vec<_> = (0..3)
            .flat_map(|triangle| {
                let x = triangle as f32 * 2.;
                [
                    [x, 0., 0., 0., 0., 1., 0., 0.],
                    [x + 1., 0., 0., 0., 0., 1., 1., 0.],
                    [x, 1., 0., 0., 0., 1., 0., 1.],
                ]
            })
            .collect();
        for spans in [
            vec![(6, 3), (0, 6)], // Complete, out of authored index order.
            vec![(0, 3), (6, 3)], // Interior gap.
            vec![(0, 6), (3, 6)], // Overlap.
            vec![(3, 6)],         // Leading gap.
            vec![(0, 6)],         // Trailing gap.
        ] {
            let input = MeshData {
                vertices: vertices.clone(),
                indices: (0..9).collect(),
                parts: spans
                    .into_iter()
                    .enumerate()
                    .map(|(surface, (start, count))| MeshPart {
                        source_key: format!("source-{surface}"),
                        name: format!("surface-{surface}"),
                        material_name: Some(format!("material-{surface}")),
                        start,
                        count,
                        color: [surface as f32, 0., 1., 0.5],
                        image: None,
                        alpha_cutoff: Some(0.25),
                        shading: None,
                    })
                    .collect(),
                skin: None,
                warnings: vec!["authored warning".into()],
            };
            // Production upload/cooking validation already rejects these spans.
            // The optimizer boundary also preserves them if an internal caller
            // reaches it without the ordered-partition precondition.
            assert!(crate::simplify::validate_geometry(&input).is_err());
            let output = mesh(&input, &Progress::default()).unwrap();
            assert_eq!(output.indices, input.indices);
            assert_eq!(
                output
                    .vertices
                    .iter()
                    .map(|v| v.map(f32::to_bits))
                    .collect::<Vec<_>>(),
                input
                    .vertices
                    .iter()
                    .map(|v| v.map(f32::to_bits))
                    .collect::<Vec<_>>()
            );
            assert_eq!(ordered_attributes(&output), ordered_attributes(&input));
            assert_eq!(output.warnings, input.warnings);
            for (surface, (before, after)) in input.parts.iter().zip(&output.parts).enumerate() {
                assert_eq!((after.start, after.count), (before.start, before.count));
                assert_eq!(after.source_key, before.source_key);
                assert_eq!(after.name, before.name);
                assert_eq!(after.material_name, before.material_name);
                assert_eq!(
                    after.color.map(f32::to_bits),
                    before.color.map(f32::to_bits)
                );
                assert_eq!(after.alpha_cutoff, before.alpha_cutoff);
                assert_eq!(output.part_bounds(surface), input.part_bounds(surface));
            }
            let before = crate::picking::MeshIndex::build(&input, &Progress::default()).unwrap();
            let after = crate::picking::MeshIndex::build(&output, &Progress::default()).unwrap();
            for triangle in 0..3 {
                let origin = glam::Vec3::new(triangle as f32 * 2. + 0.25, 0.25, 1.);
                let accept = |candidate| candidate == triangle;
                let source_hit = before.cast_filtered(&input, origin, glam::Vec3::NEG_Z, &accept);
                let output_hit = after.cast_filtered(&output, origin, glam::Vec3::NEG_Z, &accept);
                assert_eq!(output_hit, source_hit);
                assert_eq!(output_hit.unwrap().triangle, triangle);
            }
        }
    }

    #[test]
    fn upload_optimizer_keeps_source_and_rejects_invalid_unused_skin_data() {
        use bozzard_scene::middleware::animation::data::{Binding, Joint, Rig};
        let rig = Arc::new(Rig {
            nodes: vec![Joint {
                name: "root".into(),
                parent: None,
                rest: Default::default(),
            }],
            bindings: vec![Binding {
                node: 0,
                inverse_bind: glam::Mat4::IDENTITY.to_cols_array(),
            }],
            clips: Vec::new(),
        });
        let weights = [0, 0, 0, 0, 1_f32.to_bits(), 0, 0, 0];
        let mut input = MeshData {
            vertices: vec![
                [0., 0., 0., 0., 0., 1., 0., 0.],
                [1., 0., 0., 0., 0., 1., 1., 0.],
                [0., 1., 0., 0., 0., 1., 0., 1.],
                [9., 9., 9., 0., 0., 1., 0., 0.],
            ],
            indices: vec![0, 1, 2],
            parts: Vec::new(),
            warnings: Vec::new(),
            skin: Some(Skin {
                rig: rig.clone(),
                vertices: vec![weights; 4],
            }),
        };
        input.skin.as_mut().unwrap().vertices[3][4] = f32::NAN.to_bits();
        assert!(input.optimized_for_upload(&Progress::default()).is_err());
        input.skin.as_mut().unwrap().vertices[3] = weights;
        let output = input.optimized_for_upload(&Progress::default()).unwrap();
        assert_eq!(input.vertices.len(), 4);
        assert_eq!(output.vertices, input.vertices[..3]);
        assert_eq!(output.indices, input.indices);
        assert_eq!(
            output.skin.as_ref().unwrap().vertices,
            input.skin.as_ref().unwrap().vertices[..3]
        );
        assert!(Arc::ptr_eq(&output.skin.as_ref().unwrap().rig, &rig));
        input.skin.as_mut().unwrap().vertices.pop();
        assert!(input.optimized_for_upload(&Progress::default()).is_err());
    }
}
