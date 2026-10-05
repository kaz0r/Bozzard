//! Guarded background clip baking. One job is shared across animation inspectors.
use anyhow::{Context as _, Result, ensure};
use bozzard_assets::{AssetData, AssetStore, job::Job};
use bozzard_scene::{
    AssetKind, Scene,
    middleware::animation::{
        Animator, Motion, StateDefinition,
        data::{Clip, Rig},
        retarget::{BoneMapping, RetargetMap},
    },
};
use eframe::egui;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct Task {
    owner: String,
    target: Arc<Rig>,
    job: Job<Clip>,
}
type SharedTask = Arc<Mutex<Option<Task>>>;
#[derive(Clone)]
struct Form {
    source: String,
    clip: usize,
    name: String,
    rate: f32,
    map: RetargetMap,
    signatures: Option<(String, u64, u64)>,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            source: String::new(),
            clip: 0,
            name: "Reused animation".into(),
            rate: 30.,
            map: RetargetMap {
                bones: vec![],
                translation_scale: 1.,
            },
            signatures: None,
        }
    }
}
fn shared_task(ui: &egui::Ui) -> SharedTask {
    let id = egui::Id::new("animation-retarget-job");
    ui.ctx().data_mut(|data| {
        data.get_temp::<SharedTask>(id).unwrap_or_else(|| {
            let task = Arc::new(Mutex::new(None));
            data.insert_temp(id, task.clone());
            task
        })
    })
}
fn poll(ui: &mut egui::Ui, task: &SharedTask, owner: &str, animator: &mut Animator) -> Result<()> {
    let mut slot = task
        .lock()
        .map_err(|_| anyhow::anyhow!("retarget job state unavailable"))?;
    let Some(current) = slot.as_ref() else {
        return Ok(());
    };
    if current.owner != owner {
        ui.small("An animation import for another object is running or ready.");
        if ui.button("Cancel other animation import").clicked() {
            *slot = None;
        }
        return Ok(());
    }
    if let Some(result) = current.job.poll() {
        let target = current.target.clone();
        *slot = None;
        let clip = result?;
        ensure!(
            animator.rig == target,
            "Animation import discarded: the target skeleton or clips changed. Try again."
        );
        ensure!(
            !animator
                .rig
                .clips
                .iter()
                .any(|existing| existing.name == clip.name),
            "An animation with that name already exists"
        );
        install_clip(animator, clip);
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label("Preparing animation…");
            if ui.button("Cancel").clicked() {
                current.job.cancel();
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    Ok(())
}
fn install_clip(animator: &mut Animator, clip: Clip) {
    let index = animator.rig.clips.len();
    Arc::make_mut(&mut animator.rig).clips.push(clip);
    if animator.states.is_empty() {
        animator.initial = "Imported animation".into();
        animator.states = Arc::new(vec![StateDefinition {
            name: animator.initial.clone(),
            motion: Motion::Clip { clip: index },
            repeat: bozzard_scene::middleware::curve::Repeat::Loop,
        }]);
    }
}
pub(super) fn editor(
    ui: &mut egui::Ui,
    animator: &mut Animator,
    assets: &AssetStore,
    scene: &Scene,
    owner: &str,
) -> Result<()> {
    let task = shared_task(ui);
    poll(ui, &task, owner, animator)?;
    let mut result = Ok(());
    ui.collapsing("Reuse an animation from another character", |ui| {
        ui.small("Map matching bones, adjust movement scale, then import a reusable clip. Limb proportions come from this character.");
        let id = ui.make_persistent_id("retarget-form");
        let mut form = ui.data_mut(|d| d.get_temp::<Form>(id).unwrap_or_default());
        egui::ComboBox::from_id_salt("retarget-source").selected_text(if form.source.is_empty() { "Choose source model" } else { &form.source })
            .show_ui(ui, |ui| {
                for (id, source) in &scene.assets {
                    if source.kind == AssetKind::Mesh {
                        ui.selectable_value(&mut form.source, id.clone(), id);
                    }
                }
            });
        if let Some(AssetData::Mesh(mesh)) = assets.handle(&form.source).and_then(|h| assets.get(h)).and_then(|e| e.data())
            && let Some(skin) = &mesh.skin {
            let signatures = (form.source.clone(), skin.rig.signature(), animator.rig.signature());
            if form.signatures.as_ref() != Some(&signatures) {
                form.map = RetargetMap::by_name(&skin.rig, &animator.rig);
                form.clip = 0;
                form.signatures = Some(signatures);
            }
            source_form(ui, &mut form, &skin.rig, &animator.rig);
            let idle = task.lock().is_ok_and(|slot| slot.is_none());
            if ui.add_enabled(idle && !form.map.bones.is_empty() && !skin.rig.clips.is_empty(),
                egui::Button::new("Import mapped animation")).clicked() {
                result = start(&task, owner, animator, &skin.rig, &form, ui.ctx().clone());
            }
        } else if !form.source.is_empty() { ui.weak("Load a model with a skeleton and animation clips."); }
        ui.data_mut(|d| d.insert_temp(id, form));
    });
    result
}
fn source_form(ui: &mut egui::Ui, form: &mut Form, source: &Rig, target: &Rig) {
    super::clip_picker(ui, "source-clip", &mut form.clip, source);
    ui.label("New clip name");
    ui.text_edit_singleline(&mut form.name);
    ui.add(
        egui::DragValue::new(&mut form.map.translation_scale)
            .speed(0.01)
            .range(0.001..=1000.)
            .prefix("Movement scale "),
    );
    ui.add(
        egui::DragValue::new(&mut form.rate)
            .range(1.0..=120.)
            .suffix(" samples / second"),
    );
    ui.collapsing(
        format!("Bone mapping · {} matched", form.map.bones.len()),
        |ui| {
            ui.small("Matching uses bone names. Review each pair; map unmatched bones explicitly.");
            mapping_rows(ui, &mut form.map, source, target);
        },
    );
}
fn mapping_rows(ui: &mut egui::Ui, map: &mut RetargetMap, source: &Rig, target: &Rig) {
    let mut remove = None;
    for (index, mapping) in map.bones.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.label("Source");
            super::required_bone_picker(ui, "source-bone", &mut mapping.source, source);
            ui.label("This character");
            super::required_bone_picker(ui, "target-bone", &mut mapping.target, target);
            ui.checkbox(&mut mapping.translation, "Copy scaled translation");
            if ui.button("Remove mapping").clicked() {
                remove = Some(index);
            }
            ui.separator();
        });
    }
    if let Some(index) = remove {
        map.bones.remove(index);
    }
    if ui.button("Add bone mapping").clicked()
        && let Some(target) =
            (0..target.nodes.len()).find(|i| !map.bones.iter().any(|b| b.target == *i))
        && let Some(source) =
            (0..source.nodes.len()).find(|i| !map.bones.iter().any(|b| b.source == *i))
    {
        map.bones.push(BoneMapping {
            source,
            target,
            translation: false,
        });
    }
}
fn start(
    task: &SharedTask,
    owner: &str,
    animator: &Animator,
    source: &Arc<Rig>,
    form: &Form,
    context: egui::Context,
) -> Result<()> {
    ensure!(
        !animator.rig.clips.iter().any(|clip| clip.name == form.name),
        "Choose a unique clip name"
    );
    form.map.validate(source, &animator.rig)?;
    let target = animator.rig.clone();
    let source = source.clone();
    let form = form.clone();
    let worker_target = target.clone();
    let job = Job::start("Retargeting animation", move |progress| {
        let clip = form.map.bake_clip_with(
            &source,
            &worker_target,
            form.clip,
            form.name,
            form.rate,
            || progress.check().is_err(),
        );
        context.request_repaint();
        clip
    })?;
    let mut slot = task.lock().ok().context("retarget job state unavailable")?;
    ensure!(slot.is_none(), "Another animation import is still running");
    *slot = Some(Task {
        owner: owner.into(),
        target,
        job,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bozzard_scene::middleware::registry::Authored;

    fn clip(name: &str) -> Clip {
        Clip {
            name: name.into(),
            duration: 1.,
            channels: vec![],
            events: vec![],
        }
    }

    #[test]
    fn first_retargeted_clip_creates_a_playable_state_and_keeps_later_authoring() {
        let mut animator = Animator::default();
        install_clip(&mut animator, clip("Wave"));
        animator.validate().unwrap();
        assert_eq!(animator.initial, "Imported animation");
        assert_eq!(animator.states[0].motion, Motion::Clip { clip: 0 });
        Arc::make_mut(&mut animator.states)[0].name = "Gesture".into();
        animator.initial = "Gesture".into();
        let expected = animator.states.clone();
        install_clip(&mut animator, clip("Aim"));
        animator.validate().unwrap();
        assert_eq!(animator.initial, "Gesture");
        assert_eq!(animator.states, expected);
        assert_eq!(animator.rig.clips.len(), 2);
    }
}
