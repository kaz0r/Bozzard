//! Typed middleware components reuse the extensible document component storage.
//! Deserialize once on spawn, then update typed ECS values; editing uses the same registry fields.
use crate::{
    AddContext, AssetKind, Component, ComponentType, Entity, FieldKind, FieldValue, Object, Scene,
    World,
};
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

/// Each middleware component declares whether a filtered Edit document needs it.
/// Gameplay-only dependencies must not survive when their targets can be hidden.
pub enum PreviewPolicy {
    /// Keep while visible. When hidden, remove the component by default;
    /// `Authored::hide_in_preview` may retain a disabled representation instead.
    Retain,
    /// Remove gameplay dependencies from both visible and hidden preview objects.
    Omit,
}

pub trait Authored: Component + Default + Serialize + DeserializeOwned {
    /// Deliberately required: adding a component must decide its preview behavior.
    const PREVIEW: PreviewPolicy;

    /// Hidden components normally disappear. Components that serve as document
    /// markers (such as Canvas) can retain a disabled representation instead.
    fn hide_in_preview(object: &mut Object) -> Result<()> {
        object.extras.remove(Self::NAME);
        Ok(())
    }
    fn validate(&self) -> Result<()>;
    fn validate_scene(&self, _owner: &Object, _scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()
    }
    fn remap_objects(&mut self, _mapping: &BTreeMap<String, String>) {}
    fn write_targets(&self, _owner: &str) -> Vec<String> {
        Vec::new()
    }
    fn initialize_runtime(&self, _world: &mut World, _owner: &str) -> Result<()> {
        Ok(())
    }
    /// Move initialization data built by a scene worker into the live world.
    fn accept_prepared(_prepared: &mut World, _live: &mut World) {}
}
type FieldValues = fn(&Object) -> Result<Vec<(crate::Field, FieldValue)>>;
pub struct Entry {
    pub component: ComponentType,
    validate: fn(&Object, &Scene, &BTreeSet<&str>) -> Result<()>,
    spawn: fn(&Object, &mut World, Entity) -> Result<()>,
    capture: fn(&mut Object, &World, Entity) -> Result<()>,
    remap: fn(&mut Object, &BTreeMap<String, String>) -> Result<()>,
    write_targets: fn(&Object) -> Result<Vec<String>>,
    field_values: FieldValues,
    accept_prepared: fn(&mut World, &mut World),
    preview: fn(&mut Object, bool) -> Result<()>,
}
pub fn get<T: Authored>(object: &Object) -> Result<Option<T>> {
    object
        .extras
        .get(T::NAME)
        .map(|value| {
            T::deserialize(value)
                .with_context(|| format!("reading {} on '{}'", T::LABEL, object.id))
        })
        .transpose()
}
pub fn set<T: Authored>(object: &mut Object, value: &T) -> Result<()> {
    value.validate()?;
    object
        .extras
        .insert(T::NAME.into(), serde_json::to_value(value)?);
    Ok(())
}
fn load<T: Authored>(object: &mut Object, value: serde_json::Value) -> Result<()> {
    let value: T = serde_json::from_value(value)?;
    set(object, &value)
}
fn save<T: Authored>(object: &Object) -> Result<Option<serde_json::Value>> {
    Ok(object.extras.get(T::NAME).cloned())
}
fn field<T: Authored>(object: &Object, key: &str) -> Option<FieldValue> {
    get::<T>(object).ok().flatten()?.field(key)
}
fn set_field<T: Authored>(object: &mut Object, key: &str, value: FieldValue) -> Result<()> {
    let mut component = get::<T>(object)?.context("component is missing")?;
    component.set_field(key, value)?;
    set(object, &component)
}
fn present<T: Authored>(object: &Object) -> bool {
    object.extras.contains_key(T::NAME)
}
fn available<T: Authored>(object: &Object) -> bool {
    !present::<T>(object)
}
fn add<T: Authored>(object: &mut Object, _context: &AddContext) -> Result<()> {
    set(object, &T::default())
}
fn remove<T: Authored>(object: &mut Object, _scene: &mut Scene) {
    object.extras.remove(T::NAME);
}
fn merge<T: Authored>(current: &mut Object, old: &Object, source: &Object) {
    if current.extras.get(T::NAME) == old.extras.get(T::NAME) {
        if let Some(value) = source.extras.get(T::NAME) {
            current.extras.insert(T::NAME.into(), value.clone());
        } else {
            current.extras.remove(T::NAME);
        }
    }
}
fn validate<T: Authored>(object: &Object, scene: &Scene, ids: &BTreeSet<&str>) -> Result<()> {
    if let Some(component) = get::<T>(object)? {
        component.validate_scene(object, scene, ids)?;
    }
    Ok(())
}
fn spawn<T: Authored>(object: &Object, world: &mut World, entity: Entity) -> Result<()> {
    if let Some(component) = get::<T>(object)? {
        component.initialize_runtime(world, &object.id)?;
        world.insert(entity, component)?;
    }
    Ok(())
}
fn capture<T: Authored>(object: &mut Object, world: &World, entity: Entity) -> Result<()> {
    if let Some(component) = world.get::<T>(entity) {
        set(object, component)?;
    } else {
        object.extras.remove(T::NAME);
    }
    Ok(())
}
fn remap<T: Authored>(object: &mut Object, mapping: &BTreeMap<String, String>) -> Result<()> {
    if let Some(mut value) = get::<T>(object)? {
        value.remap_objects(mapping);
        set(object, &value)?;
    }
    Ok(())
}
pub const fn entry<T: Authored>() -> Entry {
    Entry {
        preview: |object, visible| match T::PREVIEW {
            PreviewPolicy::Omit => {
                object.extras.remove(T::NAME);
                Ok(())
            }
            PreviewPolicy::Retain if !visible => T::hide_in_preview(object),
            PreviewPolicy::Retain => Ok(()),
        },
        accept_prepared: T::accept_prepared,
        component: ComponentType {
            name: T::NAME,
            label: T::LABEL,
            ui: T::UI,
            help: T::HELP,
            fields: T::fields,
            get: field::<T>,
            set: set_field::<T>,
            present: present::<T>,
            available: available::<T>,
            add: add::<T>,
            remove: remove::<T>,
            merge: merge::<T>,
            load: load::<T>,
            save: save::<T>,
        },
        validate: validate::<T>,
        spawn: spawn::<T>,
        capture: capture::<T>,
        remap: remap::<T>,
        field_values: |object| {
            let Some(component) = get::<T>(object)? else {
                return Ok(Vec::new());
            };
            Ok(T::fields()
                .iter()
                .filter(|f| f.visible.is_none_or(|visible| visible(object)))
                .filter_map(|field| component.field(field.key).map(|value| (*field, value)))
                .collect())
        },
        write_targets: |object| {
            Ok(get::<T>(object)?.map_or_else(Vec::new, |c| c.write_targets(&object.id)))
        },
    }
}

