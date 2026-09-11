//! Release-mode CPU timings for the editor paths, separate from rendering/GPU time.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use bozzard_scene::Layer;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn report(label: &str, times: &mut [f64]) {
    times.sort_by(f64::total_cmp);
    let n = times.len();
    let median = (times[(n - 1) / 2] + times[n / 2]) * 0.5;
    let p95 = times[(n * 95).div_ceil(100) - 1];
    println!("editor_benchmark path={label} samples={n} median_ms={median:.6} p95_ms={p95:.6}");
}

fn measure<T>(
    label: &str,
    samples: usize,
    mut operation: impl FnMut(usize) -> Result<T>,
) -> Result<()> {
    let mut times = Vec::with_capacity(samples);
    for iteration in 0..samples + 10 {
        let start = Instant::now();
        black_box(operation(iteration % samples)?);
        if iteration >= 10 {
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    report(label, &mut times);
    Ok(())
}

fn measure_picks(
    editor: &Editor,
    projection: glam::Mat4,
    samples: usize,
    grid: bool,
) -> Result<()> {
    let mut times = [Vec::new(), Vec::new()];
    for i in 0..samples + 10 {
        let ndc = if grid {
            [
                (i % samples % 17) as f32 / 8.0 - 1.0,
                (((i % samples) * 7 / 17) % 17) as f32 / 8.0 - 1.0,
            ]
        } else {
            [0., 0.]
        };
        let mut hits = [None, None];
        // Alternate which implementation runs first; compare identical queries outside timing.
        for mode in [i % 2, 1 - i % 2] {
            let start = Instant::now();
            hits[mode] = black_box(if mode == 0 {
                editor.pick_surface_with_projection(Layer::ThreeD, projection, ndc)?
            } else {
                editor.pick_surface_reference_with_projection(Layer::ThreeD, projection, ndc)?
            });
            if i >= 10 {
                times[mode].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        ensure!(
            hits[0] == hits[1],
            "accelerated picking differs at {ndc:?}: {hits:?}"
        );
    }
    for (mode, times) in times.iter_mut().enumerate() {
        report(
            &format!(
                "pick_{}_{}",
                if grid { "grid" } else { "center" },
                if mode == 0 { "bvh" } else { "linear" }
            ),
            times,
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(
        args.next()
            .context("usage: benchmark_editor SCENE.json [SAMPLES]")?,
    );
    let samples = args.next().map(|n| n.parse()).transpose()?.unwrap_or(200);
    ensure!(
        (10..=2000).contains(&samples),
        "samples must be in 10..2000"
    );
    let start = Instant::now();
    let mut editor = Editor::open(&path)?;
    println!(
        "editor_benchmark build={} load_ms={:.3} scene={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        start.elapsed().as_secs_f64() * 1000.0,
        path.display()
    );
    let original = editor.scene().clone();
    for entry in editor.assets.entries() {
        if let Some(stats) = entry.mesh_pick_stats() {
            println!(
                "mesh_picking_index asset={} triangles={} nodes={} resident_bytes={} build_ms={:.3}",
                entry.id, stats.triangles, stats.nodes, stats.bytes, stats.build_ms
            );
        }
    }
    let layer = Layer::ThreeD;
    let projection = editor.render(layer, 1.6)?.view_projection;
    let hit = editor.pick_surface_with_projection(layer, projection, [0., 0.])?;
    println!("editor_benchmark center_hit={hit:?}");
    editor.select_pick(hit)?;
    measure("render_extract", samples, |_| editor.render(layer, 1.6))?;
    measure("collision_overlay", samples, |_| editor.collisions())?;
    measure("selected_surface_bounds", samples, |_| {
        editor.selected_surface_corners(layer)
    })?;
    measure_picks(&editor, projection, samples, false)?;
    measure_picks(&editor, projection, samples, true)?;
    // Wider than the viewport to include misses and silhouette/grazing rays.
    let mut hits = 0;
    for y in -20..=20 {
        for x in -20..=20 {
            let ndc = [x as f32 / 16., y as f32 / 16.];
            let actual = editor.pick_surface_with_projection(layer, projection, ndc)?;
            ensure!(
                actual == editor.pick_surface_reference_with_projection(layer, projection, ndc)?,
                "pick oracle mismatch at {ndc:?}"
            );
            hits += usize::from(actual.is_some());
        }
    }
    println!("editor_pick_oracle_ok rays=1681 hits={hits} exact_object_and_surface=true");
    ensure!(
        editor.scene() == &original && !editor.dirty() && editor.undo_label().is_none(),
        "benchmark changed the document/history"
    );
    Ok(())
}
