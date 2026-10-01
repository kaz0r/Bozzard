//! Native GPU proof using editor Play's prepared-frame pipeline and synthetic refresh clocks.
use anyhow::{Result, ensure};
use bozzard_editor::Editor;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::Layer;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

fn main() -> Result<()> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "work/interpolation/native".into());
    std::fs::create_dir_all(&output)?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/interpolation-lab.json");
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    println!("adapter={:?}", gpu.adapter.get_info());
    let mut reference = BTreeMap::new();
    for hz in [60_u128, 120, 144] {
        for interpolated in [false, true] {
            for threaded in [false, true] {
                let mut editor = Editor::open(&path)?;
                editor.start_play()?;
                let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
                for entry in editor.assets.entries() {
                    if let Some(data) = entry.data() {
                        bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
                    }
                }
                let step = editor.play.as_ref().unwrap().app.timestep().as_nanos();
                let mut time = 0;
                for frame in 0..hz + 2 {
                    let next_time = (frame + 1) * step * 60 / hz;
                    editor.prepare_simulation_frame_with_interpolation(
                        Duration::from_nanos((next_time - time) as u64),
                        threaded,
                        interpolated,
                    )?;
                    if frame >= hz - 1 {
                        let scene = editor.render(Layer::ThreeD, 16. / 9.)?;
                        let capture =
                            bozzard_render::capture_offscreen(&gpu, 960, 540, |target| {
                                editor.render_with_simulation(|| {
                                    renderer.draw(&gpu, target, [960, 540], &scene)
                                })?
                            })?;
                        let key = (hz, interpolated, frame);
                        if threaded {
                            ensure!(
                                reference[&key] == capture.rgba,
                                "worker changed pixels: {key:?}"
                            );
                        } else {
                            reference.insert(key, capture.rgba.clone());
                        }
                        capture.write_ppm(&output.join(format!(
                            "hz{hz}-interpolation{interpolated}-threaded{threaded}-frame{frame}.ppm"
                        )))?;
                    }
                    editor.finish_simulation_frame()?;
                    time = next_time;
                }
                editor.stop_play();
            }
        }
    }
    ensure!(
        reference
            .iter()
            .filter(|((hz, interp, _), _)| *hz == 144 && *interp)
            .any(|((hz, _, frame), image)| image != &reference[&(*hz, false, *frame)]),
        "interpolation did not change moving-world pixels"
    );
    println!(
        "interpolation_capture_ok: 36 captures; serial/worker pixels identical at 60/120/144 Hz"
    );
    Ok(())
}
