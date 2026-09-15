//! Navigation baking and state-machine authoring use the same undo transaction as other fields.
use anyhow::Result;
use bozzard_scene::{
    Object, Scene,
    middleware::{
        navigation::{Behavior, Condition, NavAgent, NavData, NavSurface, State, Transition},
        registry,
    },
};
use eframe::egui::{self, Color32, Stroke};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Clone, Default)]
struct BakeJob {
    done: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<std::result::Result<NavData, String>>>>,
}
#[derive(Clone)]
struct NavPreview {
    scene: std::sync::Weak<Scene>,
    owner: String,
    signature: u64,
    rect: egui::Rect,
    mesh: Arc<egui::Mesh>,
    triangles: usize,
    current: bool,
}
fn object_choice(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<String>,
    scene: &Scene,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.as_deref().unwrap_or("None"))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "None");
            for object in &scene.objects {
                ui.selectable_value(value, Some(object.id.clone()), &object.name);
            }
        });
}
fn vector(ui: &mut egui::Ui, value: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        for n in value {
            ui.add(egui::DragValue::new(n).speed(0.1).range(-1e6..=1e6));
        }
    });
}
pub fn component(
    ui: &mut egui::Ui,
    object: &mut Object,
    name: &str,
    snapshot: &Arc<Scene>,
) -> Result<()> {
    let scene = snapshot.as_ref();
    if name == "nav_surface" {
        let Some(mut surface) = registry::get::<NavSurface>(object)? else {
            return Ok(());
        };
        let job_id = egui::Id::new(("navigation_bake", &object.id));
        let job = ui.ctx().data(|data| data.get_temp::<BakeJob>(job_id));
        if let Some(job) = job {
            let result = job.result.lock().unwrap().take();
            if let Some(result) = result {
                ui.ctx().data_mut(|data| data.remove::<BakeJob>(job_id));
                if !job.cancel.load(Ordering::Relaxed) {
                    let baked = result.map_err(anyhow::Error::msg)?;
                    anyhow::ensure!(
                        baked.settings == surface.settings,
                        "Navigation settings changed during bake; bake again"
                    );
                    let demo = bozzard_demo::SceneDemo::new(scene)?;
                    let geometry = demo.instance().navigation_geometry(&demo.app.world)?;
                    anyhow::ensure!(
                        bozzard_scene::middleware::navigation::geometry_signature(&geometry)
                            == baked.geometry_signature,
                        "Colliders changed during navigation bake; bake again"
                    );
                    surface.baked = Some(Arc::new(baked));
                    registry::set(object, &surface)?;
                }
            } else {
                let total = surface
                    .settings
                    .dimensions()?
                    .into_iter()
                    .product::<usize>();
                ui.add(
                    egui::ProgressBar::new(job.done.load(Ordering::Relaxed) as f32 / total as f32)
                        .text("Baking navigation…"),
                );
                if ui.button("Cancel bake").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(40));
            }
        } else if ui.button("Bake navigation from scene colliders").clicked() {
            let mut document = scene.clone();
            if let Some(o) = document.objects.iter_mut().find(|o| o.id == object.id) {
                *o = object.clone();
            }
            let settings = surface.settings.clone();
            let job = BakeJob::default();
            ui.ctx()
                .data_mut(|data| data.insert_temp(job_id, job.clone()));
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<NavData> {
                    let demo = bozzard_demo::SceneDemo::new(&document)?;
                    demo.instance()
                        .bake_navigation(&demo.app.world, &settings, |done, _| {
                            anyhow::ensure!(
                                !job.cancel.load(Ordering::Relaxed),
                                "Navigation bake canceled"
                            );
                            job.done.store(done, Ordering::Relaxed);
                            Ok(())
                        })
                })()
                .map_err(|error| format!("{error:#}"));
                *job.result.lock().unwrap() = Some(result);
                ctx.request_repaint();
            });
        }
        if let Some(data) = &surface.baked {
            let size = egui::vec2(ui.available_width().min(300.), 180.);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 2., Color32::from_gray(20));
            let id = egui::Id::new("navigation_preview_cache");
            let source = Arc::downgrade(snapshot);
            let mut preview = ui.data(|d| d.get_temp::<NavPreview>(id));
            if preview.as_ref().is_none_or(|p| {
                !p.scene.ptr_eq(&source)
                    || p.owner != object.id
                    || p.signature != data.geometry_signature
                    || p.rect != rect
            }) {
                let scale = (rect.width() / data.dimensions[0] as f32)
                    .min(rect.height() / data.dimensions[1] as f32);
                let origin = rect.center()
                    - egui::vec2(data.dimensions[0] as f32, data.dimensions[1] as f32)
                        * scale
                        * 0.5;
                let mut mesh = egui::Mesh::default();
                let mut triangles = 0;
                for (i, cell) in data.cells.iter().enumerate() {
                    if cell.is_some() {
                        let min = origin
                            + egui::vec2(
                                (i % data.dimensions[0]) as f32,
                                (i / data.dimensions[0]) as f32,
                            ) * scale;
                        mesh.add_colored_rect(
                            egui::Rect::from_min_size(min, egui::Vec2::splat(scale)),
                            Color32::from_rgb(44, 145, 108),
                        );
                        triangles += 2;
                    }
                }
                let demo = bozzard_demo::SceneDemo::new(scene)?;
                let current = bozzard_scene::middleware::navigation::geometry_signature(
                    &demo.instance().navigation_geometry(&demo.app.world)?,
                ) == data.geometry_signature;
                preview = Some(NavPreview {
                    scene: source,
                    owner: object.id.clone(),
                    signature: data.geometry_signature,
                    rect,
                    mesh: Arc::new(mesh),
                    triangles,
                    current,
                });
                ui.data_mut(|d| d.insert_temp(id, preview.clone().unwrap()));
            }
            let preview = preview.unwrap();
            ui.painter().add(egui::Shape::Mesh(preview.mesh));
            ui.label(format!(
                "{} walkable triangles · {} cells",
                preview.triangles,
                data.cells.len()
            ));
            if preview.current {
                ui.small("Bake matches current static collision geometry.");
            } else {
                ui.colored_label(
                    Color32::YELLOW,
                    "Stale bake · static colliders changed; rebake before Play.",
                );
            }
            ui.painter().rect_stroke(
                rect,
                2.,
                Stroke::new(1., Color32::GRAY),
                egui::StrokeKind::Inside,
            );
            ui.small(
                "Top view · green is walkable. Rebake after moving or changing static colliders.",
            );
        } else {
            ui.weak("No navigation mesh baked yet.");
        }
    } else if name == "nav_agent" {
        let Some(mut agent) = registry::get::<NavAgent>(object)? else {
            return Ok(());
        };
        let before = agent.clone();
        ui.label("Perception target");
        object_choice(ui, "perception", &mut agent.perception_target, scene);
        egui::ComboBox::from_id_salt("initial")
            .selected_text(&agent.initial)
            .show_ui(ui, |ui| {
                for state in agent.states.iter() {
                    ui.selectable_value(&mut agent.initial, state.name.clone(), &state.name);
                }
            });
        let mut remove = None;
        for index in 0..agent.states.len() {
            let mut state = agent.states[index].clone();
            let old = state.clone();
            egui::CollapsingHeader::new(format!("State · {}", state.name))
                .id_salt(("nav_state", index))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut state.name).char_limit(128));
                        if ui
                            .add_enabled(agent.states.len() > 1, egui::Button::new("Remove"))
                            .clicked()
                        {
                            remove = Some(index);
                        }
                    });
                    egui::ComboBox::from_id_salt("behavior")
                        .selected_text(format!("{:?}", state.behavior))
                        .show_ui(ui, |ui| {
                            for mode in [
                                Behavior::Idle,
                                Behavior::MoveTo,
                                Behavior::Follow,
                                Behavior::Flee,
                                Behavior::Patrol,
                            ] {
                                if ui
                                    .selectable_value(
                                        &mut state.behavior,
                                        mode,
                                        format!("{mode:?}"),
                                    )
                                    .changed()
                                    && mode == Behavior::Patrol
                                    && state.patrol.is_empty()
                                {
                                    state.patrol.push(object.transform.translation);
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Speed multiplier");
                        ui.add(
                            egui::DragValue::new(&mut state.speed)
                                .speed(0.05)
                                .range(0.0..=4.),
                        );
                    });
                    if matches!(state.behavior, Behavior::Follow | Behavior::Flee) {
                        object_choice(ui, "target", &mut state.target, scene);
                    }
                    if state.behavior == Behavior::MoveTo {
                        ui.label("World destination");
                        vector(ui, &mut state.destination);
                    }
                    if state.behavior == Behavior::Patrol {
                        let mut delete = None;
                        for (i, p) in state.patrol.iter_mut().enumerate() {
                            ui.push_id(i, |ui| {
                                vector(ui, p);
                                if ui.small_button("Remove waypoint").clicked() {
                                    delete = Some(i);
                                }
                            });
                        }
                        if state.patrol.len() > 1
                            && let Some(i) = delete
                        {
                            state.patrol.remove(i);
                        }
                        if ui
                            .add_enabled(
                                state.patrol.len() < 128,
                                egui::Button::new("Add waypoint"),
                            )
                            .clicked()
                        {
                            state.patrol.push(object.transform.translation);
                        }
                    }
                });
            if state != old {
                if state.name != old.name {
                    if agent.initial == old.name {
                        agent.initial = state.name.clone();
                    }
                    for t in Arc::make_mut(&mut agent.transitions) {
                        if t.from == old.name {
                            t.from = state.name.clone();
                        }
                        if t.to == old.name {
                            t.to = state.name.clone();
                        }
                    }
                }
                Arc::make_mut(&mut agent.states)[index] = state;
            }
        }
        if let Some(i) = remove {
            let name = Arc::make_mut(&mut agent.states).remove(i).name;
            Arc::make_mut(&mut agent.transitions).retain(|t| t.from != name && t.to != name);
            if agent.initial == name {
                agent.initial = agent.states[0].name.clone();
            }
        }
        if ui
            .add_enabled(agent.states.len() < 64, egui::Button::new("Add state"))
            .clicked()
        {
            let mut n = agent.states.len() + 1;
            while agent.states.iter().any(|s| s.name == format!("State {n}")) {
                n += 1;
            }
            Arc::make_mut(&mut agent.states).push(State {
                name: format!("State {n}"),
                ..Default::default()
            });
        }
        ui.separator();
        ui.label("Transitions (first matching rule wins)");
        let mut remove = None;
        for i in 0..agent.transitions.len() {
            let mut t = agent.transitions[i].clone();
            let old = t.clone();
            ui.push_id(("transition", i), |ui| {
                ui.horizontal(|ui| {
                    for (key, value, any) in [("from", &mut t.from, true), ("to", &mut t.to, false)]
                    {
                        egui::ComboBox::from_id_salt(key)
                            .selected_text(value.as_str())
                            .show_ui(ui, |ui| {
                                if any {
                                    ui.selectable_value(value, "*".into(), "Any state");
                                }
                                for state in agent.states.iter() {
                                    ui.selectable_value(value, state.name.clone(), &state.name);
                                }
                            });
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
                egui::ComboBox::from_id_salt("condition")
                    .selected_text(format!("{:?}", t.condition))
                    .show_ui(ui, |ui| {
                        for c in [
                            Condition::SeeTarget,
                            Condition::LostTarget,
                            Condition::Arrived,
                            Condition::After,
                            Condition::Blocked,
                        ] {
                            ui.selectable_value(&mut t.condition, c, format!("{c:?}"));
                        }
                    });
                if t.condition == Condition::After {
                    ui.add(
                        egui::DragValue::new(&mut t.seconds)
                            .speed(0.1)
                            .range(0.0..=86400.)
                            .suffix(" seconds"),
                    );
                }
            });
            if t != old {
                Arc::make_mut(&mut agent.transitions)[i] = t;
            }
        }
        if let Some(i) = remove {
            Arc::make_mut(&mut agent.transitions).remove(i);
        }
        if ui
            .add_enabled(
                agent.transitions.len() < 256,
                egui::Button::new("Add transition"),
            )
            .clicked()
        {
            Arc::make_mut(&mut agent.transitions).push(Transition {
                from: agent.initial.clone(),
                to: agent.states.last().unwrap().name.clone(),
                ..Default::default()
            });
        }
        if agent != before {
            registry::set(object, &agent)?;
        }
    }
    Ok(())
}
