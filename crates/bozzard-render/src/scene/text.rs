use super::*;
use epaint::{
    Color32, FontFamily, FontId,
    text::{FontDefinitions, Fonts, LayoutJob, TextOptions},
};
use std::{cell::RefCell, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMesh {
    pub text: String,
    pub font_size: f32,
    pub max_width: Option<f32>,
    pub monospace: bool,
    pub alignment: TextAlignment,
    pub opacity: f32,
}
impl Default for TextMesh {
    fn default() -> Self {
        Self {
            text: "Text".into(),
            font_size: 0.5,
            max_width: None,
            monospace: false,
            alignment: TextAlignment::Left,
            opacity: 1.,
        }
    }
}
type Key = (String, u32, Option<u32>, bool, TextAlignment);
impl TextMesh {
    fn key(&self) -> Key {
        (
            self.text.clone(),
            self.font_size.to_bits(),
            self.max_width.map(f32::to_bits),
            self.monospace,
            self.alignment,
        )
    }
    fn validate(&self) -> Result<()> {
        ensure!(self.text.len() <= 4096, "text exceeds 4096 UTF-8 bytes");
        ensure!(
            self.font_size.is_finite() && (0.001..=1000.).contains(&self.font_size),
            "invalid text font size"
        );
        ensure!(
            self.max_width
                .is_none_or(|v| v.is_finite() && (0.001..=10000.).contains(&v)),
            "invalid text width"
        );
        ensure!(
            self.opacity.is_finite() && (0.0..=1.).contains(&self.opacity),
            "invalid text opacity"
        );
        Ok(())
    }
}
// ponytail: 64-pixel/em antialiased glyphs; add distance fields if extreme magnification needs them.
const EM: f32 = 64.;
fn options() -> TextOptions {
    TextOptions {
        max_texture_side: 4096,
        ..Default::default()
    }
}
fn fonts() -> Fonts {
    Fonts::new(options(), FontDefinitions::default())
}
fn layout(fonts: &mut Fonts, text: &TextMesh) -> Result<Arc<epaint::text::Galley>> {
    text.validate()?;
    let mut job = LayoutJob::simple(
        text.text.clone(),
        FontId::new(
            EM,
            if text.monospace {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            },
        ),
        Color32::WHITE,
        text.max_width
            .map_or(f32::INFINITY, |w| w * EM / text.font_size),
    );
    job.halign = match text.alignment {
        TextAlignment::Left => epaint::emath::Align::LEFT,
        TextAlignment::Center => epaint::emath::Align::Center,
        TextAlignment::Right => epaint::emath::Align::RIGHT,
    };
    let galley = fonts.with_pixels_per_point(1.).layout_job(job);
    ensure!(
        fonts.font_atlas_fill_ratio() < 1.,
        "text glyph atlas is full; reduce distinct glyphs in this view"
    );
    Ok(galley)
}
thread_local! { static BOUNDS_FONTS: RefCell<Fonts> = RefCell::new(fonts()); }
/// Local layout envelope, also used for editor picking and framing. No GPU required.
pub fn text_bounds(text: &TextMesh) -> Result<Option<[Vec3; 2]>> {
    BOUNDS_FONTS.with_borrow_mut(|fonts| {
        fonts.begin_pass(options());
        let galley = layout(fonts, text)?;
        if galley.num_indices == 0 {
            return Ok(None);
        }
        let rect = galley.rect.union(galley.mesh_bounds);
        let scale = text.font_size / EM;
        Ok(Some([
            Vec3::new(rect.min.x, -rect.max.y, 0.) * scale,
            Vec3::new(rect.max.x, -rect.min.y, 0.) * scale,
        ]))
    })
}

pub(super) struct TextRenderer {
    fonts: Fonts,
    meshes: BTreeMap<Key, MeshBuffers>,
    texture: Option<wgpu::Texture>,
    pub view: Option<wgpu::TextureView>,
    size: [usize; 2],
}
impl TextRenderer {
    fn new() -> Self {
        Self {
            fonts: fonts(),
            meshes: BTreeMap::new(),
            texture: None,
            view: None,
            size: [0; 2],
        }
    }
    pub fn mesh(&self, text: &TextMesh) -> Option<&MeshBuffers> {
        self.meshes.get(&text.key())
    }
    fn prepare(&mut self, gpu: &Gpu, items: &[DrawItem]) -> Result<bool> {
        ensure!(
            gpu.device.limits().max_texture_dimension_2d >= 4096,
            "text requires a 4096-pixel font atlas limit"
        );
        let reset = self.fonts.font_atlas_fill_ratio() > 0.8;
        self.fonts.begin_pass(options());
        let mut galleys = BTreeMap::new();
        let mut bytes = 0;
        for item in items {
            if let MeshKind::Text(text) = &item.mesh {
                text.validate()?;
                bytes += text.text.len();
                ensure!(bytes <= 65536, "text view exceeds 65536 UTF-8 bytes");
                let key = text.key();
                if let std::collections::btree_map::Entry::Vacant(entry) = galleys.entry(key) {
                    entry.insert(layout(&mut self.fonts, text)?);
                }
            }
        }
        let size = self.fonts.font_image_size();
        let resized = self.size != size;
        if reset || resized {
            self.meshes.clear();
        }
        self.meshes.retain(|key, _| galleys.contains_key(key));
        for (key, galley) in galleys {
            if galley.num_indices == 0 || self.meshes.contains_key(&key) {
                continue;
            }
            let scale = f32::from_bits(key.1) / EM;
            let mut vertices = Vec::with_capacity(galley.num_vertices);
            let mut indices = Vec::with_capacity(galley.num_indices);
            for row in &galley.rows {
                let start = vertices.len() as u32;
                for v in &row.visuals.mesh.vertices {
                    let p = v.pos + row.pos.to_vec2();
                    vertices.push([
                        p.x * scale,
                        -p.y * scale,
                        0.,
                        0.,
                        0.,
                        1.,
                        v.uv.x / size[0] as f32,
                        v.uv.y / size[1] as f32,
                    ]);
                }
                // Flipping Y reverses winding; preserve +Z as the front face.
                for triangle in row.visuals.mesh.indices.chunks_exact(3) {
                    indices.extend([
                        start + triangle[0],
                        start + triangle[2],
                        start + triangle[1],
                    ]);
                }
            }
            self.meshes.insert(key, mesh(gpu, &vertices, &indices));
        }
        if let Some(delta) = self.fonts.font_image_delta() {
            if resized {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("text glyph atlas"),
                    size: wgpu::Extent3d {
                        width: size[0] as u32,
                        height: size[1] as u32,
                        depth_or_array_layers: 1,
                    },
                    // Keep one level: epaint's one-pixel glyph padding cannot isolate mip footprints.
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                self.view = Some(texture.create_view(&Default::default()));
                self.texture = Some(texture);
                self.size = size;
            }
            let epaint::ImageData::Color(image) = delta.image;
            let rgba: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|p| [255, 255, 255, p.a()])
                .collect();
            let pos = delta.pos.unwrap_or([0, 0]);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: self.texture.as_ref().unwrap(),
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: pos[0] as u32,
                        y: pos[1] as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.size[0] as u32 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: image.size[0] as u32,
                    height: image.size[1] as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(resized)
    }
}
impl SceneRenderer {
    pub(super) fn prepare_text(&mut self, gpu: &Gpu, scene: &RenderScene) -> Result<()> {
        if scene
            .items
            .iter()
            .any(|i| matches!(i.mesh, MeshKind::Text(_)))
        {
            if self
                .text
                .get_or_insert_with(TextRenderer::new)
                .prepare(gpu, &scene.items)?
            {
                // The atlas view was replaced: bindings must not retain its old texture.
                self.objects.clear();
            }
        } else if self.text.take().is_some() {
            self.objects.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires hardware GPU; run with --ignored --nocapture"]
    fn text_gpu_depth_atlas_growth_edits_and_cleanup() -> Result<()> {
        let instance = crate::instance(crate::Backend::native());
        let gpu = pollster::block_on(Gpu::request(&instance, None, false))?;
        gpu.require_hardware()?;
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut scene = RenderScene {
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: Default::default(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: Mat4::from_scale(Vec3::new(0.5, 0.5, 0.1)),
            items: vec![],
        };
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
            crate::capture_offscreen(&gpu, 320, 240, |view| {
                renderer.draw_linear(&gpu, view, [320, 240], scene)
            })
        };
        let empty = capture(&mut renderer, &scene)?;
        let item = DrawItem {
            model: Mat4::from_translation(Vec3::new(-1.5, 1., 1.)),
            mesh: MeshKind::Text(TextMesh {
                text: "Hi!".into(),
                ..Default::default()
            }),
            material: Material {
                surface_overrides: Default::default(),
                tint: [1., 0.1, 0.05],
                uv_scale: [1.; 2],
                texture: TextureKind::Text,
                lit: false,
            },
        };
        scene.items.push(item.clone());
        let original = capture(&mut renderer, &scene)?;
        assert!(
            original
                .rgba
                .chunks_exact(4)
                .filter(|p| p[0] > 100 && u16::from(p[0]) > u16::from(p[1]) * 2)
                .count()
                > 100
        );
        let old_size = renderer.text.as_ref().unwrap().size;
        let mut extra = item.clone();
        extra.model = Mat4::from_translation(Vec3::new(50., 0., 1.));
        extra.mesh = MeshKind::Text(TextMesh {
            text: (32..0x500).filter_map(char::from_u32).collect(),
            max_width: Some(8.),
            ..Default::default()
        });
        scene.items.push(extra);
        let grown = capture(&mut renderer, &scene)?;
        assert_ne!(
            old_size,
            renderer.text.as_ref().unwrap().size,
            "must exercise atlas growth"
        );
        assert!(
            original.rgba == grown.rgba,
            "atlas resizing must preserve existing glyph UVs"
        );
        scene.items.pop();
        assert_eq!(capture(&mut renderer, &scene)?.rgba, original.rgba);
        let mut occluder = item.clone();
        occluder.mesh = MeshKind::Quad;
        occluder.material.texture = TextureKind::White;
        occluder.model = Mat4::from_scale_rotation_translation(
            Vec3::new(4., 4., 1.),
            glam::Quat::IDENTITY,
            Vec3::new(0., 0., 0.5),
        );
        scene.items.push(occluder.clone());
        let hidden = capture(&mut renderer, &scene)?;
        scene.items = vec![occluder];
        assert_eq!(
            hidden.rgba,
            capture(&mut renderer, &scene)?.rgba,
            "opaque geometry must occlude text"
        );
        scene.items = vec![item.clone()];
        if let MeshKind::Text(text) = &mut scene.items[0].mesh {
            text.opacity = 0.;
        }
        assert_eq!(capture(&mut renderer, &scene)?.rgba, empty.rgba);
        scene.items = vec![item];
        let mut baseline = None;
        for i in 0..100 {
            if let MeshKind::Text(text) = &mut scene.items[0].mesh {
                text.text = format!("Frame {i}");
            }
            capture(&mut renderer, &scene)?;
            let text = renderer.text.as_ref().unwrap();
            assert_eq!(text.meshes.len(), 1);
            assert!(text.fonts.num_galleys_in_cache() <= 3);
            gpu.wait()?;
            let report = instance.generate_report().context("missing GPU report")?;
            let counts = [
                report.hub.buffers.num_allocated,
                report.hub.textures.num_allocated,
                report.hub.bind_groups.num_allocated,
            ];
            if i == 20 {
                baseline = Some(counts);
            }
            if let Some(base) = baseline {
                assert!(counts.iter().zip(base).all(|(a, b)| *a <= b + 2));
            }
        }
        scene.items.clear();
        assert_eq!(capture(&mut renderer, &scene)?.rgba, empty.rgba);
        assert!(renderer.text.is_none());
        println!("text_gpu_ok pixels depth opacity atlas_growth bounded_edits cleanup");
        Ok(())
    }

    #[test]
    fn layout_wraps_aligns_scales_and_validates() {
        let mut text = TextMesh {
            text: "Hello café\n世界".into(),
            ..Default::default()
        };
        let b = text_bounds(&text).unwrap().unwrap();
        assert!(b[1].x > b[0].x && b[0].y < -text.font_size);
        text.font_size *= 2.;
        let big = text_bounds(&text).unwrap().unwrap();
        assert!(big[0].abs_diff_eq(b[0] * 2., 1e-5));
        text.text = "hello world hello world".into();
        let wide = text_bounds(&text).unwrap().unwrap();
        text.max_width = Some(3.);
        let wrapped = text_bounds(&text).unwrap().unwrap();
        assert!(wrapped[1].x < wide[1].x && wrapped[0].y < wide[0].y);
        text.alignment = TextAlignment::Center;
        let centered = text_bounds(&text).unwrap().unwrap();
        assert!((centered[0].x + centered[1].x).abs() < 0.05);
        text.alignment = TextAlignment::Right;
        assert!(text_bounds(&text).unwrap().unwrap()[1].x < 0.05);
        text.text.clear();
        assert!(text_bounds(&text).unwrap().is_none());
        text.font_size = f32::NAN;
        assert!(text_bounds(&text).is_err());
    }
}
