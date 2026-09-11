use super::*;

#[test]
fn fog_player_extraction_matches_authored_settings_and_disables_2d() {
    let mut scene = bozzard_demo::scene_document().unwrap();
    scene.fog = bozzard_scene::FogSettings {
        enabled: true,
        color: [0.2, 0.3, 0.4],
        distance_density: 0.1,
        start_distance: 2.,
        height_density: 0.2,
        base_height: -3.,
        height_falloff: 0.5,
    };
    let assets = bozzard_assets::AssetStore::new(std::path::Path::new("."), &scene.assets).unwrap();
    let demo = SceneDemo::new(&scene).unwrap();
    let fog = extract(&demo, &assets, Layer::ThreeD, 1.).unwrap().fog;
    assert!(fog.enabled);
    assert_eq!(fog.color, scene.fog.color);
    assert_eq!(fog.distance_density, scene.fog.distance_density);
    assert_eq!(fog.start_distance, scene.fog.start_distance);
    assert_eq!(fog.height_density, scene.fog.height_density);
    assert_eq!(fog.base_height, scene.fog.base_height);
    assert_eq!(fog.height_falloff, scene.fog.height_falloff);
    assert!(
        !extract(&demo, &assets, Layer::TwoD, 1.)
            .unwrap()
            .fog
            .enabled
    );
}
