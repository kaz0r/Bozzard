//! Physical gameplay keys only; editor shortcuts continue using egui logical keys.
use bozzard_demo::SceneDemo;
use bozzard_scene::GameplayInput;
use eframe::egui::{Event, Key, Modifiers, RawInput};

#[derive(Default)]
pub struct GameplayControls {
    held: [bool; 4],
    // raw_input_hook runs before egui derives repeat flags. Keep this latch across
    // gameplay resets so a still-held key cannot resume motion or auto-jump.
    pressed: [bool; 5],
    // Only focus discontinuities can hide releases. Same-window cancellation
    // retains the press latch without suggesting that focus was lost.
    rearm: [bool; 5],
    focused: bool,
    jump: bool,
}
impl GameplayControls {
    pub fn reset(&mut self) {
        self.held = [false; 4];
        self.jump = false;
    }

    pub fn rearm_hint(&self) -> Option<String> {
        let keys: Vec<_> = ["A", "D", "W", "S", "Space"]
            .into_iter()
            .zip(self.rearm)
            .filter_map(|(key, pending)| pending.then_some(key))
            .collect();
        (self.focused && !keys.is_empty()).then(|| {
            format!(
                "Focus may have missed key releases. Press and release physical {} inside this refocused editor, then press again to play.",
                keys.join(" / ")
            )
        })
    }

    /// Runs in raw_input_hook, before Editor::advance can consume queued motion/edges.
    pub fn prepare(
        &mut self,
        input: &RawInput,
        previous_modifiers: Modifiers,
        eligible: bool,
        play: Option<&mut SceneDemo>,
    ) {
        let modified = |m: Modifiers| m.command || m.ctrl || m.alt;
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
        if !input.focused {
            self.mark_focus_discontinuity();
        }
        self.jump = false;
        for event in &input.events {
            if let Event::WindowFocused(focused) = event {
                self.focused = *focused;
                if !focused {
                    self.mark_focus_discontinuity();
                }
            }
            if let Event::Key {
                physical_key: Some(key),
                pressed,
                repeat,
                ..
            } = event
                && let Some(index) = [Key::A, Key::D, Key::W, Key::S, Key::Space]
                    .iter()
                    .position(|k| k == key)
            {
                let fresh = *pressed && !self.pressed[index] && !repeat;
                self.pressed[index] = *pressed;
                if !pressed {
                    self.rearm[index] = false;
                } else if !self.focused {
                    self.rearm[index] = true;
                }
                if index < 4 {
                    if !pressed {
                        self.held[index] = false;
                    } else if fresh {
                        self.held[index] = true;
                    }
                } else {
                    self.jump |= fresh;
                }
            }
        }
        let Some(play) = play else {
            self.reset();
            return;
        };
        if cancelled || play.gameplay().is_none() {
            self.reset();
            play.clear_gameplay_input();
        }
    }

    fn mark_focus_discontinuity(&mut self) {
        for (pending, pressed) in self.rearm.iter_mut().zip(self.pressed) {
            *pending |= pressed;
        }
    }

    pub fn take_input(&mut self, orbit: [f32; 2]) -> GameplayInput {
        GameplayInput {
            movement: [
                self.held[1] as u8 as f32 - self.held[0] as u8 as f32,
                self.held[2] as u8 as f32 - self.held[3] as u8 as f32,
            ],
            jump: std::mem::take(&mut self.jump),
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
                jump: true,
                orbit: [30.0, 10.0],
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
            assert_eq!(input.orbit, [0.0; 2]);
            let mut idle = self::demo();
            demo.app.step();
            idle.app.step();
            assert_eq!(
                demo.instance.capture(&demo.app.world).unwrap(),
                idle.instance.capture(&idle.app.world).unwrap()
            );
        }
    }
}
