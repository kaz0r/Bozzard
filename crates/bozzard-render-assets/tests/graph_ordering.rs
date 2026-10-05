//! Parameterized graph pipelines preserve the literal compiler's opaque winners.
use anyhow::Result;
use bozzard_assets::AssetStore;
use bozzard_render::{
    Backend, Frame, Gpu, RenderScene, SceneRenderer, capture_offscreen, instance, wgpu,
};
use bozzard_render_assets::{RenderSceneCache, render_scene, set_graph_parameterization_enabled};
use bozzard_scene::{
    Drawable, Layer, Mesh, RenderView, Texture,
    shader_graph::{Node, NodeKind, ShaderGraph, Socket, Value, Wire},
};
use glam::{Mat4, Vec3};
use std::{collections::BTreeMap, sync::Arc};

type GraphRow = (u64, Arc<ShaderGraph>);

struct RestoreParameterMode;
impl Drop for RestoreParameterMode {
    fn drop(&mut self) {
        set_graph_parameterization_enabled(true);
    }
}

fn graph(color: [f32; 3]) -> Result<Arc<ShaderGraph>> {
    let mut graph = ShaderGraph::default();
    let mut color_node = Node::new(2, NodeKind::Color, [0., 0.]);
    color_node.inputs[0] = Value::Vector(color);
    graph.nodes.push(color_node);
    graph.connect(Wire {
        from: Socket { node: 2, port: 0 },
        to: Socket { node: 1, port: 3 },
    })?;
    Ok(Arc::new(graph))
}

fn view<D>(rows: &[GraphRow], wrap: impl Fn(Drawable) -> D) -> RenderView<D> {
    RenderView {
        sprites: Vec::new(),
        skin_poses: BTreeMap::new(),
        object_ids: rows.iter().map(|(id, _)| *id).collect(),
        compute_textures: BTreeMap::new(),
        shader_graphs: rows.iter().map(|(_, graph)| Some(graph.clone())).collect(),
        material_instances: vec![None; rows.len()],
        particles: Vec::new(),
        display_time: 0.,
        texts: Vec::new(),
        shared_texts: Vec::new(),
        fog: Default::default(),
        lights: Vec::new(),
        environment: bozzard_scene::EnvironmentSettings {
            intensity: 0.,
            star_intensity: 0.,
            ..Default::default()
        },
        display: bozzard_scene::DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
        lighting: bozzard_scene::Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.),
        objects: rows
            .iter()
            .map(|_| {
                (
                    Mat4::from_translation(Vec3::new(0., 0., -1.)),
                    wrap(Drawable {
                        metallic: None,
                        roughness: None,
                        gi_static: true,
                        material_overrides: Vec::new(),
                        layer: Layer::ThreeD,
                        mesh: Mesh::Quad,
                        texture: Texture::White,
                        color: [1.; 3],
                        uv_scale: [1.; 2],
                    }),
                )
            })
            .collect(),
    }
}

fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> Result<Frame> {
    capture_offscreen(gpu, 64, 64, |target| {
        renderer.draw_linear(gpu, target, [64; 2], scene)
    })
}

