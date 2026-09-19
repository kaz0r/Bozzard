use bozzard_assets::{AssetData, AssetStore};
use bozzard_scene::{AssetKind, AssetSource};
use std::{collections::BTreeMap, fs};

#[test]
fn fonts_load_fail_and_reload_without_replacing_old_snapshots() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("bozzard-fonts-{}", std::process::id()));
    fs::create_dir_all(&root)?;
    // Hack Regular from epaint_default_fonts; license accompanies the fixture.
    fs::write(root.join("test.ttf"), include_bytes!("fonts/test.ttf"))?;
    let sources = BTreeMap::from([(
        "font".into(),
        AssetSource {
            kind: AssetKind::Font,
            path: "test.ttf".into(),
        },
    )]);
    let mut store = AssetStore::new(&root, &sources)?;
    store.load_pending()?;
    let old = store
        .get(store.handle("font").unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    let AssetData::Font(font) = old.as_ref() else {
        panic!("font did not import")
    };
    assert!(bozzard_text::bounds_with_font("test", 1., None, false, 0, Some(font))?.is_some());
    fs::write(root.join("test.ttf"), b"not a font")?;
    let mut broken = AssetStore::new(&root, &sources)?;
    assert!(broken.load_pending().is_err());
    assert!(
        broken
            .get(broken.handle("font").unwrap())
            .unwrap()
            .data()
            .is_none()
    );
    assert!(bozzard_text::bounds_with_font("test", 1., None, false, 0, Some(font))?.is_some());
    fs::remove_dir_all(root)?;
    Ok(())
}
