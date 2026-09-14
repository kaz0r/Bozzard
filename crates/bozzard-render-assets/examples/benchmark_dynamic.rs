//! Independent frame histories for animated shadows and repeated graph-preview switches.
use anyhow::{Result, ensure};
use bozzard_render::*;
use bozzard_scene::shader_graph::{Node, NodeKind, ShaderGraph, Socket, Value, Wire};
use glam::{Mat4, Vec3};
use std::time::Instant;

#[path = "support/scene.rs"]
mod support;
use support::fixture;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    (values[(values.len() - 1) / 2] + values[values.len() / 2]) * 0.5
}

fn main() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("dynamic benchmark"),
            size: wgpu::Extent3d {
                width: 800,
                height: 500,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    for workload in ["moving_caster", "moving_light", "graph_switch"] {
        let mut renderers = [
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm),
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm),
        ];
        renderers[0].set_state_caching_enabled(false);
        let mut scenes = [fixture(), fixture()];
        let original = scenes[0].items[0].model;
        let graphs = (0..4)
            .map(|i| {
                let mut graph = ShaderGraph::default();
                graph.nodes.push(Node::new(2, NodeKind::Color, [0.; 2]));
                graph.nodes[1].inputs[0] = Value::Vector([0.2 + i as f32 * 0.15, 0.3, 0.6]);
                graph.connect(Wire {
                    from: Socket { node: 2, port: 0 },
                    to: Socket { node: 1, port: 0 },
                })?;
                bozzard_render_assets::shader_source(&graph)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut cpu = [Vec::new(), Vec::new()];
        let mut wall = [Vec::new(), Vec::new()];
        let mut shadow_draws = [0, 0];
        let samples = if workload == "graph_switch" { 24 } else { 100 };
        for i in 0..samples + 8 {
            for mode in [i % 2, 1 - i % 2] {
                let scene = &mut scenes[mode];
                match workload {
                    "moving_caster" => {
                        scene.items[0].model =
                            Mat4::from_translation(Vec3::new((i as f32 * 0.17).sin(), 0., 0.))
                                * original
                    }
                    "moving_light" => scene.lights[0].position[0] = -20. + (i as f32 * 0.17).sin(),
                    _ => {
                        scene.items.truncate(1);
                        scene.items[0].material.shader = Some(graphs[i % graphs.len()].clone());
                    }
                }
                let start = Instant::now();
                renderers[mode].draw(&gpu, &target, [800, 500], scene)?;
                let stats = renderers[mode].frame_stats();
                gpu.wait()?;
                if i >= 8 {
                    cpu[mode].push(stats.cpu_ms);
                    wall[mode].push(start.elapsed().as_secs_f64() * 1000.);
                    shadow_draws[mode] += stats.shadow_draws;
                }
            }
            // Readback repeats the sampled pose outside timing, equally in both histories.
            if i % 13 == 0 || i + 1 == samples + 8 {
                let mut pixels = Vec::new();
                for mode in 0..2 {
                    pixels.push(
                        capture_offscreen(&gpu, 800, 500, |view| {
                            renderers[mode].draw(&gpu, view, [800, 500], &scenes[mode])
                        })?
                        .rgba,
                    );
                }
                ensure!(
                    pixels[0] == pixels[1],
                    "independent-history pixels differ: {workload}, frame {i}"
                );
            }
        }
        for mode in 0..2 {
            println!(
                "dynamic_benchmark workload={workload} cached={} samples={samples} cpu_ms={:.6} wall_ms={:.6} shadow_draws={}",
                mode == 1,
                median(&mut cpu[mode]),
                median(&mut wall[mode]),
                shadow_draws[mode]
            );
        }
    }
    Ok(())
}
