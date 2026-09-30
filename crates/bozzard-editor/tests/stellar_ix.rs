//! Native previews and ground-illumination checks for the Stellar-IX game view.
use bozzard_editor::Editor;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::Layer;

#[test]
#[ignore = "requires a native graphics adapter; writes Stellar-IX previews"]
fn title_assembler_journal_and_powered_night_render() -> anyhow::Result<()> {
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?;
    let showroom = std::fs::read_to_string(
        path.parent()
            .unwrap()
            .join("scripts/foundation_showroom.rhai"),
    )?;
    let showroom = format!(
        "fn foundation_setup(me){}",
        showroom.split_once("fn on_start(me)").unwrap().1
    );
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    for label in [
        "foundation-outside",
        "foundation-window",
        "foundation-inside",
        "foundation-door",
        "title",
        "coop",
        "journal",
        "assembler",
        "night",
        "inventory",
        "constructor",
        "rocket",
        "half",
        "site",
        "ship",
        "unlocks",
        "ship-journal",
        "tooltip",
        "resources",
        "moon",
        "moon-map",
        "moon-rocket",
        "moon-deposit",
        "placement",
        "power-context",
        "inspect",
        "backpack-menu",
        "takeoff",
        "landing",
        "cross-chunk",
        "saves",
        "expansion-manufacturer",
        "expansion-refinery",
        "dev-world",
        "materials-belts",
    ] {
        if std::env::var("BOZZARD_PREVIEW_LABEL")
            .is_ok_and(|only| only.split(',').all(|candidate| candidate != label))
        {
            continue;
        }
        let mut editor = Editor::open(&path)?;
        editor.assets.require_ready()?;
        editor.start_play()?;
        if label != "title" && label != "coop" {
            let source = source
                .replace("fn on_start(me)", "fn original_start(me)")
                .replace(
                    "sin(data::session_value(120) * 0.10)",
                    if [
                        "half",
                        "site",
                        "ship",
                        "resources",
                        "dev-world",
                        "materials-belts",
                        "expansion-manufacturer",
                        "expansion-refinery",
                    ]
                    .contains(&label)
                        || label.starts_with("foundation-")
                    {
                        "1.0"
                    } else {
                        "-1.0"
                    },
                );
            let source = format!("{source}\n{showroom}");
            let script = format!(
                r#"{source}
            fn on_start(me) {{
                if "{label}"=="materials-belts" {{
                    navigation::show_title(false);set_object_variable("creative",true);world::begin_world(4,true);
                    world::enter_chunk(0,1);set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",0.0);
                    set_position("camera-rig",[0.0,0.0,15.0]);set_object_variable("camera_pan_progress",1.0);
                    set_camera_size("camera",9.0);set_object_variable("camera_zoom",9.0);set_object_variable("camera_zoom_target",9.0);
                    set_ui_visible("game-hud",false);return;
                }}
                if "{label}"=="dev-world" {{
                    navigation::show_title(false);set_object_variable("creative",true);world::begin_world(4,true);return;
                }}
                if "{label}".starts_with("expansion-") {{
                    navigation::show_title(false);set_object_variable("creative",true);world::begin_world(4);
                    let kind=if "{label}"=="expansion-manufacturer" {{24.0}}else{{17.0}};
                    for row in [[157,9.0],[130,kind]] {{
                        let nodes=get_scene_list("nodes");nodes[row[0]]=0.0;set_scene_list("nodes",nodes);
                        set_scene_variable("cursor_x",grid::cell_x(row[0]).to_float());set_scene_variable("cursor_z",grid::cell_z(row[0]).to_float());
                        set_scene_variable("selected",row[1]);building::place_selected();
                    }}
                    power::connect_power(power::power_id(112),power::power_id(157));power::connect_power(power::power_id(157),power::power_id(130));power::update_power();
                    if kind==24.0 {{machines::select_recipe(130,49.0);}}
                    machines::feed_assembler(130);panels::set_assembler(130);set_object_variable("bar",if kind==24.0 {{5.0}}else{{4.0}});return;
                }}
                if "{label}".starts_with("moon") {{
                    navigation::show_title(false); set_object_variable("creative",true); world::begin_world(4);
                    set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);
                    world::travel_to_other_planet();
                    for x in -2..3 {{ for z in -2..3 {{ chunks::discover_chunk(x,z); }} }}
                    if "{label}" == "moon-map" {{ panels::set_map(true); }}
                    if "{label}" == "moon-rocket" {{ panels::set_extra_panel("rocket"); }}
                    if "{label}" == "moon-deposit" {{
                        let site=deposits::moon_techtorium_sites(4)[0]; world::enter_chunk(site[0],site[1]);
                        set_position("camera-rig",[site[0].to_float()*15.0,0.0,site[1].to_float()*15.0]);
                        set_object_variable("camera_pan_progress",1.0);
                        set_ui_visible("game-hud",false);
                    }}
                    return;
                }}
                if "{label}" == "cross-chunk" {{
                    navigation::show_title(false); set_object_variable("creative",true); world::begin_world(4);
                    for row in [[6,1],[7,2],[8,2],[9,3],[10,2],[11,4]] {{
                        let cx=if row[0]>7 {{ 1 }} else {{ 0 }}; world::enter_chunk(cx,0);
                        let x=row[0]-cx*15; let cell=grid::index(x,0);
                        let nodes=get_scene_list("nodes"); nodes[cell]=if row[1]==1 {{ 1.0 }} else {{ 0.0 }}; set_scene_list("nodes",nodes);
                        grid::cache_put("chunk_nodes",grid::current_chunk(),grid::pack_numbers(nodes));
                        set_scene_variable("cursor_x",x.to_float()); set_scene_variable("cursor_z",0.0);
                        set_scene_variable("direction",0.0); set_scene_variable("selected",row[1].to_float()); building::place_selected();
                    }}
                    world::enter_chunk(0,0);
                    let nodes=get_scene_list("nodes"); nodes[157]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",3.0); set_scene_variable("selected",9.0); building::place_selected();
                    power::connect_power(144*225+112,144*225+157);
                    power::connect_power(144*225+157,144*225+118);
                    power::connect_power(144*225+157,145*225+106);
                    world::enter_chunk(1,0); power::update_power();
                    set_scene_variable("cursor_x",-5.0); set_scene_variable("cursor_z",3.0);
                    set_position("camera-rig",[7.5,0.0,0.0]); set_object_variable("camera_pan_progress",1.0);
                    set_camera_size("camera",13.0); set_object_variable("camera_zoom",13.0); set_object_variable("camera_zoom_target",13.0);
                    return;
                }}
                if "{label}" == "resources" {{
                    navigation::show_title(false); world::begin_world(4);
                    for x in 4..9 {{ for z in -1..4 {{ chunks::discover_chunk(x,z); }} }}
                    world::enter_chunk(6,1);
                    set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",0.0);
                    set_position("camera-rig",[90.0,0.0,15.0]); set_object_variable("camera_pan_progress",1.0);
                    set_camera_size("camera",32.0); set_object_variable("camera_zoom",32.0); set_object_variable("camera_zoom_target",32.0);
                    return;
                }}
                navigation::show_title(false); set_object_variable("creative",true); world::begin_world(4);
                if !"{label}".starts_with("foundation-") {{
                for row in [[113,9.0],[115,9.0],[145,9.0],[147,9.0],[146,5.0],[116,3.0]] {{
                    let cell = row[0]; let nodes = get_scene_list("nodes"); nodes[cell]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",grid::cell_x(cell).to_float()); set_scene_variable("cursor_z",grid::cell_z(cell).to_float());
                    set_scene_variable("selected",row[1]); building::place_selected();
                }}
                for edge in [[112,113],[113,115],[115,145],[145,147],[147,146],[115,116]] {{ power::connect_power(power::power_id(edge[0]),power::power_id(edge[1])); }}
                power::update_power();
                }}
                set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",0.0);
                if "{label}" == "tooltip" {{ set_scene_variable("cursor_x",2.0); set_scene_variable("cursor_z",1.0); }}
                if "{label}".starts_with("foundation-") {{
                    foundation_setup(me);
                    set_scene_variable("cursor_x",if "{label}"=="foundation-window" {{-2.0}}else{{-5.0}});
                    set_scene_variable("cursor_z",if "{label}"=="foundation-window" || "{label}"=="foundation-inside" {{-3.0}}else{{if "{label}"=="foundation-door" {{-1.0}}else{{1.0}}}});
                    set_camera_size("camera",10.0);set_object_variable("camera_zoom",10.0);set_object_variable("camera_zoom_target",10.0);
                    set_ui_visible("game-hud",false);interiors::update(0.0);
                }}
                if "{label}" == "journal" {{ set_scene_variable("selected",9.0); set_object_variable("journal_open",true); set_object_variable("journal_page",2.0); data::session_set(3,22.0); }}
                if "{label}" == "assembler" {{ panels::set_assembler(146); machines::select_recipe(146,18.0); machines::feed_assembler(146); }}
                if ["inventory","backpack-menu"].contains("{label}") {{
                    let stock = get_object_list("stock"); for item in [1,2,8,9,10,11,12,14,15,16,17,18,21] {{ stock[item]=item.to_float()*3.0; }}
                    backpack::set_stock(stock); panels::set_extra_panel("inventory");
                    backpack::move_stack(0,24); backpack::give(1.0,140.0);
                    if "{label}"=="backpack-menu" {{ data::session_set(48,24.0); set_ui_screen_position("backpack-menu",0.72,0.64); }}
                }}
                if "{label}" == "constructor" {{
                    let nodes = get_scene_list("nodes"); nodes[120]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",grid::cell_x(120).to_float()); set_scene_variable("cursor_z",grid::cell_z(120).to_float());
                    set_scene_variable("selected",11.0); building::place_selected(); panels::set_assembler(120); machines::select_recipe(120,16.0); machines::feed_assembler(120);
                }}
                if "{label}" == "rocket" {{ panels::set_extra_panel("rocket"); }}
                if "{label}" == "saves" {{ world::archive_chunk(); persistence::open(1); }}
                if "{label}" == "placement" {{ data::session_set(40,1.0); }}
                if "{label}" == "power-context" {{ pointer::show_menu(2.0,power::power_id(113),power::power_id(115),0.58,0.48); }}
                if "{label}" == "inspect" {{
                    let nodes=get_scene_list("nodes"); nodes[157]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",3.0); set_scene_variable("selected",7.0); building::place_selected();
                    let items=get_scene_list("items"); items[157]=11.0; set_scene_list("items",items);
                    let amounts=get_scene_list("item_amounts"); amounts[157]=8.0; set_scene_list("item_amounts",amounts);
                    inspection::open(157);
                }}
                if ["takeoff","landing"].contains("{label}") {{
                    set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);
                    if "{label}"=="landing" {{ world::travel_to_other_planet(); }}
                    flight::start(); data::session_set(45,if "{label}"=="takeoff" {{ 0.35 }} else {{ 1.35 }});
                    if "{label}"=="landing" {{ data::session_set(44,2.0); }}
                }}
                if "{label}" == "unlocks" {{ set_object_variable("phase",6.0); set_object_variable("journal_open",true); set_object_variable("journal_page",1.0); }}
                if "{label}" == "ship-journal" {{ set_object_variable("journal_open",true); set_object_variable("journal_page",3.0); }}
                if "{label}" == "half" {{ set_object_variable("phase",6.0); }}
                if "{label}" == "site" {{ set_object_variable("phase",5.0); }}
                if ["half","site","ship"].contains("{label}") {{ set_ui_visible("game-hud",false); set_camera_size("camera",9.0); set_object_variable("camera_zoom",9.0); set_object_variable("camera_zoom_target",9.0); }}
            }}"#
            );
            editor.play.as_mut().unwrap().with_instance(|instance, _| {
                instance.register_script("earth-factory".into(), script)
            })?;
        }
        let play = editor.play.as_mut().unwrap();
        for _ in 0..if label == "foundation-door" { 60 } else { 24 } {
            play.app.step();
            play.check_simulation()?;
        }
        if label == "coop" {
            play.enable_multiplayer(None)?;
            play.pump_multiplayer()?;
            let ui = play
                .instance()
                .ui_frame(&play.app.world, Layer::ThreeD, [1080., 600.])?;
            let rect = ui.element("coop-open-title").unwrap().rect;
            let point = [
                rect.min[0] + rect.size[0] * 0.5,
                rect.min[1] + rect.size[1] * 0.5,
            ];
            for input in [
                bozzard_scene::middleware::ui::Input::PointerDown(point),
                bozzard_scene::middleware::ui::Input::PointerUp(point),
            ] {
                play.ui_input(Layer::ThreeD, [1080., 600.], input)?;
            }
        }
        if label == "cross-chunk" {
            for _ in 0..600 {
                play.app.step();
                play.check_simulation()?;
            }
        }
        if label == "saves" {
            use bozzard_demo::factory::{Session, saves, set_session, state::State};
            let directory =
                std::env::temp_dir().join(format!("stellar-save-preview-{}", std::process::id()));
            let state = State::capture(
                play.app
                    .world
                    .resource::<bozzard_scene::BlueprintRuntime>()
                    .unwrap(),
            )?;
            saves::write(&directory, 1, &saves::Save::new(state))?;
            play.app.world.resource_mut::<Session>().unwrap().directory = directory.clone();
            set_session(&mut play.app.world, 116, 6.)?;
            for _ in 0..24 {
                play.app.step();
                play.check_simulation()?;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::fs::remove_dir_all(directory)?;
        }
        if label == "placement" {
            let play = editor.play.as_mut().unwrap();
            let camera = play
                .app
                .world
                .get::<bozzard_scene::Camera>(play.instance().entity("camera").unwrap())
                .unwrap();
            let matrix = camera.projection(1280. / 800.)?
                * play.instance().global_transforms(&play.app.world)?["camera"].inverse();
            let p = matrix * glam::Vec3::new(3., 0.11, 3.).extend(1.);
            play.ui_input(
                Layer::ThreeD,
                [1280., 800.],
                bozzard_scene::middleware::ui::Input::PointerMove([
                    (p.x / p.w * 0.5 + 0.5) * 1280.,
                    (0.5 - p.y / p.w * 0.5) * 800.,
                ]),
            )?;
            play.app.step();
            play.check_simulation()?;
        }
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
            }
        }
        for size in [[1280, 800], [900, 600], [1400, 600], [1920, 1080]] {
            let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
            let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
            if label.starts_with("moon") {
                assert!(render.environment.star_intensity > 1.0);
                assert!(render.lighting.sun_intensity < 0.5);
                assert!(render.environment.zenith.iter().all(|v| *v < 0.01));
            }
            if label == "tooltip" {
                assert_eq!(ui.element("objective-tier").unwrap().text, "Creative");
                assert_eq!(
                    ui.element("nearby-tooltip").unwrap().text,
                    "Fuel dock / INPUT  [E] Inspect"
                );
            }
            let panel_id = if label.starts_with("foundation-") {
                ""
            } else {
                match label {
                    "title" => "title-content",
                    "coop" => "coop-panel",
                    "journal" | "unlocks" | "ship-journal" => "journal-book",
                    "assembler"
                    | "constructor"
                    | "expansion-manufacturer"
                    | "expansion-refinery" => "assembler-panel",
                    "inventory" | "backpack-menu" => "player-inventory-panel",
                    "power-context" => "world-context",
                    "inspect" => "machine-inspect-panel",
                    "rocket" => "player-rocket-panel",
                    "moon-rocket" => "player-rocket-panel",
                    "moon-map" => "map-panel",
                    "saves" => "saves-panel",
                    "moon-deposit" | "materials-belts" => "",
                    "half" | "site" | "ship" => "",
                    _ => "build-panel",
                }
            };
            if !panel_id.is_empty() {
                let panel = ui.element(panel_id).unwrap().rect;
                assert!(panel.min[0] >= 0. && panel.min[1] >= 0.);
                assert!(panel.min[0] + panel.size[0] <= size[0] as f32 + 1.);
                assert!(panel.min[1] + panel.size[1] <= size[1] as f32 + 1.);
            }
            if label == "title" {
                assert!(ui.element("build-panel").is_none());
                assert!(render.lights.is_empty());
            }
            if label == "night" {
                assert_eq!(
                    render
                        .lights
                        .iter()
                        .filter(|light| light.intensity == 9.0)
                        .count(),
                    4,
                    "four powered pole lights, alongside machine indicators"
                );
                assert_eq!(
                    render
                        .lights
                        .iter()
                        .filter(|light| light.intensity == 2.5)
                        .count(),
                    1,
                    "the rocket's active navigation lamp"
                );
                let lit = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                    renderer.draw(&gpu, target, size, &render)
                })?;
                let lights = std::mem::take(&mut render.lights);
                let dark = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                    renderer.draw(&gpu, target, size, &render)
                })?;
                let brighter = lit
                    .rgba
                    .chunks_exact(4)
                    .zip(dark.rgba.chunks_exact(4))
                    .filter(|(a, b)| {
                        a[..3].iter().map(|v| *v as i32).sum::<i32>()
                            > b[..3].iter().map(|v| *v as i32).sum::<i32>() + 15
                    })
                    .count();
                assert!(
                    brighter > 200,
                    "pole lights must visibly illuminate surfaces: {brighter}"
                );
                render.lights = lights;
                let navigation = render
                    .lights
                    .iter()
                    .position(|light| light.intensity == 2.5)
                    .unwrap();
                let navigation = render.lights.remove(navigation);
                let without_navigation =
                    bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                        renderer.draw(&gpu, target, size, &render)
                    })?;
                let illuminated = lit
                    .rgba
                    .chunks_exact(4)
                    .zip(without_navigation.rgba.chunks_exact(4))
                    .filter(|(a, b)| {
                        a[..3].iter().map(|v| *v as i32).sum::<i32>()
                            > b[..3].iter().map(|v| *v as i32).sum::<i32>() + 8
                    })
                    .count();
                assert!(
                    illuminated > 50,
                    "navigation light must illuminate surfaces: {illuminated}"
                );
                render.lights.push(navigation);
            }
            render
                .items
                .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
            let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                renderer.draw(&gpu, target, size, &render)
            })?;
            capture.write_ppm(
                &std::env::temp_dir().join(format!("stellar-{label}-{}.ppm", size[0])),
            )?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a native graphics adapter; writes logistics port preview"]
