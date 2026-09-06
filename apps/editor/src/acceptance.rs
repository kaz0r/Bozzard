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
                object.transform.translation = [0.5, 0.5, 0.0];
                object.spin = Some(Spin([0.0, 90.0, 0.0]));
                self.editor.apply("Smoke transform", scene)?;
                self.editor.finish_gesture();
                let authored = self.editor.scene().clone();
                self.editor.start_play()?;
                for _ in 0..120 {
                    self.editor.play.as_mut().unwrap().app.step();
                }
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
                    viewport.write_ppm(&output.join("viewport.ppm"))?;
                    self.gpu.wait()?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        self.smoke_passed.store(true, Ordering::Relaxed);
                        println!(
                            "editor_smoke_ok authored_commands play_isolation save_load native_ui_capture"
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
