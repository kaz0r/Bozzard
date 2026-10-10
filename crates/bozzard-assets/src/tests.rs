use super::*;
const PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
const NEXT_PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette-reloaded.png");
const OBJ: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/quad.obj");
fn no_dependencies() -> SourceSnapshot {
    SourceSnapshot {
        primary: Ok(Vec::new()),
        dependencies: Vec::new(),
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bozzard-assets-{}-{}",
            std::process::id(),
            NEXT_STORE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn background_refresh_keeps_live_data_until_published_and_recovers() {
    use std::time::{Duration, Instant};
    let wait = |job: job::Job<(AssetStore, Vec<Handle>)>| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = job.poll() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "refresh timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let dir = Temp::new();
    let sources = BTreeMap::from([(
        "palette".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "palette.png".into(),
        },
    )]);
    std::fs::write(dir.0.join("palette.png"), PNG).unwrap();
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    store.refresh();
    let handle = store.handle("palette").unwrap();
    let original = store.get(handle).unwrap().data().unwrap() as *const AssetData;
    let (unchanged, changes) = wait(store.refresh_job().unwrap());
    assert!(changes.is_empty());
    assert_eq!(
        unchanged.get(handle).unwrap().data().unwrap() as *const AssetData,
        original
    );
    std::fs::write(dir.0.join("palette.png"), b"broken").unwrap();
    let (failed, changes) = wait(store.refresh_job().unwrap());
    assert_eq!(changes, vec![handle]);
    assert_eq!(store.get(handle).unwrap().state(), &LoadState::Ready);
    assert!(matches!(
        failed.get(handle).unwrap().state(),
        LoadState::Failed(_)
    ));
    assert_eq!(
        failed.get(handle).unwrap().data().unwrap() as *const AssetData,
        original
    );
    std::fs::write(dir.0.join("palette.png"), NEXT_PNG).unwrap();
    let (recovered, changes) = wait(failed.refresh_job().unwrap());
    assert_eq!(changes, vec![handle]);
    assert_eq!(recovered.get(handle).unwrap().revision(), 2);
    assert_eq!(store.get(handle).unwrap().revision(), 1);
    recovered.require_ready().unwrap();
}

#[test]
fn reload_keeps_handles_and_last_good_data_through_corruption_deletion_and_recovery() {
    let dir = Temp::new();
    let sources = BTreeMap::from([(
        "palette".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "palette.png".into(),
        },
    )]);
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    let h = store.handle("palette").unwrap();
    assert_eq!(store.get(h).unwrap().state(), &LoadState::Pending);
    let other = AssetStore::new(&dir.0, &sources).unwrap();
    assert!(other.get(h).is_none());
    std::fs::write(dir.0.join("palette.png"), PNG).unwrap();
    assert_eq!(store.refresh(), vec![h]);
    assert_eq!(store.get(h).unwrap().revision(), 1);
    assert!(store.refresh().is_empty());
    std::fs::write(dir.0.join("palette.png"), b"broken").unwrap();
    assert_eq!(store.refresh(), vec![h]);
    assert!(matches!(
        store.get(h).unwrap().state(),
        LoadState::Failed(_)
    ));
    assert_eq!(store.get(h).unwrap().revision(), 1);
    let AssetData::Image(image) = store.get(h).unwrap().data().unwrap() else {
        panic!()
    };
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
    assert!(store.refresh().is_empty());
    std::fs::remove_file(dir.0.join("palette.png")).unwrap();
    assert_eq!(store.refresh(), vec![h]);
    assert_eq!(store.get(h).unwrap().revision(), 1);
    std::fs::write(dir.0.join("palette.png"), NEXT_PNG).unwrap();
    assert_eq!(store.refresh(), vec![h]);
    store.require_ready().unwrap();
    assert_eq!(store.get(h).unwrap().revision(), 2);
    let AssetData::Image(image) = store.get(h).unwrap().data().unwrap() else {
        panic!()
    };
    assert_eq!(&image.rgba[..4], &[0, 255, 255, 255]);
}