fn logistics_ports_render() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_start(me)", "fn original_start(me)")
        .replace("sin(data::session_value(120) * 0.10)", "1.0");
    let mut editor = Editor::open(&path)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let script = format!(
        r#"{source}
        fn on_start(me) {{
            navigation::show_title(false); set_object_variable("creative",true); world::begin_world(4);
            set_object_variable("phase",4.0);
            for kind in [7,8] {{
                for facing in 0..4 {{
                    let x=facing*2-3; let z=if kind==7 {{ -2 }} else {{ 2 }};
                    let cell=grid::index(x,z); let nodes=get_scene_list("nodes");
                    if nodes[cell]!=0.0 {{ set_visible(get_scene_list("node_visuals")[cell],false); }}
                    nodes[cell]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",x.to_float()); set_scene_variable("cursor_z",z.to_float());
                    set_scene_variable("selected",kind.to_float()); set_scene_variable("direction",facing.to_float()); building::place_selected();
                }}
            }}
            set_ui_visible("game-hud",false);
            set_camera_size("camera",11.0); set_object_variable("camera_zoom",11.0); set_object_variable("camera_zoom_target",11.0);
        }}"#
    );
    editor
        .play
        .as_mut()
        .unwrap()
        .with_instance(|instance, _| instance.register_script("earth-factory".into(), script))?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..4 {
        play.app.step();
        play.check_simulation()?;
    }
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
    let size = [1280, 800];
    let render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
    let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
        renderer.draw(&gpu, target, size, &render)
    })?;
    capture.write_ppm(&std::env::temp_dir().join("stellar-logistics-ports.ppm"))?;
    Ok(())
}

