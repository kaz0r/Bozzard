use anyhow::Result;
use bozzard_editor::{Editor, OpenScenes};
use bozzard_scene::{
    Layer,
    middleware::{
        animation::{self, Animator},
        registry,
    },
};
use std::path::{Path, PathBuf};

fn scene() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/animation-lab.json")
}

#[test]
fn animation_preview_changes_only_the_isolated_pose_and_can_be_cleared() -> Result<()> {
    let mut editor = Editor::open(&scene())?;
    let authored = editor.scene().clone();
    let before = editor.render(Layer::ThreeD, 1.6)?.skin_poses;
    let frozen = editor.render_frame(Layer::ThreeD, 1.6)?;
    assert_eq!(frozen.skin_poses, before);
    editor.scrub_animation_preview("movement-human", "Jump", 0.5)?;
    let sampled = editor.render(Layer::ThreeD, 1.6)?.skin_poses;
    assert_ne!(sampled, before);
    assert_eq!(editor.render_frame(Layer::ThreeD, 1.6)?.skin_poses, sampled);
    assert_eq!(frozen.skin_poses, before);
    assert_eq!(editor.scene(), &authored);
    assert!(!editor.dirty());
    assert_eq!(
        editor
            .timeline_preview_transform("foot-step")?
            .unwrap()
            .translation,
        [1.36, 0.22, 0.0]
    );
    editor.clear_timeline_preview();
    let cleared = editor.render(Layer::ThreeD, 1.6)?.skin_poses;
    assert_eq!(editor.render_frame(Layer::ThreeD, 1.6)?.skin_poses, cleared);
    assert_eq!(cleared.len(), before.len());
    // A new isolated world receives new render IDs; compare the multiset of actual poses.
    for expected in before.values() {
        assert_eq!(
            before.values().filter(|pose| *pose == expected).count(),
            cleared.values().filter(|pose| *pose == expected).count()
        );
    }
    editor.start_play()?;
    assert!(
        editor
            .scrub_animation_preview("movement-human", "Movement", 0.3)
            .is_err()
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
    Ok(())
}

#[test]
fn character_layer_edit_undo_save_and_reopen_preserve_the_controller() -> Result<()> {
    let mut editor = Editor::open(&scene())?;
    let original = editor.scene().clone();
    let mut edited = original.clone();
    let object = edited
        .objects
        .iter_mut()
        .find(|o| o.id == "movement-human")
        .unwrap();
    let mut a = registry::get::<Animator>(object)?.unwrap();
    std::sync::Arc::make_mut(&mut a.layers)[0].fade = 0.35;
    registry::set(object, &a)?;
    editor.apply("Change gesture fade", edited.clone())?;
    editor.undo()?;
    assert_eq!(editor.scene(), &original);
    editor.redo()?;
    assert_eq!(editor.scene(), &edited);
    let directory = scene().parent().unwrap().join(format!(
        "../../../work/animation-save-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory)?;
    let destination = directory.join("scene.json");
    editor.save(&destination)?;
    let reopened = Editor::open(&destination)?;
    assert_eq!(reopened.scene(), editor.scene());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn every_animation_demo_object_can_be_hidden_without_breaking_preview_targets() -> Result<()> {
    let editor = Editor::open(&scene())?;
    let mut open = OpenScenes::default();
    for object in &editor.scene().objects {
        open.set_object_visible(open.active(), &object.id, false);
        open.sync_view(&editor)?;
        open.view(&editor).render(Layer::ThreeD, 1.6)?;
        open.set_object_visible(open.active(), &object.id, true);
        open.sync_view(&editor)?;
    }
    assert!(!editor.dirty());
    Ok(())
}

#[test]
fn captured_green_station_keeps_feet_on_moving_supports_and_bends_both_knees() -> Result<()> {
    let mut editor = Editor::open(&scene())?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    let actor = play.instance().entity("grounded-human").unwrap();
    let animator = play.app.world.get::<Animator>(actor).unwrap().clone();
    let mut heights = [(f32::INFINITY, f32::NEG_INFINITY); 2];
    let mut bends = heights;
    for tick in 0..360 {
        play.app.step();
        play.check_simulation()?;
        if tick < 30 || tick % 15 != 0 {
            continue;
        }
        assert_eq!(
            play.app
                .world
                .get::<bozzard_scene::Transform>(actor)
                .unwrap()
                .translation,
            [1.2, 0.09, 0.0],
            "The demo must move the leg poses rather than carrying the whole actor"
        );
        for (index, support_id) in ["foot-step", "foot-slope"].into_iter().enumerate() {
            let chain = &animator.ik[index];
            let positions = [chain.root, chain.middle, chain.tip].map(|bone| {
                play.instance()
                    .animation_bone_transform(&play.app.world, "grounded-human", bone)
                    .map(|matrix| matrix.w_axis.truncate())
            });
            let [hip, knee, foot] = positions;
            let (hip, knee, foot) = (hip?, knee?, foot?);
            let bend = (knee - hip)
                .normalize()
                .dot((foot - knee).normalize())
                .clamp(-1., 1.)
                .acos();
            heights[index].0 = heights[index].0.min(foot.y);
            heights[index].1 = heights[index].1.max(foot.y);
            bends[index].0 = bends[index].0.min(bend);
            bends[index].1 = bends[index].1.max(bend);

            let runtime = play.app.world.resource::<animation::Runtime>().unwrap();
            let contact = &runtime.players["grounded-human"].ik[index];
            assert!(contact.planted);
            assert_eq!(contact.support.as_deref(), Some(support_id));
            let support = play
                .app
                .world
                .get::<bozzard_scene::Transform>(play.instance().entity(support_id).unwrap())
                .unwrap()
                .matrix();
            let normal = support
                .inverse()
                .transpose()
                .transform_vector3(glam::Vec3::from_array(contact.support_normal))
                .normalize();
            let animation::IkTarget::Ground { sole_height, .. } = chain.target else {
                unreachable!()
            };
            let expected = support.transform_point3(glam::Vec3::from_array(contact.support_local))
                + normal * sole_height;
            assert!(
                foot.distance(expected) < 0.03,
                "{support_id}: foot lost contact"
            );
        }
    }
    for index in 0..2 {
        assert!(
            heights[index].1 - heights[index].0 > 0.2,
            "Foot {index} must visibly rise and fall in the GIF"
        );
        assert!(
            bends[index].1 - bends[index].0 > 0.5,
            "Knee {index} must visibly bend in the GIF"
        );
    }
    Ok(())
}

#[test]
fn human_showcase_runs_all_stations_and_restores_without_resetting_poses() -> Result<()> {
    let mut editor = Editor::open(&scene())?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..780 {
        play.app.step();
        play.check_simulation()?;
    }
    let runtime = play.app.world.resource::<animation::Runtime>().unwrap();
    assert_eq!(runtime.players.len(), 4);
    for player in runtime.players.values() {
        assert!(!player.palette.is_empty());
        for joint in player.pose.iter() {
            joint.validate()?;
        }
    }
    assert!(runtime.players["retargeted-human"].layers[1].weight > 0.9);
    let reaching = play
        .app
        .world
        .get::<Animator>(play.instance().entity("interaction-human").unwrap())
        .unwrap();
    let hand = reaching
        .rig
        .nodes
        .iter()
        .position(|bone| bone.name == "RightHand")
        .unwrap();
    let contact = play
        .app
        .world
        .get::<bozzard_scene::Transform>(play.instance().entity("interaction-block").unwrap())
        .unwrap()
        .matrix()
        .transform_point3(glam::Vec3::new(-0.43, 0.33, -0.5));
    assert!(
        play.instance()
            .animation_bone_transform(&play.app.world, "interaction-human", hand)?
            .w_axis
            .truncate()
            .distance(contact)
            < 0.01
    );
    assert!(
        runtime.players["grounded-human"]
            .ik
            .iter()
            .any(|state| state.planted)
    );
    let palettes: Vec<_> = runtime
        .players
        .values()
        .map(|p| p.palette.clone())
        .collect();
    let save = play.instance().save_game_json(&play.app.world)?;
    play.with_instance(|instance, world| instance.load_game_json(world, &save))?;
    assert_eq!(
        play.app
            .world
            .resource::<animation::Runtime>()
            .unwrap()
            .players
            .values()
            .map(|p| p.palette.clone())
            .collect::<Vec<_>>(),
        palettes
    );
    Ok(())
}
