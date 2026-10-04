use super::*;

/// Frustum-visible batch plan before GPU/cached occlusion. Excludes HUD and particles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
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

/// Lives with a certified color plan: its exact metadata checks guard peer IDs.
/// Hidden peers are counted only if they actually occur in this frame's output.
#[derive(Default)]
pub(super) struct Cache {
    groups: Vec<Option<usize>>,
    counts: Vec<usize>,
    visible: Vec<bool>,
    stats: Option<BatchingStats>,
}
impl Cache {
    pub fn collect(
        &mut self,
        draws: &[PreparedDraw],
        batches: &[Batch],
        visible: &[bool],
        graphs: bool,
    ) -> (BatchingStats, bool) {
        if self.visible == visible
            && let Some(stats) = self.stats
        {
            return (stats, true);
        }
        if self.groups.is_empty() {
            let mut groups = HashMap::new();
            self.groups.extend(draws.iter().map(|draw| {
                key(draw, graphs).map(|key| {
                    let next = groups.len();
                    *groups.entry(key).or_insert(next)
                })
            }));
            self.counts.resize(groups.len(), 0);
        }
        self.counts.fill(0);
        let stats = tally(draws, batches, |index| {
            if let Some(group) = self.groups[index] {
                self.counts[group] += 1;
            }
        });
        let stats = classify(draws, batches, true, graphs, stats, |index| {
            self.groups[index].map_or(0, |group| self.counts[group])
        });
        self.visible.clear();
        self.visible.extend_from_slice(visible);
        self.stats = Some(stats);
        (stats, false)
    }
}

pub(super) fn collect(
    draws: &[PreparedDraw],
    batches: &[Batch],
    enabled: bool,
    graphs: bool,
) -> BatchingStats {
    // Reference path intentionally rebuilds the borrowed-key table each frame.
    let mut peers = HashMap::new();
    let stats = tally(draws, batches, |index| {
        if enabled && let Some(key) = key(&draws[index], graphs) {
            *peers.entry(key).or_insert(0usize) += 1;
        }
    });
    classify(draws, batches, enabled, graphs, stats, |index| {
        key(&draws[index], graphs).map_or(0, |key| peers[&key])
    })
}

fn tally(draws: &[PreparedDraw], batches: &[Batch], mut peer: impl FnMut(usize)) -> BatchingStats {
    let mut stats = BatchingStats {
        planned_draws: batches.len(),
        ..Default::default()
    };
    for batch in batches {
        stats.size_histogram[batch.indices.len().ilog2() as usize] += 1;
        for &index in &batch.indices {
            peer(index);
            if draws[index].shader.is_some() {
                stats.graph_surfaces += 1;
                stats.graph_instanced_surfaces += usize::from(batch.indices.len() > 1);
            }
        }
    }
    stats
}

fn classify(
    draws: &[PreparedDraw],
    batches: &[Batch],
    enabled: bool,
    graphs: bool,
    mut stats: BatchingStats,
    peers: impl Fn(usize) -> usize,
) -> BatchingStats {
    for batch in batches.iter().filter(|batch| batch.indices.len() == 1) {
        stats.singleton_draws += 1;
        let index = batch.indices[0];
        let draw = &draws[index];
        if !enabled {
            stats.singleton_disabled += 1;
        } else if draw.transparent {
            stats.singleton_transparent += 1;
        } else if draw.deformation != 0 {
            stats.singleton_deformed += 1;
        } else if !graphs && draw.shader.is_some() {
            stats.singleton_shader += 1;
        } else {
            match peers(index) {
                0 => stats.singleton_unsupported_mesh += 1,
                1 => stats.singleton_unique_key += 1,
                _ => stats.singleton_split += 1,
            }
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
                plan_index: None,
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
    fn retained_peer_ids_and_histograms_match_rebuilt_diagnostics_through_visibility_churn() {
        let mut draws: Vec<_> = (0..131).map(|_| draw()).collect();
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.object.material.texture = if index % 7 == 0 {
                TextureKind::Checker
            } else {
                TextureKind::White
            };
            if index % 11 == 0 {
                draw.shader = Some(index as u64 % 3);
            }
            if index % 13 == 0 {
                draw.transparent = true;
            }
            if index % 17 == 0 {
                draw.deformation = 1;
            }
            if index % 19 == 0 {
                draw.object.mesh = MeshKind::Text(Default::default());
            }
        }
        for graphs in [false, true] {
            let mut cache = Cache::default();
            let mut random = 17_u64;
            for frame in 0..200 {
                let visible: Vec<_> = draws
                    .iter()
                    .map(|_| {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        frame != 50 && (frame == 0 || random >> 61 != 0)
                    })
                    .collect();
                let batches = super::super::batches(&draws, &visible, true, graphs);
                let expected = collect(&draws, &batches, true, graphs);
                assert_eq!(
                    cache.collect(&draws, &batches, &visible, graphs).0,
                    expected
                );
                assert_eq!(
                    cache.collect(&draws, &batches, &visible, graphs),
                    (expected, true)
                );
            }
        }
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
                plan_index: None,
                slot: None,
            },
            Batch {
                indices: vec![64],
                plan_index: None,
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
