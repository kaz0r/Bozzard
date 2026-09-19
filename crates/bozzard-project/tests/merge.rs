use bozzard_project::merge_scenes;
use bozzard_scene::{Object, Scene};
use serde_json::json;

fn scene() -> Scene {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"Merge fixture","views":{},"objects":[]}"#)
            .unwrap();
    scene.objects = ["a", "b", "c"]
        .into_iter()
        .map(|id| Object {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        })
        .collect();
    scene
}

#[test]
fn merges_by_identity_across_insertions_deletions_and_independent_fields() -> anyhow::Result<()> {
    let base = scene();
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours.objects[0].name = "Renamed".into();
    ours.objects.remove(1);
    theirs.objects[0].transform.translation = [1., 2., 3.];
    theirs.objects.push(Object {
        id: "d".into(),
        name: "New object".into(),
        ..Default::default()
    });
    let merged = merge_scenes(&base, &ours, &theirs)?.resolved_scene()?;
    assert_eq!(
        merged
            .objects
            .iter()
            .map(|o| o.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "c", "d"]
    );
    assert_eq!(merged.objects[0].name, "Renamed");
    assert_eq!(merged.objects[0].transform.translation, [1., 2., 3.]);
    assert_eq!(
        merge_scenes(&base, &base, &theirs)?.resolved_scene()?,
        theirs
    );
    Ok(())
}

#[test]
fn reports_field_delete_edit_and_same_id_creation_conflicts_without_silent_resolution()
-> anyhow::Result<()> {
    let base = scene();
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours.objects[0].name = "Left".into();
    theirs.objects[0].name = "Right".into();
    let conflict = merge_scenes(&base, &ours, &theirs)?;
    assert_eq!(conflict.conflicts.len(), 1);
    assert_eq!(conflict.conflicts[0].path, "/objects/a/name");
    assert_eq!(conflict.conflicts[0].base, Some(json!("a")));
    assert!(conflict.resolved_scene().is_err());
    ours.objects.remove(0);
    let conflict = merge_scenes(&base, &ours, &theirs)?;
    assert_eq!(conflict.conflicts[0].path, "/objects/a");
    assert!(conflict.conflicts[0].ours.is_none());
    let mut empty = base.clone();
    empty.objects.clear();
    let conflict = merge_scenes(&empty, &ours, &theirs)?;
    assert!(conflict.resolved_scene().is_ok()); // shared additions are identical
    ours.objects[0].name = "Other new b".into();
    assert!(
        merge_scenes(&empty, &ours, &theirs)?
            .resolved_scene()
            .is_err()
    );
    Ok(())
}

#[test]
fn preserves_unilateral_order_and_reports_competing_order_changes() -> anyhow::Result<()> {
    let base = scene();
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours.objects.swap(0, 1);
    theirs.objects[0].name = "Edited".into();
    let merged = merge_scenes(&base, &ours, &theirs)?.resolved_scene()?;
    assert_eq!(merged.objects[0].id, "b");
    assert_eq!(merged.objects[1].name, "Edited");
    theirs.objects.swap(1, 2);
    let conflict = merge_scenes(&base, &ours, &theirs)?;
    assert!(
        conflict
            .conflicts
            .iter()
            .any(|c| c.path == "/objects/$order")
    );
    Ok(())
}

#[test]
fn validates_combined_hierarchy_and_preserves_unknown_components() -> anyhow::Result<()> {
    let mut base = scene();
    base.objects[0]
        .extras
        .insert("custom_data".into(), json!({"health":10,"team":1}));
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours.objects[0].extras.get_mut("custom_data").unwrap()["health"] = json!(20);
    theirs.objects[0].extras.get_mut("custom_data").unwrap()["team"] = json!(2);
    let merged = merge_scenes(&base, &ours, &theirs)?.resolved_scene()?;
    assert_eq!(
        merged.objects[0].extras["custom_data"],
        json!({"health":20,"team":2})
    );
    ours.objects[0].parent = Some("b".into());
    theirs.objects[1].parent = Some("a".into());
    let conflict = merge_scenes(&base, &ours, &theirs)?;
    assert!(conflict.conflicts.is_empty());
    assert!(conflict.validation_error.is_some());
    assert!(conflict.resolved_scene().is_err());
    Ok(())
}
