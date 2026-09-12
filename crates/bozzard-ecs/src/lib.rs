//! A small, safe ECS with generational entities and dense per-type component storage.
//!
//! Entity handles belong to one world and are not persistent scene/network IDs.
//! Query order is unspecified: removing a component can change iteration order.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
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

struct Storage<T> {
    sparse: Vec<Option<usize>>,
    entities: Vec<Entity>,
    values: Vec<T>,
}

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entities: Vec::new(),
            values: Vec::new(),
        }
    }
}

impl<T> Storage<T> {
    fn position(&self, entity: Entity) -> Option<usize> {
        let index = self.sparse.get(entity.index as usize).copied().flatten()?;
        (self.entities[index] == entity).then_some(index)
    }

    fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        if let Some(index) = self.position(entity) {
            return Some(std::mem::replace(&mut self.values[index], value));
        }
        self.sparse
            .resize(self.sparse.len().max(entity.index as usize + 1), None);
        self.sparse[entity.index as usize] = Some(self.values.len());
        self.entities.push(entity);
        self.values.push(value);
        None
    }

    fn remove(&mut self, entity: Entity) -> Option<T> {
        let index = self.position(entity)?;
        self.sparse[entity.index as usize] = None;
        self.entities.swap_remove(index);
        let value = self.values.swap_remove(index);
        if let Some(moved) = self.entities.get(index) {
            self.sparse[moved.index as usize] = Some(index);
        }
        Some(value)
    }

    fn get(&self, entity: Entity) -> Option<&T> {
        self.position(entity).map(|index| &self.values[index])
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
        let storage = self
            .components
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Storage::<T>::default()));
        Ok(storage
            .as_any_mut()
            .downcast_mut::<Storage<T>>()
            .expect("component storage type")
            .insert(entity, value))
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.storage::<T>()?.get(entity)
    }

    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let storage = self.storage_mut::<T>()?;
        let index = storage.position(entity)?;
        Some(&mut storage.values[index])
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
            .flat_map(|s| s.entities.iter().copied().zip(s.values.iter()))
    }

    pub fn query_mut<T: Component>(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.storage_mut::<T>()
            .into_iter()
            .flat_map(|s| s.entities.iter().copied().zip(s.values.iter_mut()))
    }

    /// Joins a mutable component with a read-only component, without unsafe aliasing.
    /// Missing storage produces an empty iterator; requesting the same type is an error.
    pub fn query_pair_mut<A: Component, B: Component>(
        &mut self,
    ) -> Result<impl Iterator<Item = (Entity, &mut A, &B)>, AliasedQuery> {
        let (a, b) = (TypeId::of::<A>(), TypeId::of::<B>());
        if a == b {
            return Err(AliasedQuery);
        }
        let [a, b] = self.components.get_disjoint_mut([&a, &b]);
        let a = a.and_then(|s| s.as_any_mut().downcast_mut::<Storage<A>>());
        let b = b.and_then(|s| s.as_any().downcast_ref::<Storage<B>>());
        Ok(a.into_iter()
            .flat_map(|s| s.entities.iter().copied().zip(s.values.iter_mut()))
            .filter_map(move |(entity, value)| Some((entity, value, b?.get(entity)?))))
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
        for (_, value, multiplier) in world.query_pair_mut::<i32, u64>().unwrap() {
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
