//! Shadow membership ignores transforms and camera/light frusta. Those still
//! refresh uniforms and per-map visibility every frame; only grouping is retained.
use super::*;

struct Metadata {
    mesh: MeshKind,
    texture: TextureKind,
    shader: Option<u64>,
    pbr: bool,
    lit: bool,
    transparent: bool,
    deformed: bool,
}
impl Metadata {
    fn new(draw: &PreparedDraw) -> Self {
        Self {
            mesh: draw.object.mesh.clone(),
            texture: draw.object.material.texture.clone(),
            shader: draw.shader,
            pbr: draw.pbr,
            lit: draw.object.material.lit,
            transparent: draw.transparent,
            deformed: draw.deformation != 0,
        }
    }
    fn matches(&self, draw: &PreparedDraw) -> bool {
        self.shader == draw.shader
            && self.pbr == draw.pbr
            && self.lit == draw.object.material.lit
            && self.transparent == draw.transparent
            && self.deformed == (draw.deformation != 0)
            && self.mesh == draw.object.mesh
            && self.texture == draw.object.material.texture
    }
}

pub(in crate::scene) struct Plan {
    inputs: Vec<Metadata>,
    pub batches: Vec<Batch>,
}
impl Plan {
    pub fn len(&self) -> usize {
        self.inputs.len()
    }
    pub fn matches(&self, draws: &[PreparedDraw]) -> bool {
        self.inputs.len() == draws.len()
            && self
                .inputs
                .iter()
                .zip(draws)
                .all(|(old, draw)| old.matches(draw))
    }
    pub fn new(draws: &[PreparedDraw], graphs: bool, retained: bool) -> Self {
        let mut batches: Vec<Batch> = Vec::new();
        let mut groups: HashMap<Key<'_>, usize> = HashMap::new();
        for (index, draw) in draws
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.transparent && d.object.material.lit)
        {
            let key = key(draw, graphs);
            if let Some(group) = key.as_ref().and_then(|key| groups.get(key)).copied()
                && batches[group].indices.len() < MAX_INSTANCES
            {
                batches[group].indices.push(index);
            } else {
                if let Some(key) = key {
                    groups.insert(key, batches.len());
                }
                batches.push(Batch {
                    indices: vec![index],
                    plan_index: None,
                    slot: None,
                });
            }
        }
        Self {
            inputs: if retained {
                draws.iter().map(Metadata::new).collect()
            } else {
                Vec::new()
            },
            batches,
        }
    }
}
