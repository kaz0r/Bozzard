//! Content-browser model thumbnails rendered by the engine itself, like Unity's asset
//! previews: a private `SceneRenderer` uploads one model at a time in the background,
//! draws it with its own PBR materials and textures under studio lighting, reads the
//! frame back and frees the model again. The viewport's resident set is untouched.
use anyhow::{Context, Result, bail};
use bozzard_assets::{AssetData, AssetStore, MeshData};
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_render_assets::Residency;
use eframe::egui::ColorImage;
use glam::{Mat4, Vec3};
use std::collections::{BTreeSet, HashMap};

/// Rendered edge; the cached image is half this after a 2x2 box filter.
const RENDER: u32 = 256;
const OUTPUT: usize = RENDER as usize / 2;
/// Upload work per frame, so a large model streams in without stalling the editor.
const UPLOAD_BYTES_PER_FRAME: usize = 8 << 20;

pub enum Preview {
    Ready(ColorImage),
    Pending,
    Failed(String),
}

/// An asset ID and the store revision its preview shows.
type Key = (String, u64);

pub struct ModelThumbnails {
    gpu: Gpu,
    renderer: Option<SceneRenderer>,
    residency: Residency,
    /// The model uploading now. Other requests wait until it is drawn.
    job: Option<Key>,
    /// Previews that finished after their tile scrolled away, kept until asked for.
    finished: HashMap<Key, Result<ColorImage, String>>,
}

impl ModelThumbnails {
    pub fn new(gpu: Gpu) -> Self {
        Self {
            gpu,
            renderer: None,
            residency: residency(),
            job: None,
            finished: HashMap::new(),
        }
    }

    /// Advance the current upload and return `id`'s preview once it exists.
    pub fn request(&mut self, store: &AssetStore, id: &str) -> Preview {
        let Some(revision) = store
            .handle(id)
            .and_then(|handle| store.get(handle))
            .map(|entry| entry.revision())
        else {
            return Preview::Failed(format!("{id} is not in the asset store"));
        };
        let key = (id.to_owned(), revision);
        if self.job.is_none() && !self.finished.contains_key(&key) {
            if self.finished.len() > 64 {
                self.finished.clear();
            }
            self.job = Some(key.clone());
        }
        if let Some(job) = self.job.clone() {
            match self.step(store, &job.0) {
                Ok(Some(image)) => {
                    self.finished.insert(job, Ok(image));
                }
                Ok(None) => {}
                Err(error) => {
                    self.finished.insert(job, Err(format!("{error:#}")));
                }
            }
        }
        match self.finished.remove(&key) {
            Some(Ok(image)) => Preview::Ready(image),
            Some(Err(error)) => Preview::Failed(error),
            None => Preview::Pending,
        }
    }

    /// One frame of upload work for the current job; the preview once it is resident.
    fn step(&mut self, store: &AssetStore, id: &str) -> Result<Option<ColorImage>> {
        let result = self.try_step(store, id);
        if !matches!(result, Ok(None)) {
            // Drawn or failed: free the model, the cached image is all the browser keeps.
            if let Some(renderer) = &mut self.renderer {
                renderer.remove_asset(store.canonical_asset_id(id));
            }
            self.residency = residency();
            self.job = None;
        }
        result
    }

    fn try_step(&mut self, store: &AssetStore, id: &str) -> Result<Option<ColorImage>> {
        let gpu = &self.gpu;
        let renderer = self
            .renderer
            .get_or_insert_with(|| SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm));
        let canonical = store.canonical_asset_id(id).to_owned();
        let Some(AssetData::Mesh(mesh)) = store
            .handle(&canonical)
            .and_then(|handle| store.get(handle))
            .and_then(|entry| entry.data())
        else {
            bail!("{id} has no loaded mesh");
        };
        self.residency
            .set_required(BTreeSet::from([canonical.clone()]));
        self.residency
            .advance(gpu, renderer, store, UPLOAD_BYTES_PER_FRAME)
            .with_context(|| format!("uploading {id} for its preview"))?;
        if !self.residency.is_current(store, &canonical)
            || self.residency.progress().is_some()
            || self.residency.preparing().is_some()
        {
            return Ok(None);
        }
        let scene = studio_scene(canonical, mesh);
        let frame = bozzard_render::capture_offscreen(gpu, RENDER, RENDER, |view| {
            renderer.draw(gpu, view, [RENDER, RENDER], &scene)
        })?;
        Ok(Some(cut_out(&frame.rgba)))
    }
}

/// Upload bookkeeping for one model at a time.
fn residency() -> Residency {
    let mut residency = Residency::default();
    residency.set_budget(Some(1));
    residency
}

