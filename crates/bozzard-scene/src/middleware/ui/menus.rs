//! One-time migration of legacy game-flow metadata into ordinary editable scene objects.
use super::*;
use crate::blueprint::{Blueprint, BlueprintAttachment, Node, NodeKind, Socket, Wire};
impl Scene {
    /// Existing canvases take precedence. Generated buttons execute ordinary Blueprint graphs.
    /// The resulting hierarchy is serialized with the document and can be freely edited/deleted.
    pub fn ensure_game_menus(&mut self) -> Result<bool> {
        if self.game_flow.is_none()
            || self
                .objects
                .iter()
                .any(|o| o.extras.contains_key(Canvas::NAME))
        {
            return Ok(false);
        }
        let layer = if self.views.contains_key(&Layer::ThreeD) {
            Layer::ThreeD
        } else {
            Layer::TwoD
        };
        let mut generated = Vec::new();
        let mut ids: BTreeSet<String> = self.objects.iter().map(|o| o.id.clone()).collect();
        let mut object = |name: &str, parent: Option<&str>| {
            let stem = format!("ui-{}", name.to_lowercase().replace(' ', "-"));
            let mut id = stem.clone();
            let mut suffix = 2;
            while !ids.insert(id.clone()) {
                id = format!("{stem}-{suffix}");
                suffix += 1;
            }
            Object {
                id,
                name: name.into(),
                parent: parent.map(str::to_owned),
                ..Default::default()
            }
        };
        for (index, (name, phase)) in [
            ("Start menu", Phase::Ready),
            ("Game HUD", Phase::Playing),
            ("Pause menu", Phase::Paused),
            ("Game over menu", Phase::GameOver),
        ]
        .into_iter()
        .enumerate()
        {
            let mut root = object(name, None);
            super::super::registry::set(
                &mut root,
                &Canvas {
                    layer,
                    phase,
                    order: 100 + index as i32,
                    ..Default::default()
                },
            )?;
            let root_id = root.id.clone();
            generated.push(root);
            let entries: &[(&str, NodeKind, &[&str])] = match phase {
                Phase::Ready => &[
                    ("Start game", NodeKind::StartGame, &["Enter"]),
                    ("Quit", NodeKind::QuitGame, &["Q"]),
                ],
                Phase::Playing => &[("Pause", NodeKind::PauseGame, &["Escape"])],
                Phase::Paused => &[
                    ("Resume", NodeKind::ResumeGame, &["Enter", "Escape"]),
                    ("Restart", NodeKind::RestartGame, &["R"]),
                    ("Quit", NodeKind::QuitGame, &["Q"]),
                ],
                _ => &[
                    ("Retry", NodeKind::RestartGame, &["Enter", "R"]),
                    ("Quit", NodeKind::QuitGame, &["Q"]),
                ],
            };
            let parent = if phase == Phase::Playing {
                root_id.clone()
            } else {
                let mut backdrop = object(&format!("{name} backdrop"), Some(&root_id));
                super::super::registry::set(
                    &mut backdrop,
                    &Widget {
                        anchors: Anchors {
                            min: [0.; 2],
                            max: [1.; 2],
                            pivot: [0.; 2],
                            size: [0.; 2],
                            ..Default::default()
                        },
                        background: [0.008, 0.012, 0.025, 0.94],
                        padding: [0.; 4],
                        ..Default::default()
                    },
                )?;
                let backdrop_id = backdrop.id.clone();
                generated.push(backdrop);
                let mut panel = object(&format!("{name} contents"), Some(&backdrop_id));
                super::super::registry::set(
                    &mut panel,
                    &Widget {
                        anchors: Anchors {
                            min: [0.5; 2],
                            max: [0.5; 2],
                            pivot: [0.5; 2],
                            size: [520., 480.],
                            ..Default::default()
                        },
                        layout: Layout::Column,
                        scrollable: true,
                        padding: [24.; 4],
                        gap: 14.,
                        background: [0.; 4],
                        ..Default::default()
                    },
                )?;
                let parent = panel.id.clone();
                generated.push(panel);
                for (order, text, binding, height, font) in [
                    (
                        0,
                        if phase == Phase::Paused {
                            "Paused"
                        } else if phase == Phase::GameOver {
                            "Game over"
                        } else {
                            ""
                        },
                        if phase == Phase::Ready {
                            "game.title"
                        } else {
                            ""
                        },
                        80.,
                        36.,
                    ),
                    (
                        1,
                        "",
                        if phase == Phase::GameOver {
                            "game.message"
                        } else {
                            "game.instructions"
                        },
                        76.,
                        20.,
                    ),
                ] {
                    let mut label = object(&format!("{name} text {order}"), Some(&parent));
                    super::super::registry::set(
                        &mut label,
                        &Widget {
                            kind: WidgetKind::Label,
                            order,
                            text: text.into(),
                            binding: binding.into(),
                            anchors: Anchors {
                                size: [440., height],
                                ..Default::default()
                            },
                            font_size: font,
                            background: [0.; 4],
                            padding: [0.; 4],
                            ..Default::default()
                        },
                    )?;
                    generated.push(label);
                }
                parent
            };
            for (i, (label, kind, keys)) in entries.iter().enumerate() {
                let mut button = object(&format!("{name} {label}"), Some(&parent));
                let anchors = if phase == Phase::Playing {
                    Anchors {
                        min: [1., 0.],
                        max: [1., 0.],
                        pivot: [1., 0.],
                        offset: [-20., 20.],
                        size: [180., 52.],
                    }
                } else {
                    Anchors {
                        size: [440., 56.],
                        ..Default::default()
                    }
                };
                super::super::registry::set(
                    &mut button,
                    &Widget {
                        kind: WidgetKind::Button,
                        anchors,
                        order: 10 + i as i32,
                        focus_order: i as i32,
                        text: (*label).into(),
                        description: format!("Shortcut: {}", keys.join(" or ")),
                        shortcuts: keys.iter().map(|s| (*s).into()).collect(),
                        background: [0.07, 0.14, 0.21, 1.],
                        ..Default::default()
                    },
                )?;
                button.blueprints.push(BlueprintAttachment {
                    enabled: true,
                    graph: Blueprint {
                        name: format!("{label} action"),
                        nodes: vec![
                            Node::new(1, NodeKind::UiEvent, [40., 40.]),
                            Node::new(2, *kind, [300., 40.]),
                        ],
                        wires: vec![Wire {
                            from: Socket { node: 1, port: 0 },
                            to: Socket { node: 2, port: 0 },
                        }],
                        variables: Default::default(),
                        ..Default::default()
                    },
                });
                generated.push(button);
            }
        }
        let original = self.objects.len();
        self.objects.extend(generated);
        if let Err(error) = self.validate() {
            self.objects.truncate(original);
            return Err(error);
        }
        Ok(true)
    }
}
