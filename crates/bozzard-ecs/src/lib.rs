//! A small, safe ECS with generational entities and dense per-type component storage.
//!
//! Entity handles belong to one world and are not persistent scene/network IDs.
//! Query order is unspecified: removing a component can change iteration order.
//!
//! # Change tracking
//!
//! Every component carries the [tick](World::change_tick) it was last written on. A reader keeps
//! the tick it last looked at and asks for what changed since, which is how a renderer, a network
//! replicator or a dirty-flag inspector avoids rescanning the whole world every frame.
//!
//! Writes are recorded through [`Mut`], the guard [`World::get_mut`], [`World::query_mut`] and
//! [`World::query_pair_mut`] return: merely holding the guard does not mark anything, dereferencing
//! it mutably or calling [`Mut::into_inner`] does. [`World::insert`] marks the component changed on
//! the current tick. Reads never mark. Removal and despawn are not tracked: presence is a query,
//! not a tick, so ask [`World::changed_since`] for values and the world's length or a query for
//! entities appearing and disappearing.
//!
//! Ticks are per world and advanced by the caller, not per system: [`World::advance_change_tick`]
//! once per simulation step, which `bozzard_app::App::step` does. That is the honest granularity
//! while systems run serially in one step; a system that needs its own window bookmarks the tick it
//! started on. A bookmark of `0` sees everything ever written.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_WORLD: AtomicU64 = AtomicU64::new(1);

/// A world-local handle. Both the world and generation are checked on access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    world: u64,
    index: u32,
    generation: u32,
}

/// All owned, thread-safe data types can be components; no derive macro is needed.
pub trait Component: Any + Send + Sync {}
impl<T: Any + Send + Sync> Component for T {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidEntity(pub Entity);

impl fmt::Display for InvalidEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "entity {:?} is dead or belongs to a different world",
            self.0
        )
    }
}
impl std::error::Error for InvalidEntity {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AliasedQuery;

impl fmt::Display for AliasedQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a mutable pair query must use two different component types")
    }
}
impl std::error::Error for AliasedQuery {}

struct Slot {
    generation: u32,
    alive: bool,
}

trait ErasedStorage: Any + Send + Sync {
    fn remove_entity(&mut self, entity: Entity);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A component value and the tick it was last written on.
struct Entry<T> {
    value: T,
    changed: u64,
}

/// Mutable access that records a write, so readers can ask what changed since they last looked.
///
/// Holding the guard is a read: nothing is marked until it is dereferenced mutably, so
/// `if guard.field > 0 { guard.field -= 1 }` marks the component only when it really changed.
/// [`Mut::bypass_change_detection`] writes without marking, for a caller that owns the decision.
pub struct Mut<'a, T: Component> {
    entry: &'a mut Entry<T>,
    tick: u64,
}

impl<'a, T: Component> Mut<'a, T> {
    /// The tick this component was last written on, before this guard writes to it.
    pub fn last_changed(&self) -> u64 {
        self.entry.changed
    }
    /// Mutable value that is not recorded as a change.
    pub fn bypass_change_detection(&mut self) -> &mut T {
        &mut self.entry.value
    }
    /// The value, recording a change.
    pub fn into_inner(self) -> &'a mut T {
        self.entry.changed = self.tick;
        &mut self.entry.value
    }
    fn mark(&mut self) {
        self.entry.changed = self.tick;
    }
}

impl<T: Component> Deref for Mut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.entry.value
    }
}

impl<T: Component> DerefMut for Mut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.mark();
        &mut self.entry.value
    }
}

struct Storage<T> {
    sparse: Vec<Option<usize>>,
    entities: Vec<Entity>,
    entries: Vec<Entry<T>>,
}

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entities: Vec::new(),
            entries: Vec::new(),
        }
    }
}

impl<T: Component> Storage<T> {
    fn position(&self, entity: Entity) -> Option<usize> {
        let index = self.sparse.get(entity.index as usize).copied().flatten()?;
        (self.entities[index] == entity).then_some(index)
    }

