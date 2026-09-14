//! Reuse validated WGSL across repeated extraction and identical material instances.
use bozzard_render::ShaderSource;
use bozzard_scene::shader_graph::ShaderGraph;
use std::{
    cell::RefCell,
    collections::VecDeque,
    hash::{Hash, Hasher},
    sync::Arc,
};

const CAPACITY: usize = 32;

#[derive(Default)]
struct SourceCache(VecDeque<(ShaderGraph, Arc<ShaderSource>)>);

impl SourceCache {
    fn get(&mut self, graph: &ShaderGraph) -> anyhow::Result<Arc<ShaderSource>> {
        // Compare actual graph data, so edits cannot hit a hash collision or stale entry.
        if let Some(index) = self.0.iter().position(|(cached, _)| cached == graph) {
            let entry = self.0.remove(index).unwrap();
            let source = entry.1.clone();
            self.0.push_front(entry);
            return Ok(source);
        }
        let surface = graph.surface_function()?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        surface.hash(&mut hash);
        let source = Arc::new(ShaderSource {
            id: hash.finish(),
            surface,
        });
        self.0.push_front((graph.clone(), source.clone()));
        self.0.truncate(CAPACITY);
        Ok(source)
    }
}

thread_local! {
    // Extraction runs on the caller's thread. No global lock, GPU objects, or unbounded
    // retention across scene changes; each thread retains at most 32 small graphs.
    static SOURCES: RefCell<SourceCache> = RefCell::default();
}

pub(super) fn source(graph: &ShaderGraph) -> anyhow::Result<Arc<ShaderSource>> {
    SOURCES.with(|cache| cache.borrow_mut().get(graph))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(first.surface, cache.get(&graph).unwrap().surface);
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
            graph.name = format!("variant {i}");
            assert_eq!(
                cache.get(&graph).unwrap().surface,
                graph.surface_function().unwrap()
            );
        }
        assert_eq!(cache.0.len(), CAPACITY);
        assert_eq!(cache.get(&graph).unwrap().surface, first.surface);
    }
}