#[test]
fn numeric_graphs_preserve_literal_coplanar_winners_through_edits_and_membership() -> Result<()> {
    let _restore = RestoreParameterMode;
    let assets = AssetStore::new(std::path::Path::new("."), &BTreeMap::new())?;
    // Binary fractions avoid conflating opaque order with literal precision.
    let mut graphs = [
        [0.75, 0.125, 0.25],
        [0.125, 0.75, 0.25],
        [0.25, 0.125, 0.75],
        [0.5, 0.25, 0.125],
    ]
    .map(graph)
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    set_graph_parameterization_enabled(false);
    let pool: Vec<_> = graphs
        .iter()
        .enumerate()
        .map(|(index, graph)| (index as u64 + 1, graph.clone()))
        .collect();
    let literal_pool = render_scene(
        view(&pool, |drawable| drawable),
        &assets,
        Layer::ThreeD,
        None,
    )?;
    let mut ranks: Vec<_> = literal_pool
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.material.shader.as_ref().unwrap().id, index))
        .collect();
    ranks.sort_unstable();
    assert!(ranks.windows(2).all(|pair| pair[0].0 != pair[1].0));
    graphs = ranks
        .iter()
        .map(|(_, index)| graphs[*index].clone())
        .collect();

    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    reference.set_instancing_enabled(false);
    reference.set_state_caching_enabled(false);
    reference.set_occlusion_enabled(false);
    let mut single_winner = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    single_winner.set_instancing_enabled(false);
    single_winner.set_state_caching_enabled(false);
    single_winner.set_occlusion_enabled(false);
    let mut candidates = std::array::from_fn::<_, 2, _>(|index| {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_instancing_enabled(index == 1);
        renderer.set_global_batching_enabled(true);
        renderer.set_occlusion_enabled(false);
        renderer
    });
    let cache = RenderSceneCache::default();
    // Descending literal hashes make source order disagree with the established
    // opaque sort. Collapsing pipeline IDs must not change the coplanar winner.
    let initial = vec![(10, graphs[3].clone()), (20, graphs[0].clone())];
    let edited = vec![(10, graphs[1].clone()), (20, graphs[3].clone())];
    let inserted = vec![
        (30, graphs[0].clone()),
        (10, graphs[1].clone()),
        (20, graphs[3].clone()),
    ];
    let reordered = vec![(20, graphs[3].clone()), (10, graphs[1].clone())];
    let frames = [
        initial.clone(),
        initial,
        edited.clone(),
        inserted,
        edited,
        reordered,
    ];
    let mut previous_reference = None;
    let mut comparisons = 0;
    for (step, rows) in frames.iter().enumerate() {
        set_graph_parameterization_enabled(false);
        let literal = render_scene(
            view(rows, |drawable| drawable),
            &assets,
            Layer::ThreeD,
            None,
        )?;
        let expected = capture(&gpu, &mut reference, &literal)?;
        let first = literal
            .items
            .iter()
            .min_by_key(|item| item.material.shader.as_ref().unwrap().id)
            .unwrap();
        let mut winner_scene = literal.clone();
        winner_scene.items = vec![first.clone()];
        assert_eq!(
            expected.rgba,
            capture(&gpu, &mut single_winner, &winner_scene)?.rgba,
            "literal coplanar winner on step {step}"
        );
        assert_ne!(
            &expected.rgba[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 3],
            &expected.rgba[..3],
            "graph fixture must draw visible emissive color"
        );
        if step == 1 {
            assert_eq!(previous_reference.as_ref().unwrap(), &expected.rgba);
        } else if step > 1 && step < 5 {
            assert_ne!(previous_reference.as_ref().unwrap(), &expected.rgba);
        }

        set_graph_parameterization_enabled(true);
        let numeric = cache.extract(view(rows, Arc::new), &assets, Layer::ThreeD, None)?;
        let topology = numeric.items[0].material.shader.as_ref().unwrap().id;
        for (literal, numeric) in literal.items.iter().zip(&numeric.items) {
            let literal = literal.material.shader.as_ref().unwrap();
            let numeric = numeric.material.shader.as_ref().unwrap();
            assert!(literal.numeric_parameters.is_empty());
            assert_eq!(numeric.numeric_parameters.len(), 1);
            assert_eq!(numeric.id, topology);
            assert_eq!(numeric.opaque_sort_id, literal.id);
        }
        for (path, candidate) in candidates.iter_mut().enumerate() {
            assert_eq!(
                expected.rgba,
                capture(&gpu, candidate, &numeric)?.rgba,
                "numeric coplanar order changed at step {step}, instanced={path}"
            );
            comparisons += 1;
        }
        assert!(candidates[1].frame_stats().color_draws < reference.frame_stats().color_draws);
        previous_reference = Some(expected.rgba);
    }
    println!(
        "numeric_graph_order_proof comparisons={comparisons} literal_opaque_winners=true shared_topology=true numeric_edits_insert_remove_reorder=true ordinary_and_instanced_exact=true draws={}->{}",
        reference.frame_stats().color_draws,
        candidates[1].frame_stats().color_draws
    );
    Ok(())
}
