//! Forward compatibility of the component set, and registering a component this crate does not
//! compile in. A component an older build does not know is data, not a parse failure.
use bozzard_scene::{
    ComponentType, Field, FieldValue, Object, Scene, Ui, available_components, component_type,
    component_type_by_label, components, register_component,
};

const CUSTOM: &str = "hover_thruster";

fn object_json(components: &str) -> String {
    format!(
        r#"{{"version":1,"name":"Probe","views":{{}},"objects":[{{"id":"probe","name":"Probe",
        "transform":{{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}
        {components}}}]}}"#
    )
}

fn probe(scene: &Scene) -> &Object {
    &scene.objects[0]
}

#[test]
fn a_component_from_a_newer_build_is_kept_verbatim() {
    let json = object_json(r#","plasma_shield":{"charge":12.5,"emit":true}"#);
    let scene = Scene::from_json(&json).unwrap();
    assert_eq!(
        probe(&scene).extra("plasma_shield"),
        Some(&serde_json::json!({"charge": 12.5, "emit": true})),
        "an unrecognized component is preserved, not dropped"
    );
    // And it comes back out of the writer unchanged, so no round trip loses it.
    let saved = scene.to_json().unwrap();
    assert!(saved.contains(r#""plasma_shield""#) && saved.contains("12.5"));
    assert_eq!(Scene::from_json(&saved).unwrap(), scene);
}

#[test]
fn identity_fields_stay_required_while_the_component_set_is_open() {
    // `transform` is not a component: a scene without one is still a bug.
    let missing = r#"{"version":1,"name":"x","views":{},"objects":[{"id":"a","name":"a"}]}"#;
    assert!(Scene::from_json(missing).is_err());
    // A misspelled identity key is preserved rather than silently dropped, which is the price of
    // open components; the editor reports unrecognized entries instead of hiding them.
    let scene = Scene::from_json(&object_json(r#","parnet":"other""#)).unwrap();
    assert_eq!(
        probe(&scene).extra("parnet"),
        Some(&serde_json::json!("other"))
    );
    assert!(probe(&scene).parent.is_none());
}

#[test]
fn a_typo_inside_a_known_component_still_fails_loudly() {
    let json = object_json(r#","light":{"enabled":true,"colour":[1,1,1]}"#);
    // The cause chain names the component and the field, so the author can find the typo.
    let error = format!("{:#}", Scene::from_json(&json).unwrap_err());
    assert!(
        error.contains("light"),
        "the error names the component: {error}"
    );
    assert!(
        error.contains("colour"),
        "the error names the field: {error}"
    );
}

#[test]
fn a_game_can_register_its_own_component() {
    assert!(
        component_type(CUSTOM).is_none(),
        "the test registers this component itself"
    );
    register_component(row()).unwrap();
    register_component(row()).expect_err("a name cannot be registered twice");
    assert_eq!(component_type(CUSTOM).unwrap().label, "Hover Thruster");
    assert_eq!(
        component_type_by_label("hover thruster").unwrap().name,
        CUSTOM
    );
    assert!(components().any(|entry| entry.name == CUSTOM));

    let entry = component_type(CUSTOM).unwrap();
    let scene =
        Scene::from_json(&object_json(&format!(r#","{CUSTOM}":{{"thrust":12.0}}"#))).unwrap();
    let mut object = probe(&scene).clone();
    assert!((entry.present)(&object), "the registered row owns its key");
    assert_eq!(
        (entry.get)(&object, "thrust"),
        Some(FieldValue::Number(12.0))
    );
    (entry.set)(&mut object, "thrust", FieldValue::Number(30.0)).unwrap();
    assert_eq!(
        (entry.save)(&object).unwrap(),
        Some(serde_json::json!({"thrust": 30.0}))
    );

    // It behaves like a built-in everywhere the registry is consulted.
    assert!(!available_components(&object).any(|row| row.name == CUSTOM));
    let mut added = probe(&scene).clone();
    added.extras.remove(CUSTOM);
    assert!(available_components(&added).any(|row| row.name == CUSTOM));
    let saved = Scene {
        objects: vec![object],
        ..scene.clone()
    }
    .to_json()
    .unwrap();
    assert_eq!(
        probe(&Scene::from_json(&saved).unwrap()).extra(CUSTOM),
        Some(&serde_json::json!({ "thrust": 30.0 }))
    );
}

/// A component row with no compiled-in Rust type: its value lives in `Object::extras`.
fn row() -> ComponentType {
    ComponentType {
        name: CUSTOM,
        label: "Hover Thruster",
        ui: Ui::Generic,
        help: "Registered by the game, not compiled into the engine.",
        fields: || {
            const FIELDS: &[Field] = &[Field::range("thrust", "Thrust (N)", 1.0, 0.0, 100_000.0)];
            FIELDS
        },
        get: |object, key| {
            let thrust = thrust(object)?;
            (key == "thrust").then_some(FieldValue::Number(thrust as f32))
        },
        set: |object, key, value| {
            anyhow::ensure!(key == "thrust", "Hover Thruster has no field '{key}'");
            object.set_extra(CUSTOM, serde_json::json!({ "thrust": value.number()? }));
            Ok(())
        },
        present: |object| object.extra(CUSTOM).is_some(),
        available: |_object| true,
        add: |object, _context| {
            object.set_extra(CUSTOM, serde_json::json!({ "thrust": 0.0 }));
            Ok(())
        },
        remove: |object, _scene| {
            object.extras.remove(CUSTOM);
        },
        merge: |current, old, source| {
            if current.extra(CUSTOM) == old.extra(CUSTOM) {
                match source.extra(CUSTOM) {
                    Some(value) => current.set_extra(CUSTOM, value.clone()),
                    None => {
                        current.extras.remove(CUSTOM);
                    }
                }
            }
        },
        load: |object, value| {
            object.set_extra(CUSTOM, value);
            Ok(())
        },
        save: |object| Ok(object.extra(CUSTOM).cloned()),
    }
}

fn thrust(object: &Object) -> Option<f64> {
    object.extra(CUSTOM)?.get("thrust")?.as_f64()
}
