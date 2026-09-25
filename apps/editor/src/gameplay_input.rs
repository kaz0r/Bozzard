//! Prefer physical gameplay keys; fall back to egui's logical key when a backend omits the
//! physical key. Editor shortcuts continue using logical keys. Any key a scene can bind is
//! tracked, so a scene chooses its own buttons.
use bozzard_demo::SceneDemo;
use bozzard_scene::{GameplayInput, keys};
use eframe::egui::{Context, Event, Key, Modifiers, PointerButton, RawInput};

/// Editor buttons can retain egui focus after Play is clicked. Only a text editor should
/// intercept gameplay typing.
pub(crate) fn keyboard_available(ctx: &Context) -> bool {
    !ctx.text_edit_focused()
}

/// Controller-free script/Blueprint scenes own Ctrl chords. System and editor
/// navigation modifiers still cancel input, as do all shortcuts in controller mode.
pub(crate) fn modifiers_block_gameplay(m: Modifiers, scene_keyboard: bool) -> bool {
    m.alt || m.mac_cmd || (m.command && !m.ctrl) || (m.ctrl && !scene_keyboard)
}

#[derive(Default)]
pub struct GameplayControls {
    // Physical keys as last observed. raw_input_hook runs before egui derives repeat flags,
    // and this latch survives gameplay resets so a still-held key cannot resume motion or
    // auto-jump after focus loss.
    pressed: u128,
    // Keys gameplay may act on this frame.
    keys: u128,
    pressed_keys: u128,
    // Only focus discontinuities can hide releases. Same-window cancellation
    // retains the press latch without suggesting that focus was lost.
    rearm: u128,
    focused: bool,
    jump: bool,
    fire: bool,
    interact: bool,
}
impl GameplayControls {
    pub fn reset(&mut self) {
        self.keys = 0;
        self.pressed_keys = 0;
        self.jump = false;
        self.fire = false;
        self.interact = false;
    }

    pub fn rearm_hint(&self) -> Option<String> {
        let keys: Vec<_> = keys::mask_names(self.rearm).collect();
        (self.focused && !keys.is_empty()).then(|| {
            format!(
                "Focus may have missed key releases. Press and release physical {} inside this refocused editor, then press again to play.",
                keys.join(" / ")
            )
        })
    }

