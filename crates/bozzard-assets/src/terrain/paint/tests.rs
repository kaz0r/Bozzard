use super::*;
use crate::{cooked_model, job::Job};
use std::{hint::black_box, time::Instant};

fn brush(layer: usize) -> TerrainPaintBrush {
    TerrainPaintBrush {
        layer,
        center: [0., 0.],
        radius: 1.,
        strength: 1.,
    }
}

#[test]
fn legacy_sources_keep_unpainted_meshes_and_paint_preserves_every_geometry_bit() -> Result<()> {
    let legacy = br#"{"version":1,"resolution":[3,3],"size":[4.0,4.0],"heights":[0.0,1.0,2.0,2.0,3.0,4.0,4.0,5.0,6.0]}"#;
    let mut terrain = Terrain::from_json(legacy)?;
    assert!(terrain.paint.is_none());
    assert_eq!(terrain.to_json()?, legacy);
    let before = terrain.mesh(&Progress::default())?;
    assert!(before.parts.is_empty());
    let samples: Vec<_> = [[-1., -1.], [0., 0.], [1., 1.]]
        .into_iter()
        .map(|p| terrain.sample(p))
        .collect();
    assert!(terrain.paint(brush(1))?);
    let after = terrain.mesh(&Progress::default())?;
    assert_eq!(after.vertices, before.vertices);
    assert_eq!(after.indices, before.indices);
    assert_eq!(
        [[-1., -1.], [0., 0.], [1., 1.]]
            .into_iter()
            .map(|p| terrain.sample(p))
            .collect::<Vec<_>>(),
        samples
    );
    assert_eq!(after.parts.len(), 1);
    assert_eq!(after.parts[0].count as usize, after.indices.len());
    assert_eq!(after.parts[0].color, [1.; 4]);
    assert!(after.parts[0].alpha_cutoff.is_none());
    let image = after.parts[0].image.as_ref().unwrap();
    assert_eq!((image.width, image.height), (512, 512));
    assert_eq!(image.rgba.len(), 1024 * 1024);
    assert!(image.rgba.chunks_exact(4).all(|p| p[3] == 255));
    let material = &after.parts[0].shading.as_ref().unwrap().material;
    assert_eq!(material.metallic, 0.);
    assert_eq!(material.base_color_sampler.wrap_u, Wrap::Clamp);
    assert_eq!(material.base_color_sampler.wrap_v, Wrap::Clamp);
    assert_eq!(Terrain::from_json(&terrain.to_json()?)?, terrain);
    Ok(())
}

#[test]
fn soft_strokes_normalize_exactly_and_only_modify_covered_vertices() -> Result<()> {
    let mut terrain = Terrain::flat([5, 5], [4., 4.])?;
    assert!(terrain.paint(brush(0))?);
    assert!(!terrain.paint(brush(0))?);
    assert!(terrain.paint(brush(1))?);
    let weights = &terrain.paint.as_ref().unwrap().weights;
    assert_eq!(weights[12], [0, 255, 0]);
    assert_eq!(weights.iter().filter(|w| **w != [255, 0, 0]).count(), 1);
    assert!(!terrain.paint(brush(1))?);
    assert!(terrain.paint(TerrainPaintBrush {
        strength: 0.5,
        ..brush(2)
    })?);
    assert_eq!(terrain.paint.as_ref().unwrap().weights[12], [0, 127, 128]);
    for step in 0..1000 {
        terrain.paint(TerrainPaintBrush {
            layer: step % 3,
            center: [0.25, -0.5],
            radius: 1.8,
            strength: 0.37,
        })?;
        terrain.validate()?;
    }
    let weights = &terrain.paint.as_ref().unwrap().weights;
    assert_eq!(weights[0], [255, 0, 0]);
    assert_eq!(weights[24], [255, 0, 0]);
    assert!(
        weights
            .iter()
            .all(|w| w.iter().map(|v| u16::from(*v)).sum::<u16>() == 255)
    );
    Ok(())
}

