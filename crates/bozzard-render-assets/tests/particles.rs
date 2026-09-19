use anyhow::Result;
use bozzard_render::*;
use bozzard_scene::{Layer, Scene};
use glam::{Mat4, Vec3};
fn render(particles: Vec<Particle>) -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles,
        view_projection: glam::camera::rh::proj::directx::orthographic(-3., 3., -2., 4., 0.1, 20.),
        items: vec![],
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 1.,
            ..Default::default()
        },
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
    }
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> Result<Frame> {
    capture_offscreen(gpu, 192, 192, |target| {
        renderer.draw(gpu, target, [192, 192], scene)
    })
}
#[test]
fn gpu_motion_matches_headless_reference_and_transparent_surfaces_are_interleaved() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let source = Scene::from_json(
        r#"{"version":1,"name":"Particle reference","views":{"3d":"camera"},"objects":[{"id":"camera","name":"Camera","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"orthographic","vertical_size":6,"near":0.1,"far":20}},{"id":"effect","name":"Effect","transform":{"translation":[0,0,-5],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"particle_emitter":{"rate":48,"lifetime":4,"max_particles":64,"seed":123,"opacity":0.7}}]}"#,
    )?;
    let mut a = Default::default();
    let mut b = Default::default();
    let mut reference = source.spawn(&mut a)?;
    let mut native = source.spawn(&mut b)?;
    native.set_gpu_particles(true);
    let mut cpu_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut gpu_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for tick in 0..90 {
        for (instance, world) in [(&mut reference, &a), (&mut native, &b)] {
            instance.advance_display(1. / 60.)?;
            instance.step_particles(world, 1. / 60.)?;
        }
        let cpu = render(bozzard_render_assets::particle_frame(
            &reference.view(&a, Layer::ThreeD, 1.)?.particles,
        ));
        let rendered = render(bozzard_render_assets::particle_frame(
            &native.view(&b, Layer::ThreeD, 1.)?.particles,
        ));
        let actual = capture(&gpu, &mut gpu_renderer, &rendered)?;
        if tick == 30 || tick == 89 {
            let expected = capture(&gpu, &mut cpu_renderer, &cpu)?;
            let mean = actual
                .rgba
                .iter()
                .zip(&expected.rgba)
                .map(|(a, b)| a.abs_diff(*b) as usize)
                .sum::<usize>() as f32
                / actual.rgba.len() as f32;
            assert!(
                mean < 0.8,
                "GPU/headless image disagreement at tick {tick}: {mean}"
            );
            assert_eq!(
                actual.rgba,
                capture(&gpu, &mut gpu_renderer, &rendered)?.rgba,
                "rendering a paused particle frame must not advance motion"
            );
            assert_eq!(gpu_renderer.frame_stats().particle_compute_dispatches, 0);
            assert_eq!(gpu_renderer.frame_stats().particle_descriptor_bytes, 0);
        }
    }
    let mut smoke = render(vec![Particle {
        simulation: None,
        id: 1,
        position: Vec3::new(0., 0., -5.),
        velocity: Vec3::ZERO,
        size: 3.,
        rotation: 0.,
        color: [1., 0., 0.],
        opacity: 1.,
        kind: ParticleKind::Smoke,
        softness: 0.1,
        trail_length: 0.,
        seed: 0.3,
    }]);
    smoke.items.push(DrawItem {
        motion_id: 1,
        model: Mat4::from_translation(Vec3::new(0., 0., -4.)),
        mesh: MeshKind::Sprite(SpriteMesh {
            opacity: 0.65,
            ..SpriteMesh::new(vec![SpriteQuad {
                rect: [-3., 3., 6., 6.],
                uv: [0., 0., 1., 1.],
            }])?
        }),
        material: Material {
            tint: [0., 0., 1.],
            texture: TextureKind::White,
            lit: false,
            metallic: None,
            roughness: None,
            uv_scale: [1.; 2],
            surface_overrides: Default::default(),
            shader: None,
        },
    });
    let behind = capture(&gpu, &mut gpu_renderer, &smoke)?;
    smoke.items[0].model = Mat4::from_translation(Vec3::new(0., 0., -6.));
    let front = capture(&gpu, &mut gpu_renderer, &smoke)?;
    assert!(
        behind
            .rgba
            .iter()
            .zip(&front.rgba)
            .filter(|(a, b)| a.abs_diff(**b) > 10)
            .count()
            > 100,
        "glass in front of smoke must composite differently from glass behind it"
    );
    let template = smoke.particles[0];
    smoke.items.clear();
    smoke.particles = (0..16_384)
        .map(|i| Particle {
            id: i as u64 + 1,
            size: 0.025,
            opacity: 0.15,
            position: Vec3::new(
                (i % 128) as f32 * 0.035 - 2.2,
                (i / 128) as f32 * 0.035 - 1.2,
                -3. - (i % 13) as f32 * 0.1,
            ),
            ..template
        })
        .collect();
    let original = capture(&gpu, &mut gpu_renderer, &smoke)?;
    assert_eq!(gpu_renderer.frame_stats().particle_compute_dispatches, 108);
    assert_eq!(
        gpu_renderer.frame_stats().particle_descriptor_bytes,
        16_384 * 128
    );
    smoke.particles.reverse();
    assert_eq!(
        original.rgba,
        capture(&gpu, &mut gpu_renderer, &smoke)?.rgba,
        "sort order depends on input order at full capacity"
    );
    smoke.particles = vec![Particle {
        simulation: Some(ParticleSimulation {
            epoch: 500,
            age: 0.5,
            reference_age: 0.,
            time: 0.5,
            gravity: 0.,
            drag: 0.,
            turbulence: 0.,
            wind: [0.; 3],
            speed: 1.,
        }),
        position: Vec3::new(-1., 0., -5.),
        velocity: Vec3::X,
        size: 1.,
        ..template
    }];
    let initial = capture(&gpu, &mut gpu_renderer, &smoke)?;
    smoke.particles[0].simulation.as_mut().unwrap().epoch += 1;
    assert_eq!(
        initial.rgba,
        capture(&gpu, &mut gpu_renderer, &smoke)?.rgba,
        "scene epoch must reset reused particle IDs"
    );
    Ok(())
}
