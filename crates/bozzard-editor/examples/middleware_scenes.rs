//! Rebuild the original, editable middleware reference scenes. No GPU or audio device is required.
use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore};
use bozzard_scene::{
    blueprint::{
        Blueprint, BlueprintAttachment, Node, NodeKind as N, ObjectRef, Socket, Value as V, Wire,
    },
    middleware::{
        animation::{Animator, BlendSample, Motion, RootMotion, StateDefinition},
        audio::{AudioMixer, AudioSource, Bus},
        curve::{Curve, Ease, Repeat},
        navigation::{BakeSettings, Behavior, Condition, NavAgent, NavSurface, State, Transition},
        particle::{Curves, Modules},
        registry,
        sprite::{Atlas, Clip, FrameEvent, Sprite, Tilemap},
        timeline::{CameraCut, Marker, Timeline},
        tween::{Property, Track, Tween},
        ui::{Anchors, Canvas, Layout, Localization, Widget, WidgetKind},
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
fn cube(id: &str, position: [f32; 3], scale: [f32; 3], color: [f32; 3]) -> Object {
    let mut o = object(id, position);
    o.transform.scale = scale;
    o.drawable = Some(Drawable {
        layer: Layer::ThreeD,
        mesh: Mesh::Cube,
        texture: Texture::White,
        color,
        gi_static: true,
        metallic: None,
        roughness: None,
        material_overrides: vec![],
        uv_scale: [1.; 2],
    });
    o
}
fn asset(scene: &mut Scene, id: &str, kind: AssetKind, filename: &str) {
    scene.assets.insert(
        id.into(),
        AssetSource {
            kind,
            path: format!("assets/{filename}"),
        },
    );
}
fn action(object: &mut Object, kind: N, inputs: &[(usize, V)], value_port: Option<usize>) {
    let mut node = Node::new(2, kind, [300., 40.]);
    for (index, value) in inputs {
        node.inputs[*index] = value.clone();
    }
    let mut graph = Blueprint {
        name: object.name.clone(),
        nodes: vec![Node::new(1, N::UiEvent, [40., 40.]), node],
        ..Default::default()
    };
    graph.wires.push(Wire {
        from: Socket { node: 1, port: 0 },
        to: Socket { node: 2, port: 0 },
    });
    if let Some(port) = value_port {
        graph.wires.push(Wire {
            from: Socket { node: 1, port: 2 },
            to: Socket { node: 2, port },
        });
    }
    object.blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
}
fn target(id: &str) -> V {
    V::Object(ObjectRef::Id(id.into()))
}
fn panel(scene: &mut Scene, layer: Layer, title: &str, help: &str) -> Result<()> {
    let mut canvas = object("lab-canvas", [0.; 3]);
    registry::set(
        &mut canvas,
        &Canvas {
            layer,
            ..Default::default()
        },
    )?;
    scene.objects.push(canvas);
    let mut panel = object("lab-controls", [0.; 3]);
    panel.parent = Some("lab-canvas".into());
    registry::set(
        &mut panel,
        &Widget {
            anchors: Anchors {
                min: [0., 0.],
                max: [0., 1.],
                pivot: [0., 0.],
                offset: [24., 24.],
                size: [330., -48.],
            },
            layout: Layout::Column,
            scrollable: true,
            padding: [20.; 4],
            gap: 10.,
            image: "panel".into(),
            border: [8.; 4],
            background: [1.; 4],
            ..Default::default()
        },
    )?;
    scene.objects.push(panel);
    for (id, text, font, height) in [
        ("lab-title", title, 29., 74.),
        ("lab-help", help, 16., 115.),
    ] {
        let mut label = object(id, [0.; 3]);
        label.parent = Some("lab-controls".into());
        registry::set(
            &mut label,
            &Widget {
                kind: WidgetKind::Label,
                text: text.into(),
                font_size: font,
                background: [0.; 4],
                padding: [0.; 4],
                anchors: Anchors {
                    size: [280., height],
                    ..Default::default()
                },
                order: scene.objects.len() as i32,
                ..Default::default()
            },
        )?;
        scene.objects.push(label);
    }
    Ok(())
}
fn control(
    scene: &mut Scene,
    id: &str,
    title: &str,
    kind: WidgetKind,
    range: [f32; 3],
) -> Result<usize> {
    let mut o = object(id, [0.; 3]);
    o.parent = Some("lab-controls".into());
    registry::set(
        &mut o,
        &Widget {
            kind,
            text: title.into(),
            locale_key: id.into(),
            anchors: Anchors {
                size: [280., 56.],
                ..Default::default()
            },
            order: scene.objects.len() as i32,
            background: [0.04, 0.12, 0.17, 1.],
            min: range[0],
            max: range[1],
            value: range[2],
            step: 0.1,
            ..Default::default()
        },
    )?;
    let index = scene.objects.len();
    scene.objects.push(o);
    Ok(index)
}
fn accessibility_controls(scene: &mut Scene) -> Result<()> {
    let i = control(
        scene,
        "text-scale",
        "Text size",
        WidgetKind::Slider,
        [1., 3., 1.],
    )?;
    action(&mut scene.objects[i], N::SetUiTextScale, &[], Some(1));
    for (id, label, lang) in [("english", "English", "en"), ("swedish", "Svenska", "sv")] {
        let i = control(scene, id, label, WidgetKind::Button, [0., 1., 0.])?;
        action(
            &mut scene.objects[i],
            N::SetUiLanguage,
            &[(1, V::Text(lang.into()))],
            None,
        );
    }
    let i = control(
        scene,
        "contrast",
        "High contrast",
        WidgetKind::Button,
        [0., 1., 0.],
    )?;
    action(
        &mut scene.objects[i],
        N::SetUiContrast,
        &[(1, V::Bool(true))],
        None,
    );
    let i = control(
        scene,
        "contrast-reset",
        "Standard contrast",
        WidgetKind::Button,
        [0., 1., 0.],
    )?;
    action(
        &mut scene.objects[i],
        N::SetUiContrast,
        &[(1, V::Bool(false))],
        None,
    );
    let mut localization = object("translations", [0.; 3]);
    let en: BTreeMap<String, String> = scene
        .objects
        .iter()
        .filter_map(|o| registry::get::<Widget>(o).ok().flatten())
        .filter(|w| !w.locale_key.is_empty())
        .map(|w| (w.locale_key, w.text))
        .collect();
    let mut sv = en.clone();
    for (key, value) in [
        ("text-scale", "Textstorlek"),
        ("volume", "Ljudvolym"),
        ("play-chime", "Spela ljud"),
        ("contrast", "Hög kontrast"),
        ("contrast-reset", "Standardkontrast"),
        ("blend", "Animationsblandning"),
        ("english", "English"),
        ("swedish", "Svenska"),
    ] {
        sv.insert(key.into(), value.into());
    }
    registry::set(
        &mut localization,
        &Localization {
            language: "en".into(),
            fallback: "en".into(),
            translations: Arc::new(BTreeMap::from([("en".into(), en), ("sv".into(), sv)])),
        },
    )?;
    scene.objects.push(localization);
    Ok(())
}
fn base(name: &str) -> Result<Scene> {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"Middleware","views":{},"objects":[]}"#)?;
    scene.name = name.into();
    asset(
        &mut scene,
        "panel",
        AssetKind::Image,
        "middleware-panel.png",
    );
    let mut camera = object("camera-3d", [10., 9., 15.]);
    camera.transform.rotation_degrees = [-26., 30., 0.];
    camera.camera = Some(Camera::Perspective {
        vertical_fov_degrees: 50.,
        near: 0.1,
        far: 150.,
    });
    scene.views.insert(Layer::ThreeD, camera.id.clone());
    scene.objects.push(camera);
    let mut camera = object("camera-2d", [0., 0., 10.]);
    camera.camera = Some(Camera::Orthographic {
        vertical_size: 10.,
        near: 0.1,
        far: 100.,
    });
    scene.views.insert(Layer::TwoD, camera.id.clone());
    scene.objects.push(camera);
    Ok(scene)
}
fn laboratory(root: &Path) -> Result<Scene> {
    let mut scene = base("Middleware Lab")?;
    asset(
        &mut scene,
        "banner",
        AssetKind::Mesh,
        "animated-banner.gltf",
    );
    for ext in ["wav", "ogg", "mp3", "flac"] {
        asset(
            &mut scene,
            &format!("chime-{ext}"),
            AssetKind::Audio,
            &format!("middleware-chime.{ext}"),
        );
    }
    let mut floor = cube(
        "walkable-floor",
        [0., -0.5, 0.],
        [12., 1., 12.],
        [0.07, 0.13, 0.18],
    );
    floor.collider = Some(BoxCollider {
        size: [1.; 3],
        ..Default::default()
    });
    scene.objects.push(floor);
    let mut wall = cube(
        "navigation-obstacle",
        [0., 0.65, 0.],
        [0.6, 1.3, 2.5],
        [0.22, 0.34, 0.42],
    );
    wall.collider = Some(BoxCollider {
        size: [1.; 3],
        ..Default::default()
    });
    scene.objects.push(wall);
    let mut store = AssetStore::new(root, &scene.assets)?;
    store.load_pending()?;
    let AssetData::Mesh(mesh) = store
        .get(store.handle("banner").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        unreachable!()
    };
    let mut rig = (*mesh.skin.as_ref().unwrap().rig).clone();
    for clip in &mut rig.clips {
        clip.events.push(Marker {
            time: 0.5,
            name: "Chime".into(),
        });
    }
    let mut animated = cube("animated-banner", [-3., 0.1, -2.], [1.8; 3], [1.; 3]);
    animated.drawable.as_mut().unwrap().mesh = Mesh::Asset("banner".into());
    let mut animator = Animator::from_rig("banner".into(), Arc::new(rig));
    animator.parameters.insert("Blend".into(), 0.5);
    animator.initial = "Blend".into();
    animator.states = Arc::new(vec![StateDefinition {
        name: "Blend".into(),
        motion: Motion::Blend1d {
            parameter: "Blend".into(),
            samples: vec![
                BlendSample {
                    threshold: 0.,
                    clip: 0,
                },
                BlendSample {
                    threshold: 1.,
                    clip: 1,
                },
            ],
        },
        repeat: Repeat::Loop,
    }]);
    registry::set(&mut animated, &animator)?;
    registry::set(
        &mut animated,
        &AudioSource {
            asset: "chime-ogg".into(),
            duration: 1.,
            volume: 0.35,
            min_distance: 5.,
            max_distance: 40.,
            ..Default::default()
        },
    )?;
    let mut sound = Node::new(2, N::PlayAudio, [320., 40.]);
    sound.inputs[1] = V::Bool(true);
    animated.blueprints.push(BlueprintAttachment {
        enabled: true,
        graph: Blueprint {
            name: "Animation event sound".into(),
            nodes: vec![Node::new(1, N::AnimationEvent, [40., 40.]), sound],
            wires: vec![Wire {
                from: Socket { node: 1, port: 0 },
                to: Socket { node: 2, port: 0 },
            }],
            ..Default::default()
        },
    });
    scene.objects.push(animated);
    let mut walker = cube("root-motion-banner", [1.5, 0.1, -3.5], [1.; 3], [1.; 3]);
    walker.drawable.as_mut().unwrap().mesh = Mesh::Asset("banner".into());
    let mut root_anim = animator.clone();
    root_anim.initial = "Walk once".into();
    root_anim.states = Arc::new(vec![StateDefinition {
        name: "Walk once".into(),
        motion: Motion::Clip { clip: 1 },
        repeat: Repeat::Once,
    }]);
    root_anim.root_motion = Some(RootMotion {
        node: 0,
        translation: [true, false, true],
        yaw: false,
    });
    registry::set(&mut walker, &root_anim)?;
    scene.objects.push(walker);
    let mut orb = cube("curve-cube", [3., 1., 0.], [0.8; 3], [0.2, 0.9, 0.65]);
    let mut translation = Track::new(Property::Translation);
    translation.channels = vec![
        Curve::constant(3.),
        Curve::linear(0.8, 2.6, 2.),
        Curve::constant(0.),
    ];
    let mut rotation = Track::new(Property::Rotation);
    rotation.channels[1] = Curve::linear(0., 180., 2.);
    let mut color = Track::new(Property::Color);
    color.channels = vec![
        Curve::linear(0.15, 1., 2.),
        Curve::linear(0.85, 0.3, 2.),
        Curve::constant(0.6),
    ];
    let mut roughness = Track::new(Property::Roughness);
    roughness.channels[0] = Curve::linear(0.08, 0.9, 2.);
    registry::set(
        &mut orb,
        &Tween {
            autoplay: true,
            duration: 2.,
            repeat: Repeat::PingPong,
            ease: Ease::Smooth,
            tracks: Arc::new(vec![translation, rotation, color, roughness]),
            ..Default::default()
        },
    )?;
    scene.objects.push(orb);
    let mut sparks = object("curve-sparks", [2., 0.2, 3.]);
    sparks.particle_emitter = Some(ParticleEmitter::preset(ParticleKind::Sparks));
    registry::set(
        &mut sparks,
        &Modules {
            curves: Arc::new(Curves {
                size: Curve::linear(1., 0.25, 1.),
                opacity: Curve::linear(1., 0., 1.),
                color: [
                    Curve::constant(1.),
                    Curve::linear(1., 0.25, 1.),
                    Curve::constant(0.25),
                ],
                ..Default::default()
            }),
            ..Default::default()
        },
    )?;
    scene.objects.push(sparks);
    let mut smoke = object("curve-smoke", [-2., 0.1, 3.]);
    smoke.particle_emitter = Some(ParticleEmitter::preset(ParticleKind::Smoke));
    registry::set(
        &mut smoke,
        &Modules {
            curves: Arc::new(Curves {
                size: Curve::linear(0.5, 2., 1.),
                ..Default::default()
            }),
            ..Default::default()
        },
    )?;
    scene.objects.push(smoke);
    let mut audio = object("streamed-music", [0.; 3]);
    registry::set(
        &mut audio,
        &AudioSource {
            asset: "chime-mp3".into(),
            duration: 1.,
            streaming: true,
            spatial: false,
            bus: Bus::Music,
            pause_with_game: false,
            volume: 0.3,
            ..Default::default()
        },
    )?;
    registry::set(&mut audio, &AudioMixer::default())?;
    scene.objects.push(audio);
    let mut camera = object("cinematic-camera", [4., 5., 10.]);
    camera.transform.rotation_degrees = [-22., 22., 0.];
    camera.camera = scene.objects[0].camera;
    scene.objects.push(camera);
    let mut timeline = object("cinematic-timeline", [0.; 3]);
    registry::set(
        &mut timeline,
        &Timeline {
            motion: Tween {
                duration: 4.,
                ..Default::default()
            },
            markers: Arc::new(vec![
                Marker {
                    time: 0.,
                    name: "Begin".into(),
                },
                Marker {
                    time: 4.,
                    name: "End".into(),
                },
            ]),
            cameras: Arc::new(vec![
                CameraCut {
                    time: 0.,
                    layer: Layer::ThreeD,
                    camera: "cinematic-camera".into(),
                },
                CameraCut {
                    time: 4.,
                    layer: Layer::ThreeD,
                    camera: "camera-3d".into(),
                },
            ]),
        },
    )?;
    scene.objects.push(timeline);
    let settings = BakeSettings {
        min: [-5.5, -1., -5.5],
        max: [5.5, 3., 5.5],
        radius: 0.2,
        height: 1.2,
        ..Default::default()
    };
    let demo = bozzard_demo::SceneDemo::new(&scene)?;
    let baked = demo
        .instance()
        .bake_navigation(&demo.app.world, &settings, |_, _| Ok(()))?;
    let mut nav = object("baked-navigation", [0.; 3]);
    registry::set(
        &mut nav,
        &NavSurface {
            settings,
            baked: Some(Arc::new(baked)),
        },
    )?;
    scene.objects.push(nav);
    let mut agent = object("patrol-agent", [-3., 0., 0.]);
    registry::set(
        &mut agent,
        &NavAgent {
            surface: "baked-navigation".into(),
            radius: 0.2,
            height: 1.2,
            eye_height: 0.8,
            speed: 2.,
            initial: "Patrol".into(),
            perception_target: Some("curve-cube".into()),
            states: Arc::new(vec![
                State {
                    name: "Patrol".into(),
                    behavior: Behavior::Patrol,
                    patrol: vec![[-3., 0., 0.], [3., 0., 0.], [3., 0., -3.], [-3., 0., -3.]],
                    ..Default::default()
                },
                State {
                    name: "Watch".into(),
                    ..Default::default()
                },
            ]),
            transitions: Arc::new(vec![
                Transition {
                    from: "Patrol".into(),
                    to: "Watch".into(),
                    condition: Condition::SeeTarget,
                    seconds: 1.,
                },
                Transition {
                    from: "Watch".into(),
                    to: "Patrol".into(),
                    condition: Condition::After,
                    seconds: 1.5,
                },
            ]),
            ..Default::default()
        },
    )?;
    scene.objects.push(agent);
    let mut visual = cube(
        "agent-visual",
        [0., 0.5, 0.],
        [0.5, 1., 0.5],
        [1., 0.65, 0.2],
    );
    visual.parent = Some("patrol-agent".into());
    scene.objects.push(visual);
    panel(
        &mut scene,
        Layer::ThreeD,
        "MIDDLEWARE\nLAB",
        "Animated glTF, event-driven audio, authored curves, a baked patrol route and GPU particles. Tab or scroll through these Blueprint controls.",
    )?;
    for (id, label, kind, params, port) in [
        (
            "blend",
            "Animation blend",
            N::SetAnimationParameter,
            vec![(1, V::Text("Blend".into())), (3, target("animated-banner"))],
            Some(2),
        ),
        (
            "volume",
            "SFX volume",
            N::SetAudioBusVolume,
            vec![(1, V::Text("Sfx".into()))],
            Some(2),
        ),
        (
            "play-chime",
            "Play streamed MP3",
            N::PlayAudio,
            vec![(1, V::Bool(true)), (2, target("streamed-music"))],
            None,
        ),
        (
            "cinematic",
            "Play camera cuts",
            N::PlayTimeline,
            vec![(1, V::Bool(true)), (2, target("cinematic-timeline"))],
            None,
        ),
    ] {
        let i = control(
            &mut scene,
            id,
            label,
            if port.is_some() {
                WidgetKind::Slider
            } else {
                WidgetKind::Button
            },
            [0., 1., 0.5],
        )?;
        action(&mut scene.objects[i], kind, &params, port);
    }
    accessibility_controls(&mut scene)?;
    store.bake_audio_metadata(&mut scene)?;
    scene.validate()?;
    Ok(scene)
}
fn two_d() -> Result<Scene> {
    let mut scene = base("UI and 2D Lab")?;
    asset(
        &mut scene,
        "atlas",
        AssetKind::Image,
        "middleware-atlas.png",
    );
    scene.game_flow=Some(GameFlowSettings{title:"Courier garden".into(),instructions:"A small, editable scene: atlas animation, solid tilemap, localized nine-slice controls, and authored start/pause/retry menus.".into()});
    // Generate editable phase menus before adding the lab's own Canvas.
    scene.views.remove(&Layer::ThreeD);
    scene.ensure_game_menus()?;
    let mut tiles = object("solid-tiles", [-1., -2., 0.]);
    registry::set(
        &mut tiles,
        &Tilemap {
            image: "atlas".into(),
            atlas: Atlas {
                columns: 5,
                rows: 1,
            },
            dimensions: [12, 3],
            tile_size: [0.65; 2],
            cells: Arc::new(vec![5; 36]),
            solid: Arc::new([5].into()),
            ..Default::default()
        },
    )?;
    scene.objects.push(tiles);
    let mut sprite = object("courier-sprite", [2., -1.35, 0.1]);
    registry::set(
        &mut sprite,
        &Sprite {
            image: "atlas".into(),
            atlas: Atlas {
                columns: 5,
                rows: 1,
            },
            size: [1.4; 2],
            initial: "Idle".into(),
            clips: Arc::new(vec![Clip {
                name: "Idle".into(),
                fps: 5.,
                frames: vec![0, 1, 2, 3],
                events: vec![FrameEvent {
                    frame: 2,
                    name: "Blink".into(),
                }],
                ..Default::default()
            }]),
            ..Default::default()
        },
    )?;
    scene.objects.push(sprite);
    let mut platform = object("floating-tiles", [1., 0.2, 0.]);
    registry::set(
        &mut platform,
        &Tilemap {
            image: "atlas".into(),
            atlas: Atlas {
                columns: 5,
                rows: 1,
            },
            dimensions: [5, 1],
            tile_size: [0.65; 2],
            cells: Arc::new(vec![5; 5]),
            solid: Arc::new([5].into()),
            ..Default::default()
        },
    )?;
    scene.objects.push(platform);
    panel(
        &mut scene,
        Layer::TwoD,
        "COURIER\nGARDEN",
        "Everything on this screen is a scene object. Edit the Sprite atlas, paint solid Tiles, resize the nine-slice controls, or switch language. Escape opens the authored pause menu.",
    )?;
    // Controls appear during gameplay; start and pause menus own keyboard input otherwise.
    let canvas = scene
        .objects
        .iter_mut()
        .find(|o| o.id == "lab-canvas")
        .unwrap();
    registry::set(
        canvas,
        &Canvas {
            layer: Layer::TwoD,
            phase: middleware::ui::Phase::Playing,
            ..Default::default()
        },
    )?;
    let i = control(
        &mut scene,
        "retry-animation",
        "Replay sprite clip",
        WidgetKind::Button,
        [0., 1., 0.],
    )?;
    action(
        &mut scene.objects[i],
        N::PlaySprite,
        &[
            (1, V::Text("Idle".into())),
            (2, V::Bool(true)),
            (3, target("courier-sprite")),
        ],
        None,
    );
    accessibility_controls(&mut scene)?;
    scene.validate()?;
    Ok(scene)
}
fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes");
    for (name, scene) in [
        ("middleware-lab", laboratory(&root)?),
        ("ui-2d-lab", two_d()?),
    ] {
        std::fs::write(root.join(format!("{name}.json")), scene.to_json()?)?;
        println!("{name}: {} authored objects", scene.objects.len());
    }
    Ok(())
}
