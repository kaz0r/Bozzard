use super::*;
impl App {
    pub fn smoke_gizmo_navigation(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        projection: glam::Mat4,
    ) -> Result<()> {
        let saved = (
            self.workspace.tool,
            self.fly_latched,
            self.mouse_captured,
            self.navigation_button,
        );
        let original = self.editor.scene().clone();
        let result = (|| -> Result<()> {
            for tool in [Tool::Move, Tool::Rotate, Tool::Scale] {
                for button in [
                    Some(egui::PointerButton::Secondary),
                    Some(egui::PointerButton::Middle),
                    None,
                ] {
                    self.workspace.tool = tool;
                    self.navigation_button = button;
                    self.fly_latched = button.is_none();
                    self.mouse_captured = button != Some(egui::PointerButton::Middle);
                    let ctx = ui.ctx().clone();
                    let layer = ui.layer_id();
                    let count =
                        || ctx.graphics(|g| g.get(layer).map_or(0, |p| p.all_entries().len()));
                    let before = count();
                    self.gizmo(ui, rect, projection)?;
                    ensure!(count() > before + 5, "navigation hid the gizmo geometry");
                    ensure!(self.drag.is_none(), "navigation started a transform drag");
                    ensure!(
                        *self.editor.scene() == original,
                        "navigation edited the scene"
                    );
                }
            }
            Ok(())
        })();
        (
            self.workspace.tool,
            self.fly_latched,
            self.mouse_captured,
            self.navigation_button,
        ) = saved;
        result?;
        println!("editor_gizmo_smoke_ok move_rotate_scale_visible_right_middle_fly no_transform");
        Ok(())
    }

    fn smoke_material_override(&mut self, output: &Path) -> Result<()> {
        let original = self.editor.selected_material_override()?;
        let mut edited = original.clone();
        edited.tint = [1.0, 0.45, 0.4];
        edited.transform.translation = [0.4, 0.1, 0.1];
        edited.transform.rotation_degrees = [0., 20., 10.];
        edited.transform.scale = [1.1, 0.9, 1.];
        edited.texture = Some(Texture::Checker);
        edited.uv_scale = [2.; 2];
        if self
            .editor
            .selected_surface()
            .unwrap()
            .part
            .shading
            .is_some()
        {
            edited.metallic = Some(0.6);
            edited.roughness = Some(0.25);
        }
        let revision = self.editor.asset_revision();
        self.editor.begin_gesture("Edit surface material");
        self.editor.set_selected_material_override(edited.clone())?;
        self.editor.finish_gesture();
        self.editor.undo()?;
        ensure!(
            self.editor.selected_material_override()? == original,
            "material Undo mismatch"
        );
        self.editor.redo()?;
        ensure!(
            self.editor.selected_material_override()? == edited,
            "material Redo mismatch"
        );
        ensure!(
            self.editor.asset_revision() == revision && self.surface_graphics_ready(),
            "material edit reloaded shared graphics"
        );
        // The initial Save As already rebased asset paths into this output directory.
        // Write a sibling document without replacing the live asset/selection identity.
        bozzard_demo::save_document(self.editor.scene(), &output.join("material-scene.json"))?;
        println!(
            "editor_material_override_smoke_ok transform texture uv material undo redo shared_residency scene_save"
        );
        Ok(())
    }

