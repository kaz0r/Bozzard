//! Document reads, GI freshness and authoring frames on a synthetic scene and on
//! the Pagoda Garden; no GPU or UI painting. Usage:
//! `benchmark_documents [all|synthetic|pagoda] [path/to/pagoda.json]`.
use anyhow::{Context, Result, ensure};
use bozzard_editor::{Editor, EffectsPreview, OpenScenes};
use bozzard_scene::{BakedGi, GI_PROBE_STRIDE, Layer};
use glam::Mat4;
use std::{
    hint::black_box,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn report(label: &str, mut times: Vec<f64>) {
    times.sort_by(f64::total_cmp);
    let n = times.len();
    println!(
        "document_benchmark path={label} samples={n} median_ms={:.6} p95_ms={:.6}",
        (times[(n - 1) / 2] + times[n / 2]) * 0.5,
        times[(n * 95).div_ceil(100) - 1]
    );
}

fn measure<T>(label: &str, mut operation: impl FnMut() -> Result<T>) -> Result<()> {
    measure_n(label, 200, &mut (), |_| Ok(()), |_, ()| operation())
}

/// Untimed `setup` runs before every sample, so state-changing operations start alike.
fn measure_n<C, S, T>(
    label: &str,
    samples: usize,
    context: &mut C,
    mut setup: impl FnMut(&mut C) -> Result<S>,
    mut operation: impl FnMut(&mut C, S) -> Result<T>,
) -> Result<()> {
    let mut times = Vec::new();
    for i in 0..samples + 10 {
        let state = setup(context)?;
        let start = Instant::now();
        black_box(operation(context, state)?);
        if i >= 10 {
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    report(label, times);
    Ok(())
}

fn synthetic() -> Result<()> {
    let mut scene = bozzard_runtime::scene_document()?;
    let mut template = scene
        .objects
        .iter()
        .find(|o| {
            o.drawable
                .as_ref()
                .is_some_and(|d| d.layer == Layer::ThreeD)
        })
        .unwrap()
        .clone();
    template.parent = None;
    template.spin = None;
    template.drawable.as_mut().unwrap().gi_static = true;
    for i in 0..1536 {
        let mut object = template.clone();
        object.id = format!("bench-{i}");
        object.transform.translation = [(i % 32) as f32 * 2., 0., (i / 32) as f32 * 2.];
        scene.objects.push(object);
    }
    scene.gi.volume.resolution = [2; 3];
    let mut editor = Editor::new(scene, std::path::Path::new("/tmp/document-benchmark.json"))?;
    let mut scene = editor.scene().clone();
    scene.gi.baked = Some(Arc::new(BakedGi::new(
        bozzard_assets::gi::source(&scene, &editor.assets, scene.gi.volume)?,
        scene.gi.volume,
        Arc::new(vec![[0.; 4]; 8 * GI_PROBE_STRIDE]),
    )?));
    scene.gi.enabled = true;
    editor.apply("Synthetic GI fixture", scene)?;
    ensure!(editor.gi_current(), "fixture fingerprint must be current");
    println!(
        "document_benchmark objects={}",
        editor.scene().objects.len()
    );
    measure("clone_document", || Ok(editor.scene().clone()))?;
    measure("shared_document", || Ok(editor.scene_snapshot()))?;
    measure("gi_freshness_reference", || {
        bozzard_assets::gi::is_current(editor.scene(), &editor.assets)
    })?;
    measure("gi_freshness_cached", || Ok(editor.gi_current()))?;
    // A display edit changes the revision but none of the bake inputs.
    measure_n(
        "gi_freshness_after_display_edit",
        200,
        &mut editor,
        |editor| {
            let mut scene = editor.scene().clone();
            scene.display.exposure_ev = if scene.display.exposure_ev == 0. {
                0.5
            } else {
                0.
            };
            editor.apply("Exposure", scene)
        },
        |editor, ()| {
            ensure!(editor.gi_current(), "display edits keep the bake current");
            Ok(())
        },
    )?;
    let demo = bozzard_runtime::SceneRuntime::new(editor.scene())?;
    measure("extract_runtime_reference", || {
        bozzard_editor::extract(&demo, &editor.assets, Layer::ThreeD, 1.6)
    })?;
    measure("extract_authoring", || editor.render(Layer::ThreeD, 1.6))?;
    let mut preview = EffectsPreview::new(&editor)?;
    measure("extract_effects_preview", || {
        preview.render(&editor, Layer::ThreeD, 1.6)
    })?;
    ensure!(
        *editor.scene_snapshot() == *editor.scene(),
        "snapshot changed the scene"
    );
    Ok(())
}

const SIZE: [f32; 2] = [1280., 720.];
const ASPECT: f32 = SIZE[0] / SIZE[1];

/// The native editor's document work for one Edit frame, in the order the app runs it:
/// workspace view, effects preview, viewport extraction and widgets, then the
/// surface/light overlays, the transform gizmo and the dirty title.
struct Session {
    editor: Editor,
    scenes: OpenScenes,
    preview: EffectsPreview,
    pose: Mat4,
}
impl Session {
    fn frame(&mut self) -> Result<()> {
        self.scenes.sync_view(&self.editor)?;
        let view = self.scenes.view(&self.editor);
        self.preview
            .advance(view, Duration::from_millis(16), true)?;
        let frame =
            self.preview
                .render_frame_from_camera(view, Layer::ThreeD, ASPECT, Some(self.pose))?;
        let widgets = view.ui_frame(Layer::ThreeD, SIZE)?;
        black_box((frame, widgets));
        self.idle_overlays()
    }
    /// Per-frame work that does not depend on extraction: light markers, gizmo and title.
    fn idle_overlays(&self) -> Result<()> {
        let hidden = self
            .scenes
            .hidden_objects_in(self.scenes.active(), &self.editor);
        black_box(self.editor.selected_surface_corners(Layer::ThreeD)?);
        black_box(self.light_transforms()?);
        if let Some(object) = self.editor.selected_object()
            && !hidden.contains(&object.id)
        {
            black_box(self.editor.selected_transform()?);
            black_box(self.editor.selected_transform_parent()?);
        }
        black_box(self.editor.dirty());
        black_box(self.scenes.any_dirty(&self.editor));
        Ok(())
    }
    fn light_transforms(&self) -> Result<usize> {
        let matrices = self.editor.world_transforms()?;
        Ok(self
            .editor
            .scene()
            .objects
            .iter()
            .zip(matrices.matrices())
            .filter(|(o, _)| o.light.is_some() || o.camera.is_some())
            .inspect(|(_, matrix)| {
                black_box(matrix);
            })
            .count())
    }
}

fn pagoda(path: PathBuf) -> Result<()> {
    let start = Instant::now();
    let editor = Editor::open(&path)?;
    println!(
        "document_benchmark pagoda objects={} assets={} load_ms={:.1}",
        editor.scene().objects.len(),
        editor.scene().assets.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    let projection = editor.render(Layer::ThreeD, ASPECT)?.view_projection;
    // The fly camera starts at the authored view.
    let pose = editor.scene().global_transforms()?[&editor.scene().views[&Layer::ThreeD]];
    let mut session = Session {
        preview: EffectsPreview::with_gpu_particles(&editor, true)?,
        editor,
        scenes: OpenScenes::default(),
        pose,
    };
    session.frame()?;
    // The legacy whole model nearest the center of the view, hit through its BVH.
    let mut points: Vec<_> = (-8..=8)
        .flat_map(|y| (-8..=8).map(move |x| [x as f32 / 10., y as f32 / 10.]))
        .collect();
    points.sort_by(|a, b| a[0].hypot(a[1]).total_cmp(&b[0].hypot(b[1])));
    let mut target = None;
    for ndc in points {
        if let Some(pick) =
            session
                .editor
                .pick_surface_with_projection(Layer::ThreeD, projection, ndc)?
            && pick.surface.is_some()
            && session.editor.splits_into_children(&pick.object)
        {
            target = Some((ndc, pick));
            break;
        }
    }
    let (ndc, pick) = target.context("no legacy model under the pagoda camera")?;
    println!(
        "document_benchmark pagoda pick={} surface={:?} ndc={ndc:?}",
        pick.object, pick.surface
    );

    measure("pagoda_validate", || session.editor.scene().validate())?;
    measure("pagoda_global_transforms", || {
        session.editor.scene().global_transforms()
    })?;
    measure("pagoda_dirty", || Ok(session.editor.dirty()))?;
    measure("pagoda_edit_world", || {
        bozzard_runtime::SceneRuntime::new(session.editor.scene())
    })?;
    measure("pagoda_effects_preview_build", || {
        EffectsPreview::with_gpu_particles(&session.editor, true)
    })?;

    // (c) Idle Edit frame with a whole model selected; the revision does not change.
    session.editor.select_object(Some(pick.object.clone()));
    session.frame()?;
    measure("pagoda_idle_overlays", || session.idle_overlays())?;
    measure("pagoda_idle_frame", || session.frame())?;

    // (e) Selection-only click on a legacy model's surface, then the next frame.
    measure_n(
        "pagoda_select_click",
        200,
        &mut session,
        |session| {
            session.editor.select_object(None);
            session.frame()
        },
        |session, ()| {
            let hit =
                session
                    .editor
                    .pick_surface_with_projection(Layer::ThreeD, projection, ndc)?;
            session.editor.select_component_pick(hit)?;
            session.frame()
        },
    )?;
    measure("pagoda_hover_pick", || {
        session
            .editor
            .pick_point_with_projection(Layer::ThreeD, projection, ndc)
    })?;
    let revision = session.editor.revision();
    ensure!(
        !session.editor.dirty() && session.editor.undo_label().is_none(),
        "selection must not edit the document (revision {revision})"
    );

    // (b) One gizmo drag frame: move the selected model, then draw the next frame.
    session.editor.select_object(Some(pick.object.clone()));
    let start = session.editor.selected_transform()?;
    session.editor.begin_gesture("Transform gizmo");
    let mut step = 0;
    measure_n(
        "pagoda_gizmo_drag_frame",
        200,
        &mut session,
        |_| Ok(()),
        |session, ()| {
            step += 1;
            let mut transform = start;
            transform.translation[0] += 0.001 * step as f32;
            black_box(session.editor.selected_transform_parent()?);
            session.editor.set_selected_transform(transform)?;
            session.frame()
        },
    )?;
    // A held mouse button without pointer movement re-applies the same transform.
    let held = session.editor.selected_transform()?;
    measure_n(
        "pagoda_gizmo_held_frame",
        200,
        &mut session,
        |_| Ok(()),
        |session, ()| {
            black_box(session.editor.selected_transform_parent()?);
            session.editor.set_selected_transform(held)?;
            session.frame()
        },
    )?;
    session.editor.finish_gesture();
    session.editor.undo()?;
    ensure!(
        session.editor.selected_transform()? == start && !session.editor.dirty(),
        "gizmo undo must restore the document"
    );
    session.frame()?;

    // (a) One structural edit (a new empty object), then the next frame.
    measure_n(
        "pagoda_structural_edit_frame",
        100,
        &mut session,
        |session| {
            if session.editor.undo_label().is_some() {
                session.editor.undo()?;
            }
            session.frame()
        },
        |session, ()| {
            session.editor.create_empty()?;
            session.frame()
        },
    )?;
    // The same edit split into the transaction and the frame that follows it.
    measure_n(
        "pagoda_structural_edit_only",
        100,
        &mut session,
        |session| {
            if session.editor.undo_label().is_some() {
                session.editor.undo()?;
            }
            session.frame()
        },
        |session, ()| session.editor.create_empty(),
    )?;
    measure_n(
        "pagoda_frame_after_edit",
        100,
        &mut session,
        |session| {
            if session.editor.undo_label().is_some() {
                session.editor.undo()?;
            }
            session.frame()?;
            session.editor.create_empty()
        },
        |session, ()| session.frame(),
    )?;
    measure_n(
        "pagoda_undo_frame",
        100,
        &mut session,
        |session| {
            session.editor.create_empty()?;
            session.frame()
        },
        |session, ()| {
            session.editor.undo()?;
            session.frame()
        },
    )?;
    while session.editor.undo_label().is_some() {
        session.editor.undo()?;
    }
    ensure!(!session.editor.dirty(), "undo must restore the saved scene");

    // (d) One background hot-reload pass over the unchanged catalog.
    measure_n(
        "pagoda_hot_reload_cycle",
        40,
        &mut session,
        |session| Ok(session.editor.assets.clone()),
        |_, mut store| {
            let changed = store.refresh_with(&Default::default())?;
            ensure!(changed.is_empty(), "nothing changed on disk");
            Ok(store)
        },
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "all".into());
    let path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/pagoda-garden/scenes/pagoda.json")
    });
    if matches!(mode.as_str(), "all" | "synthetic") {
        synthetic()?;
        // The inspector compares the selected prefab instance with its baseline each frame.
        let mut editor = Editor::open(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/demo/scenes/prefab-lab.json"),
        )?;
        editor.select_object(Some("cargo-2".into()));
        measure("prefab_inspector_overrides", || editor.prefab_overrides())?;
    }
    if matches!(mode.as_str(), "all" | "pagoda") {
        pagoda(path)?;
    }
    Ok(())
}
