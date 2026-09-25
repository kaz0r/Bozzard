//! Compiled factory module for a game-specific editor Play session.
use crate::{multiplayer::Action, scene::SceneSource, sim::Game, stage::Stage};
use anyhow::{Result, ensure};
use bozzard_app::{App, Plugin, Tick, World};
use bozzard_scene::{Scene, SceneInstance};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};

pub const NAME: &str = "bozz-torio";
const MAX_ACTIONS: usize = 256;

pub struct FactoryModule {
    state: Arc<Mutex<FactoryPlay>>,
}
struct FactoryPlay {
    initial: Game,
    game: Game,
    stage: Stage,
    actions: VecDeque<Action>,
    error: Option<String>,
    rejection: Option<String>,
    installed: bool,
}

impl FactoryModule {
    pub fn from_scene(scene: &Scene, path: &Path) -> Result<Self> {
        let source = SceneSource::from_scene(path.to_path_buf(), scene)?;
        let game = source.new_game()?;
        let stage = Stage::new(&source, &game)?;
        Ok(Self {
            state: Arc::new(Mutex::new(FactoryPlay {
                initial: game.clone(),
                game,
                stage,
                actions: VecDeque::new(),
                error: None,
                rejection: None,
                installed: false,
            })),
        })
    }
    pub fn enqueue(&self, action: Action) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure!(state.installed, "start factory Play before sending actions");
        ensure!(
            state.actions.len() < MAX_ACTIONS,
            "factory action queue is full"
        );
        state.actions.push_back(action);
        Ok(())
    }
    pub fn game(&self) -> Game {
        self.state.lock().unwrap().game.clone()
    }
    pub fn error(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state.error.clone().or_else(|| state.rejection.clone())
    }
}
impl Clone for FactoryModule {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

fn sync(stage: &mut Stage, game: &Game) -> Result<()> {
    stage.sync_terrain(
        game,
        [
            game.hub[0].saturating_sub(18),
            game.hub[1].saturating_sub(11),
        ],
    )?;
    stage.canvas("hud", true)?;
    stage.canvas("menu", false)?;
    stage.sync_outputs(game)?;
    stage.text(
        "phase",
        format!("TIER {} / PHASE {} · 256² WORLD", game.tier(), game.phase()),
    )?;
    let order = game.order();
    stage.text(
        "contract",
        format!(
            "CONTRACT {:02} · {}",
            game.order_index + 1,
            order.item.name().to_uppercase()
        ),
    )?;
    stage.text(
        "progress",
        format!("{} / {} DELIVERED", game.order_progress, order.amount),
    )?;
    stage.text("credits", format!("CREDITS  ¤{}", game.credits))?;
    let (power, capacity) = game.power_network();
    stage.text(
        "electricity",
        format!("POWER {} / {capacity}", game.energy_used),
    )?;
    stage.sync_progress(game, 0., &power)?;
    Ok(())
}

fn with_stage_world(state: &mut FactoryPlay, world: &mut World, tick: Tick) -> Result<()> {
    let runtime = world
        .remove_resource::<SceneInstance>()
        .expect("factory scene instance");
    let authored = std::mem::replace(&mut state.stage.instance, runtime);
    state.stage.world.insert_resource(authored);
    std::mem::swap(world, &mut state.stage.world);
    let result = (|| {
        let mut structural = false;
        for action in std::mem::take(&mut state.actions) {
            match action.apply(&mut state.game) {
                Ok(()) => {
                    structural |= action.structural();
                    state.rejection = None;
                }
                Err(reason) => state.rejection = Some(reason.into()),
            }
        }
        if structural {
            let diagnostics = state
                .stage
                .world
                .remove_resource::<bozzard_diagnostics::Diagnostics>();
            let control = state
                .stage
                .world
                .remove_resource::<bozzard_diagnostics::ExecutionControl>();
            let input = state
                .stage
                .world
                .remove_resource::<bozzard_scene::GameplayInput>();
            state.stage.rebuild(&state.game)?;
            if let Some(value) = diagnostics {
                state.stage.world.insert_resource(value);
            }
            if let Some(value) = control {
                state.stage.world.insert_resource(value);
            }
            if let Some(value) = input {
                state.stage.world.insert_resource(value);
            }
        }
        let advanced = tick.number.is_multiple_of(12);
        if advanced {
            state.game.tick();
        }
        if structural || advanced {
            sync(&mut state.stage, &state.game)?;
        }
        Ok(())
    })();
    std::mem::swap(world, &mut state.stage.world);
    let authored = state
        .stage
        .world
        .remove_resource::<SceneInstance>()
        .expect("authoring placeholder instance");
    let runtime = std::mem::replace(&mut state.stage.instance, authored);
    world.insert_resource(runtime);
    result
}

impl Plugin for FactoryModule {
    fn name(&self) -> &'static str {
        NAME
    }
    fn build(&self, app: &mut App) {
        let mut state = self.state.lock().unwrap();
        assert!(!state.installed, "factory module already installed");
        std::mem::swap(&mut app.world, &mut state.stage.world);
        let authored = state
            .stage
            .world
            .remove_resource::<SceneInstance>()
            .expect("editor Play scene instance");
        let runtime = std::mem::replace(&mut state.stage.instance, authored);
        app.world.insert_resource(runtime);
        if let Some(diagnostics) = state
            .stage
            .world
            .remove_resource::<bozzard_diagnostics::Diagnostics>()
        {
            app.world.insert_resource(diagnostics);
        }
        if let Some(control) = state
            .stage
            .world
            .remove_resource::<bozzard_diagnostics::ExecutionControl>()
        {
            app.world.insert_resource(control);
        }
        if let Some(input) = state
            .stage
            .world
            .remove_resource::<bozzard_scene::GameplayInput>()
        {
            app.world.insert_resource(input);
        }
        state.installed = true;
        drop(state);
        let shared = Arc::clone(&self.state);
        app.add_named_system("Bozz-torio factory", move |world, _, tick| {
            let mut state = shared.lock().unwrap();
            if state.error.is_some()
                || (!tick.number.is_multiple_of(12) && state.actions.is_empty())
            {
                return;
            }
            if let Err(error) = with_stage_world(&mut state, world, tick) {
                let message = format!("{error:#}");
                state.error = Some(message.clone());
                bozzard_diagnostics::log(
                    world,
                    bozzard_diagnostics::Level::Error,
                    "Bozz-torio",
                    &message,
                    Default::default(),
                );
            }
        });
    }
    fn cleanup(&self, app: &mut App) {
        let mut state = self.state.lock().unwrap();
        state.actions.clear();
        if state.installed {
            let runtime = app
                .world
                .remove_resource::<SceneInstance>()
                .expect("factory instance during cleanup");
            let authored = std::mem::replace(&mut state.stage.instance, runtime);
            state.stage.world.insert_resource(authored);
            if let Some(value) = app
                .world
                .remove_resource::<bozzard_diagnostics::Diagnostics>()
            {
                state.stage.world.insert_resource(value);
            }
            if let Some(value) = app
                .world
                .remove_resource::<bozzard_diagnostics::ExecutionControl>()
            {
                state.stage.world.insert_resource(value);
            }
            if let Some(value) = app.world.remove_resource::<bozzard_scene::GameplayInput>() {
                state.stage.world.insert_resource(value);
            }
            std::mem::swap(&mut app.world, &mut state.stage.world);
            state.game = state.initial.clone();
            let reset = state.game.clone();
            if let Err(error) = state.stage.rebuild(&reset) {
                state.error = Some(format!("resetting factory module: {error:#}"));
            } else {
                state.error = None;
            }
            state.rejection = None;
        }
        state.installed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Direction, Item, Kind, PATCH_X, PATCH_Y, WIDTH};
    use bozzard_scene::middleware::ui::Runtime as UiRuntime;

