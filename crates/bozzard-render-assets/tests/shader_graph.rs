//! Shader graph modules: codegen output compiles and overrides surface channels
//! through the same lighting pipeline as stock materials.
use bozzard_render::*;
use bozzard_scene::shader_graph::{Node, NodeKind, ShaderGraph, Value};
use glam::{Mat4, Vec3};

fn graph_surface(graph: ShaderGraph) -> std::sync::Arc<ShaderSource> {
    bozzard_render_assets::shader_source(&graph).unwrap()
}

fn item(shader: Option<std::sync::Arc<ShaderSource>>) -> DrawItem {
    DrawItem {
        motion_id: 0,
        mesh: MeshKind::Quad,
        model: Mat4::from_translation(Vec3::new(0., 0., -5.))
            * Mat4::from_scale(Vec3::new(4., 3., 1.)),
        material: Material {
            metallic: None,
            roughness: None,
            tint: [1., 1., 1.],
            lit: false,
            texture: TextureKind::White,
            uv_scale: [1.; 2],
            surface_overrides: Default::default(),
            shader,
        },
    }
}

fn scene(items: Vec<DrawItem>) -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: Vec::new(),
        view_projection: glam::camera::rh::proj::directx::orthographic(-4., 4., -3., 3., 0.1, 30.),
        items,
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.,
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

fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * frame.width + x) * 4) as usize;
    frame.rgba[i..i + 3].try_into().unwrap()
}

fn render(items: Vec<DrawItem>, size: [u32; 2]) -> anyhow::Result<Frame> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let scene = scene(items);
    capture_offscreen(&gpu, size[0], size[1], |target| {
        renderer.draw_linear(&gpu, target, size, &scene)
    })
}

#[test]
fn base_color_graph_replaces_material_and_tint() -> anyhow::Result<()> {
    let mut graph = ShaderGraph::default();
    let color = Node::new(2, NodeKind::Color, [0., 0.]);
    graph.nodes.push(color);
    graph
        .connect(bozzard_scene::shader_graph::Wire {
            from: bozzard_scene::shader_graph::Socket { node: 2, port: 0 },
            to: bozzard_scene::shader_graph::Socket { node: 1, port: 0 },
        })
        .unwrap();
    graph.nodes[1].inputs[0] = Value::Vector([0.9, 0.1, 0.1]);
    // The tint would turn the quad cyan without the graph override.
    let mut item = item(Some(graph_surface(graph)));
    item.material.tint = [0., 1., 1.];
    let frame = render(vec![item], [32, 24])?;
    let center = pixel(&frame, 16, 12);
    // Linear RGB through draw_linear: 0.9 -> 229, 0.1 -> 25.
    assert!(
        (226..=232).contains(&center[0])
            && (22..=28).contains(&center[1])
            && (22..=28).contains(&center[2]),
        "expected red quad, got {center:?}"
    );
    Ok(())
}

#[test]
fn alpha_graph_discards_pixels() -> anyhow::Result<()> {
    let mut graph = ShaderGraph::default();
    graph.nodes.push(Node::new(2, NodeKind::Float, [0., 0.]));
    graph
        .connect(bozzard_scene::shader_graph::Wire {
            from: bozzard_scene::shader_graph::Socket { node: 2, port: 0 },
            to: bozzard_scene::shader_graph::Socket { node: 1, port: 4 },
        })
        .unwrap();
    // Alpha 0 discards the whole quad; the clear color shows through.
    let frame = render(vec![item(Some(graph_surface(graph)))], [32, 24])?;
    let center = pixel(&frame, 16, 12);
    assert_eq!(
        center,
        [5, 6, 10],
        "expected clear background, got {center:?}"
    );
    Ok(())
}

#[test]
fn sphere_preview_renders_graph_geometry() {
    // Preview geometry: a base-color graph on the UV sphere, unlit for exact colors.
    let mut graph = ShaderGraph::default();
    graph.nodes.push(Node::new(2, NodeKind::Color, [0., 0.]));
    graph.nodes[1].inputs[0] = Value::Vector([0.9, 0.1, 0.1]);
    graph
        .connect(bozzard_scene::shader_graph::Wire {
            from: bozzard_scene::shader_graph::Socket { node: 2, port: 0 },
            to: bozzard_scene::shader_graph::Socket { node: 1, port: 0 },
        })
        .unwrap();
    let mut item = item(Some(graph_surface(graph)));
    item.mesh = bozzard_render::MeshKind::Sphere;
    let frame = render(vec![item], [48, 36]).expect("sphere render");
    let center = pixel(&frame, 24, 18);
    assert!(
        (226..=232).contains(&center[0])
            && (22..=28).contains(&center[1])
            && (22..=28).contains(&center[2]),
        "expected red sphere at center, got {center:?}"
    );
    // Corner stays background: the sphere does not fill the frame.
    let corner = pixel(&frame, 2, 2);
    assert_eq!(
        corner,
        [5, 6, 10],
        "expected background at corner, got {corner:?}"
    );
}

