//! Synthetic scaling of document reads and GI freshness; no GPU or UI painting.
use anyhow::{Result, ensure};
use bozzard_editor::{Editor, EffectsPreview};
use bozzard_scene::{BakedGi, GI_PROBE_STRIDE, Layer};
use std::{hint::black_box, sync::Arc, time::Instant};

fn measure<T>(label: &str, mut operation: impl FnMut() -> Result<T>) -> Result<()> {
    let mut times = Vec::new();
    for i in 0..210 {
        let start = Instant::now();
        black_box(operation()?);
        if i >= 10 {
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "document_benchmark path={label} median_ms={:.6} p95_ms={:.6}",
        (times[99] + times[100]) * 0.5,
        times[189]
    );
    Ok(())
}

fn main() -> Result<()> {
    let mut scene = bozzard_demo::scene_document()?;
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
    let demo = bozzard_demo::SceneDemo::new(editor.scene())?;
    measure("extract_runtime_reference", || {
        bozzard_editor::extract(&demo, &editor.assets, Layer::ThreeD, 1.6)
    })?;
    measure("extract_authoring", || editor.render(Layer::ThreeD, 1.6))?;
    let preview = EffectsPreview::new(&editor)?;
    measure("extract_effects_preview", || {
        preview.render(&editor, Layer::ThreeD, 1.6)
    })?;
    ensure!(
        *editor.scene_snapshot() == *editor.scene(),
        "snapshot changed the scene"
    );
    Ok(())
}