#[test]
fn picking_index_follows_shared_geometry_across_reload_and_catalog_snapshots() {
    let dir = Temp::new();
    let sources = BTreeMap::from([(
        "mesh".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "mesh.obj".into(),
        },
    )]);
    let path = dir.0.join("mesh.obj");
    std::fs::write(&path, OBJ).unwrap();
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    let handle = store.handle("mesh").unwrap();
    assert!(
        store
            .get(handle)
            .unwrap()
            .raycast(Vec3::Z, -Vec3::Z)
            .is_none()
    );
    store.refresh();
    let original = store.clone();
    let old_index = original.get(handle).unwrap().mesh_index.as_ref().unwrap();
    let old_hit = original.get(handle).unwrap().raycast(Vec3::Z, -Vec3::Z);
    assert!(old_hit.is_some());
    let catalog = store.for_catalog(&dir.0, &sources).unwrap();
    assert!(Arc::ptr_eq(
        old_index,
        catalog
            .get(catalog.handle("mesh").unwrap())
            .unwrap()
            .mesh_index
            .as_ref()
            .unwrap()
    ));
    let renamed = BTreeMap::from([("other-scene-mesh".into(), sources["mesh"].clone())]);
    let renamed = store.for_catalog(&dir.0, &renamed).unwrap();
    let renamed_entry = renamed
        .get(renamed.handle("other-scene-mesh").unwrap())
        .unwrap();
    assert_eq!(renamed_entry.id, "other-scene-mesh");
    assert!(Arc::ptr_eq(
        old_index,
        renamed_entry.mesh_index.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        original.get(handle).unwrap().data.as_ref().unwrap(),
        renamed_entry.data.as_ref().unwrap()
    ));
    std::fs::write(&path, "broken mesh").unwrap();
    store.refresh();
    assert!(Arc::ptr_eq(
        old_index,
        store.get(handle).unwrap().mesh_index.as_ref().unwrap()
    ));
    assert_eq!(
        store.get(handle).unwrap().raycast(Vec3::Z, -Vec3::Z),
        old_hit
    );
    std::fs::write(&path, "v 4 -1 0\nv 6 -1 0\nv 5 1 0\nf 1 2 3\n").unwrap();
    let job = store.refresh_job().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let loaded = loop {
        if let Some(result) = job.poll() {
            break result.unwrap().0;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(
        store.get(handle).unwrap().raycast(Vec3::Z, -Vec3::Z),
        old_hit,
        "worker must not mutate published data"
    );
    let next = loaded.get(handle).unwrap();
    assert!(!Arc::ptr_eq(old_index, next.mesh_index.as_ref().unwrap()));
    assert!(next.raycast(Vec3::Z, -Vec3::Z).is_none());
    assert!(next.raycast(Vec3::new(5., 0., 1.), -Vec3::Z).is_some());
    assert_eq!(
        original.get(handle).unwrap().raycast(Vec3::Z, -Vec3::Z),
        old_hit,
        "Undo snapshot retains matching old index"
    );
}

#[test]
fn obj_import_generates_normals_and_flips_uv_origin() {
    let AssetData::Mesh(mesh) = import(
        AssetKind::Mesh,
        Path::new("quad.obj"),
        OBJ,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(mesh.indices.len(), 6);
    assert_eq!(mesh.vertices[0], [-0.5, -0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
    for invalid in [
        b"f 1 2 3".as_slice(),
        b"v 0 0 0\nf 1 1 1",
        b"mtllib ignored.mtl\nv 0 0 0\nf 1 1 1",
    ] {
        assert!(
            import(
                AssetKind::Mesh,
                Path::new("bad.obj"),
                invalid,
                &no_dependencies()
            )
            .is_err()
        );
    }
}

#[test]
fn jpeg_import_and_image_limits_are_enforced() {
    let encode = |image: image::DynamicImage, format| {
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, format).unwrap();
        output.into_inner()
    };
    let jpeg = encode(
        image::RgbImage::from_pixel(2, 2, image::Rgb([100, 150, 200])).into(),
        image::ImageFormat::Jpeg,
    );
    let AssetData::Image(decoded) = import(
        AssetKind::Image,
        Path::new("photo.jpg"),
        &jpeg,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((decoded.width, decoded.height), (2, 2));
    assert!(
        decoded.rgba[..3]
            .iter()
            .zip([100, 150, 200])
            .all(|(a, b)| a.abs_diff(b) <= 4)
    );
    let alpha = encode(
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 128])).into(),
        image::ImageFormat::Png,
    );
    let AssetData::Image(decoded) = import(
        AssetKind::Image,
        Path::new("alpha.png"),
        &alpha,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(decoded.rgba[3], 128);
    let wide = encode(
        image::RgbImage::new(4097, 1).into(),
        image::ImageFormat::Png,
    );
    assert!(
        import(
            AssetKind::Image,
            Path::new("wide.png"),
            &wide,
            &no_dependencies()
        )
        .is_err()
    );
}

fn triangle_bytes() -> Vec<u8> {
    [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect()
}

#[test]
fn imported_surface_labels_preserve_names_and_distinguish_primitives() {
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let mut document: serde_json::Value =
        serde_json::from_slice(&gltf_document(&uri, None)).unwrap();
    document["nodes"][0]["name"] = "Archway".into();
    document["meshes"][0]["name"] = "Stonework".into();
    document["materials"][0]["name"] = "Weathered stone".into();
    let primitive = document["meshes"][0]["primitives"][0].clone();
    document["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(primitive);
    let AssetData::Mesh(mesh) = import(
        AssetKind::Mesh,
        Path::new("names.gltf"),
        &serde_json::to_vec(&document).unwrap(),
        &no_dependencies(),
    )
    .unwrap() else {
        panic!();
    };
    assert_eq!(mesh.parts[0].name, "Archway / Stonework / Surface 1");
    assert_eq!(mesh.parts[1].name, "Archway / Stonework / Surface 2");
    assert_eq!(
        mesh.parts[0].material_name.as_deref(),
        Some("Weathered stone")
    );
    let AssetData::Mesh(unnamed) = import(
        AssetKind::Mesh,
        Path::new("names.gltf"),
        &gltf_document(&uri, None),
        &no_dependencies(),
    )
    .unwrap() else {
        panic!();
    };
    assert_eq!(unnamed.parts[0].name, "Node 0 / Mesh 0 / Surface 1");
    assert!(unnamed.parts[0].material_name.is_none());
}

#[test]
fn surface_signatures_allow_material_edits_but_reject_geometry_or_slot_changes() {
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let mut doc: serde_json::Value = serde_json::from_slice(&gltf_document(&uri, None)).unwrap();
    let key = |doc: &serde_json::Value| {
        let AssetData::Mesh(mesh) = import(
            AssetKind::Mesh,
            Path::new("keys.gltf"),
            &serde_json::to_vec(doc).unwrap(),
            &no_dependencies(),
        )
        .unwrap() else {
            panic!();
        };
        mesh.parts[0].source_key.clone()
    };
    let original = key(&doc);
    assert_eq!(original.len(), 16);
    doc["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] = 0.2.into();
    doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] =
        serde_json::json!([0.1, 0.2, 0.3, 1.0]);
    assert_eq!(key(&doc), original);
    doc["nodes"][0]["translation"] = serde_json::json!([3.0, 0.0, 0.0]);
    assert_ne!(key(&doc), original);
    doc["nodes"][0]["translation"] = serde_json::json!([2.0, 0.0, 0.0]);
    let same = doc["materials"][0].clone();
    doc["materials"].as_array_mut().unwrap().push(same);
    doc["meshes"][0]["primitives"][0]["material"] = 1.into();
    assert_ne!(
        key(&doc),
        original,
        "coincident geometry cannot inherit a different material slot's override"
    );
}

fn gltf_document(buffer_uri: &str, image_uri: Option<&str>) -> Vec<u8> {
    let mut pbr = serde_json::json!({ "baseColorFactor": [0.25, 0.5, 0.75, 0.5] });
    if image_uri.is_some() {
        pbr["baseColorTexture"] = serde_json::json!({ "index": 0 });
    }
    let mut document = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": 36, "uri": buffer_uri }],
        "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }],
        "materials": [{ "pbrMetallicRoughness": pbr, "alphaMode": "MASK", "alphaCutoff": 0.3 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
        "nodes": [{ "mesh": 0, "translation": [2.0, 0.0, 0.0], "scale": [-1.0, 1.0, 1.0] }],
        "scenes": [{ "nodes": [0] }], "scene": 0
    });
    if let Some(uri) = image_uri {
        document["bufferViews"][0]["byteStride"] = serde_json::json!(12);
        document["accessors"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"bufferView":0,"componentType":5126,"count":3,"type":"VEC2"}));
        document["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = serde_json::json!(1);
        document["images"] = serde_json::json!([{ "uri": uri }]);
        document["textures"] = serde_json::json!([{ "source": 0 }]);
    }
    serde_json::to_vec(&document).unwrap()
}

#[test]
fn gltf_data_uri_bakes_transforms_materials_textures_and_winding() {
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([4, 5, 6, 128]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)
    .unwrap();
    let buffer_uri = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let image_uri = format!(
        "data:image/png;base64,{}",
        STANDARD.encode(encoded.into_inner())
    );
    let bytes = gltf_document(&buffer_uri, Some(&image_uri));
    let AssetData::Mesh(mesh) = import(
        AssetKind::Mesh,
        Path::new("model.gltf"),
        &bytes,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(mesh.vertices[0][0], 2.0);
    assert_eq!(mesh.indices, vec![0, 2, 1]);
    assert_eq!(
        mesh.vertices[0][5], 1.0,
        "mirrored winding and generated normals agree"
    );
    assert_eq!(mesh.parts[0].color, [0.25, 0.5, 0.75, 0.5]);
    assert_eq!(mesh.parts[0].alpha_cutoff, Some(0.3));
    assert_eq!(
        mesh.parts[0].image.as_ref().unwrap().rgba,
        vec![4, 5, 6, 128]
    );
}

#[test]
fn gltf_external_dependencies_reload_and_portabilize() {
    let dir = Temp::new();
    std::fs::write(dir.0.join("model.gltf"), gltf_document("mesh.bin", None)).unwrap();
    std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
    let sources = BTreeMap::from([(
        "model".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "model.gltf".into(),
        },
    )]);
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    let handle = store.handle("model").unwrap();
    assert_eq!(store.refresh(), vec![handle]);
    store.require_ready().unwrap();
    assert_eq!(store.get(handle).unwrap().revision(), 1);
    std::fs::write(dir.0.join("mesh.bin"), b"short").unwrap();
    assert_eq!(store.refresh(), vec![handle]);
    assert!(matches!(
        store.get(handle).unwrap().state(),
        LoadState::Failed(_)
    ));
    assert_eq!(store.get(handle).unwrap().revision(), 1);
    std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
    assert_eq!(store.refresh(), vec![handle]);
    assert_eq!(store.get(handle).unwrap().revision(), 2);
    let portable = portable_gltf(&dir.0.join("model.gltf")).unwrap();
    assert!(
        std::str::from_utf8(&portable)
            .unwrap()
            .contains("data:application/octet-stream;base64,")
    );
    assert!(
        import(
            AssetKind::Mesh,
            Path::new("portable.gltf"),
            &portable,
            &no_dependencies()
        )
        .is_ok()
    );
}
#[test]
fn gltf_surfaces_share_decoded_images_without_merging_alpha_variants() {
    let buffer = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
    let mut json: serde_json::Value =
        serde_json::from_slice(&gltf_document(&buffer, Some(&texture))).unwrap();
    let primitive = json["meshes"][0]["primitives"][0].clone();
    json["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(primitive.clone());
    let mut opaque = json["materials"][0].clone();
    opaque["alphaMode"] = serde_json::json!("OPAQUE");
    json["materials"][0]["alphaMode"] = serde_json::json!("BLEND");
    json["materials"].as_array_mut().unwrap().push(opaque);
    let mut third = primitive;
    third["material"] = serde_json::json!(1);
    json["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(third);
    let bytes = serde_json::to_vec(&json).unwrap();
    let AssetData::Mesh(mesh) = import(
        AssetKind::Mesh,
        Path::new("shared.gltf"),
        &bytes,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert!(Arc::ptr_eq(
        mesh.parts[0].image.as_ref().unwrap(),
        mesh.parts[1].image.as_ref().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        mesh.parts[0].image.as_ref().unwrap(),
        mesh.parts[2].image.as_ref().unwrap()
    ));
}

#[test]
fn gltf_emission_strength_survives_portable_and_cooked_roundtrips() {
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let base: serde_json::Value = serde_json::from_slice(&gltf_document(&uri, None)).unwrap();
    let load = |json: &serde_json::Value| {
        import(
            AssetKind::Mesh,
            Path::new("emission.gltf"),
            &serde_json::to_vec(json).unwrap(),
            &no_dependencies(),
        )
    };
    for strength in [None, Some(0.), Some(6.)] {
        for required in [false, true] {
            let mut json = base.clone();
            json["materials"][0]["emissiveFactor"] = serde_json::json!([1., 0.25, 0.5]);
            if let Some(strength) = strength {
                json["extensionsUsed"] = serde_json::json!(["KHR_materials_emissive_strength"]);
                json["materials"][0]["extensions"]["KHR_materials_emissive_strength"] =
                    serde_json::json!({"emissiveStrength":strength});
                if required {
                    json["extensionsRequired"] = json["extensionsUsed"].clone();
                }
            }
            let AssetData::Mesh(mesh) = load(&json).unwrap() else {
                panic!()
            };
            let expected = [1., 0.25, 0.5].map(|v| v * strength.unwrap_or(1.));
            assert_eq!(
                mesh.parts[0]
                    .shading
                    .as_ref()
                    .unwrap()
                    .material
                    .emissive_factor,
                expected
            );
            let portable = mesh_gltf(&mesh, &job::Progress::default()).unwrap();
            let AssetData::Mesh(restored) =
                load(&serde_json::from_slice(&portable).unwrap()).unwrap()
            else {
                panic!()
            };
            assert_eq!(
                restored.parts[0]
                    .shading
                    .as_ref()
                    .unwrap()
                    .material
                    .emissive_factor,
                expected
            );
            let cooked = cooked_model::encode(&mesh, &[], &job::Progress::default()).unwrap();
            let restored = cooked_model::decode(&cooked).unwrap();
            assert_eq!(
                restored.parts[0]
                    .shading
                    .as_ref()
                    .unwrap()
                    .material
                    .emissive_factor,
                expected
            );
        }
    }
    for invalid in [-1., 1e39] {
        let mut json = base.clone();
        json["materials"][0]["extensions"]["KHR_materials_emissive_strength"] =
            serde_json::json!({"emissiveStrength":invalid});
        assert!(load(&json).is_err());
    }
    let mut json = base;
    json["extensionsRequired"] = serde_json::json!(["KHR_materials_unlit"]);
    assert!(
        load(&json).is_err(),
        "unrelated required extensions must remain rejected"
    );
}

#[test]
fn gltf_pbr_maps_share_images_preserve_uvs_samplers_and_tangents() {
    let mut buffer = triangle_bytes();
    buffer.extend(
        [[0.0_f32, 0.0], [0.0, 1.0], [1.0, 0.0]]
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes),
    );
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(&buffer)
    );
    let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
    let mut json: serde_json::Value =
        serde_json::from_slice(&gltf_document(&uri, Some(&texture))).unwrap();
    json["buffers"][0]["byteLength"] = serde_json::json!(buffer.len());
    json["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"buffer":0,"byteOffset":36,"byteLength":24}));
    json["accessors"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}));
    json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = serde_json::json!(2);
    json["samplers"] = serde_json::json!([
        {"wrapS":33071,"wrapT":33648,"magFilter":9728,"minFilter":9729},
        {"minFilter":9986}
    ]);
    json["textures"] = serde_json::json!([{"source":0,"sampler":0},{"source":0,"sampler":1}]);
    let material = &mut json["materials"][0];
    material["pbrMetallicRoughness"]["metallicRoughnessTexture"] = serde_json::json!({"index":1});
    material["pbrMetallicRoughness"]["metallicFactor"] = serde_json::json!(0.3);
    material["pbrMetallicRoughness"]["roughnessFactor"] = serde_json::json!(0.7);
    material["normalTexture"] = serde_json::json!({"index":0,"texCoord":1,"scale":0.6});
    material["occlusionTexture"] = serde_json::json!({"index":1,"strength":0.4});
    material["emissiveTexture"] = serde_json::json!({"index":0});
    material["emissiveFactor"] = serde_json::json!([0.1, 0.2, 0.3]);
    material["doubleSided"] = serde_json::json!(true);
    let load = |json: &serde_json::Value| {
        import(
            AssetKind::Mesh,
            Path::new("pbr.gltf"),
            &serde_json::to_vec(json).unwrap(),
            &no_dependencies(),
        )
    };
    let AssetData::Mesh(mesh) = load(&json).unwrap() else {
        panic!()
    };
    let part = &mesh.parts[0];
    let shading = part.shading.as_ref().unwrap();
    let m = &shading.material;
    assert_eq!(
        (
            m.metallic,
            m.roughness,
            m.normal_scale,
            m.occlusion_strength
        ),
        (0.3, 0.7, 0.6, 0.4)
    );
    assert_eq!(m.emissive_factor, [0.1, 0.2, 0.3]);
    assert!(m.double_sided);
    for map in [&m.normal, &m.metallic_roughness, &m.occlusion, &m.emissive]
        .into_iter()
        .flatten()
    {
        assert!(Arc::ptr_eq(part.image.as_ref().unwrap(), &map.image));
    }
    assert_eq!(
        m.base_color_sampler,
        Sampler {
            wrap_u: Wrap::Clamp,
            wrap_v: Wrap::Mirror,
            mag: Filter::Nearest,
            min: Filter::Linear,
            mip: None
        }
    );
    assert_eq!(
        m.metallic_roughness.as_ref().unwrap().sampler.min,
        Filter::Nearest
    );
    assert_eq!(
        m.metallic_roughness.as_ref().unwrap().sampler.mip,
        Some(Filter::Linear)
    );
    assert_eq!(&shading.vertices[1][4..6], &[0., 1.]);
    assert_eq!(&shading.vertices[1][6..8], &[1., 0.]);
    // Normal UV1 swaps UV axes; mirrored node flips tangent handedness back.
    assert_eq!(&shading.vertices[0][..4], &[0., 1., 0., 1.]);
    // Generated LODs use this same portable material path. Verify all maps,
    // alternate UV channels, alpha policy and samplers survive re-import.
    let portable = mesh_gltf(&mesh, &job::Progress::default()).unwrap();
    let portable: serde_json::Value = serde_json::from_slice(&portable).unwrap();
    assert_eq!(portable["images"].as_array().unwrap().len(), 1);
    let AssetData::Mesh(roundtrip) = load(&portable).unwrap() else {
        panic!()
    };
    let rematerial = &roundtrip.parts[0].shading.as_ref().unwrap().material;
    assert_eq!(roundtrip.indices.len(), mesh.indices.len());
    for (&before, &after) in mesh.indices.iter().zip(&roundtrip.indices) {
        assert_eq!(
            roundtrip.vertices[after as usize],
            mesh.vertices[before as usize]
        );
        assert_eq!(
            roundtrip.parts[0].shading.as_ref().unwrap().vertices[after as usize],
            shading.vertices[before as usize]
        );
    }
    assert_eq!(roundtrip.parts[0].color, part.color);
    assert_eq!(roundtrip.parts[0].alpha_cutoff, part.alpha_cutoff);
    assert_eq!(rematerial.metallic, m.metallic);
    assert_eq!(rematerial.roughness, m.roughness);
    assert_eq!(rematerial.normal_scale, m.normal_scale);
    assert_eq!(rematerial.occlusion_strength, m.occlusion_strength);
    assert_eq!(rematerial.double_sided, m.double_sided);
    assert_eq!(rematerial.emissive_factor, m.emissive_factor);
    assert_eq!(rematerial.base_color_sampler, m.base_color_sampler);
    for (before, after) in [&m.normal, &m.metallic_roughness, &m.occlusion, &m.emissive]
        .into_iter()
        .zip([
            &rematerial.normal,
            &rematerial.metallic_roughness,
            &rematerial.occlusion,
            &rematerial.emissive,
        ])
    {
        let (before, after) = (before.as_ref().unwrap(), after.as_ref().unwrap());
        assert_eq!(before.sampler, after.sampler);
        assert_eq!(before.image.rgba, after.image.rgba);
    }
    let mut missing_uv = json.clone();
    missing_uv["materials"][0]["emissiveTexture"]["texCoord"] = serde_json::json!(3);
    assert!(
        load(&missing_uv)
            .unwrap_err()
            .to_string()
            .contains("TEXCOORD_3")
    );

    let tangent_offset = buffer.len();
    buffer.extend(
        [[1.0_f32, 0.0, 0.0, 1.0]; 3]
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes),
    );
    json["buffers"][0]["byteLength"] = serde_json::json!(buffer.len());
    json["buffers"][0]["uri"] = serde_json::json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(&buffer)
    ));
    json["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"buffer":0,"byteOffset":tangent_offset,"byteLength":48}));
    json["accessors"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"}));
    json["meshes"][0]["primitives"][0]["attributes"]["TANGENT"] = serde_json::json!(3);
    let AssetData::Mesh(mesh) = load(&json).unwrap() else {
        panic!()
    };
    assert_eq!(
        &mesh.parts[0].shading.as_ref().unwrap().vertices[0][..4],
        &[-1., 0., 0., -1.]
    );
    for i in 0..3 {
        let offset = tangent_offset + i * 16;
        buffer[offset..offset + 16].copy_from_slice(
            &[0.0_f32, 0., 1., 1.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    json["buffers"][0]["uri"] = serde_json::json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(&buffer)
    ));
    let AssetData::Mesh(mesh) = load(&json).unwrap() else {
        panic!()
    };
    assert_eq!(
        &mesh.parts[0].shading.as_ref().unwrap().vertices[0][..4],
        &[0., 1., 0., 1.]
    );
    assert!(
        mesh.warnings
            .iter()
            .any(|w| w.contains("Repaired 3 degenerate"))
    );
}

#[test]
fn gltf_uses_requested_uv_set_and_accepts_small_nonzero_scale() {
    let buffer = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(triangle_bytes())
    );
    let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
    let mut json: serde_json::Value =
        serde_json::from_slice(&gltf_document(&buffer, Some(&texture))).unwrap();
    json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"] =
        serde_json::json!(1);
    json["meshes"][0]["primitives"][0]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("TEXCOORD_0");
    json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = serde_json::json!(1);
    json["nodes"][0]["scale"] = serde_json::json!([0.001, 0.001, 0.001]);
    let bytes = serde_json::to_vec(&json).unwrap();
    let AssetData::Mesh(mesh) = import(
        AssetKind::Mesh,
        Path::new("small.gltf"),
        &bytes,
        &no_dependencies(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(&mesh.vertices[1][6..], &[1.0, 0.0]);
    let mut cyclic = json.clone();
    cyclic["nodes"][0]["children"] = serde_json::json!([0]);
    assert!(
        import(
            AssetKind::Mesh,
            Path::new("cycle.gltf"),
            &serde_json::to_vec(&cyclic).unwrap(),
            &no_dependencies()
        )
        .is_err()
    );

    for (key, value) in [
        ("animations", serde_json::json!([{}])),
        ("skins", serde_json::json!([{}])),
        (
            "extensionsRequired",
            serde_json::json!(["KHR_texture_transform"]),
        ),
    ] {
        let mut invalid = json.clone();
        invalid[key] = value;
        assert!(
            import(
                AssetKind::Mesh,
                Path::new("invalid.gltf"),
                &serde_json::to_vec(&invalid).unwrap(),
                &no_dependencies()
            )
            .is_err()
        );
    }
}
#[test]
fn model_texture_reload_preserves_last_good_and_recovers() {
    let dir = Temp::new();
    std::fs::write(
        dir.0.join("model.gltf"),
        gltf_document("mesh.bin", Some("paint.png")),
    )
    .unwrap();
    std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
    std::fs::write(dir.0.join("paint.png"), PNG).unwrap();
    let sources = BTreeMap::from([(
        "model".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "model.gltf".into(),
        },
    )]);
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    store.refresh();
    store.require_ready().unwrap();
    let handle = store.handle("model").unwrap();
    std::fs::write(dir.0.join("paint.png"), b"broken").unwrap();
    assert_eq!(store.refresh(), vec![handle]);
    assert_eq!(store.get(handle).unwrap().revision(), 1);
    assert!(store.get(handle).unwrap().data().is_some());
    assert!(store.require_ready().is_err());
    std::fs::write(dir.0.join("paint.png"), NEXT_PNG).unwrap();
    assert_eq!(store.refresh(), vec![handle]);
    store.require_ready().unwrap();
    assert_eq!(store.get(handle).unwrap().revision(), 2);
}
#[test]
fn canonical_file_aliases_share_exact_snapshots_and_keep_content_keys_distinct() {
    // These are literal filename characters, not URI suffixes. Windows
    // reserves '?', so exercise that spelling only where it is valid.
    let variant_paths = [
        "palette#variant=other.png",
        #[cfg(unix)]
        "palette?variant=other.png",
    ];
    for variant_path in variant_paths {
        let dir = Temp::new();
        std::fs::write(dir.0.join("palette.png"), PNG).unwrap();
        std::fs::write(dir.0.join(variant_path), NEXT_PNG).unwrap();
        let sources = [
            ("a", "palette.png"),
            ("b", "./palette.png"),
            ("c", variant_path),
        ]
        .into_iter()
        .map(|(id, path)| {
            (
                id.into(),
                AssetSource {
                    kind: AssetKind::Image,
                    path: path.into(),
                },
            )
        })
        .collect();
        let mut store = AssetStore::new(&dir.0, &sources).unwrap();
        store.load_pending().unwrap();
        let data = |id| {
            store
                .get(store.handle(id).unwrap())
                .unwrap()
                .shared_data()
                .unwrap()
        };
        assert!(Arc::ptr_eq(&data("a"), &data("b")));
        assert!(!Arc::ptr_eq(&data("a"), &data("c")));
        assert_eq!(store.canonical_asset_id("b"), "a");
        assert_eq!(store.canonical_asset_id("c"), "c");
        let frozen = data("a");
        let old_publication = store.publication_identity().clone();
        std::fs::write(dir.0.join("palette.png"), NEXT_PNG).unwrap();
        assert_eq!(store.refresh().len(), 2);
        let a = store
            .get(store.handle("a").unwrap())
            .unwrap()
            .shared_data()
            .unwrap();
        let b = store
            .get(store.handle("b").unwrap())
            .unwrap()
            .shared_data()
            .unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &frozen));
        assert!(!Arc::ptr_eq(&old_publication, store.publication_identity()));
    }
}

#[cfg(unix)]
#[test]
fn canonical_primary_aliases_keep_authored_relative_dependencies_distinct() {
    let dir = Temp::new();
    for name in ["target", "alias"] {
        std::fs::create_dir(dir.0.join(name)).unwrap();
        std::fs::write(dir.0.join(name).join("mesh.bin"), triangle_bytes()).unwrap();
    }
    std::fs::write(
        dir.0.join("target/mesh.gltf"),
        gltf_document("mesh.bin", Some("paint.png")),
    )
    .unwrap();
    std::fs::write(dir.0.join("target/paint.png"), PNG).unwrap();
    std::fs::write(dir.0.join("alias/paint.png"), NEXT_PNG).unwrap();
    std::os::unix::fs::symlink("../target/mesh.gltf", dir.0.join("alias/mesh.gltf")).unwrap();
    let sources = [("a", "target/mesh.gltf"), ("b", "alias/mesh.gltf")]
        .into_iter()
        .map(|(id, path)| {
            (
                id.into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: path.into(),
                },
            )
        })
        .collect();
    let mut store = AssetStore::new(&dir.0, &sources).unwrap();
    store.load_pending().unwrap();
    let data = |id| {
        store
            .get(store.handle(id).unwrap())
            .unwrap()
            .shared_data()
            .unwrap()
    };
    let a = data("a");
    let b = data("b");
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(store.canonical_asset_id("a"), "a");
    assert_eq!(store.canonical_asset_id("b"), "b");
    let image = |data: &AssetData| {
        let AssetData::Mesh(mesh) = data else {
            panic!()
        };
        mesh.parts[0].image.as_ref().unwrap().rgba.clone()
    };
    assert_ne!(image(&a), image(&b));
    let mut relocated = store
        .for_catalog(
            &dir.0,
            &BTreeMap::from([(
                "a".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: "alias/mesh.gltf".into(),
                },
            )]),
        )
        .unwrap();
    relocated.load_pending().unwrap();
    assert_eq!(
        image(&b),
        image(
            &relocated
                .get(relocated.handle("a").unwrap())
                .unwrap()
                .shared_data()
                .unwrap()
        )
    );
    std::fs::write(dir.0.join("alias/paint.png"), PNG).unwrap();
    assert_eq!(store.refresh(), vec![store.handle("b").unwrap()]);
    assert!(Arc::ptr_eq(
        &a,
        &store
            .get(store.handle("a").unwrap())
            .unwrap()
            .shared_data()
            .unwrap()
    ));
    assert_eq!(
        image(&a),
        image(
            &store
                .get(store.handle("b").unwrap())
                .unwrap()
                .shared_data()
                .unwrap()
        )
    );
    assert_ne!(image(&a), image(&b), "frozen alias snapshot changed");
}
