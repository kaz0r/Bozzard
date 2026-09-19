//! Structural three-way scene merges. Objects are matched by persistent ID, never row number.
use anyhow::{Result, ensure};
use bozzard_scene::Scene;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct MergeConflict {
    /// JSON-pointer escaped path. ID-keyed arrays use the persistent ID as a segment.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ours: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theirs: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct SceneMerge {
    /// Reviewable draft; conflicts retain our value. Do not publish it as a resolved scene.
    pub candidate: Value,
    pub conflicts: Vec<MergeConflict>,
    pub validation_error: Option<String>,
}
impl SceneMerge {
    pub fn resolved_scene(&self) -> Result<Scene> {
        ensure!(
            self.conflicts.is_empty(),
            "scene merge has {} unresolved conflicts",
            self.conflicts.len()
        );
        ensure!(
            self.validation_error.is_none(),
            "merged scene is invalid: {}",
            self.validation_error.as_deref().unwrap_or_default()
        );
        Scene::from_json(&serde_json::to_string(&self.candidate)?)
    }
}

pub fn merge_scenes(base: &Scene, ours: &Scene, theirs: &Scene) -> Result<SceneMerge> {
    for scene in [base, ours, theirs] {
        scene.validate()?;
    }
    let [base, ours, theirs] = [base, ours, theirs].map(serde_json::to_value);
    let (base, ours, theirs) = (base?, ours?, theirs?);
    let mut conflicts = Vec::new();
    let candidate = merge("", Some(&base), Some(&ours), Some(&theirs), &mut conflicts).unwrap();
    let validation_error = Scene::from_json(&serde_json::to_string(&candidate)?)
        .err()
        .map(|error| format!("{error:#}"));
    Ok(SceneMerge {
        candidate,
        conflicts,
        validation_error,
    })
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}
fn by_id(values: &[Value]) -> Option<BTreeMap<&str, &Value>> {
    let mut result = BTreeMap::new();
    for value in values {
        let id = value.as_object()?.get("id")?.as_str()?;
        if result.insert(id, value).is_some() {
            return None;
        }
    }
    Some(result)
}
fn ids(values: &[Value]) -> Vec<&str> {
    values
        .iter()
        .map(|value| value["id"].as_str().unwrap())
        .collect()
}
fn conflict(
    path: &str,
    base: Option<&Value>,
    ours: Option<&Value>,
    theirs: Option<&Value>,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<Value> {
    conflicts.push(MergeConflict {
        path: path.into(),
        base: base.cloned(),
        ours: ours.cloned(),
        theirs: theirs.cloned(),
    });
    ours.cloned()
}
fn merge(
    path: &str,
    base: Option<&Value>,
    ours: Option<&Value>,
    theirs: Option<&Value>,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<Value> {
    if ours == theirs || theirs == base {
        return ours.cloned();
    }
    if ours == base {
        return theirs.cloned();
    }
    match (base, ours, theirs) {
        (Some(Value::Object(b)), Some(Value::Object(o)), Some(Value::Object(t))) => {
            let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).collect();
            Some(Value::Object(
                keys.into_iter()
                    .filter_map(|key| {
                        merge(
                            &child(path, key),
                            b.get(key),
                            o.get(key),
                            t.get(key),
                            conflicts,
                        )
                        .map(|value| (key.clone(), value))
                    })
                    .collect(),
            ))
        }
        (Some(Value::Array(b)), Some(Value::Array(o)), Some(Value::Array(t))) => {
            let (Some(bm), Some(om), Some(tm)) = (by_id(b), by_id(o), by_id(t)) else {
                return conflict(path, base, ours, theirs, conflicts);
            };
            // Respect a unilateral reordering; conflicting reorders require a decision.
            let common: BTreeSet<_> = bm
                .keys()
                .copied()
                .filter(|id| om.contains_key(id) && tm.contains_key(id))
                .collect();
            let filtered = |values: &[Value]| -> Vec<String> {
                ids(values)
                    .into_iter()
                    .filter(|id| common.contains(id))
                    .map(str::to_owned)
                    .collect()
            };
            let (bo, oo, to) = (filtered(b), filtered(o), filtered(t));
            if oo != bo && to != bo && oo != to {
                conflicts.push(MergeConflict {
                    path: child(path, "$order"),
                    base: Some(Value::from(ids(b))),
                    ours: Some(Value::from(ids(o))),
                    theirs: Some(Value::from(ids(t))),
                });
            }
            let mut order = if oo == bo && to != bo { ids(t) } else { ids(o) };
            let mut seen: BTreeSet<_> = order.iter().copied().collect();
            for id in ids(t).into_iter().chain(ids(b)) {
                if seen.insert(id) {
                    order.push(id);
                }
            }
            Some(Value::Array(
                order
                    .into_iter()
                    .filter_map(|id| {
                        merge(
                            &child(path, id),
                            bm.get(id).copied(),
                            om.get(id).copied(),
                            tm.get(id).copied(),
                            conflicts,
                        )
                    })
                    .collect(),
            ))
        }
        _ => conflict(path, base, ours, theirs, conflicts),
    }
}
