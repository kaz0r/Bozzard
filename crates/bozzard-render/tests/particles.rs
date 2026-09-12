use bozzard_render::*;
use glam::{Mat4, Vec3};
fn scene() -> RenderScene {
    RenderScene {
        particles: vec![],
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 20.),
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
fn particle() -> Particle {
    Particle {
        id: 1,
        position: Vec3::new(0., 0., -5.),
        velocity: Vec3::new(0., 2., 0.),
        size: 1.,
        rotation: 0.,
        color: [0.6; 3],
        opacity: 0.8,
        kind: ParticleKind::Smoke,
        softness: 0.5,
        trail_length: 0.,
        seed: 0.3,
    }
}
fn capture(
    gpu: &Gpu,
    r: &mut SceneRenderer,
    s: &RenderScene,
    size: [u32; 2],
    raw: bool,
) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, size[0], size[1], |target| {
        if raw {
            r.draw_linear(gpu, target, size, s)
        } else {
            r.draw(gpu, target, size, s)
        }
    })
}
fn delta(a: &Frame, b: &Frame) -> usize {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 2))
        .count()
}
#[test]
fn lit_particles_soft_depth_trails_ordering_and_resize_budget() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let size = [160, 160];
    let mut scene = scene();
    let empty = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles.push(particle());
    let smoke = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(delta(&empty, &smoke) > 200);
    assert_eq!(renderer.frame_stats().particles, 1);
    assert_eq!(renderer.frame_stats().particle_triangles, 2);
    scene.lighting.ambient_color = [1., 0.05, 0.05];
    let red = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        delta(&smoke, &red) > 200,
        "smoke responds to scene illumination"
    );
    scene.items = vec![DrawItem {
        motion_id: 1,
        mesh: MeshKind::Quad,
        model: Mat4::from_translation(Vec3::new(0., 0., -4.9)) * Mat4::from_scale(Vec3::splat(8.)),
        material: Material {
            metallic: None,
            roughness: None,
            tint: [0.1; 3],
            lit: false,
            texture: TextureKind::White,
            uv_scale: [1.; 2],
            surface_overrides: Default::default(),
        },
    }];
    let hidden = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles.clear();
    let occluder = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert_eq!(
        hidden.rgba, occluder.rgba,
        "particles behind geometry are invisible"
    );
    let mut p = particle();
    p.position.z = -4.91;
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0., 0., -5.)) * Mat4::from_scale(Vec3::splat(8.));
    scene.particles = vec![p];
    let soft = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles[0].softness = 0.001;
    let hard = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        delta(&soft, &hard) > 200,
        "intersection fades before the depth boundary"
    );
    scene.items.clear();
    scene.particles[0] = particle();
    scene.particles[0].kind = ParticleKind::Sparks;
    scene.particles[0].size = 0.08;
    let dot = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles[0].trail_length = 0.4;
    let trail = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        delta(&dot, &trail) > 25,
        "spark velocity produces a luminous trail"
    );
    let raw = capture(&gpu, &mut renderer, &scene, size, true)?;
    scene.particles.clear();
    assert_eq!(
        raw.rgba,
        capture(&gpu, &mut renderer, &scene, size, true)?.rgba,
        "raw diagnostics bypass particles"
    );
    capture(&gpu, &mut renderer, &scene, [33, 25], false)?;
    capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles = vec![particle()];
    let resized = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert_eq!(
        resized.rgba, red.rgba,
        "empty resize roundtrip does not leave stale depth bindings"
    );
    let mut far = particle();
    far.id = 2;
    far.position.z = -6.;
    far.color = [0.1, 0.3, 1.];
    scene.particles.push(far);
    let sorted = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.particles.reverse();
    assert_eq!(
        sorted.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        "input order does not change transparent sorting"
    );
    scene.particles = vec![particle(); 16_385];
    assert!(
        capture(&gpu, &mut renderer, &scene, size, false).is_err(),
        "GPU particle uploads enforce the global budget"
    );
    Ok(())
}