    #[test]
    fn compiled_module_runs_factory_route_in_editor_play_and_cleans_up() {
        let path = SceneSource::default_path();
        let source = SceneSource::open(path.clone()).unwrap();
        let module = FactoryModule::from_scene(&source.authored, &path).unwrap();
        let mut play = bozzard_demo::SceneDemo::new(&source.authored).unwrap();
        play.app
            .install_modules(vec![Box::new(module.clone())])
            .unwrap();
        assert_eq!(play.app.installed_modules().collect::<Vec<_>>(), vec![NAME]);
        let y = PATCH_Y + 7;
        for (x, kind) in [(PATCH_X + 4, Kind::Miner), (PATCH_X + 5, Kind::Furnace)]
            .into_iter()
            .chain((6..18).map(|x| (PATCH_X + x, Kind::Belt)))
        {
            module
                .enqueue(Action::Place {
                    x: x as u16,
                    y: y as u16,
                    kind,
                    direction: Direction::East,
                })
                .unwrap();
        }
        for _ in 0..4800 {
            play.app.step();
        }
        assert!(module.error().is_none(), "{:?}", module.error());
        let game = module.game();
        assert!(game.produced[Item::IronBar.index()] >= 8);
        assert!(game.delivered[Item::IronBar.index()] >= 8);
        assert!(game.order_index >= 1);
        assert!(
            play.instance()
                .entity(&format!("bt-building-{}", y * WIDTH + PATCH_X + 4))
                .is_some()
        );
        let contract = play
            .app
            .world
            .resource::<UiRuntime>()
            .and_then(|ui| ui.widgets.get("bt-ui-contract"))
            .and_then(|widget| widget.text.as_deref());
        assert!(contract.is_some_and(|text| text.contains("CONTRACT 02")));
        play.app.stop_modules();
        assert_eq!(play.app.installed_modules().count(), 0);
        assert_eq!(
            play.instance().document().objects.len(),
            source.authored.objects.len()
        );
        assert!(
            play.app
                .world
                .resource::<bozzard_diagnostics::Diagnostics>()
                .is_some()
        );
        play.app.step();
        play.app
            .install_modules(vec![Box::new(module.clone())])
            .unwrap();
        assert_eq!(module.game().order_index, 0);
        play.app.stop_modules();
        drop(play);
    }
}
