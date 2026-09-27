//! Populated presentation fixtures, without contacting other Steam accounts.
//! Live invitation/replication coverage is tracked separately in the co-op checklist.
use anyhow::Result;
use bozzard_editor::Editor;
use bozzard_render::{ScreenText, TextMesh, text_bounds};
use bozzard_scene::{
    Layer, NetworkFrame,
    middleware::ui::{Control, Input},
};

fn fixture(view: &str) -> Result<Editor> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_start(me)", "fn original_start(me)");
    let play = editor.play.as_mut().unwrap();
    play.with_instance(|instance, _| instance.register_script("earth-factory".into(), format!(r#"{source}
        fn on_start(me) {{
            navigation::show_title(false); set_object_variable("creative",true); world::begin_world(4);
        }}"#)))?;
    for _ in 0..24 {
        play.app.step();
        play.check_simulation()?;
    }
    let names = ["Host", "Red Explorer", "Orange Explorer", "Green Explorer"];
    play.app.world.insert_resource(NetworkFrame {
        active: true,
        state: serde_json::json!({"factory_host":true,"local":1,"presentation":0,
        "changes":[],"rotations":[],"flights":[],"members":[
            {"peer":1,"slot":0,"name":names[0],"position":{"planet":0,"x":0,"z":0}},
            {"peer":2,"slot":1,"name":names[1],"position":{"planet":0,"x":-3,"z":3}},
            {"peer":3,"slot":2,"name":names[2],"position":{"planet":0,"x":3,"z":3}},
            {"peer":4,"slot":3,"name":names[3],"position":{"planet":0,"x":4,"z":0}}
        ]}),
        objects: Default::default(),
    });
    play.app.step();
    play.check_simulation()?;
    if view == "players" {
        return Ok(editor);
    }
    let mut chat = bozzard_network::chat::ChatLog::default();
    for (i, name) in names.iter().enumerate() {
        let text = format!(
            "{} {} END-{}",
            "W".repeat(80),
            "The powered factory on Earth is still producing.".repeat(3),
            i
        );
        let mut bytes = bozzard_network::chat::CHAT_PREFIX.to_vec();
        bytes.extend(text.chars().take(160).collect::<String>().as_bytes());
        assert!(chat.receive(name, &bytes));
    }
    let roster = names
        .iter()
        .map(|name| format!("{name} {}", "W".repeat(40)))
        .collect::<Vec<_>>()
        .join("\n");
    let draft = format!("> {}▏", "W".repeat(160));
    play.with_instance(|instance, world| -> Result<()> {
        for (id, control) in [
            ("coop-overlay", Control::Visible(view == "lobby")),
            ("coop-overlay", Control::Enabled(view == "lobby")),
            ("coop-chat-overlay", Control::Visible(view == "chat")),
            ("coop-chat-overlay", Control::Enabled(view == "chat")),
            (
                "coop-status",
                Control::Text(
                    "4 / 4 players · You are hosting\nLobby created. Invite friends or start now."
                        .into(),
                ),
            ),
            ("coop-members", Control::Text(roster)),
            (
                "coop-hint",
                Control::Text("Invite friends anytime. The host keeps the world and saves.".into()),
            ),
            ("coop-lobby-log", Control::Text(chat.text())),
            ("coop-chat-log", Control::Text(chat.text())),
            ("coop-lobby-draft", Control::Text(draft.clone())),
            ("coop-chat-draft", Control::Text(draft)),
        ] {
            instance.control_ui(world, id, control)?;
        }
        for i in 0..4 {
            let id = format!("coop-friend-{i}");
            instance.control_ui(world, &id, Control::Visible(true))?;
            instance.control_ui(
                world,
                &id,
                Control::Text(format!("Invite {}", "W".repeat(64))),
            )?;
        }
        Ok(())
    })?;
    Ok(editor)
}

#[test]
fn populated_coop_text_remains_reachable_at_small_and_large_sizes() -> Result<()> {
    for view in ["lobby", "chat"] {
        let mut editor = fixture(view)?;
        for size in [[900., 600.], [1280., 800.], [1920., 1080.]] {
            let ui = editor.ui_frame(Layer::ThreeD, size)?;
            let ids: &[&str] = if view == "lobby" {
                &[
                    "coop-members",
                    "coop-lobby-log",
                    "coop-lobby-draft",
                    "coop-friend-0",
                ]
            } else {
                &["coop-chat-log", "coop-chat-draft"]
            };
            for id in ids {
                let e = ui.element(id).unwrap();
                let padding = e.widget.padding.map(|p| p * e.scale);
                let bounds = text_bounds(&TextMesh {
                    text: e.text.clone(),
                    font_size: e.font_size,
                    max_width: Some(e.rect.size[0] - padding[0] - padding[2]),
                    screen: Some(ScreenText {
                        anchor: [0.; 2],
                        offset: [0.; 2],
                    }),
                    ..Default::default()
                })?
                .unwrap();
                assert!(
                    bounds[1].y - bounds[0].y <= e.rect.size[1] - padding[1] - padding[3] + 1.,
                    "{view}/{id} clips text at {size:?}: needs {}, has {}",
                    bounds[1].y - bounds[0].y,
                    e.rect.size[1] - padding[1] - padding[3]
                );
                let parent = ui
                    .scroll_ancestor(e)
                    .expect("long content must be scrollable");
                assert!(parent.scroll_max > 0.);
                let parent = parent.owner.clone();
                // This fixture owns presentation data, so route the wheel to
                // the UI runtime without refreshing it from an empty Steam lobby.
                editor
                    .play
                    .as_mut()
                    .unwrap()
                    .with_instance(|instance, world| {
                        instance.ui_input(
                            world,
                            Layer::ThreeD,
                            size,
                            Input::ScrollObject {
                                owner: parent.clone(),
                                delta: 10000.,
                            },
                        )
                    })?;
                let scrolled = editor.ui_frame(Layer::ThreeD, size)?;
                let viewport = scrolled.element(&parent).unwrap();
                assert!((viewport.scroll - viewport.scroll_max).abs() < 0.01);
                if *id != "coop-friend-0" {
                    let content = scrolled.element(id).unwrap();
                    assert!(
                        content.rect.min[1] + content.rect.size[1]
                            <= viewport.rect.min[1] + viewport.rect.size[1] + 1.
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn nearby_names_hide_for_distance_other_planets_and_open_panels() -> Result<()> {
    let mut editor = fixture("players")?;
    let labels = |editor: &Editor| -> Result<Vec<bool>> {
        let ui = editor.ui_frame(Layer::ThreeD, [900., 600.])?;
        Ok((1..4)
            .map(|slot| ui.element(&format!("coop-name-{slot}")).is_some())
            .collect())
    };
    assert_eq!(labels(&editor)?, [true, true, true]);
    {
        let frame = editor
            .play
            .as_mut()
            .unwrap()
            .app
            .world
            .resource_mut::<NetworkFrame>()
            .unwrap();
        frame.state["members"][1]["position"]["x"] = 7.into();
        frame.state["members"][1]["position"]["z"] = 7.into();
        frame.state["members"][2]["position"]["planet"] = 1.into();
    }
    editor.play.as_mut().unwrap().app.step();
    editor.play.as_ref().unwrap().check_simulation()?;
    assert_eq!(labels(&editor)?, [false, false, true]);
    editor
        .play
        .as_mut()
        .unwrap()
        .app
        .world
        .resource_mut::<NetworkFrame>()
        .unwrap()
        .state["session_panel"] = true.into();
    editor.play.as_mut().unwrap().app.step();
    editor.play.as_ref().unwrap().check_simulation()?;
    assert_eq!(labels(&editor)?, [false, false, false]);
    Ok(())
}

#[test]
#[ignore = "requires a native graphics adapter; writes populated co-op UI previews"]
fn populated_coop_and_player_labels_render() -> Result<()> {
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    for view in ["lobby", "chat", "players"] {
        let mut editor = fixture(view)?;
        if view != "players" {
            // Match native follow_text: show the newest message and typing end.
            let ui = editor
                .play
                .as_mut()
                .unwrap()
                .app
                .world
                .resource_mut::<bozzard_scene::middleware::ui::Runtime>()
                .unwrap();
            for id in [
                "coop-lobby-log-scroll",
                "coop-lobby-draft-scroll",
                "coop-chat-log-scroll",
                "coop-chat-draft-scroll",
            ] {
                ui.widgets.entry(id.into()).or_default().scroll = 1_000_000.;
            }
        }
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
            }
        }
        for size in [[900, 600], [1280, 800], [1920, 1080]] {
            let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
            let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
            if view == "players" {
                for slot in 1..4 {
                    assert!(ui.element(&format!("coop-name-{slot}")).is_some());
                }
                assert!(ui.element("coop-name-0").is_none());
            }
            render
                .items
                .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
            bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                renderer.draw(&gpu, target, size, &render)
            })?
            .write_ppm(
                &std::env::temp_dir().join(format!("stellar-populated-{view}-{}.ppm", size[0])),
            )?;
        }
    }
    Ok(())
}
