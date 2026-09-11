use bozzard_assets::AssetData;
use bozzard_editor::Editor;
use bozzard_render::MeshKind;
use bozzard_scene::{Layer, Scene};
use glam::Vec3;
use std::{path::Path, time::Duration};

#[test]
fn material_gallery_imports_pbr_maps_extracts_and_plays() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/material-gallery.json");
    let mut editor = Editor::open(&path).unwrap();
    editor.assets.require_ready().unwrap();
    let scene = editor.scene().clone();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    assert_eq!(scene.assets.len(), 11);

    for entry in editor.assets.entries() {
        let Some(AssetData::Mesh(mesh)) = entry.data() else {
            panic!("{} is not an imported mesh", entry.id);
        };
        assert!(
            mesh.warnings.is_empty(),
            "{}: {:?}",
            entry.id,
            mesh.warnings
        );
        assert_eq!(mesh.parts.len(), 1);
        let part = &mesh.parts[0];
        assert_eq!(part.material_name.as_deref(), Some(entry.id.as_str()));
        let shading = part
            .shading
            .as_ref()
            .expect("must use glTF PBR, not legacy");
        assert_eq!(shading.vertices.len(), mesh.vertices.len());
        assert!(shading.vertices.iter().flatten().all(|v| v.is_finite()));
        // Both reusable solids must have outward winding, including pole triangles.
        for tri in mesh.indices.chunks_exact(3) {
            let [a, b, c] =
                [tri[0], tri[1], tri[2]].map(|i| Vec3::from_slice(&mesh.vertices[i as usize][..3]));
            assert!((b - a).cross(c - a).dot(a + b + c) > 0.);
        }
        let material = &shading.material;
        match entry.id.as_str() {
            "gold-polished" | "gold-satin" | "gold-matte" | "copper" => {
                assert_eq!(material.metallic, 1.);
                let expected = match entry.id.as_str() {
                    "gold-polished" => 0.08,
                    "gold-satin" => 0.32,
                    "gold-matte" => 0.72,
                    _ => 0.20,
                };
                assert_eq!(material.roughness, expected);
            }
            "walnut" | "marble" => {
                assert_eq!(material.metallic, 0.);
                let color = part.image.as_ref().unwrap();
                let normal = &material.normal.as_ref().unwrap().image;
                let mr = &material.metallic_roughness.as_ref().unwrap().image;
                for image in [color, normal, mr] {
                    assert_eq!((image.width, image.height), (128, 128));
                    assert!(image.rgba.chunks_exact(4).all(|p| p[3] == 255));
                }
                let pixels: Vec<_> = mr.rgba.chunks_exact(4).collect();
                assert!(pixels.iter().all(|p| p[2] == 0));
                assert!(pixels.iter().any(|p| p[1] != pixels[0][1]));
            }
            "lacquer" | "polymer" => {
                assert_eq!(material.metallic, 0.);
                assert_eq!(
                    material.roughness,
                    if entry.id == "lacquer" { 0.12 } else { 0.58 }
                );
            }
            _ => {}
        }
    }

    let rendered = editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    assert_eq!(rendered.lights.len(), 2);
    assert!(rendered.environment.intensity > 0.);
    assert!(rendered.lighting.shadows);
    assert_eq!(
        rendered.items.len(),
        scene
            .objects
            .iter()
            .filter(|o| o.drawable.is_some())
            .count()
    );
    for item in &rendered.items {
        assert!(matches!(item.mesh, MeshKind::Imported(_)));
        assert_eq!(item.material.tint, [1.; 3]);
    }
    for id in [
        "gold-polished",
        "gold-satin",
        "gold-matte",
        "copper",
        "walnut",
        "marble",
        "lacquer",
        "polymer",
    ] {
        let object = scene.objects.iter().find(|o| o.id == id).unwrap();
        let center = Vec3::from_array(object.transform.translation);
        let clip = rendered.view_projection * center.extend(1.);
        assert!(clip.w > 0.);
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() < 0.85 && ndc.y.abs() < 0.85 && (0. ..1.).contains(&ndc.z),
            "{id}: {ndc}"
        );
    }
    editor.start_play().unwrap();
    for _ in 0..120 {
        editor.advance(Duration::from_secs_f64(1. / 60.));
    }
    assert_eq!(
        editor.render(Layer::ThreeD, 16. / 9.).unwrap().items.len(),
        rendered.items.len()
    );
    editor.render(Layer::TwoD, 16. / 9.).unwrap();
    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
}
