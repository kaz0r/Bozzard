use super::*;
use crate::{AssetData, AssetStore};
use bozzard_scene::{AssetKind, AssetSource};
use std::path::PathBuf;

fn load(name: &str) -> MeshData {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
    let sources = [(
        "model".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: name.into(),
        },
    )]
    .into();
    let mut store = AssetStore::new(&root, &sources).unwrap();
    store.load_pending().unwrap();
    store.require_ready().unwrap();
    let AssetData::Mesh(m) = store
        .get(store.handle("model").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    m.clone()
}

#[test]
fn native_models_preserve_geometry_materials_shared_maps_and_animated_poses() {
    for name in ["octahedron.obj", "courier.glb", "animated-banner.gltf"] {
        let mesh = load(name);
        let bytes = encode(&mesh, &[], &Progress::default()).unwrap();
        let restored = decode(&bytes).unwrap();
        assert_eq!(restored.vertices, mesh.vertices);
        assert_eq!(restored.indices, mesh.indices);
        assert_eq!(restored.parts.len(), mesh.parts.len());
        assert_eq!(encode(&restored, &[], &Progress::default()).unwrap(), bytes);
        for (a, b) in mesh.parts.iter().zip(&restored.parts) {
            assert_eq!(a.source_key, b.source_key);
            assert_eq!(a.name, b.name);
            assert_eq!(a.color, b.color);
            assert_eq!(a.alpha_cutoff, b.alpha_cutoff);
            assert_eq!(
                a.image.as_ref().map(|i| &i.rgba),
                b.image.as_ref().map(|i| &i.rgba)
            );
            if let Some(sa) = &a.shading {
                let sb = b.shading.as_ref().unwrap();
                assert_eq!(sa.vertices, sb.vertices);
                assert_eq!(
                    sa.material.base_color_sampler,
                    sb.material.base_color_sampler
                );
                for (ma, mb) in [
                    &sa.material.normal,
                    &sa.material.metallic_roughness,
                    &sa.material.occlusion,
                    &sa.material.emissive,
                ]
                .into_iter()
                .zip([
                    &sb.material.normal,
                    &sb.material.metallic_roughness,
                    &sb.material.occlusion,
                    &sb.material.emissive,
                ]) {
                    assert_eq!(
                        ma.as_ref().map(|m| (&m.image.rgba, m.sampler)),
                        mb.as_ref().map(|m| (&m.image.rgba, m.sampler))
                    );
                }
            }
        }
        match (&mesh.skin, &restored.skin) {
            (Some(a), Some(b)) => {
                assert_eq!(a.vertices, b.vertices);
                assert_eq!(a.rig, b.rig);
                for clip in 0..a.rig.clips.len() {
                    for time in [0., 0.125, 0.5, 1.] {
                        let pose = a.rig.sample(clip, time).unwrap();
                        let restored = b.rig.sample(clip, time).unwrap();
                        assert_eq!(pose, restored);
                        assert_eq!(
                            a.rig.palette(&pose).unwrap(),
                            b.rig.palette(&restored).unwrap()
                        );
                    }
                }
            }
            (None, None) => {}
            _ => panic!("skin lost"),
        }
    }
}

#[test]
fn compression_cooks_shared_model_images_once_with_required_color_spaces() {
    let mut mesh = load("courier.glb");
    let part = mesh
        .parts
        .iter_mut()
        .find(|p| p.image.is_some() && p.shading.is_some())
        .unwrap();
    let image = part.image.as_ref().unwrap().clone();
    let s = &mut part.shading.as_mut().unwrap().material;
    s.occlusion = Some(TextureMap {
        image: image.clone(),
        sampler: Default::default(),
    });
    s.emissive = Some(TextureMap {
        image: image.clone(),
        sampler: Default::default(),
    });
    let bytes = encode(
        &mesh,
        &[Compression::Bc3, Compression::Astc4x4],
        &Progress::default(),
    )
    .unwrap();
    let restored = decode(&bytes).unwrap();
    let p = restored
        .parts
        .iter()
        .find(|p| {
            p.image.as_ref().is_some_and(|i| i.rgba == image.rgba)
                && p.shading
                    .as_ref()
                    .is_some_and(|s| s.material.occlusion.is_some())
        })
        .unwrap();
    let m = &p.shading.as_ref().unwrap().material;
    assert!(Arc::ptr_eq(
        p.image.as_ref().unwrap(),
        &m.occlusion.as_ref().unwrap().image
    ));
    assert!(Arc::ptr_eq(
        p.image.as_ref().unwrap(),
        &m.emissive.as_ref().unwrap().image
    ));
    let cooked = p.image.as_ref().unwrap().compressed.as_ref().unwrap();
    assert_eq!(cooked.variants().len(), 4);
    assert_eq!(restored.vertices, mesh.vertices);
    assert_eq!(restored.indices, mesh.indices);
}

fn resign(bytes: &mut [u8]) {
    let n = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..n]);
    bytes[n..].copy_from_slice(&digest);
}
fn mutate_metadata(original: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let n = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
    let mut meta: serde_json::Value = serde_json::from_slice(&original[16..16 + n]).unwrap();
    edit(&mut meta);
    let json = serde_json::to_vec(&meta).unwrap();
    let mut bytes = original[..12].to_vec();
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(json);
    bytes.extend(&original[16 + n..]);
    resign(&mut bytes);
    bytes
}

#[test]
fn corrupt_counts_references_geometry_rigs_and_trailing_data_are_rejected() {
    let mesh = load("animated-banner.gltf");
    let bytes = encode(&mesh, &[], &Progress::default()).unwrap();
    for n in [0, 8, 16, 32, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(&bytes[..n]).is_err());
    }
    let mut corrupt = bytes.clone();
    corrupt[24] ^= 255;
    assert!(decode(&corrupt).is_err());
    let bad = [
        mutate_metadata(&bytes, |m| m["vertices"] = 0.into()),
        mutate_metadata(&bytes, |m| m["indices"] = u64::MAX.into()),
        mutate_metadata(&bytes, |m| m["images"] = u64::MAX.into()),
        mutate_metadata(&bytes, |m| m["parts"][0]["image"] = u64::MAX.into()),
        mutate_metadata(&bytes, |m| m["parts"][0]["start"] = u32::MAX.into()),
        mutate_metadata(&bytes, |m| {
            m["parts"][0]["shading"]["vertices"] = u64::MAX.into()
        }),
        mutate_metadata(&bytes, |m| m["rig"]["nodes"][0]["parent"] = 0.into()),
        mutate_metadata(&bytes, |m| m["unknown"] = true.into()),
    ];
    for bytes in bad {
        assert!(decode(&bytes).is_err());
    }
    let start = 16 + u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut nan = bytes.clone();
    nan[start..start + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    resign(&mut nan);
    assert!(decode(&nan).is_err());
    let mut trailing = bytes[..bytes.len() - 32].to_vec();
    trailing.push(0);
    trailing.extend([0; 32]);
    resign(&mut trailing);
    assert!(decode(&trailing).is_err());
    let mut invalid = mesh;
    invalid.skin.as_mut().unwrap().vertices[0][0] = u32::MAX;
    assert!(encode(&invalid, &[], &Progress::default()).is_err());
}
