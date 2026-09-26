//! Native preview of the factory's chunk map at several viewport sizes.
use bozzard_editor::Editor;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::{
    Layer,
    blueprint::{BlackboardValue, Value},
};

#[test]
#[ignore = "requires a native graphics adapter; writes map previews"]
fn map_fits_wide_and_small_viewports() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(1.)));
    editor.apply("Map preview route", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_start(me)", "fn factory_start(me)");
    let source = format!(
        r#"{source}
        fn on_start(me) {{
            factory_start(me);
            for x in -2..4 {{ for z in -2..2 {{ discover_chunk(x, z); }} }}
            for x in 3..8 {{ discover_chunk(x, 1); }}
            enter_chunk(4, 1);
            set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
            set_map(true);
        }}
    "#
    );
    let play = editor.play.as_mut().unwrap();
    play.with_instance(|instance, _| instance.register_script("earth-factory".into(), source))?;
    play.app.step();
    play.check_simulation()?;
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
        }
    }
    for size in [[1280, 800], [900, 700], [1100, 450]] {
        let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
        let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
        let panel = ui.element("map-panel").unwrap().rect;
        assert!(panel.min[0] >= 0. && panel.min[1] >= 0.);
        assert!(panel.min[0] + panel.size[0] <= size[0] as f32);
        assert!(panel.min[1] + panel.size[1] <= size[1] as f32);
        for id in [
            "map-cell-0",
            "map-cell-288",
            "map-counts",
            "map-close",
            "map-footer",
        ] {
            let element = ui.element(id).unwrap().rect;
            assert!(element.min[0] >= panel.min[0] && element.min[1] >= panel.min[1]);
            assert!(element.min[0] + element.size[0] <= panel.min[0] + panel.size[0]);
            assert!(element.min[1] + element.size[1] <= panel.min[1] + panel.size[1]);
        }
        assert_eq!(ui.element("map-region").unwrap().text, "Region 4, 1");
        render
            .items
            .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
        let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        capture
            .write_ppm(&std::env::temp_dir().join(format!("earth-factory-map-{}.ppm", size[0])))?;
    }
    editor.stop_play();
    Ok(())
}