    pub fn smoke_step(&mut self, ctx: &egui::Context) {
        let Some(output) = self.smoke.clone() else {
            return;
        };
        self.smoke_frames += 1;
        if self.smoke_frames == 3 {
            let result = (|| -> Result<()> {
                let original = self.editor.scene().clone();
                self.editor.create(Mesh::Cube, Layer::ThreeD)?;
                let id = self
                    .editor
                    .selected
                    .clone()
                    .context("new object not selected")?;
                self.editor.begin_gesture("Smoke transform");
                let mut scene = self.editor.scene().clone();
                let object = scene.objects.iter_mut().find(|o| o.id == id).unwrap();
                // Isolated above the demo set so the pixel oracle is unobstructed.
                object.transform.translation = [0.0, 2.5, 0.0];
                object.spin = Some(Spin([0.0, 90.0, 0.0]));
                object.collider = Some(bozzard_scene::BoxCollider::default());
                self.editor.apply("Smoke transform", scene)?;
                self.editor.finish_gesture();
                ensure!(
                    self.editor.collisions()?.boxes.iter().any(|b| b.id == id),
                    "new collider missing from query"
                );
                let authored = self.editor.scene().clone();
                self.editor.start_play()?;
                for _ in 0..120 {
                    self.editor.play.as_mut().unwrap().app.step();
                }
                // Add a temporary runtime-only floor using a known scene member.
                // This exercises response even when the supplied scene has no colliders.
                let play = self.editor.play.as_mut().unwrap();
                let floor = play.instance.camera_entity(Layer::ThreeD)?;
                *play.app.world.get_mut::<Transform>(floor).unwrap() = Transform {
                    translation: [0.0, -5.0, 0.0],
                    ..Transform::default()
                };
                play.app.world.insert(
                    floor,
                    bozzard_scene::BoxCollider {
                        size: [30.0, 1.0, 30.0],
                        ..Default::default()
                    },
                )?;
                let movement = self.editor.move_selected_box(Vec3::new(0.0, -20.0, 0.0))?;
                ensure!(
                    !movement.contacts.is_empty()
                        && movement.applied.y > -19.0
                        && movement.applied.y < 0.0,
                    "swept box crossed the runtime floor: {movement:?}"
                );
                let play = self.editor.play.as_mut().unwrap();
                let mover = play.instance.entity(&id).context("missing smoke mover")?;
                play.app.world.remove::<Spin>(mover)?;
                *play.app.world.get_mut::<Transform>(mover).unwrap() = Transform {
                    translation: [50.0, 0.0, 50.0],
                    ..Default::default()
                };
                play.app
                    .world
                    .get_mut::<Transform>(floor)
                    .unwrap()
                    .translation = [50.0, -5.0, 50.0];
                play.app
                    .world
                    .insert(mover, bozzard_scene::Gravity::default())?;
                for _ in 0..180 {
                    play.app.step();
                    play.check_simulation()?;
                }
                let state = play
                    .app
                    .world
                    .get::<bozzard_scene::GravityState>(mover)
                    .unwrap();
                ensure!(
                    state.grounded && state.vertical_velocity == 0.0,
                    "gravity did not settle: {state:?}"
                );
                let height = play.app.world.get::<Transform>(mover).unwrap().translation[1];
                ensure!(
                    (height + 4.0).abs() < 0.001,
                    "gravity crossed floor: {height}"
                );
                self.editor.stop_play();
                ensure!(
                    *self.editor.scene() == authored,
                    "Play changed authored state"
                );
                self.editor.undo()?;
                self.editor.undo()?;
                ensure!(
                    *self.editor.scene() == original,
                    "Undo failed to restore original scene"
                );
                self.editor.redo()?;
                self.editor.redo()?;
                self.editor.select_object(Some(id));
                self.smoke_selection = self.editor.selected.clone();
                let path = std::path::absolute(output.join("edited-scene.json"))?;
                self.save_scene(path);
                ensure!(self.loading.is_some(), "background save did not start");
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("editor_smoke_failed: {error:#}");
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        if (4..=9).contains(&self.smoke_frames) {
            let result = (|| -> Result<()> {
                ensure!(!self.error, "background operation failed: {}", self.status);
                match self.smoke_frames {
                    4 => {
                        ensure!(!self.editor.dirty(), "background save did not finish");
                        self.smoke_expected = Some(self.editor.scene().clone());
                        self.request(Pending::Open(self.editor.path.clone()));
                        ensure!(self.loading.is_some(), "background open did not start");
                    }
                    5 => {
                        ensure!(
                            Some(self.editor.scene()) == self.smoke_expected.as_ref(),
                            "background save/open mismatch"
                        );
                        self.editor.selected = self.smoke_selection.clone();
                    }
                    6 => {
                        let source = std::path::absolute(output.join("async-palette.png"))?;
                        std::fs::write(
                            &source,
                            include_bytes!("../../../examples/demo/scenes/assets/palette.png"),
                        )?;
                        self.start_import(source.clone());
                        self.start_import(source);
                        ensure!(
                            self.loading.is_some() && self.import_queue.len() == 1,
                            "bounded import queue did not start"
                        );
                    }
                    7 => {
                        ensure!(
                            self.editor.scene().assets.len()
                                == self.smoke_expected.as_ref().unwrap().assets.len() + 2,
                            "queued imports did not publish"
                        );
                        self.smoke_expected = Some(self.editor.scene().clone());
                    }
                    8 => {
                        self.start_import(std::path::absolute(output.join("async-palette.png"))?);
                        self.loading
                            .as_ref()
                            .context("cancel test import missing")?
                            .cancel();
                    }
                    9 => {
                        ensure!(
                            Some(self.editor.scene()) == self.smoke_expected.as_ref(),
                            "cancelled import changed scene"
                        );
                        self.status = "Editor acceptance: background save/open · queued imports · cancellation · authored commands passed".into();
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("editor_smoke_failed: {error:#}");
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        if self.smoke_frames
            >= self
                .smoke_blueprint_frame
                .or(self.smoke_prefab_frame)
                .or(self.smoke_gi_frame)
                .or(self.smoke_light_frame)
                .or(self.smoke_surface_frame)
                .unwrap_or(12)
            && self.loading.is_none()
            && !self.smoke_requested
            && !self.error
            && (self.viewport_rect.is_some() || self.smoke_blueprint_frame.is_some())
            && self.target.is_some()
            && self.residency.has_all(&self.editor.assets)
        {
            self.smoke_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let result = (|| -> Result<()> {
                    ensure!(!self.error, "viewport error during smoke: {}", self.status);
                    ensure!(
                        self.smoke_gizmo_verified,
                        "gizmo navigation check did not run"
                    );
                    ensure!(
                        image.size[0] >= 600 && image.size[1] >= 400,
                        "editor screenshot is too small"
                    );
                    let frame = bozzard_render::Frame {
                        width: image.size[0] as u32,
                        height: image.size[1] as u32,
                        rgba: image.pixels.iter().flat_map(|p| p.to_array()).collect(),
                    };
                    if self.smoke_blueprint_frame.is_some() {
                        frame.write_ppm(&output.join("editor-blueprints.ppm"))?;
                        ensure!(
                            self.workspace.blueprints_visible
                                && self
                                    .editor
                                    .selected_object()
                                    .is_some_and(|o| o.blueprints.len() == 2),
                            "blueprint pane or attachments missing"
                        );
                        ensure!(
                            self.editor.play.is_none(),
                            "blueprint authoring is still in Play"
                        );
                        println!(
                            "editor_blueprint_smoke_ok dedicated_node_pane multiple_graphs runtime_play_isolation portable_save native_ui_capture"
                        );
                        return Ok(());
                    }
                    if self.smoke_prefab_frame.is_some() {
                        frame.write_ppm(&output.join("editor-prefabs.ppm"))?;
                        ensure!(
                            self.editor.scene().prefabs.len() == 3,
                            "prefab links missing"
                        );
                        ensure!(
                            self.editor.selected_prefab_root() == Some("cargo-1"),
                            "prefab selection missing"
                        );
                        ensure!(
                            self.residency.has_all(&self.editor.assets),
                            "CPU-only prefab blocks GPU residency"
                        );
                        println!(
                            "editor_prefab_smoke_ok linked_hierarchy asset_panel inspector shared_source native_ui_capture"
                        );
                        return Ok(());
                    }
                    if self.smoke_gi_frame.is_some() {
                        frame.write_ppm(&output.join("editor-gi.ppm"))?;
                        std::fs::write(
                            output.join("gi-inspection.json"),
                            self.editor.scene().to_json()?,
                        )?;
                        ensure!(
                            self.editor.gi_current(),
                            "native bake failed to publish: status={}; loading={}; baked={}; current={:?}",
                            self.status,
                            self.loading.is_some(),
                            self.editor.scene().gi.baked.is_some(),
                            bozzard_assets::gi::is_current(
                                self.editor.scene(),
                                &self.editor.assets
                            )
                        );
                        ensure!(
                            self.editor.render(Layer::ThreeD, 1.)?.gi.is_some(),
                            "native bake not extracted"
                        );
                        self.editor.save(&output.join("gi-scene.json"))?;
                        println!(
                            "editor_gi_smoke_ok background_bake source_current extraction guides save native_ui_capture"
                        );
                        return Ok(());
                    }
                    if self.smoke_light_frame.is_some() {
                        frame.write_ppm(&output.join("editor-light.ppm"))?;
                        ensure!(
                            self.editor.selected_object().is_some_and(|o| o
                                .light
                                .is_some_and(|l| l.kind == bozzard_scene::LightKind::Point
                                    && l.requests_shadow_map())),
                            "shadowed point-light selection lost"
                        );
                        ensure!(
                            self.editor
                                .render(Layer::ThreeD, 1.)?
                                .lights
                                .iter()
                                .any(|l| l.spot_angles.is_none() && l.shadows.is_some()),
                            "point-light shadow extraction missing"
                        );
                        println!(
                            "editor_light_smoke_ok authored_component shadow_settings extraction inspector guides native_ui_capture"
                        );
                        return Ok(());
                    }
                    if self.smoke_surface_frame.is_some() {
                        ensure!(
                            self.smoke_surface_gizmo_verified,
                            "submesh gizmo check did not run"
                        );
                        frame.write_ppm(&output.join("editor-surface.ppm"))?;
                        let surface = self
                            .editor
                            .selected_surface()
                            .context("surface selection lost")?;
                        ensure!(surface.part.count > 0, "empty inspected surface");
                        let target = self.target.as_ref().context("missing framed viewport")?;
                        bozzard_render::read_texture(
                            &self.gpu,
                            &target.texture,
                            target.size[0],
                            target.size[1],
                        )?
                        .write_ppm(&output.join("surface-viewport.ppm"))?;
                        ensure!(
                            self.editor.frame_selection_bounds(self.layer())?.is_some(),
                            "surface bounds missing"
                        );
                        println!(
                            "editor_surface_smoke_ok material_inspector source_names framed_selection native_ui_capture"
                        );
                        return Ok(());
                    }
                    frame.write_ppm(&output.join("editor.ppm"))?;
                    let target = self.target.as_ref().context("missing viewport target")?;
                    let viewport = bozzard_render::read_texture(
                        &self.gpu,
                        &target.texture,
                        target.size[0],
                        target.size[1],
                    )?;
                    // Keep the evidence even when the pixel oracle rejects the frame.
                    viewport.write_ppm(&output.join("viewport.ppm"))?;
                    let first = &viewport.rgba[..4];
                    ensure!(
                        viewport
                            .rgba
                            .chunks_exact(4)
                            .filter(|p| *p != first)
                            .count()
                            > 100,
                        "viewport rendered only a clear color"
                    );
                    // Pixel oracle: the smoke-created teal cube must shade the
                    // region its camera projection predicts, not just anywhere.
                    let cube = self.editor.selected.clone().context("smoke cube lost")?;
                    let demo = bozzard_demo::SceneDemo::new(self.editor.scene())?;
                    let matrices = demo.instance.global_transforms(&demo.app.world)?;
                    let center = matrices[&cube].transform_point3(Vec3::ZERO);
                    let aspect = target.size[0] as f32 / target.size[1] as f32;
                    let clip = self.editor.render(self.layer(), aspect)?.view_projection
                        * center.extend(1.0);
                    ensure!(clip.w > 0.0, "smoke cube is behind the camera");
                    let ndc = clip.truncate() / clip.w;
                    ensure!(
                        (0.0..=1.0).contains(&ndc.z),
                        "smoke cube is clipped by the camera"
                    );
                    let px = ((ndc.x + 1.0) * 0.5 * target.size[0] as f32) as i64;
                    let py = ((1.0 - ndc.y) * 0.5 * target.size[1] as f32) as i64;
                    let (mut teal, mut samples) = (0u32, 0u32);
                    for y in (py - 6)..=(py + 6) {
                        for x in (px - 6)..=(px + 6) {
                            if x < 0
                                || y < 0
                                || x >= target.size[0] as i64
                                || y >= target.size[1] as i64
                            {
                                continue;
                            }
                            let i = ((y as u32 * target.size[0] + x as u32) * 4) as usize;
                            let (r, g, b) = (
                                viewport.rgba[i] as i32,
                                viewport.rgba[i + 1] as i32,
                                viewport.rgba[i + 2] as i32,
                            );
                            samples += 1;
                            // Tint [0.25, 0.8, 0.7] keeps g dominant under any
                            // diffuse factor; the clear color is blue-dominant.
                            if g > r + 25 && g > b && g > 100 {
                                teal += 1;
                            }
                        }
                    }
                    ensure!(
                        samples > 0 && teal * 2 > samples,
                        "projected cube region is not the expected teal ({teal}/{samples})"
                    );
                    self.gpu.wait()?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        if self.smoke_surface_frame.is_none() && self.smoke_light_frame.is_none() {
                            let model = self
                                .editor
                                .scene()
                                .objects
                                .iter()
                                .filter_map(|object| {
                                    let Mesh::Asset(id) = &object.drawable.as_ref()?.mesh else {
                                        return None;
                                    };
                                    let bozzard_assets::AssetData::Mesh(mesh) = self
                                        .editor
                                        .assets
                                        .get(self.editor.assets.handle(id)?)?
                                        .data()?
                                    else {
                                        return None;
                                    };
                                    // Prefer a PBR part so the native capture covers all controls.
                                    let index = mesh
                                        .parts
                                        .iter()
                                        .position(|p| p.shading.is_some())
                                        .unwrap_or(0);
                                    mesh.parts.get(index).map(|part| {
                                        (part.shading.is_some(), object.id.clone(), index)
                                    })
                                })
                                .max_by_key(|(pbr, _, _)| *pbr);
                            if let Some((_, model, index)) = model {
                                self.editor.select_object(Some(model));
                                if let Err(error) = self
                                    .editor
                                    .select_surface(index)
                                    .and_then(|()| self.smoke_material_override(&output))
                                {
                                    eprintln!("editor_smoke_failed: {error:#}");
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    return;
                                } else {
                                    self.hierarchy_frame_requested = true;
                                    self.smoke_surface_frame = Some(self.smoke_frames + 2);
                                    self.smoke_requested = false;
                                    continue;
                                }
                            }
                        }
                        if self.smoke_light_frame.is_none() {
                            let result = (|| -> Result<()> {
                                self.editor.create_light(bozzard_scene::LightKind::Point)?;
                                let selected = self
                                    .editor
                                    .selected
                                    .as_ref()
                                    .context("missing new point light")?;
                                let mut scene = self.editor.scene().clone();
                                scene
                                    .objects
                                    .iter_mut()
                                    .find(|o| &o.id == selected)
                                    .unwrap()
                                    .light
                                    .as_mut()
                                    .unwrap()
                                    .shadows = true;
                                self.editor.apply("Enable point-light shadows", scene)?;
                                Ok(())
                            })();
                            if let Err(error) = result {
                                eprintln!("editor_smoke_failed: {error:#}");
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                return;
                            }
                            self.workspace.camera = None;
                            self.workspace.ortho_zoom = 1.;
                            self.workspace.layer_2d = false;
                            // Leave enough vertical space to visually verify both shadow biases.
                            self.workspace.assets_visible = false;
                            self.smoke_light_frame = Some(self.smoke_frames + 3);
                            self.smoke_requested = false;
                            continue;
                        }
                        if self.smoke_gi_frame.is_none() {
                            let result = (|| -> Result<()> {
                                let mut scene = bozzard_scene::Scene::from_json(include_str!(
                                    "../../../examples/demo/scenes/gi-lab.json"
                                ))?;
                                scene.gi.volume.resolution = [4; 3];
                                scene.gi.volume.samples = 64;
                                scene.gi.volume.bounces = 2;
                                self.editor = Editor::new(scene, &output.join("gi-scene.json"))?;
                                self.refresh = None;
                                self.workspace.camera = None;
                                self.workspace.gi_visible = true;
                                self.workspace.colliders_visible = false;
                                self.loading =
                                    Some(loading::Loading::BakeGi(self.editor.bake_gi_job()?));
                                Ok(())
                            })();
                            if let Err(error) = result {
                                eprintln!("editor_smoke_failed: {error:#}");
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                return;
                            }
                            self.smoke_gi_frame = Some(self.smoke_frames + 3);
                            self.smoke_requested = false;
                            continue;
                        }
                        if self.smoke_prefab_frame.is_none() {
                            let result = (|| -> Result<()> {
                                // Embedded fixtures keep packaged smoke independent of the build checkout.
                                std::fs::create_dir_all(output.join("prefab-assets"))?;
                                std::fs::write(
                                    output.join("prefab-assets/cargo.prefab.json"),
                                    include_bytes!(
                                        "../../../examples/demo/scenes/assets/cargo.prefab.json"
                                    ),
                                )?;
                                let mut scene = bozzard_scene::Scene::from_json(include_str!(
                                    "../../../examples/demo/scenes/prefab-lab.json"
                                ))?;
                                scene.assets.get_mut("cargo-prefab").unwrap().path =
                                    "prefab-assets/cargo.prefab.json".into();
                                self.editor =
                                    Editor::new(scene, &output.join("prefab-scene.json"))?;
                                self.editor.save(&output.join("prefab-scene.json"))?;
                                self.editor.selected = Some("cargo-1".into());
                                self.asset_browser.reveal("cargo-prefab".into());
                                self.workspace.assets_visible = true;
                                self.workspace.camera = None;
                                self.workspace.gi_visible = false;
                                self.workspace.colliders_visible = false;
                                self.workspace.layer_2d = false;
                                self.refresh = None;
                                Ok(())
                            })();
                            if let Err(error) = result {
                                eprintln!("editor_smoke_failed: {error:#}");
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                return;
                            }
                            self.smoke_prefab_frame = Some(self.smoke_frames + 3);
                            self.smoke_requested = false;
                            continue;
                        }
                        if self.smoke_blueprint_frame.is_none() {
                            let result = (|| -> Result<()> {
                                let scene = bozzard_scene::Scene::from_json(include_str!(
                                    "../../../examples/demo/scenes/blueprint-lab.json"
                                ))?;
                                self.editor =
                                    Editor::new(scene, &output.join("blueprint-scene.json"))?;
                                self.editor.select_object(Some("hero-cube".into()));
                                let authored = self.editor.scene().clone();
                                self.editor.start_play()?;
                                let play = self.editor.play.as_mut().unwrap();
                                for _ in 0..120 {
                                    play.app.step();
                                }
                                play.check_simulation()?;
                                ensure!(
                                    play.instance.capture(&play.app.world)? != authored,
                                    "blueprint did not execute"
                                );
                                self.editor.stop_play();
                                ensure!(
                                    *self.editor.scene() == authored,
                                    "Play modified authored blueprints"
                                );
                                self.editor.save(&output.join("blueprint-scene.json"))?;
                                self.editor.save_blueprint(
                                    "hero-cube",
                                    0,
                                    &output.join("spin.blueprint.json"),
                                )?;
                                self.workspace.blueprints_visible = true;
                                self.workspace.settings_visible = false;
                                self.workspace.assets_visible = false;
                                self.refresh = None;
                                Ok(())
                            })();
                            if let Err(error) = result {
                                eprintln!("editor_smoke_failed: {error:#}");
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                return;
                            }
                            self.smoke_blueprint_frame = Some(self.smoke_frames + 3);
                            self.smoke_requested = false;
                            continue;
                        }
                        self.smoke_passed.store(true, Ordering::Relaxed);
                        println!(
                            "editor_smoke_ok authored_commands play_isolation collision_response gravity_landing background_save_open queued_imports cancellation native_ui_capture viewport_pixel_oracle"
                        );
                    }
                    Err(error) => eprintln!("editor_smoke_failed: {error:#}"),
                };
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.smoke_start.elapsed() > Duration::from_secs(30) {
            eprintln!("editor_smoke_failed: no rendered UI capture within 30 seconds");
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
