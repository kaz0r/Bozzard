//! Keep the scene texture while only editor chrome changes. Animation and temporal
//! effects still render continuously; input-driven egui repaints remain immediate.
use bozzard_render::DisplaySettings;
use glam::Mat4;

#[derive(PartialEq)]
pub(super) struct ViewportStamp {
    pub revision: u64,
    pub catalog: u64,
    pub assets: Vec<(String, u64, bool)>,
    pub size: [u32; 2],
    pub scale: f32,
    pub projection: Mat4,
    pub layer_2d: bool,
    pub playing: bool,
    pub bypass: bool,
    pub display: DisplaySettings,
}

pub(super) fn continuous(
    display: DisplaySettings,
    playing: bool,
    preview: bool,
    particles: bool,
) -> bool {
    playing
        || display.temporal_aa.enabled
        || display.motion_blur.enabled
        || display.auto_exposure.enabled
        || (preview
            && (particles
                || display.grain.intensity > 0.
                || (display.heat_distortion.enabled
                    && display.heat_distortion.strength > 0.
                    && display.heat_distortion.speed != 0.)
                || (display.volumetric_fog.enabled
                    && display.volumetric_fog.density > 0.
                    && display.volumetric_fog.noise_amount > 0.
                    && display.volumetric_fog.wind != [0.; 3])))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_and_paused_previews_sleep_but_animation_and_temporal_effects_do_not() {
        let mut d = DisplaySettings::default();
        assert!(!continuous(d, false, true, false));
        assert!(continuous(d, true, false, false));
        assert!(continuous(d, false, true, true));
        assert!(!continuous(d, false, false, true));
        d.grain.intensity = 0.3;
        assert!(continuous(d, false, true, false));
        assert!(!continuous(d, false, false, false));
        d = DisplaySettings::default();
        d.volumetric_fog.enabled = true;
        d.volumetric_fog.density = 0.1;
        d.volumetric_fog.noise_amount = 0.5;
        d.volumetric_fog.wind = [0.1, 0., 0.];
        assert!(continuous(d, false, true, false));
        assert!(!continuous(d, false, false, false));
        d.volumetric_fog.wind = [0.; 3];
        assert!(!continuous(d, false, true, false));
        d.temporal_aa.enabled = true;
        assert!(continuous(d, false, false, false));
        d.temporal_aa.enabled = false;
        d.auto_exposure.enabled = true;
        assert!(continuous(d, false, false, false));
        d.auto_exposure.enabled = false;
        d.motion_blur.enabled = true;
        assert!(continuous(d, false, false, false));
    }
}
