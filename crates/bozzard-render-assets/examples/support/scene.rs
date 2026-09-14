use bozzard_render::*;
use glam::{Mat4, Vec3};

pub fn fixture() -> RenderScene {
    let mut items = Vec::new();
    for room in [-20., 20.] {
        for i in 0..48 {
            items.push(DrawItem {
                motion_id: items.len() as u64 + 1,
                mesh: MeshKind::Cube,
                model: Mat4::from_translation(Vec3::new(
                    room + (i % 8) as f32 * 1.5 - 5.,
                    0.,
                    -5. - (i / 8) as f32 * 1.5,
                )),
                material: Material {
                    metallic: Some(0.2),
                    roughness: Some(0.6),
                    tint: [0.7, 0.6, 0.4],
                    lit: true,
                    texture: TextureKind::White,
                    uv_scale: [1.; 2],
                    surface_overrides: Default::default(),
                    shader: None,
                },
            });
        }
    }
    let mut lights = Vec::new();
    for room in [-20., 20.] {
        for spot in [false, true] {
            lights.push(LocalLight {
                directional: false,
                position: [room, 5., -9.],
                direction: [0., -1., 0.],
                color: [1., 0.8, 0.6],
                intensity: 8.,
                range: 14.,
                spot_angles: spot.then_some([30., 65.]),
                shadows: Some(Default::default()),
            });
        }
    }
    RenderScene {
        shader_time: 0.,
        particles: Vec::new(),
        items,
        lights,
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -35., 35., -10., 10., 0.1, 80.,
        ) * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0., 15., 20.),
            Vec3::new(0., 0., -9.),
            Vec3::Y,
        ),
        lighting: Lighting {
            shadows: true,
            shadow_resolution: 512,
            ..Default::default()
        },
        environment: EnvironmentSettings::disabled(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings::default(),
    }
}
