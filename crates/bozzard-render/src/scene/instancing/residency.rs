//! CPU-only ownership of packed color buffers. Visibility may remove a batch
//! from the output, but must not shift every later group's resident records.
use super::*;

#[derive(Default)]
pub(super) struct Residency {
    owners: Vec<Option<usize>>,
    slots: Vec<Option<usize>>,
    active: Vec<bool>,
}

impl Residency {
    pub fn reset(&mut self) {
        self.owners.fill(None);
        self.slots.clear();
    }

    pub fn assign(&mut self, batches: &mut [Batch], plan_len: usize, buffers: usize) {
        // Publication/disable can release buffers without changing the plan.
        if self.owners.len() != buffers {
            self.owners.clear();
            self.owners.resize(buffers, None);
            self.slots.clear();
        }
        self.slots.resize(plan_len, None);
        self.active.clear();
        self.active.resize(buffers, false);
        for batch in batches.iter_mut() {
            batch.slot = None;
            if batch.indices.len() > 1 {
                let owner = batch.plan_index.unwrap();
                if let Some(slot) = self.slots[owner] {
                    batch.slot = Some(slot);
                    self.active[slot] = true;
                }
            }
        }
        // Reserve every surviving group before recycling any inactive buffer.
        // Otherwise an early newcomer could steal a later survivor's allocation.
        let mut spare = 0;
        for batch in batches
            .iter_mut()
            .filter(|b| b.indices.len() > 1 && b.slot.is_none())
        {
            while spare < self.active.len() && self.active[spare] {
                spare += 1;
            }
            let owner = batch.plan_index.unwrap();
            if spare == self.owners.len() {
                self.owners.push(Some(owner));
                self.active.push(true);
            } else {
                if let Some(previous) = self.owners[spare] {
                    self.slots[previous] = None;
                }
                self.owners[spare] = Some(owner);
                self.active[spare] = true;
            }
            self.slots[owner] = Some(spare);
            batch.slot = Some(spare);
        }
    }

    pub fn retire<T>(&mut self, batches: &mut [Batch], buffers: &mut Vec<T>) {
        let limit = self.active.iter().filter(|active| **active).count() + 8;
        while buffers.len() > limit {
            let slot = self.active.iter().rposition(|active| !active).unwrap();
            if let Some(owner) = self.owners[slot] {
                self.slots[owner] = None;
            }
            buffers.swap_remove(slot);
            self.owners.swap_remove(slot);
            self.active.swap_remove(slot);
            if slot < self.owners.len()
                && let Some(owner) = self.owners[slot]
            {
                self.slots[owner] = Some(slot);
            }
        }
        for batch in batches.iter_mut().filter(|b| b.indices.len() > 1) {
            batch.slot = self.slots[batch.plan_index.unwrap()];
        }
        // CPU maps are proportional to the current plan/pool, not historical peaks.
        for vec in [&mut self.owners, &mut self.slots] {
            if vec.capacity() > 256 && vec.capacity() > vec.len().saturating_mul(4) {
                vec.shrink_to(vec.len().max(64));
            }
        }
        if self.active.capacity() > 256
            && self.active.capacity() > self.active.len().saturating_mul(4)
        {
            self.active.shrink_to(self.active.len().max(64));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batches(owners: &[usize]) -> Vec<Batch> {
        owners
            .iter()
            .map(|&owner| Batch {
                indices: vec![owner * 2, owner * 2 + 1],
                plan_index: Some(owner),
                slot: None,
            })
            .collect()
    }
    #[test]
    fn survivors_keep_their_slots_and_returning_groups_use_bounded_spares() {
        let mut pool = Residency::default();
        let mut buffers = Vec::new();
        let mut all = batches(&(0..20).collect::<Vec<_>>());
        pool.assign(&mut all, 20, 0);
        buffers.resize(20, 0);
        pool.retire(&mut all, &mut buffers);
        let survivor = all[19].slot.unwrap();
        let mut view = batches(&[1, 19]);
        pool.assign(&mut view, 20, buffers.len());
        assert_eq!(view[1].slot, Some(survivor));
        pool.retire(&mut view, &mut buffers);
        assert_eq!(buffers.len(), 10);
        assert_eq!(pool.owners[view[1].slot.unwrap()], Some(19));
        let mut returning = batches(&[0, 1, 19]);
        pool.assign(&mut returning, 20, buffers.len());
        pool.retire(&mut returning, &mut buffers);
        for batch in &returning {
            assert_eq!(pool.owners[batch.slot.unwrap()], batch.plan_index);
        }
        let mut empty = Vec::new();
        pool.assign(&mut empty, 20, buffers.len());
        pool.retire(&mut empty, &mut buffers);
        assert_eq!(buffers.len(), 8);
        pool.reset();
        assert!(pool.owners.iter().all(Option::is_none));
    }
    #[test]
    fn randomized_visibility_keeps_owner_maps_and_the_inactive_buffer_bound_consistent() {
        let mut pool = Residency::default();
        let mut buffers: Vec<Option<usize>> = Vec::new();
        let mut random = 123_u64;
        for frame in 0..500 {
            let mut view = batches(
                &(0..80)
                    .filter(|_| {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        frame % 23 != 0 && random >> 62 == 0
                    })
                    .collect::<Vec<_>>(),
            );
            let before = pool.slots.clone();
            pool.assign(&mut view, 80, buffers.len());
            for batch in &view {
                let owner = batch.plan_index.unwrap();
                if let Some(Some(slot)) = before.get(owner) {
                    assert_eq!(batch.slot, Some(*slot));
                }
            }
            buffers.resize(pool.owners.len(), None);
            for batch in &view {
                buffers[batch.slot.unwrap()] = batch.plan_index;
            }
            pool.retire(&mut view, &mut buffers);
            assert!(buffers.len() <= view.len() + 8);
            assert_eq!(pool.owners.len(), buffers.len());
            for batch in &view {
                assert_eq!(buffers[batch.slot.unwrap()], batch.plan_index);
            }
            for (owner, slot) in pool.slots.iter().enumerate() {
                if let Some(slot) = slot {
                    assert_eq!(pool.owners[*slot], Some(owner));
                }
            }
        }
    }

    #[test]
    fn a_new_early_group_cannot_steal_a_later_survivors_slot() {
        let mut pool = Residency::default();
        let mut old = batches(&[4, 5]);
        pool.assign(&mut old, 6, 0);
        let mut buffers = vec![0; 2];
        pool.retire(&mut old, &mut buffers);
        let mut next = batches(&[0, 4, 5]);
        pool.assign(&mut next, 6, buffers.len());
        assert_eq!(next[1].slot, old[0].slot);
        assert_eq!(next[2].slot, old[1].slot);
        assert_eq!(next[0].slot, Some(2));
    }
}
