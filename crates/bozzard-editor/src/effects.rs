use super::*;
/// An isolated authoring preview. Only particle simulation and display time run;
/// gameplay, physics, scripts, and the saved scene never advance here.
pub struct EffectsPreview {
    revision: u64,
    demo: SceneDemo,
}
impl EffectsPreview {
    pub fn new(editor: &Editor) -> Result<Self> {
        let mut demo = SceneDemo::new(editor.scene())?;
        if editor
            .scene()
            .objects
            .iter()
            .any(|o| o.particle_emitter.is_some())
        {
            for _ in 0..24 {
                demo.with_instance(|instance, world| {
                    instance
                        .advance_display(0.1)
                        .and_then(|()| instance.step_particles(world, 0.1))
                })?;
            }
        }
        Ok(Self {
            revision: editor.revision(),
            demo,
        })
    }
    pub fn advance(&mut self, editor: &Editor, delta: Duration, running: bool) -> Result<()> {
        if self.revision != editor.revision() {
            *self = Self::new(editor)?;
        }
        if running {
            let dt = delta.as_secs_f32().min(0.1);
            self.demo.with_instance(|instance, world| {
                instance
                    .advance_display(dt)
                    .and_then(|()| instance.step_particles(world, dt))
            })?;
        }
        Ok(())
    }
    pub fn render(&self, editor: &Editor, layer: Layer, aspect: f32) -> Result<RenderScene> {
        extract(&self.demo, &editor.assets, layer, aspect)
    }
}
impl Editor {
    pub fn apply_wet_material(&mut self) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play to edit materials");
        let id = self
            .selected
            .as_ref()
            .context("Select a mesh to apply a wet material")?;
        let mut scene = self.scene.clone();
        let object = scene
            .objects
            .iter_mut()
            .find(|o| &o.id == id)
            .context("selected object was removed")?;
        let drawable = object
            .drawable
            .as_ref()
            .context("Select a mesh to apply a wet material")?;
        let material = object
            .material
            .get_or_insert_with(|| bozzard_scene::Material::from_drawable(drawable));
        material.metallic = Some(0.05);
        material.roughness = Some(0.12);
        scene.display.reflections.enabled = true;
        self.apply("Apply wet material and reflections", scene)
    }
    pub fn create_effect_volume(&mut self, position: Vec3) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play to edit effects");
        let mut scene = self.scene.clone();
        ensure!(
            scene.post_process_volumes.len() < 32,
            "Effect volume limit reached"
        );
        scene
            .post_process_volumes
            .push(bozzard_scene::PostProcessVolume {
                name: format!("Effect volume {}", scene.post_process_volumes.len() + 1),
                center: position.to_array(),
                display: scene.display,
                ..Default::default()
            });
        self.apply("Create effect volume", scene)
    }
}

impl Editor {
    /// Create a complete particle effect at the selected object's world position.
    pub fn create_particle_emitter(&mut self, kind: bozzard_scene::ParticleKind) -> Result<()> {
        let mut scene = self.scene.clone();
        let position = self
            .selected
            .as_ref()
            .and_then(|id| {
                scene
                    .global_transforms()
                    .ok()
                    .and_then(|m| m.get(id).copied())
            })
            .map(|m| m.transform_point3(Vec3::ZERO))
            .unwrap_or(Vec3::Y);
        let id = unique_id(&scene, "particles");
        scene.objects.push(Object {
            particle_emitter: Some(bozzard_scene::ParticleEmitter::preset(kind)),
            id: id.clone(),
            name: kind.name().into(),
            transform: Transform {
                translation: position.to_array(),
                ..Default::default()
            },
            parent: None,
            material: None,
            blueprints: Vec::new(),
            light: None,
            camera: None,
            drawable: None,
            spin: None,
            collider: None,
            mesh_collider: None,
            text_rendering: None,
            gravity: None,
            player_controller: None,
            trigger: None,
        });
        self.apply("Create particle effect", scene)?;
        self.selected = Some(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_preview_preserves_scene_history_and_pauses_particles() -> Result<()> {
        let scene = bozzard_demo::scene_document()?;
        let mut editor = Editor::new(
            scene,
            std::path::Path::new("/tmp/bozzard-effects-preview.json"),
        )?;
        let original = editor.scene().clone();
        editor.create_particle_emitter(bozzard_scene::ParticleKind::Smoke)?;
        let authored = editor.scene().clone();
        let mut preview = EffectsPreview::new(&editor)?;
        let first = preview.render(&editor, Layer::ThreeD, 1.)?;
        assert!(!first.particles.is_empty());
        preview.advance(&editor, Duration::from_secs_f32(0.1), false)?;
        let paused = preview.render(&editor, Layer::ThreeD, 1.)?;
        assert_eq!(first.particles, paused.particles);
        preview.advance(&editor, Duration::from_secs_f32(0.1), true)?;
        let moved = preview.render(&editor, Layer::ThreeD, 1.)?;
        assert_ne!(first.particles, moved.particles);
        assert_eq!(first.items[0].motion_id, moved.items[0].motion_id);
        assert_eq!(editor.scene(), &authored);
        assert!(
            preview
                .render(&editor, Layer::TwoD, 1.)?
                .particles
                .is_empty()
        );
        editor.undo()?;
        assert_eq!(editor.scene(), &original);
        preview.advance(&editor, Duration::ZERO, false)?;
        assert!(
            preview
                .render(&editor, Layer::ThreeD, 1.)?
                .particles
                .is_empty()
        );
        Ok(())
    }
    #[test]
    fn wet_material_volume_roundtrip_and_undo() -> Result<()> {
        let mut editor = Editor::new(
            bozzard_demo::scene_document()?,
            std::path::Path::new("/tmp/bozzard-effects-material.json"),
        )?;
        editor.selected = editor
            .scene
            .objects
            .iter()
            .find(|o| {
                o.drawable
                    .as_ref()
                    .is_some_and(|d| d.layer == Layer::ThreeD)
            })
            .map(|o| o.id.clone());
        let original = editor.scene().clone();
        editor.apply_wet_material()?;
        let render = editor.render(Layer::ThreeD, 1.)?;
        assert!(render.display.reflections.enabled);
        assert!(
            render
                .items
                .iter()
                .any(|i| i.material.roughness == Some(0.12))
        );
        editor.create_effect_volume(Vec3::new(1., 2., 3.))?;
        assert_eq!(editor.scene.post_process_volumes[0].center, [1., 2., 3.]);
        assert_eq!(Scene::from_json(&editor.scene.to_json()?)?, editor.scene);
        editor.undo()?;
        editor.undo()?;
        assert_eq!(editor.scene(), &original);
        Ok(())
    }
}
