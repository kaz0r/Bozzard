//! Rebuild the editable animation showcase from the original Blender GLBs.
use anyhow::{Context, Result};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_scene::{
    middleware::{
        animation::{data::Rig, retarget::RetargetMap, *},
        curve::Repeat,
        registry,
        ui::{Anchors, Canvas, Layout, Widget, WidgetKind},
    },
    *,
};
use std::{collections::BTreeMap, path::Path, sync::Arc};

fn object(id: &str, position: [f32; 3]) -> Object {
    Object {
        id: id.into(),
        name: id.replace('-', " "),
        transform: Transform {
            translation: position,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn drawable(mesh: Mesh, color: [f32; 3], gi_static: bool) -> Drawable {
    Drawable {
        mesh,
        color,
        gi_static,
        layer: Layer::ThreeD,
        texture: Texture::White,
        metallic: None,
        roughness: None,
        material_overrides: vec![],
        uv_scale: [1.; 2],
    }
}
fn cube(id: &str, position: [f32; 3], size: [f32; 3], color: [f32; 3]) -> Object {
    let mut o = object(id, position);
    o.transform.scale = size;
    o.drawable = Some(drawable(Mesh::Cube, color, true));
    o.collider = Some(BoxCollider {
        size: [1.; 3],
        ..Default::default()
    });
    o
}
fn index(rig: &Rig, name: &str) -> usize {
    rig.clips.iter().position(|c| c.name == name).unwrap()
}
fn bone(rig: &Rig, name: &str) -> usize {
    rig.nodes.iter().position(|b| b.name == name).unwrap()
}
fn feet(rig: &Rig) -> Vec<IkConstraint> {
    let mut globals = Vec::new();
    rig.globals_into(&rig.rest_pose(), &mut globals).unwrap();
    ["Left", "Right"]
        .map(|side| IkConstraint {
            name: format!("{side} foot"),
            root: bone(rig, &format!("{side}UpperLeg")),
            middle: bone(rig, &format!("{side}LowerLeg")),
            tip: bone(rig, &format!("{side}Foot")),
            target: IkTarget::Ground {
                sole_height: globals[bone(rig, &format!("{side}Foot"))].w_axis.y,
                ray_up: 0.65,
                ray_down: 0.8,
                layers: u32::MAX,
                release_height: 0.16,
                plant: true,
                align_normal: true,
            },
            ..Default::default()
        })
        .to_vec()
}
fn states(rig: &Rig) -> Vec<StateDefinition> {
    vec![
        StateDefinition {
            name: "Movement".into(),
            repeat: Repeat::Loop,
            motion: Motion::Blend2d {
                parameters: ["Strafe".into(), "Forward".into()],
                samples: [
                    ("Idle", [0., 0.]),
                    ("WalkForward", [0., 1.]),
                    ("RunForward", [0., 2.]),
                    ("WalkLeft", [-1., 0.]),
                    ("WalkRight", [1., 0.]),
                    ("WalkBack", [0., -1.]),
                ]
                .map(|(name, position)| BlendPoint {
                    position,
                    clip: index(rig, name),
                })
                .to_vec(),
            },
        },
        StateDefinition {
            name: "Jump".into(),
            repeat: Repeat::Once,
            motion: Motion::Clip {
                clip: index(rig, "Jump"),
            },
        },
        StateDefinition {
            name: "Reach".into(),
            repeat: Repeat::Once,
            motion: Motion::Clip {
                clip: index(rig, "Reach"),
            },
        },
    ]
}
fn controller(asset: &str, rig: Arc<Rig>) -> Animator {
    let mut a = Animator::from_rig(asset.into(), rig);
    a.initial = "Movement".into();
    a.states = Arc::new(states(&a.rig));
    a.parameters = BTreeMap::from([
        ("Strafe".into(), 0.),
        ("Forward".into(), 1.),
        ("Wave".into(), 0.),
        ("Aim".into(), 0.),
        ("Return".into(), 1.),
    ]);
    a.transitions = Arc::new(vec![Transition {
        from: "Jump".into(),
        to: "Movement".into(),
        parameter: "Return".into(),
        comparison: Comparison::Above,
        threshold: 0.5,
        fade: 0.12,
        exit_time: Some(1.),
    }]);
    a.layers = Arc::new(action_layers(&a.rig));
    a.ik = Arc::new(feet(&a.rig));
    a.foot_placement = Some(FootPlacement {
        pelvis: bone(&a.rig, "Hips"),
        ..Default::default()
    });
    a.root_motion = Some(RootMotion {
        node: bone(&a.rig, "Root"),
        translation: [true, false, true],
        yaw: true,
    });
    a.warps = Arc::new(vec![MotionWarp {
        name: "Interact".into(),
        state: "Reach".into(),
        start: 0.1,
        end: 0.85,
        translation: [true, false, true],
        yaw: true,
        target: WarpTarget::Point {
            position: [0.; 3],
            yaw_degrees: 180.,
        },
    }]);
    a
}
fn action_layers(rig: &Rig) -> Vec<AnimationLayer> {
    [
        ("Wave", LayerBlend::Additive),
        ("Aim", LayerBlend::Override),
    ]
    .map(|(name, blend)| AnimationLayer {
        name: name.into(),
        motion: Motion::Clip {
            clip: index(rig, name),
        },
        blend,
        mask: BoneMask {
            root: Some(bone(rig, "Spine")),
            ..Default::default()
        },
        weight_parameter: Some(name.into()),
        fade: 0.18,
        reference_clip: (blend == LayerBlend::Additive).then(|| index(rig, "Idle")),
        ..Default::default()
    })
    .to_vec()
}
fn actor(scene: &mut Scene, id: &str, position: [f32; 3], animator: Animator) -> Result<()> {
    let mut o = object(id, position);
    o.transform.rotation_degrees[1] = 180.;
    use bozzard_scene::blueprint::{BlackboardValue as B, Value as V};
    o.blackboard = BTreeMap::from([
        ("manual".into(), B::Scalar(V::Bool(false))),
        ("wave".into(), B::Scalar(V::Bool(id == "movement-human"))),
        ("aim".into(), B::Scalar(V::Bool(id == "retargeted-human"))),
        ("strafe".into(), B::Scalar(V::Number(0.))),
        ("forward".into(), B::Scalar(V::Number(1.))),
        ("cycle".into(), B::Scalar(V::Number(0.))),
    ]);
    o.drawable = Some(drawable(
        Mesh::Asset(animator.asset.clone()),
        [1.; 3],
        false,
    ));
    o.script_manager = Some(ScriptManager {
        scripts: vec![ScriptAttachment {
            enabled: true,
            script: "showcase".into(),
        }],
    });
    registry::set(&mut o, &animator)?;
    scene.objects.push(o);
    Ok(())
}
fn rig(store: &AssetStore, id: &str) -> Result<Arc<Rig>> {
    let AssetData::Mesh(mesh) = store
        .get(store.handle(id).context("model not loaded")?)
        .unwrap()
        .data()
        .unwrap()
    else {
        anyhow::bail!("expected human mesh")
    };
    let mut rig = mesh
        .skin
        .as_ref()
        .context("human has no rig")?
        .rig
        .as_ref()
        .clone();
    for clip in &mut rig.clips {
        clip.compact(&rig.nodes)?;
    }
    Ok(Arc::new(rig))
}
fn stage(scene: &mut Scene) {
    scene.objects.push(cube(
        "stage",
        [0., -0.15, 0.],
        [12., 0.3, 7.],
        [0.08, 0.10, 0.135],
    ));
    for (index, x) in [-3.6, -1.2, 1.2, 3.6].into_iter().enumerate() {
        let color = [
            [0.03, 0.30, 0.40],
            [0.45, 0.16, 0.055],
            [0.19, 0.31, 0.12],
            [0.36, 0.16, 0.37],
        ][index];
        scene.objects.push(cube(
            &format!("station-{index}"),
            [x, -0.003, 0.],
            [2.25, 0.012, 3.6],
            color,
        ));
    }
    // Independently moving supports stay under the feet throughout the captured loop.
    let mut step = cube(
        "foot-step",
        [1.36, 0.22, 0.],
        [0.24, 0.16, 0.48],
        [0.26, 0.38, 0.19],
    );
    step.drawable.as_mut().unwrap().gi_static = false;
    scene.objects.push(step);
    let mut slope = cube(
        "foot-slope",
        [1.03, 0.22, 0.],
        [0.25, 0.10, 0.50],
        [0.24, 0.35, 0.17],
    );
    slope.transform.rotation_degrees[0] = 12.;
    slope.drawable.as_mut().unwrap().gi_static = false;
    scene.objects.push(slope);
    let mut platform = object("platform-driver", [0.; 3]);
    platform.script_manager = Some(ScriptManager {
        scripts: vec![ScriptAttachment {
            enabled: true,
            script: "platform".into(),
        }],
    });
    scene.objects.push(platform);
    scene.objects.push(cube(
        "interaction-block",
        [3.6, 0.60, 1.45],
        [0.46, 1.2, 0.35],
        [0.32, 0.21, 0.39],
    ));
    scene.objects.push(object("reach-target", [3.6, 0., 0.95]));
}
fn hud(scene: &mut Scene) -> Result<()> {
    let mut canvas = object("demo-ui", [0.; 3]);
    registry::set(
        &mut canvas,
        &Canvas {
            layer: Layer::ThreeD,
            ..Default::default()
        },
    )?;
    scene.objects.push(canvas);
    let mut panel = object("demo-help", [0.; 3]);
    panel.parent = Some("demo-ui".into());
    registry::set(&mut panel, &Widget { kind: WidgetKind::Label,
        text: "CHARACTER ANIMATION\nWASD move · Shift run · Space jump · Q wave · E aim · R reach\nThe other characters demonstrate reused motion, grounded feet and interaction alignment.".into(),
        anchors: Anchors { min: [0., 0.], max: [1., 0.], pivot: [0., 0.], offset: [22., 18.], size: [-44., 84.] },
        font_size: 17., background: [0.015, 0.025, 0.04, 0.88], padding: [12.; 4], ..Default::default() })?;
    scene.objects.push(panel);
    let mut row = object("station-labels", [0.; 3]);
    row.parent = Some("demo-ui".into());
    registry::set(
        &mut row,
        &Widget {
            layout: Layout::Row,
            anchors: Anchors {
                min: [0., 1.],
                max: [1., 1.],
                pivot: [0., 1.],
                offset: [20., -20.],
                size: [-40., 42.],
            },
            gap: 12.,
            background: [0.; 4],
            padding: [0.; 4],
            ..Default::default()
        },
    )?;
    scene.objects.push(row);
    for (order, text) in [
        "01  MOVE + GESTURE",
        "02  RETARGETED HUMAN",
        "03  FOOT PLACEMENT",
        "04  REACH TARGET",
    ]
    .into_iter()
    .enumerate()
    {
        let mut label = object(&format!("label-{order}"), [0.; 3]);
        label.parent = Some("station-labels".into());
        registry::set(
            &mut label,
            &Widget {
                kind: WidgetKind::Label,
                text: text.into(),
                order: order as i32,
                grow: 1.,
                font_size: 13.,
                anchors: Anchors {
                    size: [175., 38.],
                    ..Default::default()
                },
                padding: [8.; 4],
                background: [0.015, 0.025, 0.04, 0.8],
                ..Default::default()
            },
        )?;
        scene.objects.push(label);
    }
    Ok(())
}
fn scene_assets(scene: &mut Scene) {
    for (id, kind, path) in [
        (
            "human",
            AssetKind::Mesh,
            "../../../assets/animation-human/human.glb",
        ),
        (
            "human-tall",
            AssetKind::Mesh,
            "../../../assets/animation-human/human-tall.glb",
        ),
        ("showcase", AssetKind::Script, "scripts/animation-lab.rhai"),
        (
            "platform",
            AssetKind::Script,
            "scripts/animation-platform.rhai",
        ),
    ] {
        scene.assets.insert(
            id.into(),
            AssetSource {
                kind,
                path: path.into(),
            },
        );
    }
}
fn models(scene: &Scene, root: &Path) -> Result<(Arc<Rig>, Arc<Rig>)> {
    let mut store = AssetStore::new(root, &scene.assets)?;
    store.load_pending()?;
    let source = rig(&store, "human")?;
    let target = rig(&store, "human-tall")?;
    let mut reused = (*target).clone();
    let mut map = RetargetMap::by_name(&source, &target);
    map.translation_scale = 1.16;
    for (clip, definition) in source.clips.iter().enumerate() {
        reused
            .clips
            .push(map.bake_clip(&source, &target, clip, definition.name.clone(), 30.)?);
    }
    Ok((source, Arc::new(reused)))
}
fn characters(scene: &mut Scene, source: Arc<Rig>, reused: Arc<Rig>) -> Result<()> {
    let mut first = controller("human", source.clone());
    first.parameters.insert("Wave".into(), 1.);
    actor(scene, "movement-human", [-3.6, 0., 0.], first)?;
    let mut second = controller("human-tall", reused);
    second.parameters.insert("Forward".into(), 2.);
    second.parameters.insert("Aim".into(), 1.);
    actor(scene, "retargeted-human", [-1.2, 0., 0.], second)?;
    let mut grounded = controller("human", source.clone());
    grounded.parameters.insert("Forward".into(), 0.);
    actor(scene, "grounded-human", [1.2, 0.09, 0.], grounded)?;
    actor(
        scene,
        "interaction-human",
        [3.6, 0., -0.9],
        reaching_controller(source),
    )?;
    Ok(())
}
fn reaching_controller(source: Arc<Rig>) -> Animator {
    let mut reaching = controller("human", source);
    reaching.initial = "Reach".into();
    reaching.parameters.insert("Reach contact".into(), 0.);
    Arc::make_mut(&mut reaching.ik).push(IkConstraint {
        name: "Hand contact".into(),
        root: bone(&reaching.rig, "RightUpperArm"),
        middle: bone(&reaching.rig, "RightLowerArm"),
        tip: bone(&reaching.rig, "RightHand"),
        pole: [1., 0., 0.],
        target: IkTarget::Object {
            object: "interaction-block".into(),
            offset: [-0.43, 0.33, -0.5],
        },
        weight_parameter: Some("Reach contact".into()),
        smoothing: 14.,
        ..Default::default()
    });
    reaching.warps = Arc::new(vec![MotionWarp {
        name: "Interact".into(),
        state: "Reach".into(),
        start: 0.10,
        end: 0.85,
        translation: [true, false, true],
        yaw: true,
        target: WarpTarget::Object {
            object: "reach-target".into(),
            offset: [0.; 3],
            yaw_degrees: 180.,
        },
    }]);
    reaching
}
fn presentation(scene: &mut Scene) {
    let mut camera = object("camera", [4.8, 3.5, 9.8]);
    camera.transform.rotation_degrees = [-14., 26., 0.];
    camera.camera = Some(Camera::Perspective {
        vertical_fov_degrees: 36.,
        near: 0.1,
        far: 100.,
    });
    scene.views.insert(Layer::ThreeD, "camera".into());
    scene.objects.push(camera);
    scene.lighting.ambient_intensity = 0.22;
    scene.lighting.sun_intensity = 2.5;
    scene.lighting.sun_direction = [-0.5, 0.9, 0.5];
    scene.environment.zenith = [0.05, 0.07, 0.12];
    scene.environment.horizon = [0.16, 0.20, 0.27];
}
fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes");
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Character Animation Lab","views":{},"objects":[]}"#,
    )?;
    scene_assets(&mut scene);
    let (source, reused) = models(&scene, &root)?;
    let stats = (source.nodes.len(), source.clips.len(), reused.clips.len());
    characters(&mut scene, source, reused)?;
    stage(&mut scene);
    presentation(&mut scene);
    hud(&mut scene)?;
    scene.validate()?;
    // The cooked clips are generated data; compact JSON avoids a multi-megabyte indentation cost.
    std::fs::write(
        root.join("animation-lab.json"),
        serde_json::to_string(&scene)?,
    )?;
    println!(
        "animation_lab_ready nodes={} clips={} retargeted_clips={}",
        stats.0, stats.1, stats.2
    );
    Ok(())
}
