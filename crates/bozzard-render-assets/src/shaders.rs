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
        let surface = graph.surface_function_mask(mask)?;
        // Unused keywords/branches may specialize to the same program. Share the
        // exact source as well as its GPU pipeline identity in that case.
        let source = self
            .entries
            .values()
            .flatten()
            .find(|entry| entry.source.surface == surface)
            .map(|entry| entry.source.clone())
            .unwrap_or_else(|| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                surface.hash(&mut hash);
                Arc::new(ShaderSource {
                    id: hash.finish(),
                    surface,
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
    static SOURCES: RefCell<SourceCache> = RefCell::default();
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
}
