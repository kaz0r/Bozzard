//! Gameplay data — prefabs and scripts — reaches the residency pass with the rest of the scene's
//! catalog but has nothing to upload. Listing the kinds in every place that asked the question let
//! a script slip through the filter and fail the upload instead of being skipped.
use bozzard_assets::{AssetData, AssetStore, ImageData, MeshData};
use bozzard_scene::{AssetKind, AssetSource};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn script() -> Arc<AssetData> {
    Arc::new(AssetData::Script("fn on_start(me) { }".into()))
}

fn prefab() -> Arc<AssetData> {
    Arc::new(AssetData::Prefab(
        bozzard_scene::Prefab::from_json(
            r#"{"version":1,"name":"p","root":"r","objects":[{"id":"r","name":"r",
                "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
        )
        .unwrap(),
    ))
}

#[test]
fn gameplay_assets_need_no_gpu_resource_and_are_refused_rather_than_uploaded() {
    assert!(!bozzard_render_assets::needs_gpu(&script()));
    assert!(!bozzard_render_assets::needs_gpu(&prefab()));
    // `UploadSource` is not `Debug`, so the error is read through `err()`.
    let error = bozzard_render_assets::upload_source(script())
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("no GPU resources"),
        "{error:#}"
    );
    assert!(bozzard_render_assets::upload_source(prefab()).is_err());
}

#[test]
fn drawable_assets_still_need_one() {
    assert!(bozzard_render_assets::needs_gpu(&Arc::new(
        AssetData::Image(ImageData {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
            compressed: None,
        })
    )));
    assert!(bozzard_render_assets::needs_gpu(&Arc::new(
        AssetData::Mesh(MeshData {
            vertices: Vec::new(),
            indices: Vec::new(),
            parts: Vec::new(),
            warnings: Vec::new(),
            skin: None,
        })
    )));
    assert!(
        bozzard_render_assets::upload_source(Arc::new(AssetData::Image(ImageData {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
            compressed: None,
        })))
        .is_ok()
    );
}

/// The gate a viewer uses before it draws anything asks the residency pass whether every entry is
/// ready. A scene with a script must not sit on that gate forever, and the editor's prefab panel
/// asks the same question of a prefab.
#[test]
fn a_store_of_gameplay_assets_is_resident_without_a_gpu() {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "bozzard-no-gpu-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("spin.rs"),
        "fn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }",
    )
    .unwrap();
    std::fs::write(
        root.join("chime.wav"),
        include_bytes!("../../../examples/demo/scenes/assets/middleware-chime.wav"),
    )
    .unwrap();
    let sources = BTreeMap::from([
        (
            "spin".to_string(),
            AssetSource {
                kind: AssetKind::Script,
                path: "spin.rs".into(),
            },
        ),
        (
            "chime".to_string(),
            AssetSource {
                kind: AssetKind::Audio,
                path: "chime.wav".into(),
            },
        ),
    ]);
    let mut store = AssetStore::new(&root, &sources).unwrap();
    store.refresh();
    store.require_ready().unwrap();
    let residency = bozzard_render_assets::Residency::default();
    assert!(residency.is_current(&store, "spin"));
    assert!(residency.is_current(&store, "chime"));
    let audio = store
        .get(store.handle("chime").unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    assert!(!bozzard_render_assets::needs_gpu(&audio));
    assert!(bozzard_render_assets::upload_source(audio).is_err());
    assert!(residency.has_all(&store));
    // An id the catalog does not hold is still not current.
    assert!(!residency.is_current(&store, "missing"));
    std::fs::remove_dir_all(&root).unwrap();
}
