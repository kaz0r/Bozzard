use super::*;

impl Editor {
    /// Change parent without moving the object or its descendants in world space.
    /// Reject shear rather than silently approximating a non-TRS local transform.
    pub fn reparent(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before reparenting");
        let object = self
            .scene
            .objects
            .iter()
            .find(|o| o.id == id)
            .context("object no longer exists")?;
        let mut ancestor = parent;
        while let Some(candidate) = ancestor {
            ensure!(
                candidate != id,
                "Cannot parent an object to itself or its descendants"
            );
            ancestor = self
                .scene
                .objects
                .iter()
                .find(|o| o.id == candidate)
                .context("parent no longer exists")?
                .parent
                .as_deref();
        }
        if object.parent.as_deref() == parent {
            return Ok(());
        }
        let demo = SceneDemo::new(&self.scene)?;
        let matrices = demo.instance.global_transforms(&demo.app.world)?;
        let world = matrices[id];
        let parent_world = parent.map_or(Mat4::IDENTITY, |id| matrices[id]);
        ensure!(
            parent_world.determinant().is_finite() && parent_world.determinant() != 0.0,
            "Parent transform is not invertible"
        );
        let local = parent_world.inverse() * world;
        ensure!(
            local.is_finite() && local.determinant().is_finite() && local.determinant() != 0.0,
            "Reparenting would create an invalid transform"
        );
        let (scale, rotation, translation) = local.to_scale_rotation_translation();
        ensure!(
            scale.is_finite() && rotation.is_finite() && translation.is_finite(),
            "Reparenting would create an invalid transform"
        );
        let (y, x, z) = rotation.to_euler(glam::EulerRot::YXZ);
        let transform = Transform {
            translation: translation.to_array(),
            rotation_degrees: [x, y, z].map(f32::to_degrees),
            scale: scale.to_array(),
        };
        ensure!(
            matrix_close(local, transform.matrix())
                && matrix_close(world, parent_world * transform.matrix()),
            "Cannot preserve world transform: this parent requires unsupported shear or loses precision"
        );
        let mut scene = self.scene.clone();
        let object = scene.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.parent = parent.map(str::to_owned);
        object.transform = transform;
        self.finish_gesture();
        self.apply("Reparent object", scene)
    }
}

fn matrix_close(a: Mat4, b: Mat4) -> bool {
    // Compare each column separately so a large translation cannot hide shear.
    a.to_cols_array_2d()
        .iter()
        .zip(b.to_cols_array_2d())
        .all(|(a, b)| {
            let magnitude = a.iter().fold(1.0_f32, |m, v| m.max(v.abs()));
            a.iter()
                .zip(b)
                .all(|(a, b)| b.is_finite() && (a - b).abs() <= 1e-5 * magnitude)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let scene = Scene::from_json(r#"{"version":1,"name":"Hierarchy","views":{},"objects":[
            {"id":"parent","name":"Parent","transform":{"translation":[3,4,5],"rotation_degrees":[20,30,10],"scale":[-2,2,2]}},
            {"id":"child","name":"Child","transform":{"translation":[1,2,3],"rotation_degrees":[5,15,25],"scale":[1,2,1]}},
            {"id":"leaf","name":"Leaf","parent":"child","transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}
        ]}"#).unwrap();
        Editor::new(scene, Path::new("work/hierarchy-test.json")).unwrap()
    }
    fn worlds(editor: &Editor) -> BTreeMap<String, Mat4> {
        let demo = SceneDemo::new(editor.scene()).unwrap();
        demo.instance.global_transforms(&demo.app.world).unwrap()
    }
    #[test]
    fn reparent_preserves_subtree_and_is_one_undoable_change() {
        let mut editor = editor();
        let original = editor.scene().clone();
        let before = worlds(&editor);
        editor.reparent("child", Some("parent")).unwrap();
        for (id, world) in worlds(&editor) {
            assert!(matrix_close(before[&id], world));
        }
        assert_eq!(editor.undo_label(), Some("Reparent object"));
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &original);
        assert!(!editor.dirty());
        editor.redo().unwrap();
        editor.reparent("child", None).unwrap();
        for (id, world) in worlds(&editor) {
            assert!(matrix_close(before[&id], world));
        }
    }
    #[test]
    fn invalid_reparent_is_transactional_and_noop_keeps_history() {
        let mut editor = editor();
        let original = editor.scene().clone();
        for (id, parent) in [
            ("child", Some("child")),
            ("child", Some("leaf")),
            ("child", Some("missing")),
            ("missing", None),
        ] {
            assert!(editor.reparent(id, parent).is_err());
            assert_eq!(editor.scene(), &original);
            assert!(editor.undo_label().is_none());
        }
        editor.reparent("child", None).unwrap();
        assert!(editor.undo_label().is_none());
        editor.start_play().unwrap();
        assert!(editor.reparent("child", Some("parent")).is_err());
        editor.stop_play();
        let mut scene = editor.scene().clone();
        scene.objects[0].transform.scale = [2.0, 1.0, 3.0];
        editor.apply("Nonuniform parent", scene).unwrap();
        let original = editor.scene().clone();
        assert!(editor.reparent("child", Some("parent")).is_err());
        assert_eq!(editor.scene(), &original);
        assert_eq!(editor.undo_label(), Some("Nonuniform parent"));
    }
}
