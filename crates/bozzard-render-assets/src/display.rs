//! One display adapter keeps editor and player effects and 2D isolation identical.
pub fn display_settings(
    source: bozzard_scene::DisplaySettings,
    layer: bozzard_scene::Layer,
    time_seconds: f32,
) -> bozzard_render::DisplaySettings {
    use bozzard_render as r;
    if layer == bozzard_scene::Layer::TwoD {
        return r::DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        };
    }
    r::DisplaySettings {
        temporal_aa: r::TemporalAntiAliasing {
            enabled: source.temporal_aa.enabled,
            history_weight: source.temporal_aa.history_weight,
        },
        motion_blur: r::MotionBlur {
            enabled: source.motion_blur.enabled,
            shutter_angle: source.motion_blur.shutter_angle,
            max_radius: source.motion_blur.max_radius,
            samples: source.motion_blur.samples,
        },
        reflections: r::ScreenSpaceReflections {
            enabled: source.reflections.enabled,
            strength: source.reflections.strength,
            max_distance: source.reflections.max_distance,
            thickness: source.reflections.thickness,
            roughness_cutoff: source.reflections.roughness_cutoff,
            steps: source.reflections.steps,
        },
        depth_of_field: r::DepthOfField {
            enabled: source.depth_of_field.enabled,
            focus_distance: source.depth_of_field.focus_distance,
            focal_length_mm: source.depth_of_field.focal_length_mm,
            aperture: source.depth_of_field.aperture,
            max_blur_radius: source.depth_of_field.max_blur_radius,
        },
        auto_exposure: r::AutoExposure {
            enabled: source.auto_exposure.enabled,
            strength: source.auto_exposure.strength,
            min_ev: source.auto_exposure.min_ev,
            max_ev: source.auto_exposure.max_ev,
            target_gray: source.auto_exposure.target_gray,
            speed_up: source.auto_exposure.speed_up,
            speed_down: source.auto_exposure.speed_down,
            center_weight: source.auto_exposure.center_weight,
        },
        volumetric_fog: r::VolumetricFog {
            enabled: source.volumetric_fog.enabled,
            density: source.volumetric_fog.density,
            albedo: source.volumetric_fog.albedo,
            anisotropy: source.volumetric_fog.anisotropy,
            base_height: source.volumetric_fog.base_height,
            height_falloff: source.volumetric_fog.height_falloff,
            start_distance: source.volumetric_fog.start_distance,
            max_distance: source.volumetric_fog.max_distance,
            noise_amount: source.volumetric_fog.noise_amount,
            noise_scale: source.volumetric_fog.noise_scale,
            wind: source.volumetric_fog.wind,
            light_intensity: source.volumetric_fog.light_intensity,
            ambient: source.volumetric_fog.ambient,
            steps: source.volumetric_fog.steps,
        },
        time_seconds,
        exposure_ev: source.exposure_ev,
        tone_mapping: source.tone_mapping,
        tone_mapper: match source.tone_mapper {
            bozzard_scene::ToneMapper::Reinhard => r::ToneMapper::Reinhard,
            bozzard_scene::ToneMapper::Filmic => r::ToneMapper::Filmic,
        },
        bloom: r::BloomSettings {
            enabled: source.bloom.enabled,
            intensity: source.bloom.intensity,
            threshold: source.bloom.threshold,
            scatter: source.bloom.scatter,
            anamorphic: source.bloom.anamorphic,
        },
        color_grading: r::ColorGrading {
            temperature: source.color_grading.temperature,
            tint: source.color_grading.tint,
            saturation: source.color_grading.saturation,
            contrast: source.color_grading.contrast,
            lift: source.color_grading.lift,
            gamma: source.color_grading.gamma,
            gain: source.color_grading.gain,
        },
        ambient_occlusion: r::AmbientOcclusion {
            enabled: source.ambient_occlusion.enabled,
            intensity: source.ambient_occlusion.intensity,
            radius: source.ambient_occlusion.radius,
            bias: source.ambient_occlusion.bias,
        },
        heat_distortion: r::HeatDistortion {
            enabled: source.heat_distortion.enabled,
            strength: source.heat_distortion.strength,
            threshold: source.heat_distortion.threshold,
            speed: source.heat_distortion.speed,
            rise: source.heat_distortion.rise,
        },
        grain: r::FilmGrain {
            intensity: source.grain.intensity,
            size: source.grain.size,
        },
        vignette: r::Vignette {
            intensity: source.vignette.intensity,
            roundness: source.vignette.roundness,
            feather: source.vignette.feather,
        },
    }
}

/// Convert transient particle frame data without coupling the renderer to the simulation.
pub fn particle_frame(source: &[bozzard_scene::Particle]) -> Vec<bozzard_render::Particle> {
    source
        .iter()
        .map(|p| bozzard_render::Particle {
            id: p.id,
            position: p.position,
            velocity: p.velocity,
            size: p.size,
            rotation: p.rotation,
            color: p.color,
            opacity: p.opacity,
            kind: match p.kind {
                bozzard_scene::ParticleKind::Smoke => bozzard_render::ParticleKind::Smoke,
                bozzard_scene::ParticleKind::Ash => bozzard_render::ParticleKind::Ash,
                bozzard_scene::ParticleKind::Sparks => bozzard_render::ParticleKind::Sparks,
            },
            softness: p.softness,
            trail_length: p.trail_length,
            seed: p.seed,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn temporal_effects_share_adapter_and_stay_out_of_2d() {
        let source =
            bozzard_scene::DisplaySettings::preset(bozzard_scene::DisplayPreset::Cinematic);
        let three = super::display_settings(source, bozzard_scene::Layer::ThreeD, 4.);
        assert!(
            three.temporal_aa.enabled && three.motion_blur.enabled && three.reflections.enabled
        );
        assert_eq!(
            three.temporal_aa.history_weight,
            source.temporal_aa.history_weight
        );
        assert_eq!(three.reflections.steps, source.reflections.steps);
        assert_eq!(three.time_seconds, 4.);
        three.validate().unwrap();
        let two = super::display_settings(source, bozzard_scene::Layer::TwoD, 4.);
        assert!(
            !two.temporal_aa.enabled
                && !two.motion_blur.enabled
                && !two.reflections.enabled
                && !two.depth_of_field.enabled
                && !two.volumetric_fog.enabled
        );
        assert_eq!(two.time_seconds, 0.);
    }
}
