use anyhow::Result;
use bozzard_assets::{
    AssetData, AssetStore, CookSource,
    blockout::{Blockout, BrushPrimitive},
    job::Progress,
    package_model,
    terrain::Terrain,
};
use bozzard_scene::{AssetKind, AssetSource};
use glam::Vec3;
use std::{fs, path::PathBuf};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn terrain_and_brushes_use_mesh_picking_raw_packages_and_immutable_cooking() -> Result<()> {
    let temp =
        Temp(std::env::temp_dir().join(format!("bozzard-procedural-pack-{}", std::process::id())));
    fs::create_dir(&temp.0)?;
    let progress = Progress::default();
    for (name, bytes, expected_height) in [
        (
            "hill.terrain.json",
            Terrain::flat([5, 5], [4., 4.])?.to_json()?,
            0.,
        ),
        (
            "ramp.brush.json",
            Blockout {
                version: 1,
                primitive: BrushPrimitive::Ramp,
            }
            .to_json()?,
            0.5,
        ),
    ] {
        let source = temp.0.join(name);
        fs::write(&source, bytes)?;
        let snapshot = CookSource::read(AssetKind::Mesh, &source, &progress)?;
        let package = package_model(&source, &progress)?;
        let relocated = temp.0.join(format!("relocated-{name}"));
        fs::create_dir(&relocated)?;
        for (name, bytes) in package.files {
            fs::write(relocated.join(name), bytes)?;
        }
        fs::remove_file(&source)?;
        let cooked = snapshot.cook(&[], &progress)?;
        fs::write(relocated.join("shape.bmesh"), cooked)?;
        let catalog = [
            (
                "source".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: package.primary,
                },
            ),
            (
                "cooked".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: "shape.bmesh".into(),
                },
            ),
        ]
        .into();
        let mut store = AssetStore::new(&relocated, &catalog)?;
        store.load_pending()?;
        store.require_ready()?;
        let AssetData::Mesh(original) = snapshot.decode()? else {
            panic!()
        };
        for id in ["source", "cooked"] {
            let entry = store.get(store.handle(id).unwrap()).unwrap();
            let AssetData::Mesh(mesh) = entry.data().unwrap() else {
                panic!()
            };
            assert_eq!(mesh.indices, original.indices);
            assert_eq!(mesh.vertices, original.vertices);
            let hit = entry.raycast(Vec3::new(0., 10., 0.), Vec3::NEG_Y).unwrap();
            assert!((hit.distance - (10. - expected_height)).abs() < 1e-5);
        }
    }
    Ok(())
}
