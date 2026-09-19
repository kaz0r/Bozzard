use super::*;

fn fixture(width: u32, height: u32) -> ImageData {
    ImageData {
        width,
        height,
        rgba: (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    [
                        (x * 255 / width.max(2)) as u8,
                        (y * 255 / height.max(2)) as u8,
                        96,
                        255,
                    ]
                })
            })
            .collect(),
        compressed: None,
    }
}
fn resign(bytes: &mut [u8]) {
    let n = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..n]);
    bytes[n..].copy_from_slice(&digest);
}
#[test]
fn codecs_preserve_quality_mips_and_lossless_fallback() {
    for (width, height) in [(1, 1), (3, 7), (32, 20)] {
        let image = fixture(width, height);
        let cooked = cook(
            &image,
            &[Compression::Bc3, Compression::Astc4x4],
            &[false, true],
            &Progress::default(),
        )
        .unwrap();
        let bytes = encode(&image, &cooked).unwrap();
        let restored = decode(&bytes).unwrap();
        assert_eq!(restored.rgba, image.rgba);
        assert_eq!(restored.width, width);
        assert_eq!(restored.height, height);
        assert_eq!(
            encode(&restored, restored.compressed.as_ref().unwrap()).unwrap(),
            bytes
        );
        for v in cooked.variants() {
            assert_eq!(v.levels().len(), mip_count(width, height) as usize);
            let expected_mips = mip_pixels(&image, v.srgb(), &Progress::default()).unwrap();
            for (level, bytes) in v.levels().iter().enumerate() {
                let (w, h) = ((width >> level).max(1), (height >> level).max(1));
                assert_eq!(bytes.len(), block_bytes(w, h));
                let mut rgba = vec![0; w as usize * h as usize * 4];
                match v.format() {
                    Compression::Bc3 => {
                        texpresso::Format::Bc3.decompress(bytes, w as usize, h as usize, &mut rgba)
                    }
                    Compression::Astc4x4 => {
                        use ctt_astcenc::{Context, Flags, Preset, Profile, Swizzle, config_init};
                        let config = config_init(
                            if v.srgb() {
                                Profile::LdrSrgb
                            } else {
                                Profile::Ldr
                            },
                            4,
                            4,
                            1,
                            Preset::Medium,
                            Flags::DECOMPRESS_ONLY,
                        )
                        .unwrap();
                        let mut codec = Context::new(&config).unwrap();
                        let mut plane = rgba.as_mut_ptr().cast();
                        codec
                            .decompress(bytes, &mut astc_image(w, h, &mut plane), Swizzle::IDENTITY)
                            .unwrap();
                    }
                }
                let expected = &expected_mips[level];
                let mse = rgba
                    .iter()
                    .zip(expected)
                    .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                    .sum::<f64>()
                    / rgba.len() as f64;
                assert!(
                    mse.sqrt() < 28.,
                    "{:?} {w}x{h} RMSE {}",
                    v.format(),
                    mse.sqrt()
                );
                assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
            }
        }
    }
}

#[test]
fn mip_filtering_respects_gamma_alpha_and_independent_data_channels() {
    let mut image = ImageData {
        width: 2,
        height: 1,
        rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
        compressed: None,
    };
    let p = Progress::default();
    let color = mip_pixels(&image, true, &p).unwrap();
    let data = mip_pixels(&image, false, &p).unwrap();
    assert!((186..=189).contains(&color[1][0]));
    assert!((127..=129).contains(&data[1][0]));
    image.rgba = vec![255, 0, 0, 255, 0, 0, 255, 0];
    let color = mip_pixels(&image, true, &p).unwrap();
    assert_eq!(&color[1][..3], &[255, 0, 0]);
    assert!((127..=129).contains(&color[1][3]));
    let data = mip_pixels(&image, false, &p).unwrap();
    assert!(data[1][2] >= 127);
}

#[test]
fn malformed_truncated_stale_and_cancelled_cooks_fail() {
    let mut image = fixture(8, 8);
    let cooked = cook(&image, &[Compression::Bc3], &[true], &Progress::default()).unwrap();
    let original = encode(&image, &cooked).unwrap();
    for n in 0..original.len() {
        assert!(decode(&original[..n]).is_err());
    }
    for offset in [0, 8, 12, 16, 20, 24, original.len() - 33] {
        let mut bytes = original.clone();
        bytes[offset] ^= 255;
        assert!(decode(&bytes).is_err());
    }
    for (offset, value) in [(8, 2_u32), (12, 4097), (16, 0), (20, u32::MAX), (24, 5)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        resign(&mut bytes);
        assert!(decode(&bytes).is_err());
    }
    let png_size = u32::from_le_bytes(original[20..24].try_into().unwrap()) as usize;
    for offset in [28 + png_size, 32 + png_size, 36 + png_size, 40 + png_size] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        resign(&mut bytes);
        assert!(decode(&bytes).is_err());
    }
    image.rgba[0] ^= 255;
    assert!(encode(&image, &cooked).is_err());
    image.rgba.pop();
    assert!(cook(&image, &[Compression::Bc3], &[true], &Progress::default()).is_err());
    let (send, recv) = std::sync::mpsc::channel();
    let job = crate::job::Job::start("Cook", move |progress| {
        recv.recv().unwrap();
        cook(&fixture(16, 16), &[Compression::Bc3], &[true], &progress)
    })
    .unwrap();
    job.cancel();
    send.send(()).unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(result) = job.poll() {
            assert!(result.unwrap_err().to_string().contains("cancelled"));
            break;
        }
        assert!(start.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