    fn insert(&mut self, entity: Entity, value: T, tick: u64) -> Option<T> {
        let entry = Entry {
            value,
            changed: tick,
        };
        if let Some(index) = self.position(entity) {
            return Some(std::mem::replace(&mut self.entries[index], entry).value);
        }
        self.sparse
            .resize(self.sparse.len().max(entity.index as usize + 1), None);
        self.sparse[entity.index as usize] = Some(self.entries.len());
        self.entities.push(entity);
        self.entries.push(entry);
        None
    }

    fn remove(&mut self, entity: Entity) -> Option<T> {
        let index = self.position(entity)?;
        self.sparse[entity.index as usize] = None;
        self.entities.swap_remove(index);
        let value = self.entries.swap_remove(index).value;
        if let Some(moved) = self.entities.get(index) {
            self.sparse[moved.index as usize] = Some(index);
        }
        Some(value)
    }

    fn get(&self, entity: Entity) -> Option<&T> {
        self.position(entity)
            .map(|index| &self.entries[index].value)
    }

    fn entry_mut(&mut self, entity: Entity, tick: u64) -> Option<Mut<'_, T>> {
        let index = self.position(entity)?;
        Some(Mut {
            entry: &mut self.entries[index],
            tick,
        })
    }

    /// Entities with their values and change ticks, split so both can be borrowed at once.
    fn split(&mut self) -> (&[Entity], &mut [Entry<T>]) {
        (&self.entities, &mut self.entries)
    }
}

impl<T: Component> ErasedStorage for Storage<T> {
    fn remove_entity(&mut self, entity: Entity) {
        self.remove(entity);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Owns entities, components, and singleton resources. Contains no graphics dependencies.
pub struct World {
    id: u64,
    slots: Vec<Slot>,
    free: Vec<u32>,
    len: usize,
    components: HashMap<TypeId, Box<dyn ErasedStorage>>,
    resources: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    change_tick: u64,
}

impl Default for World {
    fn default() -> Self {
        let id = NEXT_WORLD
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("world ID space exhausted");
        Self {
            id,
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            components: HashMap::new(),
            resources: HashMap::new(),
            // Ticks start at 1 so that a bookmark of 0 means "everything ever written".
            change_tick: 1,
        }
    }
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn spawn(&mut self) -> Entity {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.slots.len()).expect("entity index space exhausted");
                self.slots.push(Slot {
                    generation: 0,
                    alive: false,
                });
                index
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.alive = true;
        self.len += 1;
        Entity {
            world: self.id,
            index,
            generation: slot.generation,
        }
    }

    pub fn contains(&self, entity: Entity) -> bool {
        entity.world == self.id
            && self
                .slots
                .get(entity.index as usize)
                .is_some_and(|slot| slot.alive && slot.generation == entity.generation)
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), InvalidEntity> {
        self.validate(entity)?;
        for storage in self.components.values_mut() {
            storage.remove_entity(entity);
        }
        let slot = &mut self.slots[entity.index as usize];
        slot.alive = false;
        // Retire a slot permanently on generation exhaustion; never resurrect an old handle.
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(entity.index);
        }
        self.len -= 1;
        Ok(())
    }