/// One model framed from above and to the right, lit by a key light and the default sky.
fn studio_scene(id: String, mesh: &MeshData) -> bozzard_render::RenderScene {
    let (min, max) = mesh.vertices.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), v| {
            let p = Vec3::new(v[0], v[1], v[2]);
            (min.min(p), max.max(p))
        },
    );
    let (center, radius) = if min.x <= max.x {
        ((min + max) * 0.5, ((max - min).length() * 0.5).max(1e-4))
    } else {
        (Vec3::ZERO, 1.0)
    };
    let fov = 30f32.to_radians();
    // The bounding sphere fills the frame, less a small margin.
    let distance = radius / (fov * 0.5).sin() * 1.02;
    let eye = center + Vec3::new(0.62, 0.5, 1.0).normalize() * distance;
    let lens = glam::camera::rh::proj::directx::perspective(
        fov,
        1.0,
        (distance - radius * 1.5).max(distance * 0.01),
        distance + radius * 1.5,
    );
    bozzard_render::RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: Vec::new(),
        trails: Vec::new(),
        decals: Vec::new(),
        view_projection: lens * glam::camera::rh::view::look_at_mat4(eye, center, Vec3::Y),
        items: vec![bozzard_render::DrawItem {
            motion_id: 0,
            mesh: bozzard_render::MeshKind::Imported(id),
            model: Mat4::IDENTITY,
            material: bozzard_render::Material {
                metallic: None,
                roughness: None,
                tint: [1.; 3],
                lit: true,
                texture: bozzard_render::TextureKind::White,
                uv_scale: [1.; 2],
                surface_overrides: Default::default(),
                shader: None,
            },
        }],
        // Toward the light: high and front-left, so the top, front and the side facing
        // the camera read as three distinct tones.
        lighting: bozzard_render::Lighting {
            shadows: false,
            sun_direction: [-0.35, 0.85, 0.55],
            sun_intensity: 4.0,
            ambient_intensity: 0.1,
            ..Default::default()
        },
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings {
            background: false,
            ..Default::default()
        },
        fog: Default::default(),
        gi: None,
        display: Default::default(),
    }
}

/// Key out the flat clear color (read from a corner) and halve the size with a 2x2 box
/// filter. Coverage becomes alpha, so edges are smooth and the tile shows through.
fn cut_out(rgba: &[u8]) -> ColorImage {
    let edge = RENDER as usize;
    let background = [rgba[0], rgba[1], rgba[2]];
    let model = |i: usize| {
        rgba[i * 4..i * 4 + 3]
            .iter()
            .zip(background)
            .any(|(a, b)| a.abs_diff(b) > 3)
    };
    let mut pixels = Vec::with_capacity(OUTPUT * OUTPUT * 4);
    for y in 0..OUTPUT {
        for x in 0..OUTPUT {
            let mut sum = [0u32; 3];
            let mut covered = 0u32;
            for i in [
                2 * y * edge + 2 * x,
                2 * y * edge + 2 * x + 1,
                (2 * y + 1) * edge + 2 * x,
                (2 * y + 1) * edge + 2 * x + 1,
            ] {
                if model(i) {
                    for (total, value) in sum.iter_mut().zip(&rgba[i * 4..i * 4 + 3]) {
                        *total += u32::from(*value);
                    }
                    covered += 1;
                }
            }
            let average = |c: u32| c.checked_div(covered).unwrap_or(0) as u8;
            pixels.extend([
                average(sum[0]),
                average(sum[1]),
                average(sum[2]),
                (covered * 255 / 4) as u8,
            ]);
        }
    }
    ColorImage::from_rgba_unmultiplied([OUTPUT, OUTPUT], &pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clear_color_becomes_transparent_and_edges_blend() {
        let edge = RENDER as usize;
        let mut rgba = vec![0u8; edge * edge * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[5, 6, 10, 255]);
        }
        // A model pixel in the top-left 2x2 block, and a fully covered block beside it.
        rgba[(edge + 1) * 4..(edge + 1) * 4 + 4].copy_from_slice(&[200, 100, 50, 255]);
        for (x, y) in [(2, 0), (3, 0), (2, 1), (3, 1)] {
            let i = (y * edge + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[40, 80, 120, 255]);
        }
        let image = cut_out(&rgba);
        assert_eq!(image.size, [OUTPUT, OUTPUT]);
        let pixel = |x: usize| image.pixels[x].to_srgba_unmultiplied();
        assert_eq!(pixel(0)[3], 63, "one of four samples covered");
        // Premultiplied storage rounds a quarter-covered pixel by a step or two.
        for (got, want) in pixel(0)[..3].iter().zip([200u8, 100, 50]) {
            assert!(
                got.abs_diff(want) <= 3,
                "background bled in: {:?}",
                pixel(0)
            );
        }
        assert_eq!(pixel(1), [40, 80, 120, 255]);
        assert_eq!(pixel(2)[3], 0);
    }
}
