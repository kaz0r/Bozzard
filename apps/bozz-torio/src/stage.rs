//! Bozzard scene components are the playable game's rendering source.
use crate::{
    scene::SceneSource,
    sim::{Direction, Game, HEIGHT, WIDTH},
};
use anyhow::{Context, Result};
use bozzard_assets::AssetStore;
use bozzard_ecs::World;
use bozzard_render::{EnvironmentSettings, RenderScene};
use bozzard_scene::middleware::{
    registry,
    signals::{Kind as SignalKind, Signals},
    sprite::{Atlas, Sprite, Tilemap},
    ui::{Canvas, Control, Frame, Input, Widget},
};
use bozzard_scene::{Layer, Object, Scene, SceneInstance, Transform};
use std::sync::Arc;

const ACTIONS: &[&str] = &[
    "pause",
    "menu-button",
    "map-button",
    "rotate-selected",
    "upgrade",
    "recipe",
    "pick-up",
    "recover",
    "inspect",
    "orientation",
    "pan-left",
    "pan-right",
    "pan-up",
    "pan-down",
    "hub",
    "zoom-out",
    "zoom-in",
    "help-button",
    "continue",
    "new",
    "menu-help",
    "quit",
    "steam-friends",
    "close-help",
    "map-nw",
    "map-ne",
    "map-sw",
    "map-se",
    "map-hub",
    "close-map",
    "carry-0",
    "carry-1",
    "carry-2",
    "carry-3",
    "carry-4",
    "carry-5",
    "tool-0",
    "tool-1",
    "tool-2",
    "tool-3",
    "tool-4",
    "use-0",
    "use-1",
    "use-2",
    "use-3",
    "use-4",
    "use-5",
    "use-6",
    "buy-0",
    "buy-1",
    "buy-2",
    "buy-3",
    "buy-4",
    "buy-5",
    "buy-6",
];

pub struct Stage {
    authored: Scene,
    pub assets: AssetStore,
    pub world: World,
    pub instance: SceneInstance,
    terrain_origin: Option<[usize; 2]>,
}

impl Stage {
    pub fn new(source: &SceneSource, game: &Game) -> Result<Self> {
        let authored = Scene::from_json(&std::fs::read_to_string(&source.path)?)?;
        let mut assets = AssetStore::new(
            source.path.parent().context("scene has no parent")?,
            &authored.assets,
        )?;
        assets.load_pending()?;
        assets.require_ready()?;
        assets.validate_scene_resources(&authored)?;
        let (world, instance) = spawn_runtime(&authored, game)?;
        Ok(Self {
            authored,
            assets,
            world,
            instance,
            terrain_origin: None,
        })
    }

    pub fn rebuild(&mut self, game: &Game) -> Result<()> {
        let (world, instance) = spawn_runtime(&self.authored, game)?;
        self.world = world;
        self.instance = instance;
        self.terrain_origin = None;
        Ok(())
    }

    pub fn reload(&mut self, source: &SceneSource, game: &Game) -> Result<()> {
        *self = Self::new(source, game)?;
        Ok(())
    }

    pub fn canvas(&mut self, name: &str, enabled: bool) -> Result<()> {
        let id = format!("bt-ui-{name}-canvas");
        let entity = self
            .instance
            .entity(&id)
            .with_context(|| format!("missing {id}"))?;
        self.world
            .get_mut::<Canvas>(entity)
            .context("missing UI Canvas")?
            .enabled = enabled;
        Ok(())
    }

    pub fn text(&mut self, name: &str, value: impl Into<String>) -> Result<()> {
        self.instance.control_ui(
            &mut self.world,
            &format!("bt-ui-{name}"),
            Control::Text(value.into()),
        )
    }

    pub fn enabled(&mut self, name: &str, enabled: bool) -> Result<()> {
        self.instance.control_ui(
            &mut self.world,
            &format!("bt-ui-{name}"),
            Control::Enabled(enabled),
        )
    }

    pub fn ui_frame(&self, size: [f32; 2]) -> Result<Frame> {
        self.instance.ui_frame(&self.world, Layer::TwoD, size)
    }

