//! Native SDK payload and scene-driven editor initialization.

/// Initialize only when this scene explicitly requests Steam multiplayer. Used both before
/// graphics startup and when publishing Play after the user opens another scene.
#[cfg(feature = "steam")]
pub fn initialize_editor(scene: &bozzard_scene::Scene) -> anyhow::Result<()> {
    initialize_with(scene, |id| {
        bozzard_network::steam::initialize_editor(id).map(|_| ())
    })
}

#[cfg(feature = "steam")]
fn initialize_with(
    scene: &bozzard_scene::Scene,
    initialize: impl FnOnce(u32) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if let Some(id) = crate::multiplayer::app_id(scene)? {
        initialize(id)?;
    }
    Ok(())
}

/// Retain the linked SDK redistributable for source-independent exports.
#[cfg(feature = "steam")]
pub fn redistributable() -> Option<(&'static str, &'static [u8])> {
    Some((
        env!("BOZZARD_STEAM_LIBRARY"),
        include_bytes!(env!("BOZZARD_STEAM_LIBRARY_PATH")),
    ))
}

#[cfg(not(feature = "steam"))]
pub fn redistributable() -> Option<(&'static str, &'static [u8])> {
    None
}

#[cfg(all(test, feature = "steam"))]
mod tests {
    use super::*;
    use bozzard_scene::Scene;

    #[test]
    fn solo_and_blank_editor_startups_do_not_call_steam_even_when_it_is_unavailable() {
        for scene in [
            crate::scene_document().unwrap(),
            Scene::from_json(include_str!("../scenes/flap-woods.json")).unwrap(),
        ] {
            initialize_with(&scene, |_| {
                panic!("solo scenes must never initialize Steam")
            })
            .unwrap();
        }
    }

    #[test]
    fn multiplayer_startup_uses_authored_app_id_and_reports_initialization_failure() {
        let mut scene =
            Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
        for id in [480, 123456] {
            let settings = scene
                .objects
                .iter_mut()
                .find_map(|object| object.extras.get_mut("steam_multiplayer"))
                .unwrap();
            settings["app_id"] = id.into();
            let mut called = false;
            let error = initialize_with(&scene, |requested| {
                called = true;
                assert_eq!(requested, id);
                anyhow::bail!("Steam is offline")
            })
            .unwrap_err();
            assert!(called);
            assert_eq!(error.to_string(), "Steam is offline");
        }
    }
}
