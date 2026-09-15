//! Shared geometry for atlas sprites, tile batches and nine-slice UI images.
use super::*;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteQuad {
    /// Left, top, width, height in local XY units (height extends toward -Y).
    pub rect: [f32; 4],
    /// Left, top, width, height in normalized texture coordinates; negative extent flips an axis.
    pub uv: [f32; 4],
}
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteGeometry {
    key: u64,
    quads: Arc<[[f32; 8]]>,
}
impl SpriteGeometry {
    pub fn new(quads: Vec<SpriteQuad>) -> Result<Arc<Self>> {
        Self::shared(
            quads
                .into_iter()
                .map(|q| {
                    [
                        q.rect[0], q.rect[1], q.rect[2], q.rect[3], q.uv[0], q.uv[1], q.uv[2],
                        q.uv[3],
                    ]
                })
                .collect::<Vec<_>>()
                .into(),
        )
    }
    /// Immutable shared CPU batches avoid copying/hashing a tilemap every frame.
    pub fn shared(quads: Arc<[[f32; 8]]>) -> Result<Arc<Self>> {
        thread_local! {static CACHE:std::cell::RefCell<BTreeMap<usize,std::sync::Weak<SpriteGeometry>>>=const{std::cell::RefCell::new(BTreeMap::new())};}
        let pointer = quads.as_ptr() as usize;
        if let Some(found) =
            CACHE.with_borrow(|cache| cache.get(&pointer).and_then(std::sync::Weak::upgrade))
            && Arc::ptr_eq(&found.quads, &quads)
        {
            return Ok(found);
        }
        ensure!(quads.len() <= 65536, "sprite batch exceeds 65536 quads");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for q in quads.iter() {
            ensure!(
                q.iter().all(|v| v.is_finite())
                    && q[..4].iter().all(|v| v.abs() <= 1e6)
                    && q[2] > 0.
                    && q[3] > 0.,
                "invalid sprite rectangle"
            );
            for axis in 0..2 {
                ensure!(
                    (-1e-6..=1.000001).contains(&q[axis + 4])
                        && (-1e-6..=1.000001).contains(&(q[axis + 4] + q[axis + 6]))
                        && q[axis + 6] != 0.,
                    "invalid sprite atlas region"
                );
            }
            for value in q {
                value.to_bits().hash(&mut hasher);
            }
        }
        let result = Arc::new(Self {
            key: hasher.finish(),
            quads,
        });
        CACHE.with_borrow_mut(|cache| {
            if cache.len() > 1024 {
                cache.retain(|_, v| v.strong_count() > 0);
            }
            cache.insert(pointer, Arc::downgrade(&result));
        });
        Ok(result)
    }
    pub fn quads(&self) -> &[[f32; 8]] {
        &self.quads
    }
    pub(crate) fn key(&self) -> u64 {
        self.key
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteMesh {
    pub geometry: Arc<SpriteGeometry>,
    pub screen: Option<ScreenText>,
    pub clip: Option<[f32; 4]>,
    pub opacity: f32,
}
impl SpriteMesh {
    pub fn new(quads: Vec<SpriteQuad>) -> Result<Self> {
        Ok(Self {
            geometry: SpriteGeometry::new(quads)?,
            screen: None,
            clip: None,
            opacity: 1.,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(screen) = self.screen {
            ensure!(
                screen
                    .anchor
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v))
                    && screen
                        .offset
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 1e6),
                "invalid sprite screen position"
            );
        }
        ensure!(
            self.opacity.is_finite() && (0.0..=1.).contains(&self.opacity),
            "invalid sprite opacity"
        );
        if let Some(rect) = self.clip {
            ensure!(
                rect.iter().all(|v| v.is_finite() && v.abs() <= 1e6)
                    && rect[2] >= 0.
                    && rect[3] >= 0.,
                "invalid sprite clipping rectangle"
            );
        }
        Ok(())
    }
}
struct Cached {
    _geometry: Arc<SpriteGeometry>,
    mesh: MeshBuffers,
    quads: usize,
    used: u64,
}
#[derive(Default)]
pub(super) struct Sprites {
    meshes: BTreeMap<u64, Cached>,
    serial: u64,
}
impl Sprites {
    pub fn mesh(&self, sprite: &SpriteMesh) -> Option<&MeshBuffers> {
        self.meshes.get(&sprite.geometry.key).map(|c| &c.mesh)
    }
    pub fn prepare(&mut self, gpu: &Gpu, items: &[DrawItem]) -> Result<()> {
        self.serial = self.serial.wrapping_add(1);
        let mut used = BTreeSet::new();
        let mut visible = 0;
        for item in items {
            let MeshKind::Sprite(sprite) = &item.mesh else {
                continue;
            };
            sprite.validate()?;
            let geometry = &sprite.geometry;
            if !used.insert(geometry.key) {
                continue;
            }
            visible += geometry.quads.len();
            ensure!(visible <= 65536, "sprite view exceeds 65536 quads");
            if let Some(cached) = self.meshes.get_mut(&geometry.key) {
                cached.used = self.serial;
                continue;
            }
            if geometry.quads.is_empty() {
                continue;
            }
            let mut vertices = Vec::with_capacity(geometry.quads.len() * 4);
            let mut indices = Vec::with_capacity(geometry.quads.len() * 6);
            for &[x, y, w, h, u, v, uw, vh] in geometry.quads.iter() {
                let start = vertices.len() as u32;
                vertices.extend([
                    [x, y, 0., 0., 0., 1., u, v],
                    [x + w, y, 0., 0., 0., 1., u + uw, v],
                    [x + w, y - h, 0., 0., 0., 1., u + uw, v + vh],
                    [x, y - h, 0., 0., 0., 1., u, v + vh],
                ]);
                indices.extend([start, start + 2, start + 1, start, start + 3, start + 2]);
            }
            self.meshes.insert(
                geometry.key,
                Cached {
                    _geometry: geometry.clone(),
                    mesh: mesh(gpu, &vertices, &indices),
                    quads: geometry.quads.len(),
                    used: self.serial,
                },
            );
        }
        let mut total: usize = self.meshes.values().map(|c| c.quads).sum();
        while self.meshes.len() > used.len() + 256 || total > 131072 {
            let oldest = self
                .meshes
                .iter()
                .filter(|(key, _)| !used.contains(key))
                .min_by_key(|(_, c)| c.used)
                .map(|(&key, _)| key);
            let Some(key) = oldest else {
                break;
            };
            total -= self.meshes.remove(&key).unwrap().quads;
        }
        if used.is_empty() {
            self.meshes.clear();
        }
        Ok(())
    }
}
