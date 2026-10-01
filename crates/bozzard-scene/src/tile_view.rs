//! Transient tile cutaways. Visibility and tint never mutate authored materials
//! or simulation entities; shadows and lights use the same view as geometry.
use glam::Vec3;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub(crate) struct TileView {
    pub cells: BTreeMap<(i32, i32), f32>,
    pub objects: BTreeMap<String, f32>,
    pub exterior: Option<f32>,
}

impl TileView {
    pub fn factor(&self, id: &str, position: Vec3) -> f32 {
        self.objects.get(id).copied().unwrap_or_else(|| {
            self.cells
                .get(&(position.x.round() as i32, position.z.round() as i32))
                .copied()
                .unwrap_or(self.exterior.unwrap_or(1.))
        })
    }
}