#[test]
#[ignore = "requires a native graphics adapter; writes the renewable power preview"]
fn renewable_power_models_render_in_the_playable_showroom() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/renewables-showroom.json");
    let mut editor = Editor::open(&path)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..12 {
        play.app.step();
        play.check_simulation()?;
    }
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
    let size = [1400, 900];
    let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
    let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
    assert_eq!(ui.element("slot-name-4").unwrap().text, "SOLAR\nPANEL");
    assert_eq!(ui.element("slot-name-5").unwrap().text, "SOLAR\nARRAY");
    assert_eq!(ui.element("slot-name-6").unwrap().text, "WIND\nTURBINE");
    render
        .items
        .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
    let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
        renderer.draw(&gpu, target, size, &render)
    })?;
    capture.write_ppm(&std::env::temp_dir().join("stellar-renewables.ppm"))?;
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_update(me, dt)", "fn normal_update(me, dt)");
    let mut previous_angle = None;
    let mut previous_pixels = None;
    for (stage, time, supply) in [
        ("calm-before", 10.0, 20.0),
        ("gust", 20.0, 26.0),
        ("gust-moving", 20.1, 26.0),
        ("calm-after", 40.0, 8.0),
        ("calm-still", 40.1, 8.0),
    ] {
        let play = editor.play.as_mut().unwrap();
        let script = format!(
            r#"{source}
            fn on_update(me,dt) {{
                data::session_set(120,{time:.1});power::update_power();wind::update();
                set_scene_variable("cursor_x",3.0);set_scene_variable("cursor_z",-2.0);
                set_visible("cursor",false);set_ui_visible("game-hud",false);
                set_position("camera-rig",[3.0,0.7,-4.0]);set_camera_size("camera",3.5);
            }}"#
        );
        play.with_instance(|instance, _| instance.register_script("earth-factory".into(), script))?;
        play.app.step();
        play.check_simulation()?;
        let rotor = play
            .instance()
            .document()
            .objects
            .iter()
            .find(|o| o.name == "Wind turbine rotor")
            .unwrap();
        let angle = play
            .app
            .world
            .get::<bozzard_scene::Transform>(play.instance().entity(&rotor.id).unwrap())
            .unwrap()
            .rotation_degrees[2];
        let boards = play
            .app
            .world
            .resource::<bozzard_scene::BlueprintRuntime>()
            .unwrap();
        assert_eq!(
            boards.scene_blackboard()["power_supply"].values()[0].number()?,
            supply
        );
        if stage == "gust-moving" {
            assert_ne!(Some(angle), previous_angle);
        }
        if stage == "calm-still" {
            assert_eq!(Some(angle), previous_angle);
        }
        let render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
        let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        if stage == "gust-moving" {
            assert_ne!(Some(&capture.rgba), previous_pixels.as_ref());
        }
        if stage == "calm-still" {
            assert_eq!(Some(&capture.rgba), previous_pixels.as_ref());
        }
        capture.write_ppm(&std::env::temp_dir().join(format!("stellar-wind-{stage}.ppm")))?;
        previous_angle = Some(angle);
        previous_pixels = Some(capture.rgba);
    }
    Ok(())
}

