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
