use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use std::path::Path;

pub struct Sprites {
    texture: TextureHandle,
}

impl Sprites {
    pub fn load(ctx: &egui::Context, path: &Path) -> anyhow::Result<Self> {
        let image = image::open(path)?.to_rgba8();
        anyhow::ensure!(
            image.width() == 320 && image.height() == 256,
            "Bozz-torio sprite atlas must be 5 × 4 frames of 64 pixels"
        );
        let size = [image.width() as usize, image.height() as usize];
        let color = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture = ctx.load_texture(
            "original-bozz-torio-sprites",
            color,
            TextureOptions::NEAREST,
        );
        Ok(Self { texture })
    }

    pub fn draw(&self, painter: &egui::Painter, index: usize, rect: Rect) {
        let col = (index % 5) as f32;
        let row = (index / 5) as f32;
        let uv = Rect::from_min_max(
            pos2(col / 5.0, row / 4.0),
            pos2((col + 1.0) / 5.0, (row + 1.0) / 4.0),
        );
        painter.image(self.texture.id(), rect, uv, egui::Color32::WHITE);
    }
}
