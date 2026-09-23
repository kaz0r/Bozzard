use crate::sim::Direction;
use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use std::path::Path;

pub struct Sprites {
    texture: TextureHandle,
}

impl Sprites {
    pub fn load(ctx: &egui::Context, path: &Path) -> anyhow::Result<Self> {
        let image = image::open(path)?.to_rgba8();
        anyhow::ensure!(
            image.width() == 640 && image.height() == 640,
            "Bozz-torio sprite atlas must be 10 × 10 frames of 64 pixels"
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
        let col = (index % 10) as f32;
        let row = (index / 10) as f32;
        let uv = Rect::from_min_max(
            pos2(col / 10.0, row / 10.0),
            pos2((col + 1.0) / 10.0, (row + 1.0) / 10.0),
        );
        painter.image(self.texture.id(), rect, uv, egui::Color32::WHITE);
    }

    pub fn draw_facing(
        &self,
        painter: &egui::Painter,
        index: usize,
        rect: Rect,
        direction: Direction,
    ) {
        let col = (index % 10) as f32;
        let row = (index / 10) as f32;
        let uv = Rect::from_min_max(
            pos2(col / 10.0, row / 10.0),
            pos2((col + 1.0) / 10.0, (row + 1.0) / 10.0),
        );
        let corners = match direction {
            Direction::East => [
                uv.left_top(),
                uv.right_top(),
                uv.left_bottom(),
                uv.right_bottom(),
            ],
            Direction::South => [
                uv.left_bottom(),
                uv.left_top(),
                uv.right_bottom(),
                uv.right_top(),
            ],
            Direction::West => [
                uv.right_bottom(),
                uv.left_bottom(),
                uv.right_top(),
                uv.left_top(),
            ],
            Direction::North => [
                uv.right_top(),
                uv.right_bottom(),
                uv.left_top(),
                uv.left_bottom(),
            ],
        };
        let mut mesh = egui::Mesh::with_texture(self.texture.id());
        mesh.vertices.extend(
            rect_corners(rect)
                .into_iter()
                .zip(corners)
                .map(|(pos, uv)| egui::epaint::Vertex {
                    pos,
                    uv,
                    color: egui::Color32::WHITE,
                }),
        );
        mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
}

fn rect_corners(rect: Rect) -> [egui::Pos2; 4] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ]
}