#[test]
fn empty_or_invalid_strokes_preserve_sources_and_do_not_allocate_paint() -> Result<()> {
    let mut terrain = Terrain::flat([5, 5], [4., 4.])?;
    let before = terrain.to_json()?;
    for empty in [
        TerrainPaintBrush {
            strength: 0.,
            ..brush(1)
        },
        TerrainPaintBrush {
            center: [100., 100.],
            ..brush(1)
        },
        TerrainPaintBrush {
            center: [f32::MAX, -f32::MAX],
            ..brush(1)
        },
        TerrainPaintBrush {
            center: [0.5, 0.5],
            radius: 0.001,
            ..brush(1)
        },
    ] {
        assert!(!terrain.paint(empty)?);
        assert!(terrain.paint.is_none());
        assert_eq!(terrain.to_json()?, before);
    }
    for invalid in [
        TerrainPaintBrush {
            layer: 3,
            ..brush(1)
        },
        TerrainPaintBrush {
            center: [f32::NAN, 0.],
            ..brush(1)
        },
        TerrainPaintBrush {
            radius: 0.,
            ..brush(1)
        },
        TerrainPaintBrush {
            radius: f32::INFINITY,
            ..brush(1)
        },
        TerrainPaintBrush {
            strength: -0.1,
            ..brush(1)
        },
        TerrainPaintBrush {
            strength: 1.1,
            ..brush(1)
        },
        TerrainPaintBrush {
            strength: f32::NAN,
            ..brush(1)
        },
    ] {
        assert!(terrain.paint(invalid).is_err());
        assert_eq!(terrain.to_json()?, before);
    }
    Ok(())
}

#[test]
fn paint_validation_rejects_bad_weights_layers_and_source_extensions() -> Result<()> {
    let mut terrain = Terrain::flat([3, 3], [4., 4.])?;
    terrain.paint = Some(TerrainPaint::new(terrain.heights.len()));
    let good = terrain.clone();
    terrain.paint.as_mut().unwrap().weights.pop();
    assert!(terrain.validate().is_err());
    terrain = good.clone();
    terrain.paint.as_mut().unwrap().weights[0] = [255, 1, 0];
    assert!(terrain.validate().is_err());
    for bad in [0., -1., f32::NAN, f32::INFINITY, 100_001.] {
        terrain = good.clone();
        terrain.paint.as_mut().unwrap().layers[0].tiling = bad;
        assert!(terrain.validate().is_err());
    }
    for bad in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
        terrain = good.clone();
        terrain.paint.as_mut().unwrap().layers[0].color[1] = bad;
        assert!(terrain.validate().is_err());
    }
    let json = good.to_json()?;
    let value: serde_json::Value = serde_json::from_slice(&json)?;
    for edit in [
        |v: &mut serde_json::Value| v["paint"]["unexpected"] = true.into(),
        |v: &mut serde_json::Value| v["paint"]["layers"][0]["unexpected"] = true.into(),
        |v: &mut serde_json::Value| v["paint"]["weights"][0][0] = 256.into(),
        |v: &mut serde_json::Value| v["paint"]["weights"][0][1] = 1.into(),
    ] {
        let mut corrupt = value.clone();
        edit(&mut corrupt);
        assert!(Terrain::from_json(&serde_json::to_vec(&corrupt)?).is_err());
    }
    assert!(Terrain::from_json(&vec![b' '; 4 * 1024 * 1024 + 1]).is_err());
    Ok(())
}

