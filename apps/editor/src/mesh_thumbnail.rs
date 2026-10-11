//! Content-browser model thumbnails: a small software rasterizer with a depth buffer.
//! Every triangle is drawn with its part's base color and texture, lit from above, so a
//! preview matches the model whether or not its GPU resources are resident.
use bozzard_assets::{ImageData, MeshData};
use eframe::egui::ColorImage;
use glam::{Mat3, Vec2, Vec3};

/// Output edge in pixels; tiles draw it scaled down, so it stays sharp on HiDPI.
const SIZE: usize = 160;
const SUPERSAMPLE: usize = 2;
/// Larger meshes are drawn with an even stride; the silhouette survives, detail thins.
const MAX_TRIANGLES: usize = 300_000;
const MARGIN: f32 = 0.08;

struct Part<'a> {
    range: std::ops::Range<usize>,
    color: Vec3,
    image: Option<&'a ImageData>,
    cutoff: Option<f32>,
}

pub fn render(mesh: &MeshData) -> ColorImage {
    let edge = SIZE * SUPERSAMPLE;
    // Seen from above and to the right: the three-quarter view people expect of an asset.
    let view = Mat3::from_rotation_x(0.52) * Mat3::from_rotation_y(-0.62);
    let positions: Vec<Vec3> = mesh
        .vertices
        .iter()
        .map(|v| view * Vec3::new(v[0], v[1], v[2]))
        .collect();
    let normals: Vec<Vec3> = mesh
        .vertices
        .iter()
        .map(|v| (view * Vec3::new(v[3], v[4], v[5])).normalize_or_zero())
        .collect();
    let used = mesh
        .indices
        .iter()
        .filter_map(|&i| positions.get(i as usize));
    let (min, max) = used.fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(min, max), p| (min.min(p.truncate()), max.max(p.truncate())),
    );
    let mut color = vec![Vec3::ZERO; edge * edge];
    let mut depth = vec![f32::NEG_INFINITY; edge * edge];
    if min.x <= max.x {
        let extent = (max - min).max_element().max(1e-6);
        let scale = edge as f32 * (1.0 - 2.0 * MARGIN) / extent;
        let center = (min + max) * 0.5;
        // Pixel space: y down, centred; depth grows toward the viewer.
        let screen: Vec<Vec3> = positions
            .iter()
            .map(|p| {
                Vec3::new(
                    edge as f32 * 0.5 + (p.x - center.x) * scale,
                    edge as f32 * 0.5 - (p.y - center.y) * scale,
                    p.z,
                )
            })
            .collect();
        let triangles = mesh.indices.len() / 3;
        let stride = triangles.div_ceil(MAX_TRIANGLES).max(1);
        for part in parts(mesh) {
            for triangle in part.range.clone().step_by(stride) {
                let Some(corners) = mesh.indices.get(triangle * 3..triangle * 3 + 3) else {
                    continue;
                };
                let index = [
                    corners[0] as usize,
                    corners[1] as usize,
                    corners[2] as usize,
                ];
                if index.iter().any(|&i| i >= screen.len()) {
                    continue;
                }
                raster(
                    &part,
                    index.map(|i| screen[i]),
                    index.map(|i| normals[i]),
                    index.map(|i| Vec2::new(mesh.vertices[i][6], mesh.vertices[i][7])),
                    edge,
                    &mut color,
                    &mut depth,
                );
            }
        }
    }
    let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut sum = Vec3::ZERO;
            let mut covered = 0;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let i = (y * SUPERSAMPLE + sy) * edge + x * SUPERSAMPLE + sx;
                    if depth[i] > f32::NEG_INFINITY {
                        sum += color[i];
                        covered += 1;
                    }
                }
            }
            let average = if covered > 0 {
                sum / covered as f32
            } else {
                Vec3::ZERO
            };
            let alpha = covered as f32 / (SUPERSAMPLE * SUPERSAMPLE) as f32;
            pixels.extend([
                to_srgb(average.x),
                to_srgb(average.y),
                to_srgb(average.z),
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &pixels)
}

fn parts(mesh: &MeshData) -> Vec<Part<'_>> {
    let triangles = mesh.indices.len() / 3;
    if mesh.parts.is_empty() {
        return vec![Part {
            range: 0..triangles,
            color: Vec3::splat(0.7),
            image: None,
            cutoff: None,
        }];
    }
    mesh.parts
        .iter()
        .map(|part| {
            let start = part.start as usize / 3;
            Part {
                range: start.min(triangles)..(start + part.count as usize / 3).min(triangles),
                color: Vec3::new(part.color[0], part.color[1], part.color[2]),
                image: part.image.as_deref(),
                cutoff: part.alpha_cutoff,
            }
        })
        .collect()
}