    pub fn insert<T: Component>(
        &mut self,
        entity: Entity,
        value: T,
    ) -> Result<Option<T>, InvalidEntity> {
        self.validate(entity)?;
        let tick = self.change_tick;
        let storage = self
            .components
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Storage::<T>::default()));
        Ok(storage
            .as_any_mut()
            .downcast_mut::<Storage<T>>()
            .expect("component storage type")
            .insert(entity, value, tick))
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.storage::<T>()?.get(entity)
    }

    /// Mutable access. The returned [guard](Mut) records a write when it is dereferenced mutably.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<'_, T>> {
        let tick = self.change_tick;
        self.storage_mut::<T>()?.entry_mut(entity, tick)
    }

    /// The tick this component was last written on, `None` if the entity has no such component.
    pub fn changed_tick<T: Component>(&self, entity: Entity) -> Option<u64> {
        let storage = self.storage::<T>()?;
        storage
            .position(entity)
            .map(|index| storage.entries[index].changed)
    }

    /// Whether the entity's component was written after `tick`.
    pub fn is_changed_since<T: Component>(&self, entity: Entity, tick: u64) -> bool {
        self.changed_tick::<T>(entity)
            .is_some_and(|changed| changed > tick)
    }

    /// Every component of this type written after `tick`, with its value.
    pub fn changed_since<T: Component>(&self, tick: u64) -> impl Iterator<Item = (Entity, &T)> {
        self.storage::<T>().into_iter().flat_map(move |storage| {
            storage
                .entities
                .iter()
                .copied()
                .zip(storage.entries.iter())
                .filter(move |(_, entry)| entry.changed > tick)
                .map(|(entity, entry)| (entity, &entry.value))
        })
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<Option<T>, InvalidEntity> {
        self.validate(entity)?;
        Ok(self
            .storage_mut::<T>()
            .and_then(|storage| storage.remove(entity)))
    }

    pub fn query<T: Component>(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.storage::<T>()
            .into_iter()
            .flat_map(|s| s.entities.iter().copied().zip(s.entries.iter()))
            .map(|(entity, entry)| (entity, &entry.value))
    }

    /// Mutable query. Each returned [guard](Mut) records a write when dereferenced mutably.
    pub fn query_mut<T: Component>(&mut self) -> impl Iterator<Item = (Entity, Mut<'_, T>)> {
        let tick = self.change_tick;
        let (entities, entries): (&[Entity], &mut [Entry<T>]) = match self.storage_mut::<T>() {
            Some(storage) => storage.split(),
            None => (&[], &mut []),
        };
        entities
            .iter()
            .copied()
            .zip(entries.iter_mut())
            .map(move |(entity, entry)| (entity, Mut { entry, tick }))
    }

    /// Joins a mutable component with a read-only component, without unsafe aliasing.
    /// Missing storage produces an empty iterator; requesting the same type is an error.
    pub fn query_pair_mut<A: Component, B: Component>(
        &mut self,
    ) -> Result<impl Iterator<Item = (Entity, Mut<'_, A>, &B)>, AliasedQuery> {
        let tick = self.change_tick;
        let (a, b) = (TypeId::of::<A>(), TypeId::of::<B>());
        if a == b {
            return Err(AliasedQuery);
        }
        let [a, b] = self.components.get_disjoint_mut([&a, &b]);
        let a = a.and_then(|s| s.as_any_mut().downcast_mut::<Storage<A>>());
        let b = b.and_then(|s| s.as_any().downcast_ref::<Storage<B>>());
        let (entities, entries): (&[Entity], &mut [Entry<A>]) = match a {
            Some(a) => a.split(),
            None => (&[], &mut []),
        };
        Ok(entities
            .iter()
            .copied()
            .zip(entries.iter_mut())
            .filter_map(move |(entity, entry)| {
                Some((entity, Mut { entry, tick }, b?.get(entity)?))
            }))
    }

    /// Advance the tick writes are recorded on. Call once per simulation step, before the systems
    /// that write; a bookmark taken after the previous step then sees exactly this step's writes.
    pub fn advance_change_tick(&mut self) -> u64 {
        self.change_tick = self.change_tick.saturating_add(1);
        self.change_tick
    }

    /// The tick writes are currently recorded on. See [`World::advance_change_tick`].
    pub fn change_tick(&self) -> u64 {
        self.change_tick
    }

    pub fn insert_resource<T: Component>(&mut self, value: T) -> Option<T> {
        self.resources
            .insert(TypeId::of::<T>(), Box::new(value))
            .map(|old| *old.downcast::<T>().expect("resource type"))
    }
    pub fn remove_resource<T: Component>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeId::of::<T>())
            .map(|value| *value.downcast::<T>().expect("resource type"))
    }
    pub fn resource<T: Component>(&self) -> Option<&T> {
        self.resources.get(&TypeId::of::<T>())?.downcast_ref()
    }
    pub fn resource_mut<T: Component>(&mut self) -> Option<&mut T> {
        self.resources.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    fn validate(&self, entity: Entity) -> Result<(), InvalidEntity> {
        if self.contains(entity) {
            Ok(())
        } else {
            Err(InvalidEntity(entity))
        }
    }
    fn storage<T: Component>(&self) -> Option<&Storage<T>> {
        self.components
            .get(&TypeId::of::<T>())?
            .as_any()
            .downcast_ref()
    }
    fn storage_mut<T: Component>(&mut self) -> Option<&mut Storage<T>> {
        self.components
            .get_mut(&TypeId::of::<T>())?
            .as_any_mut()
            .downcast_mut()
    }
}

