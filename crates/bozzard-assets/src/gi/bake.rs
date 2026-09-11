use super::*;
use crate::job::Progress;
use bozzard_scene::GI_PROBE_STRIDE;
use glam::{Vec2, Vec3};
use std::sync::Arc;

/// Deterministic CPU diffuse transport. No graphics adapter or platform RT extension.
pub fn bake(
    scene: &Scene,
    assets: &AssetStore,
    volume: GiVolumeSettings,
    progress: &Progress,
) -> Result<BakedGi> {
    volume.validate()?;
    progress.stage("Preparing global illumination")?;
    let mut snapshot = scene.clone();
    snapshot.gi.volume = volume;
    let tracer = trace::TraceScene::new(&snapshot, assets, progress)?;
    let source = source(&snapshot, assets, volume)?;
    let mut probes = vec![[0.; 4]; volume.probe_count() * GI_PROBE_STRIDE];
    let convolution = [1., 2. / 3., 2. / 3., 2. / 3., 0.25, 0.25, 0.25, 0.25, 0.25];
    for (index, probe) in probes.chunks_exact_mut(GI_PROBE_STRIDE).enumerate() {
        progress.stage(format!(
            "Baking GI: probe {}/{}",
            index + 1,
            volume.probe_count()
        ))?;
        let origin = volume.position(index);
        let mut seed = (index as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ 0x424f5a5a;
        let rotation = sampling::random(&mut seed);
        let mut backfaces = 0u32;
        let mut hits = 0u32;
        let mut coefficients = [Vec3::ZERO; 9];
        for ray in 0..volume.samples {
            if ray.is_multiple_of(16) {
                progress.check()?;
            }
            let d = sampling::sphere(ray, volume.samples, rotation);
            if let Some(hit) = tracer.hit(origin, d, tracer.distance_limit) {
                hits += 1;
                if !hit.front && !hit.double_sided {
                    backfaces += 1;
                }
            }
            let radiance = tracer.radiance(origin, d, volume.bounces, &mut seed);
            for (coefficient, basis) in coefficients.iter_mut().zip(sampling::sh(d)) {
                *coefficient += radiance * basis;
            }
        }
        for (band, (coefficient, weight)) in coefficients.into_iter().zip(convolution).enumerate() {
            let coefficient =
                coefficient * (4. * std::f32::consts::PI / volume.samples as f32) * weight;
            probe[band] = coefficient.extend(0.).to_array();
        }
        // Reject probes inside closed opaque geometry; open sky rays count as outside.
        probe[0][3] = if hits > 0 && backfaces * 2 > volume.samples {
            0.
        } else {
            1.
        };
        // Directional moments reduce interpolation through intervening geometry.
        // Four stratified rays per texel, independent of the radiance sample count.
        for y in 0..8 {
            for x in 0..8 {
                progress.check()?;
                let mut mean = 0_f64;
                let mut square = 0_f64;
                for (u, v) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let d =
                        sampling::oct_decode(Vec2::new((x as f32 + u) / 8., (y as f32 + v) / 8.));
                    let distance = tracer
                        .hit(origin, d, tracer.distance_limit)
                        .map_or(tracer.distance_limit, |hit| hit.distance)
                        as f64;
                    mean += distance / 4.;
                    square += distance * distance / 4.;
                }
                let texel = y * 8 + x;
                let slot = 9 + texel / 2;
                let component = (texel % 2) * 2;
                probe[slot][component] = mean as f32;
                probe[slot][component + 1] = square as f32;
            }
        }
    }
    progress.check()?;
    let result = BakedGi::new(source, volume, Arc::new(probes))?;
    ensure!(
        result
            .probes
            .chunks_exact(GI_PROBE_STRIDE)
            .any(|p| p[0][3] > 0.),
        "All GI probes are inside geometry; adjust volume placement/resolution"
    );
    Ok(result)
}
