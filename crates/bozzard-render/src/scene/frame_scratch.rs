use super::*;
#[derive(Default)]
pub(super) struct Scratch {
    pub bounds: Vec<[Vec3; 2]>,
    pub visible: Vec<bool>,
    pub frustum: Vec<bool>,
    pub items: Vec<bool>,
    pub individual: Vec<bool>,
    pub shadow: Vec<bool>,
}
impl Scratch {
    pub fn compact(&mut self) {
        fn compact<T>(rows: &mut Vec<T>) {
            if rows.capacity() > 256 && rows.capacity() > rows.len().saturating_mul(4) {
                rows.shrink_to(rows.len().max(64));
            }
        }
        compact(&mut self.bounds);
        compact(&mut self.visible);
        compact(&mut self.frustum);
        compact(&mut self.items);
        compact(&mut self.individual);
        compact(&mut self.shadow);
    }
    pub fn bytes(&self) -> usize {
        self.bounds.capacity() * std::mem::size_of::<[Vec3; 2]>()
            + self.visible.capacity()
            + self.frustum.capacity()
            + self.items.capacity()
            + self.individual.capacity()
            + self.shadow.capacity()
    }
}