fn raster(
    part: &Part<'_>,
    p: [Vec3; 3],
    n: [Vec3; 3],
    uv: [Vec2; 3],
    edge: usize,
    color: &mut [Vec3],
    depth: &mut [f32],
) {
    let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
    if area.abs() < 1e-8 {
        return;
    }
    let face = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
    let bound = |values: [f32; 3], floor: bool| {
        let v = if floor {
            values.iter().copied().fold(f32::INFINITY, f32::min).floor()
        } else {
            values
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
        };
        v.clamp(0.0, (edge - 1) as f32) as usize
    };
    let (x0, x1) = (bound(p.map(|p| p.x), true), bound(p.map(|p| p.x), false));
    let (y0, y1) = (bound(p.map(|p| p.y), true), bound(p.map(|p| p.y), false));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((p[1].x - px) * (p[2].y - py) - (p[2].x - px) * (p[1].y - py)) / area;
            let w1 = ((p[2].x - px) * (p[0].y - py) - (p[0].x - px) * (p[2].y - py)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
            let i = y * edge + x;
            if z <= depth[i] {
                continue;
            }
            let mut base = part.color;
            if let Some(image) = part.image {
                let texel = sample(image, uv[0] * w0 + uv[1] * w1 + uv[2] * w2);
                if part.cutoff.is_some_and(|cutoff| texel[3] < cutoff) {
                    continue;
                }
                base *= Vec3::new(texel[0], texel[1], texel[2]);
            }
            let mut normal = (n[0] * w0 + n[1] * w1 + n[2] * w2).normalize_or(face);
            // Two-sided: the visible side of a thin surface faces the viewer.
            if normal.z < 0.0 {
                normal = -normal;
            }
            let key = normal.dot(Vec3::new(-0.35, 0.8, 0.5).normalize()).max(0.0);
            let sky = 0.5 + 0.5 * normal.y;
            depth[i] = z;
            color[i] = base * (0.22 + 0.16 * sky + 0.78 * key);
        }
    }
}

/// Nearest texel, wrapped, as linear RGB plus alpha.
fn sample(image: &ImageData, uv: Vec2) -> [f32; 4] {
    if image.width == 0 || image.height == 0 {
        return [1.0; 4];
    }
    let x = (uv.x.rem_euclid(1.0) * image.width as f32) as usize % image.width as usize;
    let y = (uv.y.rem_euclid(1.0) * image.height as f32) as usize % image.height as usize;
    let i = (y * image.width as usize + x) * 4;
    let Some(texel) = image.rgba.get(i..i + 4) else {
        return [1.0; 4];
    };
    [
        to_linear(texel[0]),
        to_linear(texel[1]),
        to_linear(texel[2]),
        texel[3] as f32 / 255.0,
    ]
}

fn to_linear(value: u8) -> f32 {
    let c = value as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(value: f32) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(offset: f32) -> (Vec<[f32; 8]>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for axis in 0..3 {
            for sign in [-1.0f32, 1.0] {
                let mut normal = [0.0; 3];
                normal[axis] = sign;
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let base = vertices.len() as u32;
                for (a, b) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                    let mut p = [0.0f32; 3];
                    p[axis] = sign * 0.5;
                    p[u] = a;
                    p[v] = b;
                    p[0] += offset;
                    vertices.push([p[0], p[1], p[2], normal[0], normal[1], normal[2], 0.0, 0.0]);
                }
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
        (vertices, indices)
    }

    fn pixel(image: &ColorImage, x: usize, y: usize) -> [u8; 4] {
        image.pixels[y * image.size[0] + x].to_srgba_unmultiplied()
    }

    #[test]
    fn a_cube_fills_the_centre_and_leaves_the_corners_transparent() {
        let (vertices, indices) = cube(0.0);
        let mesh = MeshData {
            skin: None,
            vertices,
            indices,
            parts: vec![],
            warnings: vec![],
        };
        let image = render(&mesh);
        assert_eq!(image.size, [SIZE, SIZE]);
        assert_eq!(pixel(&image, SIZE / 2, SIZE / 2)[3], 255);
        assert_eq!(pixel(&image, 1, 1)[3], 0);
        assert_eq!(pixel(&image, SIZE - 2, SIZE - 2)[3], 0);
    }

    #[test]
    fn each_part_keeps_its_own_base_color() {
        let (mut vertices, mut indices) = cube(-0.75);
        let (right, right_indices) = cube(0.75);
        let base = vertices.len() as u32;
        let left_count = indices.len() as u32;
        vertices.extend(right);
        indices.extend(right_indices.iter().map(|i| i + base));
        let part = |start: u32, count: u32, color: [f32; 4]| bozzard_assets::MeshPart {
            source_key: String::new(),
            name: String::new(),
            material_name: None,
            start,
            count,
            color,
            image: None,
            alpha_cutoff: None,
            shading: None,
        };
        let mesh = MeshData {
            skin: None,
            vertices,
            indices,
            parts: vec![
                part(0, left_count, [1.0, 0.0, 0.0, 1.0]),
                part(left_count, left_count, [0.0, 0.0, 1.0, 1.0]),
            ],
            warnings: vec![],
        };
        let image = render(&mesh);
        let row = SIZE / 2;
        let opaque: Vec<_> = (0..SIZE)
            .map(|x| pixel(&image, x, row))
            .filter(|p| p[3] == 255)
            .collect();
        assert!(opaque.iter().any(|p| p[0] > 100 && p[2] < 30), "red part");
        assert!(opaque.iter().any(|p| p[2] > 100 && p[0] < 30), "blue part");
    }
}
