//! Reuse validated, specialized WGSL across shared material instances.
use bozzard_render::ShaderSource;
use bozzard_scene::shader_graph::ShaderGraph;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
    sync::Arc,
};

const CAPACITY: usize = 256;
struct Entry {
    graph: ShaderGraph,
    source: Arc<ShaderSource>,
    used: u64,
}
#[derive(Default)]
struct SourceCache {
    entries: HashMap<(u64, u8), Vec<Entry>>,
    length: usize,
    clock: u64,
    numeric_enabled: bool,
}
impl SourceCache {
    fn get(&mut self, graph: &ShaderGraph) -> anyhow::Result<Arc<ShaderSource>> {
        self.variant(graph, &BTreeMap::new())
    }
    fn variant(
        &mut self,
        graph: &ShaderGraph,
        keywords: &BTreeMap<String, bool>,
    ) -> anyhow::Result<Arc<ShaderSource>> {
        self.mask(graph, graph.keyword_mask(keywords)?)
    }
    fn mask(&mut self, graph: &ShaderGraph, mask: u8) -> anyhow::Result<Arc<ShaderSource>> {
        graph.validate_presentation()?;
        let key = (graph.program_fingerprint(), mask);
        self.clock = self.clock.saturating_add(1);
        if let Some(entry) = self.entries.get_mut(&key).and_then(|bucket| {
            bucket
                .iter_mut()
                .find(|entry| entry.graph.same_program(graph))
        }) {
            entry.used = self.clock;
            return Ok(entry.source.clone());
        }
        let (surface, numeric_parameters) = if self.numeric_enabled {
            graph.surface_function_parameters_mask(mask)?
        } else {
            (graph.surface_function_mask(mask)?, Vec::new())
        };
        // Unused keywords/branches may specialize to the same program. Share the
        // exact source as well as its GPU pipeline identity in that case.
        let source = self
            .entries
            .values()
            .flatten()
            .find(|entry| {
                entry.source.surface == surface
                    && entry.source.numeric_parameters.len() == numeric_parameters.len()
                    && entry
                        .source
                        .numeric_parameters
                        .iter()
                        .zip(&numeric_parameters)
                        .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits))
            })
            .map(|entry| entry.source.clone())
            .unwrap_or_else(|| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                surface.hash(&mut hash);
                Arc::new(ShaderSource {
                    id: hash.finish(),
                    surface,
                    numeric_parameters: numeric_parameters.into(),
                })
            });
        if self.length == CAPACITY {
            let (key, index) = self
                .entries
                .iter()
                .flat_map(|(key, bucket)| {
                    bucket
                        .iter()
                        .enumerate()
                        .map(move |(index, entry)| (*key, index, entry.used))
                })
                .min_by_key(|(_, _, used)| *used)
                .map(|(key, index, _)| (key, index))
                .unwrap();
            let bucket = self.entries.get_mut(&key).unwrap();
            bucket.swap_remove(index);
            if bucket.is_empty() {
                self.entries.remove(&key);
            }
            self.length -= 1;
        }
        self.entries.entry(key).or_default().push(Entry {
            graph: graph.clone(),
            source: source.clone(),
            used: self.clock,
        });
        self.length += 1;
        Ok(source)
    }
}
thread_local! {
    // Thread-local and bounded across scene changes. No lock or graph clone on a
    // hit, and no linear scan across unrelated graph programs.
    static SOURCES: RefCell<SourceCache> = RefCell::new(SourceCache { numeric_enabled: true, ..Default::default() });
}
pub(super) fn parameterization_enabled() -> bool {
    SOURCES.with(|cache| cache.borrow().numeric_enabled)
}
/// Select the literal compiler oracle on this presentation thread. Cached frame
/// adapters observe this setting and rebuild their immutable shader records.
pub fn set_graph_parameterization_enabled(enabled: bool) {
    SOURCES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.numeric_enabled != enabled {
            *cache = SourceCache {
                numeric_enabled: enabled,
                ..Default::default()
            };
        }
    });
}
pub(super) fn source(graph: &ShaderGraph) -> anyhow::Result<Arc<ShaderSource>> {
    SOURCES.with(|cache| cache.borrow_mut().get(graph))
}
pub(super) fn variant(
    graph: &ShaderGraph,
    keywords: &BTreeMap<String, bool>,
) -> anyhow::Result<Arc<ShaderSource>> {
    SOURCES.with(|cache| cache.borrow_mut().variant(graph, keywords))
}
pub(super) fn mask(graph: &ShaderGraph, mask: u8) -> anyhow::Result<Arc<ShaderSource>> {
    SOURCES.with(|cache| cache.borrow_mut().mask(graph, mask))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variants_share_exact_programs_but_reject_unknown_names_and_bad_layouts() {
        let mut graph = ShaderGraph::default();
        graph.keywords.insert("DETAIL".into(), false);
        let mut cache = SourceCache::default();
        let source = cache.get(&graph).unwrap();
        // An unused keyword cannot create a different GPU program.
        let alternate = cache
            .variant(&graph, &BTreeMap::from([("DETAIL".into(), true)]))
            .unwrap();
        assert!(Arc::ptr_eq(&source, &alternate));
        graph.name = "Same shader, different label".into();
        graph.nodes[0].position = [42., 91.];
        assert!(Arc::ptr_eq(&source, &cache.get(&graph).unwrap()));
        assert!(
            cache
                .variant(&graph, &BTreeMap::from([("OTHER".into(), true)]))
                .is_err()
        );
        graph.nodes[0].position[0] = f32::NAN;
        assert!(cache.get(&graph).is_err());
        assert_eq!(cache.length, 2);
    }

    #[test]
    fn reuse_edits_errors_and_eviction_preserve_the_compiler_result() {
        let scene = bozzard_scene::Scene::from_json(include_str!(
            "../../../examples/demo/scenes/shader-node-lab.json"
        ))
        .unwrap();
        let mut graph = scene
            .objects
            .iter()
            .find_map(|o| o.shader_graph.clone())
            .unwrap();
        let mut cache = SourceCache::default();
        let first = cache.get(&graph).unwrap();
        assert_eq!(first.surface, graph.surface_function().unwrap());
        assert!(Arc::ptr_eq(&first, &cache.get(&graph.clone()).unwrap()));

        // Layout edits still compile to exactly the same shader; value edits don't.
        graph.nodes[0].position[0] += 10.;
        assert!(Arc::ptr_eq(&first, &cache.get(&graph).unwrap()));
        let mut changed = graph.clone();
        let time = changed
            .nodes
            .iter_mut()
            .find(|n| n.kind == bozzard_scene::shader_graph::NodeKind::Time)
            .unwrap();
        time.kind = bozzard_scene::shader_graph::NodeKind::Float;
        time.inputs = vec![bozzard_scene::shader_graph::Value::Float(2.0)];
        let edited = cache.get(&changed).unwrap();
        assert_eq!(edited.surface, changed.surface_function().unwrap());
        assert_ne!(edited.id, first.id);
        let old = graph.clone();
        let input = graph
            .nodes
            .iter_mut()
            .flat_map(|n| &mut n.inputs)
            .find_map(|v| {
                if let bozzard_scene::shader_graph::Value::Float(f) = v {
                    Some(f)
                } else {
                    None
                }
            })
            .unwrap();
        *input = f32::NAN;
        assert!(cache.get(&graph).is_err());
        graph = old;
        for i in 0..CAPACITY + 4 {
            graph
                .nodes
                .iter_mut()
                .find(|node| node.kind == bozzard_scene::shader_graph::NodeKind::Master)
                .unwrap()
                .inputs[0] = bozzard_scene::shader_graph::Value::Vector([i as f32; 3]);
            assert_eq!(
                cache.get(&graph).unwrap().surface,
                graph.surface_function().unwrap()
            );
        }
        assert_eq!(cache.length, CAPACITY);
        assert_eq!(cache.get(&graph).unwrap().surface, first.surface);
    }
    #[test]
    fn numeric_values_share_pipeline_identity_but_keep_immutable_parameters() {
        use bozzard_scene::shader_graph::{Node, NodeKind, Socket, Value, Wire};
        let mut graph = ShaderGraph::default();
        graph.nodes.push(Node::new(2, NodeKind::Color, [0., 0.]));
        graph.nodes.push(Node::new(3, NodeKind::Float, [0., 0.]));
        graph
            .connect(Wire {
                from: Socket { node: 2, port: 0 },
                to: Socket { node: 1, port: 0 },
            })
            .unwrap();
        graph
            .connect(Wire {
                from: Socket { node: 3, port: 0 },
                to: Socket { node: 1, port: 2 },
            })
            .unwrap();
        let mut cache = SourceCache {
            numeric_enabled: true,
            ..Default::default()
        };
        let first = cache.get(&graph).unwrap();
        assert_eq!(first.numeric_parameters.len(), 2);
        assert!(first.surface.contains("graph_numeric(0u).xyz"));
        assert!(first.surface.contains("graph_numeric(1u).x"));
        graph.nodes[1].inputs[0] = Value::Vector([0.2, 0.4, 0.8]);
        graph.nodes[2].inputs[0] = Value::Float(0.7);
        let edited = cache.get(&graph).unwrap();
        assert_eq!(first.id, edited.id);
        assert_eq!(first.surface, edited.surface);
        assert!(!Arc::ptr_eq(&first, &edited));
        assert_ne!(first.numeric_parameters, edited.numeric_parameters);
        assert_eq!(
            edited.numeric_parameters.as_ref(),
            [[0.2, 0.4, 0.8, 0.], [0.7, 0., 0., 0.]]
        );
        let mut literal = SourceCache::default();
        assert_eq!(
            literal.get(&graph).unwrap().surface,
            graph.surface_function().unwrap()
        );
        assert!(literal.get(&graph).unwrap().numeric_parameters.is_empty());
        graph.nodes[2].inputs[0] = Value::Float(f32::NAN);
        assert!(cache.get(&graph).is_err());
    }
    #[test]
    fn active_constants_above_portable_parameter_limit_keep_literal_compiler() {
        use bozzard_scene::shader_graph::{Node, NodeKind, Socket, Wire};
        let mut graph = ShaderGraph::default();
        graph
            .nodes
            .extend((2..=18).map(|id| Node::new(id, NodeKind::Add, [0., 0.])));
        for id in 2..18 {
            graph
                .connect(Wire {
                    from: Socket { node: id, port: 0 },
                    to: Socket {
                        node: id + 1,
                        port: 0,
                    },
                })
                .unwrap();
        }
        graph
            .connect(Wire {
                from: Socket { node: 18, port: 0 },
                to: Socket { node: 1, port: 2 },
            })
            .unwrap();
        let mut cache = SourceCache {
            numeric_enabled: true,
            ..Default::default()
        };
        let source = cache.get(&graph).unwrap();
        assert!(source.numeric_parameters.is_empty());
        assert!(!source.surface.contains("graph_numeric("));
        assert_eq!(source.surface, graph.surface_function().unwrap());
    }
}