    pub fn input(&mut self, size: [f32; 2], input: Input) -> Result<Vec<&'static str>> {
        if self.world.resource::<Signals>().is_none() {
            self.world.insert_resource(Signals::default());
        }
        self.world
            .resource_mut::<Signals>()
            .unwrap()
            .begin(SignalKind::Ui);
        self.instance
            .ui_input(&mut self.world, Layer::TwoD, size, input)?;
        let signals = self
            .world
            .resource::<Signals>()
            .context("missing UI signals")?;
        Ok(ACTIONS
            .iter()
            .copied()
            .filter(|name| {
                signals
                    .for_owner(&format!("bt-ui-{name}"), SignalKind::Ui)
                    .next()
                    .is_some()
            })
            .collect())
    }

    pub fn camera(
        &mut self,
        size: [f32; 2],
        board: [f32; 2],
        view: [usize; 2],
        zoom: f32,
    ) -> Result<()> {
        let entity = self.instance.camera_entity(Layer::TwoD)?;
        let center_x = view[0] as f32 - 127.5 - (board[0] + zoom * 0.5 - size[0] * 0.5) / zoom;
        let center_y = 127.5 - view[1] as f32 - (size[1] * 0.5 - board[1] - zoom * 0.5) / zoom;
        self.world
            .get_mut::<Transform>(entity)
            .context("missing camera transform")?
            .translation = [center_x, center_y, 25.];
        if let bozzard_scene::Camera::Orthographic { vertical_size, .. } = &mut *self
            .world
            .get_mut::<bozzard_scene::Camera>(entity)
            .context("missing camera")?
        {
            *vertical_size = size[1] / zoom;
        }
        Ok(())
    }

    pub fn conveyor_preview(&mut self, facing: Direction) -> Result<()> {
        let entity = self
            .instance
            .entity("bt-ui-tool-icon-3")
            .context("missing conveyor UI image")?;
        let column = match facing {
            Direction::North => 0.,
            Direction::East => 1.,
            Direction::South => 2.,
            Direction::West => 3.,
        };
        self.world
            .get_mut::<Widget>(entity)
            .context("missing conveyor UI Widget")?
            .uv = [column * 0.25, 0., 0.25, 1.];
        Ok(())
    }

    pub fn sync_terrain(&mut self, game: &Game, view: [usize; 2]) -> Result<()> {
        const CHUNK: usize = 64;
        let origin = [view[0].min(WIDTH - CHUNK), view[1].min(HEIGHT - CHUNK)];
        if self.terrain_origin == Some(origin) {
            return Ok(());
        }
        let entity = self
            .instance
            .entity("factory-floor")
            .context("missing factory floor")?;
        let cells = (0..CHUNK)
            .flat_map(|row| {
                (0..CHUNK).map(move |col| {
                    let frame = game.terrain[(origin[1] + row) * WIDTH + origin[0] + col];
                    if frame == 255 {
                        0
                    } else {
                        u32::from(frame) + 1
                    }
                })
            })
            .collect();
        self.world
            .get_mut::<Tilemap>(entity)
            .context("missing factory Tilemap")?
            .cells = Arc::new(cells);
        self.world
            .get_mut::<Transform>(entity)
            .context("missing floor transform")?
            .translation = [origin[0] as f32 - 128., 128. - origin[1] as f32, 0.];
        self.instance.step_sprites(&mut self.world, 0.)?;
        self.terrain_origin = Some(origin);
        Ok(())
    }

    pub fn sync_outputs(&mut self, game: &Game) -> Result<()> {
        for (index, tile) in game.tiles.iter().enumerate() {
            let Some(building) = &tile.building else {
                continue;
            };
            let name = format!("bt-output-{index}");
            let Some(entity) = self.instance.entity(&name) else {
                continue;
            };
            if let Some(item) = building.output {
                self.world
                    .get_mut::<Sprite>(entity)
                    .context("missing output sprite")?
                    .enabled = true;
                self.instance.control_sprite(
                    &mut self.world,
                    &name,
                    bozzard_scene::middleware::sprite::Control::Frame(item.sprite() as u32),
                )?;
            } else {
                self.world
                    .get_mut::<Sprite>(entity)
                    .context("missing output sprite")?
                    .enabled = false;
            }
        }
        Ok(())
    }

    pub fn render_scene(&self, size: [f32; 2]) -> Result<RenderScene> {
        let view = self
            .instance
            .view(&self.world, Layer::TwoD, size[0] / size[1])?;
        let ui = self.ui_frame(size)?;
        let mut items = bozzard_render_assets::sprite_items(&view.sprites)?;
        for (model, text) in &view.texts {
            items.push(bozzard_render_assets::text_item(
                *model,
                text,
                &self.assets,
            )?);
        }
        items.extend(bozzard_render_assets::widget_items(&ui, &self.assets)?);
        Ok(RenderScene {
            skin_poses: Default::default(),
            shader_time: view.display_time,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: EnvironmentSettings::disabled(),
            display: bozzard_render_assets::display_settings(
                view.display,
                Layer::TwoD,
                view.display_time,
            ),
            lighting: Default::default(),
            view_projection: view.view_projection,
            items,
        })
    }
}

