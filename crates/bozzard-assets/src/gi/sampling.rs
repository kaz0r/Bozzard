use crate::{Filter, ImageData, Sampler, Wrap};
use glam::{Vec2, Vec3, Vec4};

pub(super) fn srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn address(i: i64, size: u32, wrap: Wrap) -> u32 {
    let n = i64::from(size);
    match wrap {
        Wrap::Clamp => i.clamp(0, n - 1) as u32,
        Wrap::Repeat => i.rem_euclid(n) as u32,
        Wrap::Mirror => {
            let p = i.rem_euclid(n * 2);
            if p < n {
                p as u32
            } else {
                (n * 2 - 1 - p) as u32
            }
        }
    }
}
pub(super) fn image(image: &ImageData, sampler: Sampler, uv: Vec2, color: bool) -> Vec4 {
    let fetch = |x: i64, y: i64| {
        let x = address(x, image.width, sampler.wrap_u);
        let y = address(y, image.height, sampler.wrap_v);
        let i = ((y * image.width + x) * 4) as usize;
        let p = &image.rgba[i..i + 4];
        let mut rgba = Vec4::new(
            f32::from(p[0]),
            f32::from(p[1]),
            f32::from(p[2]),
            f32::from(p[3]),
        ) / 255.;
        if color {
            rgba.x = srgb(rgba.x);
            rgba.y = srgb(rgba.y);
            rgba.z = srgb(rgba.z);
        }
        rgba
    };
    let p = uv * Vec2::new(image.width as f32, image.height as f32);
    if sampler.mag == Filter::Nearest {
        return fetch(p.x.floor() as i64, p.y.floor() as i64);
    }
    let p = p - Vec2::splat(0.5);
    let base = p.floor();
    let f = p - base;
    fetch(base.x as i64, base.y as i64)
        .lerp(fetch(base.x as i64 + 1, base.y as i64), f.x)
        .lerp(
            fetch(base.x as i64, base.y as i64 + 1)
                .lerp(fetch(base.x as i64 + 1, base.y as i64 + 1), f.x),
            f.y,
        )
}
/// Deterministic low-discrepancy spherical samples; independent per-probe rotation.
pub(super) fn sphere(index: u32, count: u32, rotation: f32) -> Vec3 {
    let y = 1. - 2. * (index as f32 + 0.5) / count as f32;
    let phi =
        std::f32::consts::TAU * ((index.reverse_bits() as f64 / 4294967296.) as f32 + rotation);
    let r = (1. - y * y).max(0.).sqrt();
    Vec3::new(r * phi.cos(), y, r * phi.sin())
}
pub(super) fn random(state: &mut u64) -> f32 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((z ^ (z >> 31)) >> 40) as f32 / 16777216.
}
pub(super) fn cosine(normal: Vec3, state: &mut u64) -> Vec3 {
    let radius = random(state).sqrt();
    let phi = random(state) * std::f32::consts::TAU;
    let helper = if normal.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = normal.cross(helper).normalize();
    let up = right.cross(normal);
    (right * (radius * phi.cos())
        + up * (radius * phi.sin())
        + normal * (1. - radius * radius).max(0.).sqrt())
    .normalize()
}
pub(super) fn sh(n: Vec3) -> [f32; 9] {
    [
        0.2820948,
        0.48860252 * n.y,
        0.48860252 * n.z,
        0.48860252 * n.x,
        1.0925485 * n.x * n.y,
        1.0925485 * n.y * n.z,
        0.31539157 * (3. * n.z * n.z - 1.),
        1.0925485 * n.x * n.z,
        0.54627424 * (n.x * n.x - n.y * n.y),
    ]
}
#[cfg(test)]
pub(super) fn oct_encode(n: Vec3) -> Vec2 {
    let p = n / (n.x.abs() + n.y.abs() + n.z.abs());
    let mut uv = p.truncate();
    if p.z < 0. {
        uv = Vec2::new(
            (1. - uv.y.abs()) * if uv.x >= 0. { 1. } else { -1. },
            (1. - uv.x.abs()) * if uv.y >= 0. { 1. } else { -1. },
        );
    }
    uv * 0.5 + Vec2::splat(0.5)
}
pub(super) fn oct_decode(uv: Vec2) -> Vec3 {
    let p = uv * 2. - Vec2::ONE;
    let mut n = Vec3::new(p.x, p.y, 1. - p.x.abs() - p.y.abs());
    let t = (-n.z).clamp(0., 1.);
    n.x += if n.x >= 0. { -t } else { t };
    n.y += if n.y >= 0. { -t } else { t };
    n.normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spherical_samples_and_octahedral_mapping_cover_both_hemispheres() {
        let mut mean = Vec3::ZERO;
        for i in 0..1024 {
            let d = sphere(i, 1024, 0.37);
            assert!((d.length() - 1.).abs() < 1e-6);
            assert!(oct_decode(oct_encode(d)).distance(d) < 1e-5);
            mean += d / 1024.;
        }
        assert!(mean.length() < 0.001);
        for d in [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::NEG_X,
            Vec3::NEG_Y,
            Vec3::NEG_Z,
        ] {
            assert!(oct_decode(oct_encode(d)).distance(d) < 1e-6);
        }
    }
}
