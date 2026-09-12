use bozzard_editor::Editor;
use bozzard_scene::{Layer, Mesh, Scene, Transform};
use std::path::PathBuf;

#[test]
fn cook_independent_surface_colliders_roundtrip_undo_and_play_without_source_assets() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/model-lab.json");
    let mut editor = Editor::open(&path).unwrap();
    let children = editor.expand_model("courier-gltf").unwrap();
    let before = editor.scene().clone();
    let mut scene = before.clone();
    let matrices = scene.global_transforms().unwrap();
    let mut count = 0;
    for child in scene
        .objects
        .iter_mut()
        .filter(|o| children.contains(&o.id))
    {
        let drawable = child.drawable.as_ref().unwrap();
        let collider = editor.assets.cook_mesh_collider(drawable).unwrap();
        let Mesh::Surface { asset, index, .. } = &drawable.mesh else {
            panic!()
        };
        let bozzard_assets::AssetData::Mesh(mesh) = editor
            .assets
            .get(editor.assets.handle(asset).unwrap())
            .unwrap()
            .data()
            .unwrap()
        else {
            panic!()
        };
        let part = &mesh.parts[*index as usize];
        let bounds = mesh.part_bounds(*index as usize).unwrap();
        let pivot = bounds[0] * 0.5 + bounds[1] * 0.5;
        assert_eq!(collider.mesh.triangles().len(), part.count as usize / 3);
        for (triangle, indices) in collider.mesh.triangles().iter().zip(
            mesh.indices[part.start as usize..(part.start + part.count) as usize].chunks_exact(3),
        ) {
            for (actual, index) in triangle.iter().zip(indices) {
                let p = mesh.vertices[*index as usize];
                let expected = matrices[&child.id]
                    .transform_point3([p[0] - pivot.x, p[1] - pivot.y, p[2] - pivot.z].into());
                assert!(
                    matrices[&child.id]
                        .transform_point3((*actual).into())
                        .abs_diff_eq(expected, 1e-5)
                );
            }
        }
        count += collider.mesh.triangles().len();
        child.mesh_collider = Some(collider);
    }
    assert!(count > 12);
    editor
        .apply("Add surface Mesh Colliders", scene.clone())
        .unwrap();
    assert_eq!(editor.collisions().unwrap().meshes.len(), children.len());
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &before);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &scene);
    editor.start_play().unwrap();
    for _ in 0..10 {
        editor.advance(std::time::Duration::from_secs_f32(1. / 60.));
    }
    editor.play.as_ref().unwrap().check_simulation().unwrap();
    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
    let mut detached = scene;
    detached.objects.retain(|o| children.contains(&o.id));
    for o in &mut detached.objects {
        o.parent = None;
        o.drawable = None;
        o.material = None;
        o.blueprints.clear();
    }
    detached.views.clear();
    detached.assets.clear();
    let standalone =
        bozzard_demo::SceneDemo::new(&Scene::from_json(&detached.to_json().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        standalone
            .instance()
            .collisions(&standalone.app.world)
            .unwrap()
            .meshes
            .len(),
        children.len()
    );

    let mut stale = before
        .objects
        .iter()
        .find(|o| o.id == children[0])
        .unwrap()
        .drawable
        .clone()
        .unwrap();
    if let Mesh::Surface { source, .. } = &mut stale.mesh {
        *source = "0000000000000000".into();
    }
    assert!(editor.assets.cook_mesh_collider(&stale).is_err());
}

#[test]
fn whole_model_bake_keeps_surface_transforms_and_builtin_shapes() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/model-lab.json");
    let mut editor = Editor::open(&path).unwrap();
    let mut drawable = editor
        .scene()
        .objects
        .iter()
        .find(|o| o.id == "courier-gltf")
        .unwrap()
        .drawable
        .clone()
        .unwrap();
    let Mesh::Asset(asset) = &drawable.mesh else {
        panic!()
    };
    let bozzard_assets::AssetData::Mesh(mesh) = editor
        .assets
        .get(editor.assets.handle(asset).unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    let mut delta =
        bozzard_scene::SurfaceMaterialOverride::inherited(0, mesh.parts[0].source_key.clone());
    delta.transform.translation = [30., 0., 0.];
    drawable.material_overrides = vec![delta];
    let cooked = editor.assets.cook_mesh_collider(&drawable).unwrap();
    let original = mesh.vertices[mesh.indices[0] as usize];
    assert!((cooked.mesh.triangles()[0][0][0] - original[0] - 30.).abs() < 1e-5);
    for (mesh, n) in [(Mesh::Cube, 12), (Mesh::Quad, 2)] {
        editor.create(mesh, Layer::ThreeD).unwrap();
        let object = editor.selected_object().unwrap();
        assert!(object.mesh_collider.is_none());
        assert_eq!(
            editor
                .assets
                .cook_mesh_collider(object.drawable.as_ref().unwrap())
                .unwrap()
                .mesh
                .triangles()
                .len(),
            n
        );
        assert_eq!(object.transform, Transform::default());
    }
}