#[test]
#[ignore = "requires a native graphics adapter; captures player-follow camera movement"]
fn player_camera_renders_centered_after_crossing_a_seam_and_orbiting() -> anyhow::Result<()> {
    use bozzard_scene::{GameplayInput, Transform, keys};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?
        .replace("fn on_start(me)", "fn original_start(me)");
    let source = format!(
        r#"{source}
        fn on_start(me) {{
            navigation::show_title(false);world::begin_world(4);
            set_scene_variable("cursor_x",6.0);set_scene_variable("cursor_z",2.0);
            environment::center_on_player();
        }}
    "#
    );
    let mut editor = Editor::open(&path)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
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
    let size = [1080, 720];
    for (stage, frames) in [("start", 0), ("follow", 4), ("settled", 60), ("orbit", 60)] {
        let play = editor.play.as_mut().unwrap();
        for frame in 0..frames {
            let input = if stage == "follow" && frame % 2 == 0 {
                keys::bit("D")
            } else if stage == "orbit" && frame == 0 {
                keys::bit("Ctrl") | keys::bit("R")
            } else {
                0
            };
            play.set_gameplay_input(GameplayInput {
                keys: input,
                ..Default::default()
            });
            play.app.step();
            play.check_simulation()?;
        }
        if stage == "settled" || stage == "orbit" {
            let rig = play
                .app
                .world
                .get::<Transform>(play.instance().entity("camera-rig").unwrap())
                .unwrap();
            assert_eq!(rig.translation, [8., 0., 2.]);
            let camera = play
                .app
                .world
                .get::<bozzard_scene::Camera>(play.instance().entity("camera").unwrap())
                .unwrap();
            let matrix = camera.projection(size[0] as f32 / size[1] as f32)?
                * play.instance().global_transforms(&play.app.world)?["camera"].inverse();
            let point = matrix * glam::Vec3::new(8., 0., 2.).extend(1.);
            assert!(
                point.x.abs() < 0.001 && point.y.abs() < 0.001,
                "player tile must project to viewport center after following/orbiting"
            );
        }
        let render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
        let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        capture
            .write_ppm(&std::env::temp_dir().join(format!("stellar-player-camera-{stage}.ppm")))?;
    }
    Ok(())
}
