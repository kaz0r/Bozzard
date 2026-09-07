use super::*;
impl App {
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
                self.editor.selected = Some(id);
                let path = std::path::absolute(output.join("edited-scene.json"))?;
                self.editor.save(&path)?;
                let restored = Editor::open(&path)?;
                ensure!(
                    *restored.scene() == *self.editor.scene(),
                    "editor save/load mismatch"
                );
                self.status="Editor acceptance: create · transform · undo/redo · play isolation · save/load passed".into();
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("editor_smoke_failed: {error:#}");
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        if self.smoke_frames >= 8 && !self.smoke_requested && !self.error {
            self.smoke_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let result = (|| -> Result<()> {
                    ensure!(!self.error, "viewport error during smoke: {}", self.status);
                    ensure!(
                        image.size[0] >= 600 && image.size[1] >= 400,
                        "editor screenshot is too small"
                    );
                    let frame = bozzard_render::Frame {
                        width: image.size[0] as u32,
                        height: image.size[1] as u32,
                        rgba: image.pixels.iter().flat_map(|p| p.to_array()).collect(),
                    };
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
                        self.smoke_passed.store(true, Ordering::Relaxed);
                        println!(
                            "editor_smoke_ok authored_commands play_isolation collision_response gravity_landing save_load native_ui_capture viewport_pixel_oracle"
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