fn spawn_runtime(authored: &Scene, game: &Game) -> Result<(World, SceneInstance)> {
    let mut scene = authored.clone();
    for object in &mut scene.objects {
        if object.id == "factory-floor" {
            object.transform.translation = [-128., 128., 0.];
            let mut map =
                registry::get::<Tilemap>(object)?.context("factory floor has no Tilemap")?;
            map.dimensions = [64, 64];
            map.cells = Arc::new(
                (0..64)
                    .flat_map(|row| {
                        (0..64).map(move |col| {
                            let frame = game.terrain[row * WIDTH + col];
                            if frame == 255 {
                                0
                            } else {
                                u32::from(frame) + 1
                            }
                        })
                    })
                    .collect(),
            );
            registry::set(object, &map)?;
        } else if registry::get::<Sprite>(object)?
            .is_some_and(|sprite| matches!(sprite.frame, 6..=13 | 17..=19))
        {
            // Gameplay sprites in the source scene seed a new factory. Runtime instances
            // below reflect the current save; other authored decorative sprites remain live.
            object.extras.remove("sprite");
        }
    }
    for (index, tile) in game.tiles.iter().enumerate() {
        let x = index % WIDTH;
        let y = index / WIDTH;
        let position = [x as f32 - 127.5, 127.5 - y as f32];
        if let Some(resource) = tile.deposit {
            add_sprite(
                &mut scene,
                format!("bt-resource-{index}"),
                "Resource node",
                [position[0], position[1], 0.2],
                resource.sprite(),
                Direction::East,
                1.,
            )?;
        }
        if let Some(building) = &tile.building {
            add_sprite(
                &mut scene,
                format!("bt-building-{index}"),
                building.kind.name(),
                [position[0], position[1], 0.3],
                building.kind.sprite(),
                building.direction,
                1.,
            )?;
            add_sprite(
                &mut scene,
                format!("bt-output-{index}"),
                "Machine output",
                [position[0] + 0.27, position[1] + 0.27, 0.4],
                building.output.map_or(0, |item| item.sprite()),
                Direction::East,
                0.42,
            )?;
        }
    }
    let mut world = World::default();
    let instance = scene.spawn(&mut world)?;
    for (index, tile) in game.tiles.iter().enumerate() {
        if tile.building.as_ref().is_some_and(|b| b.output.is_none()) {
            let entity = instance
                .entity(&format!("bt-output-{index}"))
                .context("missing output sprite")?;
            world
                .get_mut::<Sprite>(entity)
                .context("missing output sprite")?
                .enabled = false;
        }
    }
    Ok((world, instance))
}

fn add_sprite(
    scene: &mut Scene,
    id: String,
    name: &str,
    position: [f32; 3],
    frame: usize,
    direction: Direction,
    size: f32,
) -> Result<()> {
    let rotation = direction_angle(direction);
    let mut object = Object {
        id,
        name: name.into(),
        transform: Transform {
            translation: position,
            rotation_degrees: [0., 0., rotation],
            ..Default::default()
        },
        ..Default::default()
    };
    registry::set(
        &mut object,
        &Sprite {
            image: "sprites".into(),
            atlas: Atlas {
                columns: 10,
                rows: 10,
            },
            frame: frame as u32,
            size: [size; 2],
            autoplay: false,
            ..Default::default()
        },
    )?;
    scene.objects.push(object);
    Ok(())
}

fn direction_angle(direction: Direction) -> f32 {
    match direction {
        Direction::East => 0.,
        Direction::North => 90.,
        Direction::West => 180.,
        Direction::South => 270.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_components_render_and_route_factory_controls() {
        let source = SceneSource::open(SceneSource::default_path()).unwrap();
        let game = source.new_game().unwrap();
        let mut stage = Stage::new(&source, &game).unwrap();
        let floor = stage.instance.entity("factory-floor").unwrap();
        assert_eq!(
            stage.world.get::<Tilemap>(floor).unwrap().dimensions,
            [64, 64]
        );
        assert!(stage.instance.entity("bt-resource-0").is_none());
        stage.canvas("hud", true).unwrap();
        for name in ["menu", "help", "map"] {
            stage.canvas(name, false).unwrap();
        }
        stage
            .text("objective-body", "Deliver the next contract")
            .unwrap();
        stage
            .sync_terrain(&game, [game.hub[0] - 18, game.hub[1] - 11])
            .unwrap();
        stage
            .camera([1320., 830.], [18., 255.], [117, 116], 32.)
            .unwrap();
        stage.conveyor_preview(Direction::North).unwrap();
        let icon = stage.instance.entity("bt-ui-tool-icon-3").unwrap();
        assert_eq!(
            stage.world.get::<Widget>(icon).unwrap().uv,
            [0., 0., 0.25, 1.]
        );
        let frame = stage.ui_frame([1320., 830.]).unwrap();
        assert_eq!(
            frame.element("bt-ui-objective-body").unwrap().text,
            "Deliver the next contract"
        );
        let tool = frame.element("bt-ui-tool-3").unwrap();
        let point = [tool.rect.min[0] + 20., tool.rect.min[1] + 20.];
        assert_eq!(frame.hit(point).unwrap().owner, "bt-ui-tool-3");
        stage
            .input([1320., 830.], Input::PointerDown(point))
            .unwrap();
        assert!(
            stage
                .input([1320., 830.], Input::PointerUp(point))
                .unwrap()
                .contains(&"tool-3")
        );
        let render = stage.render_scene([1320., 830.]).unwrap();
        assert!(
            render
                .items
                .iter()
                .any(|item| matches!(item.mesh, bozzard_render::MeshKind::Sprite(_)))
        );
        assert!(
            render
                .items
                .iter()
                .any(|item| matches!(item.mesh, bozzard_render::MeshKind::Text(_)))
        );
    }
}
