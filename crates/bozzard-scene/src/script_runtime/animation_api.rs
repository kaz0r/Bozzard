//! Animation commands use the same validated controller as Blueprint and editor previews.
use super::{Command, Host, fail, vector_of};
use crate::middleware::animation::{Control, WarpGoal};
use rhai::{Engine, EvalAltResult, ImmutableString};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct View {
    pub state: String,
    pub progress: f32,
    pub playing: bool,
}

pub(super) fn register(engine: &mut Engine, host: Arc<Mutex<Host>>) {
    macro_rules! command {
        ($name:literal, ($($arg:ident : $ty:ty),*), $control:expr) => {{
            let host = host.clone();
            engine.register_fn($name, move |target: ImmutableString, $($arg: $ty),*| -> Result<(), Box<EvalAltResult>> {
                let mut host = host.lock().unwrap_or_else(|error| error.into_inner());
                let target = host.target_of(&target)?;
                let control = $control;
                host.record(Command::Animation { target, control });
                Ok(())
            });
        }};
    }
    command!("play_animation", (state: ImmutableString, fade: f32), Control::Play { state: state.to_string(), fade });
    command!("pause_animation", (), Control::Pause);
    command!("stop_animation", (), Control::Stop);
    command!("seek_animation", (phase: f32), Control::Seek(phase));
    command!("set_animation_parameter", (name: ImmutableString, value: f32), Control::Parameter { name: name.to_string(), value });
    command!("restart_animation_layer", (name: ImmutableString), Control::RestartLayer { name: name.to_string() });
    command!("set_animation_warp_target", (name: ImmutableString, position: rhai::Array, yaw: f32), Control::WarpTarget {
        name: name.to_string(), goal: WarpGoal { position: vector_of(position)?, yaw_degrees: yaw },
    });
    command!("clear_animation_warp_target", (name: ImmutableString), Control::ClearWarpTarget { name: name.to_string() });
    macro_rules! query {
        ($name:literal, |$view:ident| $value:expr, $ty:ty) => {{
            let host = host.clone();
            engine.register_fn(
                $name,
                move |target: ImmutableString| -> Result<$ty, Box<EvalAltResult>> {
                    let host = host.lock().unwrap_or_else(|error| error.into_inner());
                    let $view = host
                        .objects
                        .get(host.object_id(&target))
                        .and_then(|v| v.animation.as_ref())
                        .ok_or_else(|| fail(format!("'{target}' has no animation controller")))?;
                    Ok($value)
                },
            );
        }};
    }
    query!("animation_state", |view| view.state.clone(), String);
    query!("animation_progress", |view| view.progress, f32);
    query!("animation_playing", |view| view.playing, bool);
}
