use super::*;

/// Frustum-visible batch plan before GPU/cached occlusion. Excludes HUD and particles.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct BatchingStats {
    pub planned_draws: usize,
    pub singleton_draws: usize,
    pub graph_surfaces: usize,
    pub graph_instanced_surfaces: usize,
    /// Singleton reasons partition singleton_draws, in the order listed below.
    pub singleton_disabled: usize,
    pub singleton_transparent: usize,
    pub singleton_deformed: usize,
    /// Opaque graph surfaces excluded by the graph-instancing diagnostic switch.
    pub singleton_shader: usize,
    pub singleton_unsupported_mesh: usize,
    /// No other visible, eligible surface shares this mesh/material/shader key.
    pub singleton_unique_key: usize,
    /// Compatible visible peers exist, but ordering, culling, capacity tails or
    /// the consecutive diagnostic path left this surface alone.
    pub singleton_split: usize,
    /// Draw counts for sizes 1, 2–3, 4–7, 8–15, 16–31, 32–63, and 64.
    pub size_histogram: [usize; 7],
}

pub(super) fn collect(
    draws: &[PreparedDraw],
    batches: &[Batch],
    enabled: bool,
    graphs: bool,
) -> BatchingStats {
    let mut stats = BatchingStats {
        planned_draws: batches.len(),
        ..Default::default()
    };
    // Borrow asset keys: diagnostics must not clone per-surface strings every frame.
    let mut peers = HashMap::new();
    for batch in batches {
        stats.size_histogram[batch.indices.len().ilog2() as usize] += 1;
        for &index in &batch.indices {
            let draw = &draws[index];
            if draw.shader.is_some() {
                stats.graph_surfaces += 1;
                stats.graph_instanced_surfaces += usize::from(batch.indices.len() > 1);
            }
            if enabled && let Some(key) = key(draw, graphs) {
                *peers.entry(key).or_insert(0usize) += 1;
            }
        }
    }
    for batch in batches.iter().filter(|batch| batch.indices.len() == 1) {
        stats.singleton_draws += 1;
        let draw = &draws[batch.indices[0]];
        if !enabled {
            stats.singleton_disabled += 1;
        } else if draw.transparent {
            stats.singleton_transparent += 1;
        } else if draw.deformation != 0 {
            stats.singleton_deformed += 1;
        } else if !graphs && draw.shader.is_some() {
            stats.singleton_shader += 1;
        } else if let Some(key) = key(draw, graphs) {
            if peers[&key] == 1 {
                stats.singleton_unique_key += 1;
            } else {
                stats.singleton_split += 1;
            }
        } else {
            stats.singleton_unsupported_mesh += 1;
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw() -> PreparedDraw {
        PreparedDraw {
            preparation: Default::default(),
            source_item: 0,
            deformation: 0,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            opacity: 1.,
            cutoff: 0.,
            transparent: false,
            depth: 0.,
            object: DrawItem {
                motion_id: 1,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Cube,
                material: Material {
                    metallic: None,
                    roughness: None,
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                    surface_overrides: Default::default(),
                },
            },
        }
    }

    fn partition(stats: BatchingStats) -> usize {
        stats.singleton_disabled
            + stats.singleton_transparent
            + stats.singleton_deformed
            + stats.singleton_shader
            + stats.singleton_unsupported_mesh
            + stats.singleton_unique_key
            + stats.singleton_split
    }

    #[test]
    fn reasons_partition_visible_singletons_and_distinguish_compatible_peers() {
        let mut draws: Vec<_> = (0..7).map(|_| draw()).collect();
        draws[0].transparent = true;
        draws[0].shader = Some(1); // Transparency wins the mutually exclusive reason.
        draws[1].deformation = 1;
        draws[1].shader = Some(1);
        draws[2].shader = Some(1);
        draws[3].object.mesh = MeshKind::Text(Default::default());
        draws[4].object.material.texture = TextureKind::Checker;
        let batches: Vec<_> = (0..7)
            .map(|index| Batch {
                indices: vec![index],
                slot: None,
            })
            .collect();
        let stats = collect(&draws, &batches, true, false);
        assert_eq!(stats.singleton_draws, 7);
        assert_eq!(partition(stats), 7);
        assert_eq!(stats.singleton_transparent, 1);
        assert_eq!(stats.singleton_deformed, 1);
        assert_eq!(stats.singleton_shader, 1);
        assert_eq!(stats.singleton_unsupported_mesh, 1);
        assert_eq!(stats.singleton_unique_key, 1);
        assert_eq!(stats.singleton_split, 2);
        assert_eq!(stats.size_histogram, [7, 0, 0, 0, 0, 0, 0]);
        let stats = collect(&draws, &batches, false, false);
        assert_eq!(stats.singleton_disabled, 7);
        assert_eq!(partition(stats), 7);
        let stats = collect(&draws, &batches[..6], true, true);
        assert_eq!(stats.singleton_shader, 0);
        assert_eq!(stats.singleton_split, 0); // Hidden peer does not count.
        assert_eq!(stats.singleton_unique_key, 3);
        assert_eq!(partition(stats), 6);
    }

    #[test]
    fn histogram_and_graph_counts_include_capacity_tails() {
        let mut draws: Vec<_> = (0..65).map(|_| draw()).collect();
        for draw in &mut draws {
            draw.shader = Some(7);
        }
        let batches = vec![
            Batch {
                indices: (0..64).collect(),
                slot: None,
            },
            Batch {
                indices: vec![64],
                slot: None,
            },
        ];
        let stats = collect(&draws, &batches, true, true);
        assert_eq!(stats.graph_surfaces, 65);
        assert_eq!(stats.graph_instanced_surfaces, 64);
        assert_eq!(stats.singleton_split, 1);
        assert_eq!(stats.size_histogram, [1, 0, 0, 0, 0, 0, 1]);
        assert_eq!(partition(stats), 1);
        assert_eq!(collect(&draws, &[], true, true).planned_draws, 0);
    }

    #[test]
    fn consecutive_graph_batches_require_matching_shader_identity_and_the_switch() {
        let mut draws: Vec<_> = (0..3).map(|_| draw()).collect();
        for draw in &mut draws {
            draw.shader = Some(11);
        }
        draws[2].shader = Some(12);
        let sizes = |graphs| {
            super::super::batches(&draws, &[true; 3], true, graphs)
                .into_iter()
                .map(|b| b.indices.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(true), [2, 1]);
        assert_eq!(sizes(false), [1, 1, 1]);
        assert!(key(&draws[0], true) == key(&draws[1], true));
        assert!(key(&draws[0], true) != key(&draws[2], true));
        assert!(key(&draws[0], false).is_none());
    }

    #[test]
    fn graph_instanced_hosts_validate_on_baseline_capabilities() {
        let source = ShaderSource {
            id: 1,
            surface: "fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { return default_material_surface(uv,normal_uv,mr_uv,ao_uv,emissive_uv,world_normal,tangent,world,view,front,time); }".into(),
        };
        for pbr in [false, true] {
            let wgsl = instance_module_text(graph_module_text(pbr, &source));
            let module = wgpu::naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