#[test]
fn material_edits_preserve_surface_keys_and_cooking_preserves_embedded_pixels() -> Result<()> {
    let mut terrain = Terrain::flat([9, 7], [16., 12.])?;
    terrain.paint(TerrainPaintBrush {
        radius: 5.,
        ..brush(1)
    })?;
    terrain.paint(TerrainPaintBrush {
        center: [4., 2.],
        radius: 3.,
        ..brush(2)
    })?;
    let progress = Progress::default();
    let mesh = terrain.mesh(&progress)?;
    let identical = terrain.mesh(&progress)?;
    assert_eq!(
        mesh.parts[0].image.as_ref().unwrap().rgba,
        identical.parts[0].image.as_ref().unwrap().rgba
    );
    assert_eq!(mesh.parts[0].source_key.len(), 16);
    assert!(
        mesh.parts[0]
            .source_key
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    );
    let bytes = cooked_model::encode(&mesh, &[], &progress)?;
    let decoded = cooked_model::decode(&bytes)?;
    let optimized = mesh.optimized_for_upload(&progress)?;
    assert_eq!(decoded.vertices, optimized.vertices);
    assert_eq!(decoded.indices, optimized.indices);
    assert_eq!(decoded.parts[0].source_key, mesh.parts[0].source_key);
    assert_eq!(
        decoded.parts[0].image.as_ref().unwrap().rgba,
        mesh.parts[0].image.as_ref().unwrap().rgba
    );
    assert_eq!(cooked_model::encode(&decoded, &[], &progress)?, bytes);
    let exported: serde_json::Value = serde_json::from_slice(&crate::mesh_gltf(&mesh, &progress)?)?;
    assert_eq!(
        exported["meshes"][0]["primitives"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(exported["images"].as_array().unwrap().len(), 1);
    assert!(
        exported["images"][0]["uri"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(
        exported["materials"][0]["pbrMetallicRoughness"]["metallicFactor"],
        0.
    );
    terrain.paint.as_mut().unwrap().layers[1].color = [0.08, 0.05, 0.02];
    terrain.paint.as_mut().unwrap().layers[2].tiling = 1.;
    let changed = terrain.mesh(&progress)?;
    assert_eq!(changed.parts[0].source_key, mesh.parts[0].source_key);
    assert_eq!(changed.vertices, mesh.vertices);
    assert_eq!(changed.indices, mesh.indices);
    assert_ne!(
        changed.parts[0].image.as_ref().unwrap().rgba,
        mesh.parts[0].image.as_ref().unwrap().rgba
    );
    terrain.heights[0] = 1.;
    assert_ne!(
        terrain.mesh(&progress)?.parts[0].source_key,
        mesh.parts[0].source_key
    );
    Ok(())
}

#[test]
fn cancelled_paint_mesh_jobs_publish_no_result() -> Result<()> {
    let mut terrain = Terrain::flat([129, 129], [128., 128.])?;
    terrain.paint(brush(1))?;
    let (release, gate) = std::sync::mpsc::channel();
    let (ready, started) = std::sync::mpsc::channel();
    let job = Job::start("Painting terrain", move |progress| {
        ready.send(())?;
        gate.recv_timeout(std::time::Duration::from_secs(5))?;
        terrain.mesh(&progress)
    })?;
    started.recv_timeout(std::time::Duration::from_secs(5))?;
    job.cancel();
    release.send(())?;
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(result) = job.poll() {
            assert!(result.unwrap_err().to_string().contains("cancelled"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}

#[test]
fn continuous_strokes_match_at_30_60_and_144_fps_including_soft_edges() -> Result<()> {
    let mut results = Vec::new();
    for fps in [30, 60, 144] {
        let mut terrain = Terrain::flat([33, 33], [16., 16.])?;
        let mut stroke = TerrainPaintStroke::new(&terrain)?;
        for (layer, rate) in [(1, 4.), (2, 1.3)] {
            let alpha = (1. - (-rate / fps as f64).exp()) as f32;
            for _ in 0..fps {
                stroke.paint(
                    &mut terrain,
                    TerrainPaintBrush {
                        layer,
                        center: [0.25, -0.5],
                        radius: 3.7,
                        strength: alpha,
                    },
                )?;
            }
        }
        terrain.validate()?;
        results.push(terrain);
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0], results[2]);
    let weights = &results[0].paint.as_ref().unwrap().weights;
    assert!(weights.iter().any(|w| w[0] > 0 && w[1] > 0 && w[2] > 0));
    Ok(())
}

#[test]
fn continuous_sub_byte_strength_accumulates_and_reaches_saturation() -> Result<()> {
    let mut terrain = Terrain::flat([5, 5], [4., 4.])?;
    let mut stroke = TerrainPaintStroke::new(&terrain)?;
    let low = (1. - (-0.01_f64 / 60.).exp()) as f32;
    for _ in 0..600 {
        stroke.paint(
            &mut terrain,
            TerrainPaintBrush {
                strength: low,
                ..brush(1)
            },
        )?;
    }
    assert_eq!(terrain.paint.as_ref().unwrap().weights[12], [231, 24, 0]);
    let normal = (1. - (-4_f64 / 60.).exp()) as f32;
    for _ in 0..600 {
        stroke.paint(
            &mut terrain,
            TerrainPaintBrush {
                strength: normal,
                ..brush(1)
            },
        )?;
    }
    assert_eq!(terrain.paint.as_ref().unwrap().weights[12], [0, 255, 0]);
    assert!(!stroke.paint(
        &mut terrain,
        TerrainPaintBrush {
            strength: normal,
            ..brush(1)
        }
    )?);
    terrain.validate()?;
    Ok(())
}

#[test]
fn continuous_stroke_rejects_stale_sources_and_keeps_empty_stamps_unchanged() -> Result<()> {
    let mut terrain = Terrain::flat([5, 5], [4., 4.])?;
    let mut stroke = TerrainPaintStroke::new(&terrain)?;
    assert!(!stroke.paint(
        &mut terrain,
        TerrainPaintBrush {
            strength: 0.,
            ..brush(1)
        }
    )?);
    assert!(!stroke.paint(
        &mut terrain,
        TerrainPaintBrush {
            center: [100., 100.],
            ..brush(1)
        }
    )?);
    assert!(terrain.paint.is_none());
    assert!(stroke.paint(&mut terrain, brush(1))?);
    for edit in [
        |terrain: &mut Terrain| terrain.heights[0] = 1.,
        |terrain: &mut Terrain| terrain.heights[0] = -0.,
        |terrain: &mut Terrain| terrain.paint.as_mut().unwrap().weights[0] = [0, 255, 0],
        |terrain: &mut Terrain| terrain.paint.as_mut().unwrap().layers[0].color[0] = 0.4,
        |terrain: &mut Terrain| terrain.paint.as_mut().unwrap().layers[0].tiling = 4.,
        |terrain: &mut Terrain| terrain.size[0] = 8.,
        |terrain: &mut Terrain| terrain.resolution = [2, 2],
        |terrain: &mut Terrain| terrain.paint = None,
    ] {
        let mut changed = terrain.clone();
        edit(&mut changed);
        let before = changed.clone();
        assert!(
            stroke
                .paint(&mut changed, brush(2))
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
        assert_eq!(changed, before);
    }
    terrain.paint.as_mut().unwrap().layers[0].color[0] = 0.;
    let mut color_stroke = TerrainPaintStroke::new(&terrain)?;
    terrain.paint.as_mut().unwrap().layers[0].color[0] = -0.;
    assert!(color_stroke.paint(&mut terrain, brush(2)).is_err());
    terrain.heights.pop();
    assert!(TerrainPaintStroke::new(&terrain).is_err());
    Ok(())
}

#[test]
fn continuous_paths_match_individual_stamps_and_reject_invalid_paths_atomically() -> Result<()> {
    let mut batched = Terrain::flat([33, 33], [16., 16.])?;
    let mut individual = batched.clone();
    let mut batched_stroke = TerrainPaintStroke::new(&batched)?;
    let mut individual_stroke = TerrainPaintStroke::new(&individual)?;
    let path: Vec<_> = (0..64)
        .map(|step| TerrainPaintBrush {
            layer: 1,
            center: [-4. + step as f32 / 8., 0.],
            radius: 1.8,
            strength: 0.021,
        })
        .collect();
    assert!(batched_stroke.paint_path(&mut batched, &path)?);
    for &brush in &path {
        individual_stroke.paint(&mut individual, brush)?;
    }
    assert_eq!(batched, individual);
    let before = batched.clone();
    let invalid = [
        brush(2),
        TerrainPaintBrush {
            layer: 3,
            ..brush(2)
        },
    ];
    assert!(batched_stroke.paint_path(&mut batched, &invalid).is_err());
    assert_eq!(batched, before);
    assert!(
        batched_stroke
            .paint_path(&mut batched, &[brush(2); 129])
            .is_err()
    );
    assert_eq!(batched, before);
    assert!(!batched_stroke.paint_path(&mut batched, &[])?);
    Ok(())
}

// Independent full-map brush used to verify the bounded brush and benchmark it.
fn paint_reference(terrain: &mut Terrain, brush: TerrainPaintBrush) -> Result<bool> {
    terrain.validate()?;
    let [nx, nz] = terrain.resolution.map(usize::from);
    let steps = [
        terrain.size[0] / (nx - 1) as f32,
        terrain.size[1] / (nz - 1) as f32,
    ];
    let mut changed = false;
    for z in 0..nz {
        for x in 0..nx {
            let dx = (x as f32 * steps[0] - terrain.size[0] * 0.5 - brush.center[0]) / brush.radius;
            let dz = (z as f32 * steps[1] - terrain.size[1] * 0.5 - brush.center[1]) / brush.radius;
            let distance = (dx * dx + dz * dz).sqrt();
            if distance >= 1. {
                continue;
            }
            let falloff = (1. - distance).powi(2) * (1. + 2. * distance);
            let amount = brush.strength * falloff;
            if amount <= 0. {
                continue;
            }
            if terrain.paint.is_none() {
                terrain.paint = Some(TerrainPaint::new(nx * nz));
                changed = true;
            }
            let weights = &mut terrain.paint.as_mut().unwrap().weights[z * nx + x];
            let target = (weights[brush.layer] as f32
                + (255. - weights[brush.layer] as f32) * amount)
                .round() as u16;
            if target == u16::from(weights[brush.layer]) {
                continue;
            }
            let others = [(brush.layer + 1) % 3, (brush.layer + 2) % 3];
            let old_others = 255 - u16::from(weights[brush.layer]);
            let first =
                (u16::from(weights[others[0]]) * (255 - target) + old_others / 2) / old_others;
            weights[brush.layer] = target as u8;
            weights[others[0]] = first as u8;
            weights[others[1]] = (255 - target - first) as u8;
            changed = true;
        }
    }
    Ok(changed)
}

#[test]
fn bounded_brush_matches_independent_full_map_reference_at_edges_and_scales() -> Result<()> {
    for (resolution, size) in [
        ([129, 129], [128., 128.]),
        ([17, 9], [3.7, 8.1]),
        ([2, 3], [0.01, 100_000.]),
    ] {
        let mut bounded = Terrain::flat(resolution, size)?;
        let mut reference = bounded.clone();
        for step in 0..60 {
            let brush = TerrainPaintBrush {
                layer: step % 3,
                center: [
                    (step as f32 * 0.71 - 20.) % size[0],
                    (step as f32 * 1.31 - 40.) % size[1],
                ],
                radius: if step % 5 == 0 { 100_000. } else { 1.8 },
                strength: if step % 11 == 0 { 0. } else { 0.47 },
            };
            assert_eq!(
                bounded.paint(brush)?,
                paint_reference(&mut reference, brush)?
            );
            assert_eq!(bounded, reference);
        }
    }
    Ok(())
}

#[test]
#[ignore = "release-only timing evidence; no timing threshold"]
fn benchmark_bounded_paint_against_full_map() -> Result<()> {
    let mut bounded = Terrain::flat([129, 129], [128., 128.])?;
    bounded.paint = Some(TerrainPaint::new(bounded.heights.len()));
    let mut reference = bounded.clone();
    const STROKES: usize = 10_000;
    let make_brush = |step| TerrainPaintBrush {
        layer: 1 + step % 2,
        radius: 1.8,
        strength: 0.31,
        ..brush(1)
    };
    let start = Instant::now();
    for step in 0..STROKES {
        black_box(bounded.paint(black_box(make_brush(step)))?);
    }
    let bounded_time = start.elapsed();
    let start = Instant::now();
    for step in 0..STROKES {
        black_box(paint_reference(
            &mut reference,
            black_box(make_brush(step)),
        )?);
    }
    let reference_time = start.elapsed();
    assert_eq!(bounded, reference);
    println!(
        "terrain_paint_benchmark strokes={STROKES} vertices=16641 candidate_vertices=9 bounded_ms={:.3} reference_ms={:.3} speedup={:.2}x exact_weights=true validation_included=true",
        bounded_time.as_secs_f64() * 1000.,
        reference_time.as_secs_f64() * 1000.,
        reference_time.as_secs_f64() / bounded_time.as_secs_f64()
    );
    Ok(())
}

#[test]
#[ignore = "release-only timing evidence; no timing threshold"]
fn benchmark_continuous_paths_against_individual_stamps() -> Result<()> {
    let base = Terrain::flat([129, 129], [128., 128.])?;
    let path: Vec<_> = (0..64)
        .map(|step| TerrainPaintBrush {
            layer: 1,
            center: [-4. + step as f32 / 8., 0.],
            radius: 1.8,
            strength: 0.021,
        })
        .collect();
    const PATHS: usize = 500;
    let mut batched_times = Vec::new();
    let mut individual_times = Vec::new();
    for sample in 0..5 {
        let mut final_batched = None;
        let mut final_individual = None;
        // Alternate order to avoid assigning warm-up consistently to one path.
        for batched in if sample % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let mut terrain = base.clone();
            let mut stroke = TerrainPaintStroke::new(&terrain)?;
            let start = Instant::now();
            for _ in 0..PATHS {
                if batched {
                    black_box(stroke.paint_path(&mut terrain, black_box(&path))?);
                } else {
                    for &brush in &path {
                        black_box(stroke.paint(&mut terrain, black_box(brush))?);
                    }
                }
            }
            let elapsed = start.elapsed();
            if batched {
                batched_times.push(elapsed);
                final_batched = Some(terrain);
            } else {
                individual_times.push(elapsed);
                final_individual = Some(terrain);
            }
        }
        assert_eq!(final_batched, final_individual);
    }
    batched_times.sort();
    individual_times.sort();
    println!(
        "terrain_paint_path_benchmark paths={PATHS} stamps_per_path=64 vertices=16641 samples=5 batched_median_ms={:.3} individual_median_ms={:.3} speedup={:.2}x exact_weights=true source_guard_once_per_path=true",
        batched_times[2].as_secs_f64() * 1000.,
        individual_times[2].as_secs_f64() * 1000.,
        individual_times[2].as_secs_f64() / batched_times[2].as_secs_f64()
    );
    Ok(())
}
