//! Visual regression for the factory's live lighting and background-only stars.
use bozzard_editor::Editor;
use bozzard_render::{Frame, Gpu, SceneRenderer, wgpu};
use bozzard_scene::{
    Layer,
    blueprint::{BlackboardValue, Value},
};

fn terrain_brightness(frame: &Frame) -> f32 {
    let mut sum = 0.;
    for y in 350..475 {
        for x in 450..830 {
            let i = ((y * frame.width + x) * 4) as usize;
            sum += frame.rgba[i..i + 3].iter().map(|v| *v as f32).sum::<f32>() / 3.;
        }
    }
    sum / (125. * 380.)
}

#[test]
#[ignore = "requires a native graphics adapter; writes day/night previews"]
fn night_is_dark_with_stars_behind_terrain_and_readable_hud() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?;
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let size = [1280, 800];
    let mut day_brightness = 0.;
    for (label, elapsed, clock) in [
        ("day", 3600., "12:00:"),
        ("dusk", 9000., "18:00:"),
        ("night", 14400., "00:00:"),
    ] {
        let mut editor = Editor::open(&path)?;
        let mut scene = editor.scene().clone();
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == "controller")
            .unwrap()
            .blackboard
            .insert(
                "title_open".into(),
                BlackboardValue::Scalar(Value::Bool(false)),
            );
        scene
            .blackboard
            .insert("seed".into(), BlackboardValue::Scalar(Value::Number(1.)));
        editor.apply("Reproducible lighting preview", scene)?;
        editor.assets.require_ready()?;
        editor.start_play()?;
        let script = format!(
            "{}\nfn on_start(me) {{ normal_start(me);data::session_set(120,{elapsed:.1}); }}",
            source.replace("fn on_start(me)", "fn normal_start(me)")
        );
        let play = editor.play.as_mut().unwrap();
        play.with_instance(|instance, _| instance.register_script("earth-factory".into(), script))?;
        for _ in 0..24 {
            play.app.step();
            play.check_simulation()?;
        }
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
            }
        }
        let mut render = editor.render(Layer::ThreeD, 1.6)?;
        let world = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        let stats = renderer.frame_stats();
        if label == "day" {
            day_brightness = terrain_brightness(&world);
            assert_eq!(render.environment.star_intensity, 0.);
        }
        if label == "night" {
            let brightness = terrain_brightness(&world);
            assert!(
                brightness < day_brightness * 0.5,
                "night {brightness} vs day {day_brightness}"
            );
            assert!(
                brightness > day_brightness * 0.04,
                "terrain should remain navigable"
            );
            let stars = render.environment.star_intensity;
            assert!(stars > 0.);
            render.environment.star_intensity = 0.;
            let no_stars = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                renderer.draw(&gpu, target, size, &render)
            })?;
            let changed = world
                .rgba
                .chunks_exact(4)
                .zip(no_stars.rgba.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert!(
                changed > 80 && changed < 15000,
                "expected sparse, visible stars: {changed} pixels"
            );
            for y in 350..475 {
                for x in 450..830 {
                    let i = ((y * size[0] + x) * 4) as usize;
                    assert_eq!(
                        &world.rgba[i..i + 4],
                        &no_stars.rgba[i..i + 4],
                        "stars must stay behind terrain"
                    );
                }
            }
            assert_eq!(renderer.frame_stats().color_draws, stats.color_draws);
            render.environment.star_intensity = stars;
        }
        let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
        assert_eq!(
            ui.element("world-status").unwrap().text,
            if label == "day" {
                "STELLAR-BX / DAY"
            } else {
                "STELLAR-BX / NIGHT"
            }
        );
        assert!(ui.element("world-clock").unwrap().text.starts_with(clock));
        for viewport in [[1280., 800.], [1080., 600.], [640., 480.]] {
            let frame = editor.ui_frame(Layer::ThreeD, viewport)?;
            let panel = frame.element("world-panel").unwrap().rect;
            let clock = frame.element("world-clock").unwrap().rect;
            let resources = frame.element("stored-iron").unwrap().rect;
            assert!(panel.contains(clock.min));
            assert!(panel.contains([clock.min[0] + clock.size[0], clock.min[1] + clock.size[1]]));
            assert!(clock.min[1] + clock.size[1] <= resources.min[1]);
            let right = [
                "world-panel",
                "power-status",
                "chunk-status",
                "menu-open",
                "map-open",
                "zoom-hint",
            ];
            for pair in right.windows(2) {
                let above = frame.element(pair[0]).unwrap().rect;
                let below = frame.element(pair[1]).unwrap().rect;
                assert!(
                    above.min[1] + above.size[1] <= below.min[1],
                    "{} overlaps {}",
                    pair[0],
                    pair[1]
                );
            }
        }
        render
            .items
            .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
        let hud = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        hud.write_ppm(&std::env::temp_dir().join(format!("earth-factory-{label}.ppm")))?;
        editor.stop_play();
        assert_eq!(
            editor
                .render(Layer::ThreeD, 1.6)?
                .environment
                .star_intensity,
            0.,
            "Stop restores authored lighting"
        );
    }
    Ok(())
}
