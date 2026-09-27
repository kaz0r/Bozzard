//! Native SDK payload and scene-driven editor initialization.

/// Declare before native application state, so sessions and callbacks are
/// destroyed before Steam shuts down, including on error/early-return paths.
pub struct ShutdownGuard;
impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        #[cfg(feature = "steam")]
        bozzard_network::steam::shutdown();
    }
}

/// Initialize only when this scene explicitly requests Steam multiplayer. Used both before
/// graphics startup and when publishing Play after the user opens another scene.
#[cfg(feature = "steam")]
pub fn initialize_editor(scene: &bozzard_scene::Scene) -> anyhow::Result<()> {
    initialize_with(scene, |id| {
        bozzard_network::steam::initialize_editor(id).map(|_| ())
    })
}

/// Native player startup, before creating a window or graphics device.
#[cfg(feature = "steam")]
pub fn initialize_player(scene: &bozzard_scene::Scene) -> anyhow::Result<()> {
    initialize_with(scene, |id| {
        bozzard_network::steam::initialize(id).map(|_| ())
    })
}

pub fn overlay_active() -> bool {
    #[cfg(feature = "steam")]
    {
        bozzard_network::steam::overlay_active()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

pub fn overlay_available() -> bool {
    #[cfg(feature = "steam")]
    {
        bozzard_network::steam::overlay_available()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

#[cfg(feature = "steam")]
fn initialize_with(
    scene: &bozzard_scene::Scene,
    initialize: impl FnOnce(u32) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    // Give the overlay a chance to hook graphics creation, while preserving
    // offline solo play. The optional adapter reports/retries SDK failures later.
    if crate::factory::is_factory(scene) {
        if let Some(id) = crate::factory::network::app_id(scene)? {
            let _ = initialize(id);
        }
        return Ok(());
    }
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

    #[test]
    fn factory_steam_is_optional_at_startup_and_configured_for_export() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../earth-factory/scenes/earth.json");
        let scene = Scene::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(crate::multiplayer::app_id(&scene).unwrap(), Some(480));
        for online in [false, true] {
            let mut called = false;
            initialize_with(&scene, |id| {
                called = true;
                assert_eq!(id, 480);
                anyhow::ensure!(online, "Steam is offline");
                Ok(())
            })
            .unwrap();
            assert!(called, "Steam must be attempted before graphics startup");
        }
    }
}
