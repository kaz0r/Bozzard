//! Offline texture cooking. BTEX retains lossless CPU pixels and checked GPU mip payloads.
//! Encoders run only when explicitly cooking, never on a render frame or eviction restore.
use crate::{ImageData, job::Progress};
use anyhow::{Context as _, Result, ensure};
use image::{ImageEncoder, Rgba, Rgba32FImage};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};

const MAGIC: &[u8; 8] = b"BOZZTEX\0";
const VERSION: u32 = 1;
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Compression {
    Bc3,
    Astc4x4,
}

#[derive(Clone, Debug)]
pub struct EncodedTexture {
    format: Compression,
    srgb: bool,
    levels: Vec<Vec<u8>>,
}
impl EncodedTexture {
    pub fn format(&self) -> Compression {
        self.format
    }
    pub fn srgb(&self) -> bool {
        self.srgb
    }
    pub fn levels(&self) -> &[Vec<u8>] {
        &self.levels
    }
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

#[derive(Clone, Debug)]
pub struct CookedTexture {
    width: u32,
    height: u32,
    // Associate compressed data with exact fallback pixels, even after programmatic edits.
    pixels: [u8; 32],
    variants: Vec<EncodedTexture>,
}
impl CookedTexture {
    pub fn variants(&self) -> &[EncodedTexture] {
        &self.variants
    }
    pub fn matches(&self, image: &ImageData) -> bool {
        self.width == image.width
            && self.height == image.height
            && self.pixels == <[u8; 32]>::from(Sha256::digest(&image.rgba))
    }
}

pub fn mip_count(width: u32, height: u32) -> u32 {
    u32::BITS - width.max(height).leading_zeros()
}
pub fn block_bytes(width: u32, height: u32) -> usize {
    width.div_ceil(4) as usize * height.div_ceil(4) as usize * 16
}
fn validate(image: &ImageData) -> Result<()> {
    ensure!(
        (1..=4096).contains(&image.width)
            && (1..=4096).contains(&image.height)
            && image.rgba.len() == image.width as usize * image.height as usize * 4,
        "texture must contain RGBA8 pixels with dimensions 1..4096"
    );
    Ok(())
}
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn gamma(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

/// Full chain, filtering color in linear space with premultiplied alpha. Data maps
/// use independent linear channels; alpha must not weight normals/roughness/AO.
fn mip_pixels(image: &ImageData, srgb: bool, progress: &Progress) -> Result<Vec<Vec<u8>>> {
    let mut result = vec![image.rgba.clone()];
    let mut floats = Rgba32FImage::from_fn(image.width, image.height, |x, y| {
        let p = &image.rgba[((y * image.width + x) * 4) as usize..][..4];
        let alpha = f32::from(p[3]) / 255.;
        Rgba(std::array::from_fn(|i| {
            let v = f32::from(p[i]) / 255.;
            if srgb && i < 3 { linear(v) * alpha } else { v }
        }))
    });
    while floats.width() > 1 || floats.height() > 1 {
        progress.check()?;
        floats = image::imageops::resize(
            &floats,
            (floats.width() / 2).max(1),
            (floats.height() / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
        result.push(
            floats
                .pixels()
                .flat_map(|p| {
                    std::array::from_fn::<_, 4, _>(|i| {
                        let v = if srgb && i < 3 {
                            gamma(if p[3] > 0. { p[i] / p[3] } else { 0. })
                        } else {
                            p[i]
                        };
                        (v.clamp(0., 1.) * 255.).round() as u8
                    })
                })
                .collect(),
        );
    }
    Ok(result)
}

fn astc_image(
    width: u32,
    height: u32,
    plane: &mut *mut std::ffi::c_void,
) -> ctt_astcenc::bindings::astcenc_image {
    ctt_astcenc::bindings::astcenc_image {
        dim_x: width,
        dim_y: height,
        dim_z: 1,
        data_type: ctt_astcenc::bindings::astcenc_type_ASTCENC_TYPE_U8,
        data: plane,
    }
}

/// Cook the requested platform formats and color spaces in stable order. At most
/// four variants are possible. Cancellation is checked between mip codec calls.
pub fn cook(
    image: &ImageData,
    formats: &[Compression],
    color_spaces: &[bool],
    progress: &Progress,
) -> Result<CookedTexture> {
    validate(image)?;
    let formats: BTreeSet<_> = formats.iter().copied().collect();
    let spaces: BTreeSet<_> = color_spaces.iter().copied().collect();
    ensure!(
        !formats.is_empty() && !spaces.is_empty(),
        "choose texture formats and color spaces"
    );
    let chain_bytes: usize = (0..mip_count(image.width, image.height))
        .map(|level| {
            block_bytes(
                (image.width >> level).max(1),
                (image.height >> level).max(1),
            )
        })
        .sum();
    ensure!(
        chain_bytes * formats.len() * spaces.len() < MAX_BYTES,
        "compressed payloads exceed 32 MiB; use fewer platform variants"
    );
    let mut variants = Vec::new();
    for srgb in spaces {
        let mut mips = mip_pixels(image, srgb, progress)?;
        for &format in &formats {
            let mut astc = if format == Compression::Astc4x4 {
                use ctt_astcenc::{Context, Flags, Preset, Profile, config_init};
                Some(Context::new(&config_init(
                    if srgb { Profile::LdrSrgb } else { Profile::Ldr },
                    4,
                    4,
                    1,
                    Preset::Medium,
                    Flags::USE_DECODE_UNORM8,
                )?)?)
            } else {
                None
            };
            let mut levels = Vec::new();
            for (level, rgba) in mips.iter_mut().enumerate() {
                progress.stage(format!(
                    "Cooking {format:?} mip {}/{}",
                    level + 1,
                    mip_count(image.width, image.height)
                ))?;
                let (w, h) = (
                    (image.width >> level).max(1),
                    (image.height >> level).max(1),
                );
                let mut bytes = vec![0; block_bytes(w, h)];
                if let Some(astc) = &mut astc {
                    // The safe codec wrapper accepts C's image descriptor. Its sole
                    // plane points to a live, checked w*h*4 byte buffer for this call.
                    let mut plane = rgba.as_mut_ptr().cast();
                    astc.compress(
                        &mut astc_image(w, h, &mut plane),
                        ctt_astcenc::Swizzle::IDENTITY,
                        &mut bytes,
                    )?;
                    astc.compress_reset()?;
                } else {
                    texpresso::Format::Bc3.compress(
                        rgba,
                        w as usize,
                        h as usize,
                        texpresso::Params {
                            weights: if srgb {
                                texpresso::COLOUR_WEIGHTS_PERCEPTUAL
                            } else {
                                texpresso::COLOUR_WEIGHTS_UNIFORM
                            },
                            weigh_colour_by_alpha: srgb,
                            ..Default::default()
                        },
                        &mut bytes,
                    );
                }
                levels.push(bytes);
            }
            variants.push(EncodedTexture {
                format,
                srgb,
                levels,
            });
        }
    }
    progress.check()?;
    Ok(CookedTexture {
        width: image.width,
        height: image.height,
        pixels: Sha256::digest(&image.rgba).into(),
        variants,
    })
}

/// Versioned, bounded and checksummed container. PNG preserves CPU tools and
/// supplies a lossless runtime fallback when a GPU cannot sample the cooked format.
pub fn encode(image: &ImageData, cooked: &CookedTexture) -> Result<Vec<u8>> {
    validate(image)?;
    ensure!(
        cooked.matches(image),
        "cooked texture does not match source pixels"
    );
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        &image.rgba,
        image.width,
        image.height,
        image::ExtendedColorType::Rgba8,
    )?;
    let size = 28
        + png.len()
        + 32
        + cooked
            .variants
            .iter()
            .map(|v| 12 + v.levels.len() * 4 + v.bytes())
            .sum::<usize>();
    ensure!(
        size <= MAX_BYTES,
        "cooked texture exceeds 32 MiB; use fewer platform variants"
    );
    let mut bytes = Vec::with_capacity(size);
    bytes.extend(MAGIC);
    for value in [
        VERSION,
        image.width,
        image.height,
        png.len() as u32,
        cooked.variants.len() as u32,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(png);
    for variant in &cooked.variants {
        for value in [
            match variant.format {
                Compression::Bc3 => 1,
                Compression::Astc4x4 => 2,
            },
            u32::from(variant.srgb),
            variant.levels.len() as u32,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        for mip in &variant.levels {
            bytes.extend((mip.len() as u32).to_le_bytes());
            bytes.extend(mip);
        }
    }
    ensure!(
        bytes.len() + 32 <= MAX_BYTES,
        "cooked texture exceeds 32 MiB; use fewer platform variants"
    );
    let digest = Sha256::digest(&bytes);
    bytes.extend(digest);
    Ok(bytes)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .context("truncated cooked texture")?;
        self.0 = tail;
        Ok(head)
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
}

pub fn decode(bytes: &[u8]) -> Result<ImageData> {
    ensure!(
        (60..=MAX_BYTES).contains(&bytes.len()),
        "invalid cooked texture size"
    );
    let (payload, digest) = bytes.split_at(bytes.len() - 32);
    ensure!(
        Sha256::digest(payload).as_slice() == digest,
        "cooked texture checksum mismatch"
    );
    let mut r = Reader(payload);
    ensure!(
        r.bytes(8)? == MAGIC && r.word()? == VERSION,
        "unsupported cooked texture version"
    );
    let (width, height, png_bytes, count) = (r.word()?, r.word()?, r.word()?, r.word()?);
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height) && (1..=4).contains(&count),
        "invalid cooked texture header"
    );
    let mut image = crate::decoded_image(r.bytes(png_bytes as usize)?, "cooked fallback")?;
    ensure!(
        image.width == width && image.height == height,
        "cooked texture dimensions mismatch"
    );
    let mut variants = Vec::new();
    let mut keys = BTreeSet::new();
    for _ in 0..count {
        let format = match r.word()? {
            1 => Compression::Bc3,
            2 => Compression::Astc4x4,
            _ => anyhow::bail!("unsupported cooked texture format"),
        };
        let srgb = match r.word()? {
            0 => false,
            1 => true,
            _ => anyhow::bail!("invalid cooked texture color space"),
        };
        ensure!(
            keys.insert((format, srgb)),
            "duplicate cooked texture variant"
        );
        let levels = r.word()?;
        ensure!(
            levels == mip_count(width, height),
            "invalid cooked texture mip count"
        );
        let mut mips = Vec::new();
        for level in 0..levels {
            let expected = block_bytes((width >> level).max(1), (height >> level).max(1));
            ensure!(
                r.word()? as usize == expected,
                "invalid cooked texture mip size"
            );
            mips.push(r.bytes(expected)?.to_vec());
        }
        variants.push(EncodedTexture {
            format,
            srgb,
            levels: mips,
        });
    }
    ensure!(r.0.is_empty(), "trailing cooked texture data");
    image.compressed = Some(Arc::new(CookedTexture {
        width,
        height,
        pixels: Sha256::digest(&image.rgba).into(),
        variants,
    }));
    Ok(image)
}

#[cfg(test)]
mod tests;
