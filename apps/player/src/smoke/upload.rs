use super::*;
use bozzard_assets::{AssetData, ImageData, MeshData, MeshPart};
use std::sync::Arc;

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let scene = RenderScene {
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
        view_projection: Mat4::IDENTITY,
        items: vec![DrawItem {
            model: Mat4::from_translation(Vec3::new(0., 0., 0.5)),
            mesh: MeshKind::Imported("staged".into()),
            material: Material {
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [128.; 2],
                texture: TextureKind::White,
                lit: false,
            },
        }],
    };
    let model = |image: Arc<ImageData>| {
        AssetData::Mesh(MeshData {
            vertices: vec![
                [-0.8, -0.8, 0., 0., 0., 1., 0., 1.],
                [0.8, -0.8, 0., 0., 0., 1., 1., 1.],
                [0.8, 0.8, 0., 0., 0., 1., 1., 0.],
                [-0.8, 0.8, 0., 0., 0., 1., 0., 0.],
            ],
            indices: vec![0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3],
            parts: [0, 6]
                .into_iter()
                .map(|start| MeshPart {
                    source_key: format!("{start:016x}"),
                    name: format!("Surface {start}"),
                    material_name: None,
                    start,
                    count: 6,
                    color: [1.; 4],
                    image: Some(image.clone()),
                    alpha_cutoff: None,
                    shading: None,
                })
                .collect(),
            warnings: vec![],
        })
    };
    bozzard_render_assets::upload(
        gpu,
        &mut renderer,
        "staged",
        &model(Arc::new(ImageData {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        })),
    )?;
    let old = capture(gpu, &mut renderer, &scene, [64, 64])?;
    pixel(&old, 32, 32, [255, 0, 0])?;
    let pixels: Vec<u8> = (0..64)
        .flat_map(|y| {
            (0..64).flat_map(move |x| {
                let c = if (x + y) % 2 == 0 { 0 } else { 255 };
                [c, c, c, 255]
            })
        })
        .collect();
    let source = Arc::new(model(Arc::new(ImageData {
        width: 64,
        height: 64,
        rgba: pixels,
    })));
    let weak = Arc::downgrade(&source);
    let mut cancelled =
        renderer.begin_upload(gpu, bozzard_render_assets::upload_source(source.clone()))?;
    cancelled.advance(gpu, &renderer, 256)?;
    drop(cancelled);
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64])?.rgba == old.rgba,
        "cancelled upload changed visible GPU data"
    );
    let mut pending = renderer.begin_upload(gpu, bozzard_render_assets::upload_source(source))?;
    let mut slices = 0;
    while !pending.progress().complete {
        let before = pending.progress().bytes_done;
        let progress = pending.advance(gpu, &renderer, 256)?;
        ensure!(
            progress.slice_bytes <= 256 && progress.bytes_done > before,
            "upload violated byte budget or stalled: {progress:?}"
        );
        slices += 1;
        if slices == 3 {
            ensure!(
                capture(gpu, &mut renderer, &scene, [64, 64])?.rgba == old.rgba,
                "partial upload published early"
            );
        }
        ensure!(slices < 1000, "upload failed to finish");
    }
    ensure!(
        slices > 64 && weak.upgrade().is_some(),
        "source lifetime or mip staging failed"
    );
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64])?.rgba == old.rgba,
        "complete staging replaced asset before finish"
    );
    pending.finish(&mut renderer, "staged")?;
    ensure!(
        weak.upgrade().is_none(),
        "finished upload retained CPU source unnecessarily"
    );
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [128; 3],
    )?;
    let stats = renderer.model_upload_stats("staged").unwrap();
    ensure!(
        stats.surfaces == 2 && stats.unique_images == 1,
        "staged texture sharing failed: {stats:?}"
    );
    let invalid = Arc::new(AssetData::Mesh(MeshData {
        vertices: vec![[0.; 8]],
        indices: vec![99; 3],
        parts: vec![],
        warnings: vec![],
    }));
    ensure!(
        renderer
            .begin_upload(gpu, bozzard_render_assets::upload_source(invalid))
            .is_err(),
        "invalid staged model accepted"
    );
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [128; 3],
    )?;
    println!(
        "upload_gpu_ok slices={slices} byte_budget source_lifetime cancellation atomic_replace mip_rows shared_images failed_prepare"
    );
    Ok(())
}