/// Project middleware into an authoring-only document without changing source data.
pub(crate) fn prepare_preview(object: &mut Object, visible: bool) -> Result<()> {
    if !visible {
        // Unknown extension components cannot contribute visuals to a hidden object.
        object.extras.retain(|name, _| {
            super::ENTRIES
                .iter()
                .any(|entry| entry.component.name == name)
        });
    }
    for entry in super::ENTRIES {
        (entry.preview)(object, visible)?;
    }
    Ok(())
}
pub(crate) fn accept_prepared(prepared: &mut World, live: &mut World) {
    for entry in super::ENTRIES {
        (entry.accept_prepared)(prepared, live);
    }
}
/// Baking excludes every object a middleware component can animate, even before playback.
pub fn dynamic_targets(scene: &Scene) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for object in &scene.objects {
        for entry in super::ENTRIES {
            match (entry.write_targets)(object) {
                Ok(ids) => targets.extend(ids),
                Err(_) => return scene.objects.iter().map(|o| o.id.clone()).collect(),
            }
        }
    }
    targets
}
pub fn validate_all(scene: &Scene) -> Result<()> {
    let ids: BTreeSet<_> = scene.objects.iter().map(|o| o.id.as_str()).collect();
    for object in &scene.objects {
        for entry in super::ENTRIES {
            (entry.validate)(object, scene, &ids)?;
        }
        for (asset, kind) in dependencies(object) {
            ensure!(
                scene.assets.get(asset).is_some_and(|a| a.kind == kind),
                "middleware asset '{asset}' is missing or has the wrong type"
            );
        }
    }
    Ok(())
}
pub(crate) fn spawn_all(object: &Object, world: &mut World, entity: Entity) -> Result<()> {
    for entry in super::ENTRIES {
        (entry.spawn)(object, world, entity)?;
    }
    Ok(())
}
pub(crate) fn capture_all(object: &mut Object, world: &World, entity: Entity) -> Result<()> {
    for entry in super::ENTRIES {
        (entry.capture)(object, world, entity)?;
    }
    Ok(())
}
pub(crate) fn remap_all(object: &mut Object, mapping: &BTreeMap<String, String>) {
    // Source objects have already passed validation; reference remapping preserves their schemas.
    for entry in super::ENTRIES {
        (entry.remap)(object, mapping).expect("validated middleware component");
    }
}
pub(crate) fn dependencies(object: &Object) -> Vec<(&str, AssetKind)> {
    let mut dependencies = Vec::new();
    for entry in super::ENTRIES {
        if let Some(value) = object.extras.get(entry.component.name) {
            for field in (entry.component.fields)() {
                if let FieldKind::Asset(kind) = field.kind
                    && let Some(asset) = value
                        .get(field.key)
                        .and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty())
                {
                    dependencies.push((asset, kind));
                }
            }
        }
    }
    dependencies
}
pub(crate) fn remap_assets(object: &mut Object, mapping: &BTreeMap<String, String>) {
    for entry in super::ENTRIES {
        if let Some(value) = object.extras.get_mut(entry.component.name) {
            for field in (entry.component.fields)() {
                if matches!(field.kind, FieldKind::Asset(_))
                    && let Some(serde_json::Value::String(id)) = value.get_mut(field.key)
                    && let Some(new) = mapping.get(id)
                {
                    *id = new.clone();
                }
            }
        }
    }
}

/// Deserialize a middleware component once for all inspector readouts. Large cooked rigs and
/// navmeshes must not be copied and parsed once per scalar field.
pub fn field_values(
    object: &Object,
    name: &str,
) -> Option<Result<Vec<(crate::Field, FieldValue)>>> {
    super::ENTRIES
        .iter()
        .find(|entry| entry.component.name == name)
        .map(|entry| (entry.field_values)(object))
}