    /// Records a physical key and reports the binding when this is a fresh press gameplay
    /// should act on. Names the engine does not bind are ignored.
    fn hold(&mut self, requested: &str, pressed: bool, repeat: bool) -> Option<&'static str> {
        let name = keys::canonical(requested)?;
        let bit = keys::bit(name);
        let fresh = pressed && self.pressed & bit == 0 && !repeat;
        if pressed {
            self.pressed |= bit;
        } else {
            self.pressed &= !bit;
            self.keys &= !bit;
            self.rearm &= !bit;
            return None;
        }
        if fresh {
            self.keys |= bit;
            self.pressed_keys |= bit;
        }
        if !self.focused {
            self.rearm |= bit;
        }
        fresh.then_some(name)
    }

    fn modifiers(&mut self, m: Modifiers) {
        for (name, held) in [("Ctrl", m.ctrl), ("Shift", m.shift), ("Alt", m.alt)] {
            self.hold(name, held, false);
            // Modifier snapshots remain reliable when egui consumed a shortcut's
            // key event. A fresh ordinary key can still form a chord afterwards.
            if held && self.focused {
                self.keys |= keys::bit(name);
                self.rearm &= !keys::bit(name);
            }
        }
    }

    /// Test shorthand when keyboard focus and pointer ownership have the same eligibility.
    #[cfg(test)]
    pub fn prepare(
        &mut self,
        input: &RawInput,
        previous_modifiers: Modifiers,
        eligible: bool,
        play: Option<&mut SceneDemo>,
    ) {
        self.prepare_with_pointer(input, previous_modifiers, eligible, eligible, play);
    }

    /// Runs in raw_input_hook, before Editor::advance can consume queued motion/edges.
    /// Keyboard focus and pointer ownership are separate in editor Play: a script can receive
    /// keys while the mouse is outside the viewport, but editor-panel clicks must stay in egui.
    pub fn prepare_with_pointer(
        &mut self,
        input: &RawInput,
        previous_modifiers: Modifiers,
        eligible: bool,
        pointer_eligible: bool,
        play: Option<&mut SceneDemo>,
    ) {
        let scene_keyboard = play
            .as_ref()
            .is_some_and(|p| p.gameplay().is_none() && p.instance().has_gameplay_logic());
        let modified = |m: Modifiers| modifiers_block_gameplay(m, scene_keyboard);
        let cancelled = !eligible
            || !input.focused
            || modified(previous_modifiers)
            || input.events.iter().any(|event| match event {
                Event::ModifiersChanged(m) => modified(*m),
                Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } => (*key == Key::Escape && *pressed) || modified(*modifiers),
                Event::PointerGone | Event::WindowFocused(false) => true,
                _ => false,
            });
        self.focused = input.focused;
        if scene_keyboard {
            self.modifiers(previous_modifiers);
        }
        if !input.focused {
            self.mark_focus_discontinuity();
        }
        self.jump = false;
        self.fire = false;
        self.interact = false;
        if !pointer_eligible {
            for name in ["MouseLeft", "MouseRight", "MouseMiddle"] {
                let bit = keys::bit(name);
                self.pressed &= !bit;
                self.keys &= !bit;
                self.rearm &= !bit;
            }
        }
        for event in &input.events {
            match event {
                Event::ModifiersChanged(m) if scene_keyboard => self.modifiers(*m),
                Event::PointerButton {
                    button, pressed, ..
                } => {
                    if !pointer_eligible {
                        continue;
                    }
                    let named = match button {
                        PointerButton::Primary => "MouseLeft",
                        PointerButton::Secondary => "MouseRight",
                        PointerButton::Middle => "MouseMiddle",
                        _ => continue,
                    };
                    if self.hold(named, *pressed, false).is_some()
                        && *button == PointerButton::Primary
                    {
                        self.fire = true;
                    }
                }
                Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => {
                    if scene_keyboard {
                        self.modifiers(*modifiers);
                    }
                    // Keep physical positions when available; some backends report only a
                    // logical key, which must still reach scripts and blueprints.
                    let gameplay_key = physical_key.unwrap_or(*key);
                    if let Some(name) = self.hold(&format!("{gameplay_key:?}"), *pressed, *repeat) {
                        match name {
                            "Space" => self.jump = true,
                            "E" => self.interact = true,
                            _ => {}
                        }
                    }
                }
                Event::WindowFocused(focused) => {
                    self.focused = *focused;
                    if !focused {
                        self.mark_focus_discontinuity();
                    }
                }
                _ => {}
            }
        }
        let Some(play) = play else {
            self.reset();
            return;
        };
        if cancelled || !play.accepts_gameplay_input() {
            self.reset();
            play.clear_gameplay_input();
        }
    }

    fn mark_focus_discontinuity(&mut self) {
        self.rearm |= self.pressed;
    }

    pub fn take_input(&mut self, orbit: [f32; 2]) -> GameplayInput {
        let down = |name: &str| u8::from(self.keys & keys::bit(name) != 0) as f32;
        GameplayInput {
            movement: [down("D") - down("A"), down("W") - down("S")],
            keys: self.keys,
            pressed_keys: std::mem::take(&mut self.pressed_keys),
            jump: std::mem::take(&mut self.jump),
            fire: std::mem::take(&mut self.fire),
            interact: std::mem::take(&mut self.interact),
            orbit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn demo() -> SceneDemo {
        let scene = bozzard_scene::Scene::from_json(include_str!(
            "../../../examples/demo/scenes/first-trail.json"
        ))
        .unwrap();
        SceneDemo::new(&scene).unwrap()
    }
    fn key(logical: Key, physical: Key, repeat: bool) -> Event {
        Event::Key {
            key: logical,
            physical_key: Some(physical),
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        }
    }
    fn raw(events: Vec<Event>) -> RawInput {
        RawInput {
            focused: true,
            events,
            ..Default::default()
        }
    }

    #[test]
    fn left_click_fires_once_and_respects_viewport_gating() {
        use eframe::egui::Pos2;
        let click = |button| Event::PointerButton {
            pos: Pos2::ZERO,
            button,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        controls.prepare(
            &raw(vec![click(PointerButton::Primary)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(controls.take_input([0.; 2]).fire);
        assert!(!controls.take_input([0.; 2]).fire, "one shot per click");
        controls.prepare(
            &raw(vec![click(PointerButton::Secondary)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(!controls.take_input([0.; 2]).fire, "right-drag only orbits");
        controls.prepare(
            &raw(vec![click(PointerButton::Primary)]),
            Modifiers::NONE,
            false,
            Some(&mut demo),
        );
        assert!(
            !controls.take_input([0.; 2]).fire,
            "selection and dialogs suppress gameplay fire"
        );
    }

    #[test]
    fn any_bound_button_reaches_gameplay_with_the_focus_latch_intact() {
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        // Digits, arrows and mouse buttons resolve from egui's own spellings.
        controls.prepare(
            &raw(vec![
                key(Key::Num3, Key::Num3, false),
                key(Key::ArrowLeft, Key::ArrowLeft, false),
            ]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        let sample = controls.take_input([0.; 2]);
        assert!(sample.keys & keys::bit("3") != 0);
        assert!(sample.keys & keys::bit("ArrowLeft") != 0);
        assert!(
            controls.take_input([0.; 2]).keys & keys::bit("3") != 0,
            "keys are a level"
        );
        // A held key that never released, then refocus: the latch still refuses to rehold it.
        let mut away = raw(vec![]);
        away.focused = false;
        controls.prepare(&away, Modifiers::NONE, true, Some(&mut demo));
        assert!(controls.rearm_hint().is_none(), "hide while unfocused");
        controls.prepare(
            &raw(vec![Event::WindowFocused(true)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(
            controls
                .rearm_hint()
                .unwrap()
                .contains("physical 3 / ArrowLeft")
        );
        controls.prepare(
            &raw(vec![key(Key::Num3, Key::Num3, false)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert_eq!(controls.take_input([0.; 2]).keys & keys::bit("3"), 0);
        // The release arrives locally, so the next press is fresh again.
        controls.prepare(
            &raw(vec![Event::Key {
                key: Key::Num3,
                physical_key: Some(Key::Num3),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        controls.prepare(
            &raw(vec![key(Key::Num3, Key::Num3, false)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(controls.take_input([0.; 2]).keys & keys::bit("3") != 0);
    }

    #[test]
    fn logical_key_fallback_reaches_gameplay_when_physical_key_is_missing() {
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        controls.prepare(
            &raw(vec![Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        let sample = controls.take_input([0.; 2]);
        assert_ne!(sample.keys & keys::bit("Enter"), 0);
        assert_ne!(sample.pressed_keys & keys::bit("Enter"), 0);
    }

    #[test]
    fn scripted_keyboard_stays_active_outside_viewport_without_firing_editor_clicks() {
        use eframe::egui::Pos2;

        let mut demo = demo();
        let mut controls = GameplayControls::default();
        controls.prepare_with_pointer(
            &raw(vec![
                key(Key::Enter, Key::Enter, false),
                Event::PointerButton {
                    pos: Pos2::ZERO,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ]),
            Modifiers::NONE,
            true,
            false,
            Some(&mut demo),
        );
        let sample = controls.take_input([0.; 2]);
        assert_ne!(sample.pressed_keys & keys::bit("Enter"), 0);
        assert_eq!(sample.keys & keys::bit("MouseLeft"), 0);
        assert!(!sample.fire);
    }

    #[test]
    fn focused_editor_button_does_not_block_gameplay_keyboard() {
        use bozzard_scene::blueprint::{BlackboardValue, Value};

        let ctx = Context::default();
        ctx.memory_mut(|memory| memory.request_focus(eframe::egui::Id::new("play_button")));
        assert!(ctx.egui_wants_keyboard_input());
        assert!(keyboard_available(&ctx));

        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/earth-factory/scenes/earth.json");
        let scene =
            bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
        for _ in 0..8 {
            demo.app.step();
        }
        let mut controls = GameplayControls::default();
        controls.prepare_with_pointer(
            &raw(vec![key(Key::Enter, Key::Enter, false)]),
            Modifiers::NONE,
            keyboard_available(&ctx),
            false,
            Some(&mut demo),
        );
        demo.set_gameplay_input(controls.take_input([0.; 2]));
        demo.app.advance(std::time::Duration::from_millis(20));
        demo.check_simulation().unwrap();
        let board = demo
            .app
            .world
            .resource::<bozzard_scene::BlueprintRuntime>()
            .unwrap();
        assert!(matches!(
            board.scene_blackboard().get("started"),
            Some(BlackboardValue::Scalar(Value::Bool(true)))
        ));
    }

    #[test]
    fn blueprints_receive_viewport_input_without_a_player_controller() {
        let scene = bozzard_scene::Scene::from_json(include_str!(
            "../../../examples/demo/scenes/blueprint-lab.json"
        ))
        .unwrap();
        let mut demo = SceneDemo::new(&scene).unwrap();
        let mut controls = GameplayControls::default();
        controls.prepare(
            &raw(vec![key(Key::Space, Key::Space, false)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        demo.set_gameplay_input(controls.take_input([0.; 2]));
        demo.app.step();
        demo.check_simulation().unwrap();
        let entity = demo.instance().entity("hero-cube").unwrap();
        assert!(
            demo.app
                .world
                .get::<bozzard_scene::BlueprintHidden>(entity)
                .unwrap()
                .0
        );
        controls.prepare(&raw(vec![]), Modifiers::NONE, false, Some(&mut demo));
        assert!(!controls.take_input([0.; 2]).jump);
    }

    #[test]
    fn a_tap_between_frames_reaches_blueprints_once_without_holding_the_key() {
        let scene = bozzard_scene::Scene::from_json(
            &include_str!("../../../examples/demo/scenes/blueprint-lab.json")
                .replace("\"key\": \"jump\"", "\"key\": \"U\""),
        )
        .unwrap();
        let mut demo = SceneDemo::new(&scene).unwrap();
        let mut controls = GameplayControls::default();
        let release = Event::Key {
            key: Key::U,
            physical_key: Some(Key::U),
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        controls.prepare(
            &raw(vec![key(Key::U, Key::U, false), release]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        let sample = controls.take_input([0.; 2]);
        assert_eq!(sample.keys, 0);
        assert_eq!(sample.pressed_keys, keys::bit("U"));
        demo.set_gameplay_input(sample);
        // Several render frames can precede a fixed simulation tick.
        for _ in 0..3 {
            demo.set_gameplay_input(controls.take_input([0.; 2]));
        }
        demo.app.step();
        demo.check_simulation().unwrap();
        let hero = demo.instance().entity("hero-cube").unwrap();
        assert!(
            demo.app
                .world
                .get::<bozzard_scene::BlueprintHidden>(hero)
                .unwrap()
                .0
        );
        demo.app.step();
        demo.check_simulation().unwrap();
        assert!(
            demo.app
                .world
                .get::<bozzard_scene::BlueprintHidden>(hero)
                .unwrap()
                .0,
            "the tap must not toggle visibility a second time"
        );
        assert_eq!(
            demo.app
                .world
                .resource::<GameplayInput>()
                .unwrap()
                .pressed_keys,
            0
        );
    }

    #[test]
    fn physical_keys_ignore_layout_and_clear_at_gameplay_boundaries() {
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        let input = raw(vec![
            key(Key::Z, Key::W, false),
            key(Key::Space, Key::Space, false),
        ]);
        controls.prepare(&input, Modifiers::NONE, true, Some(&mut demo));
        let sample = controls.take_input([0.0; 2]);
        assert_eq!(sample.movement, [0.0, 1.0]);
        assert!(sample.jump);
        assert!(!controls.take_input([0.0; 2]).jump);
        controls.prepare(&input, Modifiers::NONE, true, Some(&mut demo));
        assert!(
            !controls.take_input([0.0; 2]).jump,
            "raw repeats are not yet marked by egui"
        );
        assert!(
            matches!(
                &input.events[0],
                Event::Key {
                    key: Key::Z,
                    physical_key: Some(Key::W),
                    ..
                }
            ),
            "logical shortcut events remain unchanged"
        );
        // The caller gates viewport, dialogs, loading, layer and keyboard UI with eligible.
        controls.prepare(&raw(vec![]), Modifiers::NONE, false, Some(&mut demo));
        controls.prepare(
            &raw(vec![key(Key::Z, Key::W, false)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert_eq!(controls.take_input([0.0; 2]).movement, [0.0; 2]);
        let release = raw(vec![Event::Key {
            key: Key::Z,
            physical_key: Some(Key::W),
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        controls.prepare(&release, Modifiers::NONE, true, Some(&mut demo));
        controls.prepare(&input, Modifiers::NONE, true, Some(&mut demo));
        assert_eq!(controls.take_input([0.0; 2]).movement, [0.0, 1.0]);
        controls.prepare(&raw(vec![]), Modifiers::NONE, true, None);
        controls.prepare(&raw(vec![]), Modifiers::NONE, true, Some(&mut demo));
        assert_eq!(controls.take_input([0.0; 2]).movement, [0.0; 2]);
        controls.prepare(&release, Modifiers::NONE, true, Some(&mut demo));
        controls.prepare(&input, Modifiers::NONE, true, Some(&mut demo));
        controls.prepare(
            &raw(vec![Event::WindowFocused(false)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert_eq!(controls.take_input([0.0; 2]).movement, [0.0; 2]);
    }

    fn release(physical: Key) -> Event {
        Event::Key {
            key: physical,
            physical_key: Some(physical),
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn missing_focus_release_hints_only_pending_keys_and_recovers_after_local_release() {
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        let presses = || {
            raw(vec![
                key(Key::Z, Key::W, false),
                key(Key::Space, Key::Space, false),
            ])
        };
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        // No release is delivered while away (notably possible on macOS).
        let mut away = raw(vec![]);
        away.focused = false;
        controls.prepare(&away, Modifiers::NONE, true, Some(&mut demo));
        assert!(controls.rearm_hint().is_none(), "hide while unfocused");
        controls.prepare(
            &raw(vec![Event::WindowFocused(true)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        let hint = controls.rearm_hint().unwrap();
        assert!(hint.contains("physical W / Space inside this refocused editor"));
        // A fresh press after an unseen release is indistinguishable from raw repeats.
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        let sample = controls.take_input([0.0; 2]);
        assert_eq!(sample.movement, [0.0; 2]);
        assert!(!sample.jump);
        controls.prepare(
            &raw(vec![release(Key::W)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(
            controls
                .rearm_hint()
                .unwrap()
                .contains("physical Space inside")
        );
        controls.prepare(
            &raw(vec![release(Key::Space)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(controls.rearm_hint().is_none());
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        let sample = controls.take_input([0.0; 2]);
        assert_eq!(sample.movement, [0.0, 1.0]);
        assert!(sample.jump);
    }

    #[test]
    fn focus_repeats_stay_cancelled_and_same_window_cancellation_has_no_focus_hint() {
        let mut demo = demo();
        let mut controls = GameplayControls::default();
        let presses = || {
            raw(vec![
                key(Key::W, Key::W, false),
                key(Key::Space, Key::Space, false),
            ])
        };
        // Viewport/modal gating and pointer loss are not focus discontinuities.
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        controls.prepare(&raw(vec![]), Modifiers::NONE, false, Some(&mut demo));
        assert!(controls.rearm_hint().is_none());
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        let sample = controls.take_input([0.0; 2]);
        assert_eq!(sample.movement, [0.0; 2]);
        assert!(!sample.jump);
        controls.prepare(
            &raw(vec![Event::PointerGone]),
            Modifiers::NONE,
            false,
            Some(&mut demo),
        );
        assert!(controls.rearm_hint().is_none());
        controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
        let sample = controls.take_input([0.0; 2]);
        assert_eq!(sample.movement, [0.0; 2]);
        assert!(!sample.jump);
        controls.prepare(
            &raw(vec![
                Event::WindowFocused(false),
                Event::WindowFocused(true),
            ]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        for _ in 0..3 {
            controls.prepare(&presses(), Modifiers::NONE, true, Some(&mut demo));
            let sample = controls.take_input([0.0; 2]);
            assert_eq!(sample.movement, [0.0; 2]);
            assert!(!sample.jump, "unmarked held repeats cannot rearm");
            assert!(controls.rearm_hint().is_some());
        }
        // Observed releases (including platform-delivered focus releases) remove the hint.
        controls.prepare(
            &raw(vec![release(Key::W), release(Key::Space)]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        controls.prepare(
            &raw(vec![
                Event::WindowFocused(false),
                Event::WindowFocused(true),
            ]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        assert!(
            controls.rearm_hint().is_none(),
            "no latched keys: no irrelevant hint"
        );
    }

    #[test]
    fn incoming_modifiers_cancel_queued_movement_jump_and_orbit_before_advance() {
        for modifiers in [Modifiers::CTRL, Modifiers::COMMAND, Modifiers::ALT] {
            let mut demo = demo();
            let mut controls = GameplayControls::default();
            demo.set_gameplay_input(GameplayInput {
                movement: [0.0, 1.0],
                keys: bozzard_scene::keys::bit("F"),
                jump: true,
                fire: true,
                interact: true,
                orbit: [30.0, 10.0],
                ..Default::default()
            });
            controls.prepare(
                &raw(vec![Event::ModifiersChanged(modifiers)]),
                Modifiers::NONE,
                true,
                Some(&mut demo),
            );
            let input = demo.app.world.resource::<GameplayInput>().unwrap();
            assert_eq!(input.movement, [0.0; 2]);
            assert!(!input.jump);
            assert!(!input.fire);
            assert_eq!(input.orbit, [0.0; 2]);
            let mut idle = self::demo();
            demo.app.step();
            idle.app.step();
            assert_eq!(
                demo.instance().capture(&demo.app.world).unwrap(),
                idle.instance().capture(&idle.app.world).unwrap()
            );
        }
    }

    #[test]
    fn scripted_ctrl_chords_survive_modifiers_and_expose_held_state() {
        let scene = bozzard_scene::Scene::from_json(include_str!(
            "../../../examples/demo/scenes/blueprint-lab.json"
        ))
        .unwrap();
        let mut demo = SceneDemo::new(&scene).unwrap();
        let mut controls = GameplayControls::default();
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let event = |pressed, repeat, modifiers| Event::Key {
            key: Key::R,
            physical_key: Some(Key::R),
            pressed,
            repeat,
            modifiers,
        };
        controls.prepare(
            &raw(vec![
                Event::ModifiersChanged(ctrl),
                event(true, false, ctrl),
            ]),
            Modifiers::NONE,
            true,
            Some(&mut demo),
        );
        let input = controls.take_input([0.; 2]);
        assert_eq!(
            input.keys & (keys::bit("Ctrl") | keys::bit("R")),
            keys::bit("Ctrl") | keys::bit("R")
        );
        assert_ne!(input.pressed_keys & keys::bit("R"), 0);
        controls.prepare(
            &raw(vec![event(true, true, ctrl)]),
            ctrl,
            true,
            Some(&mut demo),
        );
        assert_eq!(
            controls.take_input([0.; 2]).pressed_keys & keys::bit("R"),
            0
        );
        controls.prepare(
            &raw(vec![
                event(false, false, ctrl),
                Event::ModifiersChanged(Modifiers::NONE),
            ]),
            ctrl,
            true,
            Some(&mut demo),
        );
        assert_eq!(controls.take_input([0.; 2]).keys, 0);
        assert!(modifiers_block_gameplay(Modifiers::ALT, true));
        assert!(modifiers_block_gameplay(ctrl, false));
    }
}
