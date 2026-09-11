use super::*;
use bozzard_render::{EnvironmentSettings, Lighting, LocalLight, LocalShadowSettings};
use std::path::Path;

fn point(position: Vec3) -> LocalLight {
    LocalLight {
        directional: false,
        position: position.to_array(),
        direction: [0., 0., -1.],
        color: [1.; 3],
        intensity: 60.,
        range: 20.,
        spot_angles: None,
        shadows: Some(LocalShadowSettings::default()),
    }
}
fn quad(model: Mat4) -> DrawItem {
    DrawItem {
        model,
        mesh: MeshKind::Quad,
        material: Material {
            surface_overrides: Default::default(),
            tint: [1.; 3],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
            lit: true,
        },
    }
}
pub(super) fn checks(gpu: &Gpu, output: &Path) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    // Move a receiver/caster rig around a translated light through all 6 axes,
    // 12 face boundaries and 8 corners. The camera sees the shadow but not the
    // caster: black pixels cannot be mistaken for visible blocker geometry.
    let origin = Vec3::new(0.3, -0.2, 0.7);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let Some(forward) = Vec3::new(x as f32, y as f32, z as f32).try_normalize() else {
                    continue;
                };
                let up = if forward.y.abs() > 0.99 {
                    Vec3::Z
                } else {
                    Vec3::Y
                };
                let camera =
                    glam::camera::rh::view::look_to_mat4(origin + forward * 3., forward, up);
                let orientation =
                    glam::camera::rh::view::look_to_mat4(Vec3::ZERO, forward, up).inverse();
                let receiver = origin + forward * 4.;
                // Large enough to cover seam pixels, small enough to leave a lit edge.
                let mut scene = RenderScene {
                    fog: Default::default(),
                    view_projection: glam::camera::rh::proj::directx::orthographic(
                        -0.8, 0.8, -0.8, 0.8, 0.1, 10.,
                    ) * camera,
                    lighting: Lighting {
                        shadows: false,
                        sun_intensity: 0.,
                        ambient_intensity: 0.,
                        ..Default::default()
                    },
                    environment: EnvironmentSettings::disabled(),
                    display: Default::default(),
                    gi: None,
                    lights: vec![point(origin)],
                    items: vec![
                        quad(
                            Mat4::from_translation(receiver)
                                * orientation
                                * Mat4::from_scale(Vec3::splat(2.)),
                        ),
                        quad(
                            Mat4::from_translation(
                                origin
                                    + forward * 2.
                                    + orientation.transform_vector3(Vec3::X * 0.1),
                            ) * orientation
                                * Mat4::from_scale(Vec3::splat(0.35)),
                        ),
                    ],
                };
                let shadow = capture(gpu, &mut renderer, &scene, [65, 65])?;
                for py in 29..=35 {
                    for px in 29..=35 {
                        pixel(&shadow, px, py, [0; 3])?;
                    }
                }
                pixel(&shadow, 49, 32, [0; 3])?;
                ensure!(
                    shadow.rgba[(32 * 65 + 16) * 4] > 180,
                    "point face ({x},{y},{z}) flipped its off-axis shadow"
                );
                ensure!(
                    renderer.frame_stats().culled_surfaces == 1,
                    "point caster must be off camera"
                );
                scene.lights[0].shadows = None;
                let clear = capture(gpu, &mut renderer, &scene, [65, 65])?;
                ensure!(
                    clear.rgba[(32 * 65 + 32) * 4] > 200,
                    "point face ({x},{y},{z}) has no direct light"
                );
                // Check a pixel well outside the occluder's projected silhouette stays lit.
                ensure!(
                    shadow.rgba[(32 * 65 + 60) * 4] > 180,
                    "point face ({x},{y},{z}) shadow escaped silhouette"
                );
                scene.lights[0].shadows = Some(Default::default());
                renderer.set_culling_enabled(false);
                ensure!(
                    capture(gpu, &mut renderer, &scene, [65, 65])?.rgba == shadow.rgba,
                    "point face ({x},{y},{z}) culling differs from reference"
                );
                renderer.set_culling_enabled(true);
                // Point orientation is irrelevant, including at face boundaries.
                scene.lights[0].direction = [1., -2., 3.];
                ensure!(
                    capture(gpu, &mut renderer, &scene, [65, 65])?.rgba == shadow.rgba,
                    "point rotation moved the shadow"
                );
                if (x, y, z) == (1, 1, 1) {
                    shadow.write_ppm(&output.join("point-shadow-corner.ppm"))?;
                }
            }
        }
    }
    mixed_lights(gpu, &mut renderer)?;
    println!(
        "point_faces_gpu_ok six_axes twelve_edges eight_corners pcf_overlap translated_light offcamera rotation reference_culling mixed_spots directional_isolation"
    );
    Ok(())
}
fn mixed_lights(gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<()> {
    let mut red = point(Vec3::new(3., 0., 3.));
    red.color = [1., 0., 0.];
    red.intensity = 40.;
    let mut blue = point(Vec3::new(-3., 0., 3.));
    blue.color = [0., 0., 1.];
    blue.intensity = 40.;
    blue.spot_angles = Some([25., 35.]);
    blue.direction = [1., 0., -1.];
    let directional = LocalLight {
        directional: true,
        position: [100., 200., 300.],
        range: 0.001,
        direction: [0., 0., -1.],
        color: [0., 1., 0.],
        intensity: 0.25,
        spot_angles: None,
        shadows: None,
    };
    let mut scene = RenderScene {
        fog: Default::default(),
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.,
            ..Default::default()
        },
        environment: EnvironmentSettings::disabled(),
        display: Default::default(),
        gi: None,
        lights: vec![red, directional, blue],
        items: vec![
            quad(Mat4::from_scale(Vec3::new(4., 4., 1.))),
            quad(Mat4::from_translation(Vec3::Z) * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.))),
        ],
    };
    let mixed = capture(gpu, renderer, &scene, [128, 128])?;
    let left = (64 * 128 + 16) * 4;
    let right = (64 * 128 + 112) * 4;
    ensure!(
        mixed.rgba[left] < 3
            && mixed.rgba[left + 2] > 60
            && mixed.rgba[right] > 60
            && mixed.rgba[right + 2] < 3
            && mixed.rgba[left + 1] > 15
            && mixed.rgba[right + 1] > 15,
        "point/spot shadow layers leaked or shadowed directional illumination"
    );
    scene.lights.swap(0, 2);
    ensure!(
        capture(gpu, renderer, &scene, [128, 128])?.rgba == mixed.rgba,
        "mixed light order changed shadows"
    );
    scene.lights[1].position = [-300., -200., -100.];
    scene.lights[1].range = 100_000.;
    ensure!(
        capture(gpu, renderer, &scene, [128, 128])?.rgba == mixed.rgba,
        "directional illumination changed with position/range among shadowed lights"
    );
    // Grow both independent arrays to their final slots at once.
    scene.lights = vec![
        LocalLight {
            intensity: 0.01,
            ..red
        };
        bozzard_render::MAX_SHADOWED_POINT_LIGHTS
    ];
    scene.lights.last_mut().unwrap().intensity = 40.;
    scene.lights.extend(vec![
        LocalLight {
            intensity: 0.01,
            ..blue
        };
        bozzard_render::MAX_SHADOWED_SPOT_LIGHTS
    ]);
    scene.lights.last_mut().unwrap().intensity = 40.;
    scene.lights.insert(2, directional);
    let full = capture(gpu, renderer, &scene, [128, 128])?;
    ensure!(
        full.rgba[left] < 3 && full.rgba[right + 2] < 3,
        "mixed final shadow slots failed"
    );
    scene.lights.push(LocalLight {
        directional: false,
        intensity: 0.,
        ..red
    });
    ensure!(
        capture(gpu, renderer, &scene, [128, 128]).is_err(),
        "disabled excess point shadow accepted"
    );
    scene.lights.pop();
    ensure!(
        capture(gpu, renderer, &scene, [128, 128])?.rgba == full.rgba,
        "invalid mixed frame corrupted shadows"
    );
    Ok(())
}