type Command = Box<dyn FnOnce(&mut World) + Send>;

/// Structural changes queued during iteration, applied in insertion order at a barrier.
#[derive(Default)]
pub struct Commands(Vec<Command>);

impl Commands {
    pub fn queue(&mut self, command: impl FnOnce(&mut World) + Send + 'static) {
        self.0.push(Box::new(command));
    }
    pub fn apply(&mut self, world: &mut World) {
        for command in self.0.drain(..) {
            command(world);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_and_foreign_handles_cannot_touch_recycled_entities() {
        let mut world = World::new();
        let old = world.spawn();
        world.insert(old, 10_i32).unwrap();
        world.insert(old, String::from("owned")).unwrap();
        world.despawn(old).unwrap();
        let new = world.spawn();
        assert_eq!(old.index, new.index);
        assert_ne!(old, new);
        assert!(world.get::<i32>(old).is_none());
        assert!(world.get::<String>(new).is_none());
        assert!(world.insert(old, 20_i32).is_err());
        assert!(world.despawn(old).is_err());
        let foreign = World::new().spawn();
        assert!(world.insert(foreign, 30_i32).is_err());
        assert_eq!(world.len(), 1);
    }

    #[test]
    fn joins_survive_swap_removal_and_component_replacement() {
        let mut world = World::new();
        let entities: Vec<_> = (0..100)
            .map(|i| {
                let e = world.spawn();
                world.insert(e, i).unwrap();
                if i % 2 == 0 {
                    world.insert(e, 2_u64).unwrap();
                }
                e
            })
            .collect();
        for e in entities.iter().step_by(3) {
            world.despawn(*e).unwrap();
        }
        for (_, mut value, multiplier) in world.query_pair_mut::<i32, u64>().unwrap() {
            *value *= *multiplier as i32;
        }
        for (i, e) in entities.iter().enumerate() {
            if i % 3 == 0 {
                assert!(world.get::<i32>(*e).is_none());
            } else {
                assert_eq!(
                    world.get::<i32>(*e),
                    Some(&((i * if i % 2 == 0 { 2 } else { 1 }) as i32))
                );
            }
        }
        assert_eq!(world.insert(entities[2], 99_i32).unwrap(), Some(4));
        assert_eq!(world.remove::<i32>(entities[2]).unwrap(), Some(99));
        assert_eq!(world.remove::<i32>(entities[2]).unwrap(), None);
        assert!(world.query_pair_mut::<i32, i32>().is_err());
        assert_eq!(world.query_pair_mut::<i32, String>().unwrap().count(), 0);
    }

    #[test]
    fn deferred_despawn_and_resources() {
        let mut world = World::new();
        for i in 0..10 {
            let e = world.spawn();
            world.insert(e, i).unwrap();
        }
        let mut commands = Commands::default();
        for (e, value) in world.query::<i32>() {
            if value % 2 == 0 {
                commands.queue(move |world| {
                    world.despawn(e).unwrap();
                });
            }
        }
        assert_eq!(world.len(), 10);
        commands.apply(&mut world);
        commands.apply(&mut world);
        assert_eq!(world.query::<i32>().count(), 5);
        assert_eq!(world.insert_resource(42_u32), None);
        *world.resource_mut::<u32>().unwrap() += 1;
        assert_eq!(world.insert_resource(7_u32), Some(43));
        assert_eq!(world.resource::<u32>(), Some(&7));
    }

    #[test]
    fn a_write_records_a_tick_and_a_read_does_not() {
        let mut world = World::new();
        let entity = world.spawn();
        let inserted = world.change_tick();
        world.insert(entity, 1_i32).unwrap();
        assert_eq!(world.changed_tick::<i32>(entity), Some(inserted));
        assert!(world.is_changed_since::<i32>(entity, inserted - 1));
        assert!(!world.is_changed_since::<i32>(entity, inserted));

        // A shared read and a deref read through the guard are both free.
        assert_eq!(world.get::<i32>(entity), Some(&1));
        let later = world.advance_change_tick();
        {
            let value = world.get_mut::<i32>(entity).unwrap();
            assert_eq!(value.last_changed(), inserted);
            assert_eq!(*value, 1);
            assert!(*value > 0);
        }
        assert_eq!(world.changed_tick::<i32>(entity), Some(inserted));
        assert!(!world.is_changed_since::<i32>(entity, later - 1));

        // A write through the guard, and `into_inner`, both mark it.
        let mut value = world.get_mut::<i32>(entity).unwrap();
        *value += 1;
        assert_eq!(world.changed_tick::<i32>(entity), Some(later));
        assert!(world.is_changed_since::<i32>(entity, later - 1));
        let newest = world.advance_change_tick();
        let mut value = world.get_mut::<i32>(entity).unwrap();
        value.bypass_change_detection();
        *value.into_inner() = 7;
        assert_eq!(world.changed_tick::<i32>(entity), Some(newest));
        assert_eq!(world.get::<i32>(entity), Some(&7));

        // Replacing a component marks it too, and a despawn leaves no tick behind.
        let replaced = world.advance_change_tick();
        assert_eq!(world.insert(entity, 8_i32).unwrap(), Some(7));
        assert_eq!(world.changed_tick::<i32>(entity), Some(replaced));
        world.despawn(entity).unwrap();
        assert_eq!(world.changed_tick::<i32>(entity), None);
    }

    #[test]
    fn a_bookmark_reports_only_what_was_written_after_it() {
        let mut world = World::new();
        let entities: Vec<_> = (0..3)
            .map(|value| {
                let entity = world.spawn();
                world.insert(entity, value).unwrap();
                entity
            })
            .collect();
        let bookmark = world.change_tick();
        assert_eq!(world.changed_since::<i32>(bookmark).count(), 0);

        world.advance_change_tick();
        for (entity, mut value) in world.query_mut::<i32>() {
            if entity == entities[1] {
                *value += 10;
            }
        }
        assert_eq!(
            world
                .changed_since::<i32>(bookmark)
                .map(|(entity, value)| (entity, *value))
                .collect::<Vec<_>>(),
            vec![(entities[1], 11)]
        );

        // A pair query marks its mutable side and leaves the read side alone.
        for entity in &entities {
            world.insert(*entity, 0_u8).unwrap();
        }
        let bookmark = world.change_tick();
        assert_eq!(world.changed_since::<u8>(bookmark).count(), 0);
        world.advance_change_tick();
        for (_, mut value, flag) in world.query_pair_mut::<i32, u8>().unwrap() {
            if *flag == 0 {
                *value = -1;
            }
        }
        assert_eq!(world.changed_since::<i32>(bookmark).count(), 3);
        assert_eq!(world.changed_since::<u8>(bookmark).count(), 0);
        // Entity 1 was already marked by the query above, so it is not "new" for this bookmark.
        let bookmark = world.change_tick();
        assert_eq!(world.changed_since::<i32>(bookmark).count(), 0);
        assert_eq!(world.changed_since::<u8>(bookmark).count(), 0);
    }

    #[test]
    fn exhausted_generation_is_retired() {
        let mut world = World::new();
        let e = world.spawn();
        world.slots[e.index as usize].generation = u32::MAX;
        let e = Entity {
            generation: u32::MAX,
            ..e
        };
        world.despawn(e).unwrap();
        assert_ne!(world.spawn().index, e.index);
    }
}