#[test]
fn cached_object_uniforms_keep_shader_time_live_but_ignore_unused_stock_time() -> anyhow::Result<()>
{
    use bozzard_scene::shader_graph::{Socket, Wire};
    let mut graph = ShaderGraph::default();
    graph.nodes.push(Node::new(2, NodeKind::Time, [0.; 2]));
    graph.nodes.push(Node::new(3, NodeKind::Append, [0.; 2]));
    graph.connect(Wire {
        from: Socket { node: 2, port: 0 },
        to: Socket { node: 3, port: 0 },
    })?;
    graph.connect(Wire {
        from: Socket { node: 3, port: 0 },
        to: Socket { node: 1, port: 0 },
    })?;
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene(vec![item(Some(graph_surface(graph)))]);
    let draw = |renderer: &mut SceneRenderer, scene: &RenderScene| {
        capture_offscreen(&gpu, 32, 24, |target| {
            renderer.draw_linear(&gpu, target, [32, 24], scene)
        })
    };
    scene.shader_time = 0.2;
    let first = draw(&mut renderer, &scene)?;
    scene.shader_time = 0.8;
    let advanced = draw(&mut renderer, &scene)?;
    assert!(pixel(&advanced, 16, 12)[0] > pixel(&first, 16, 12)[0] + 100);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 1);
    assert_eq!(draw(&mut renderer, &scene)?.rgba, advanced.rgba);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 0);
    renderer.set_state_caching_enabled(false);
    assert_eq!(draw(&mut renderer, &scene)?.rgba, advanced.rgba);
    renderer.set_state_caching_enabled(true);
    scene.items[0].material.shader = None;
    let stock = draw(&mut renderer, &scene)?;
    scene.shader_time = 1.5;
    assert_eq!(draw(&mut renderer, &scene)?.rgba, stock.rgba);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 0);
    Ok(())
}

#[test]
fn recently_used_graph_pipelines_survive_switches_with_bounded_retention() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let graphs: Vec<_> = (0..12)
        .map(|i| {
            let mut graph = ShaderGraph::default();
            graph.nodes.push(Node::new(2, NodeKind::Color, [0.; 2]));
            graph.nodes[1].inputs[0] = Value::Vector([0.1 + i as f32 * 0.05, 0.2, 0.3]);
            graph
                .connect(bozzard_scene::shader_graph::Wire {
                    from: bozzard_scene::shader_graph::Socket { node: 2, port: 0 },
                    to: bozzard_scene::shader_graph::Socket { node: 1, port: 0 },
                })
                .unwrap();
            graph_surface(graph)
        })
        .collect();
    let mut pixels = Vec::new();
    for i in (0..12).chain([11, 10, 9, 4, 0]) {
        let frame = capture_offscreen(&gpu, 32, 24, |target| {
            renderer.draw_linear(
                &gpu,
                target,
                [32, 24],
                &scene(vec![item(Some(graphs[i].clone()))]),
            )
        })?;
        let stats = renderer.frame_stats();
        assert!(
            stats.resident_graphs <= 9,
            "idle graph cache grew without a bound"
        );
        if pixels.len() < 12 {
            assert_eq!(stats.graph_compilations, 1);
            pixels.push(frame.rgba);
        } else {
            assert_eq!(
                stats.graph_compilations,
                usize::from(i == 0),
                "unexpected eviction at {i}"
            );
            assert_eq!(frame.rgba, pixels[i], "switching graph changed pixels");
        }
    }
    // Every simultaneously active graph must survive, even above the idle limit.
    let all = scene(
        graphs
            .iter()
            .cloned()
            .map(|graph| item(Some(graph)))
            .collect(),
    );
    for pass in 0..2 {
        capture_offscreen(&gpu, 32, 24, |target| {
            renderer.draw_linear(&gpu, target, [32, 24], &all)
        })?;
        assert_eq!(renderer.frame_stats().resident_graphs, 12);
        if pass == 1 {
            assert_eq!(renderer.frame_stats().graph_compilations, 0);
        }
    }
    Ok(())
}
