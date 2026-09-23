//! The Bozzard editor scene is the authored source for every new factory.
use crate::sim::{
    Building, Direction, Game, Kind, PATCH_HEIGHT, PATCH_WIDTH, PATCH_X, PATCH_Y, Resource, WIDTH,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct SceneSource {
    pub path: PathBuf,
    pub name: String,
    terrain: Vec<u8>,
    deposits: Vec<(usize, usize, Resource)>,
    machines: Vec<(usize, usize, Kind, Direction)>,
    hub: [usize; 2],
    inventory: Vec<u16>,
    stock: [u16; 6],
    credits: u32,
    first_order_amount: u32,
    first_order_reward: u32,
    world_seed: u64,
}

impl SceneSource {
    pub fn default_path() -> PathBuf {
        let packaged = std::env::current_exe().ok().and_then(|exe| {
            exe.parent()
                .map(|parent| parent.join("scene/bozz-torio.json"))
        });
        packaged
            .filter(|path| path.is_file())
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("scene/bozz-torio.json"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("opening Bozz-torio editor scene {}", path.display()))?;
        Self::parse(path, &text)
    }

    fn parse(path: PathBuf, text: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(text).context("reading Bozzard scene JSON")?;
        ensure!(
            value["version"].as_u64() == Some(1),
            "Bozz-torio needs a Bozzard version 1 scene"
        );
        ensure!(
            value["views"]["2d"].is_string(),
            "Bozz-torio scene needs a 2D camera"
        );
        let name = value["name"]
            .as_str()
            .context("scene has no name")?
            .to_owned();
        let atlas = value["assets"]["sprites"]["path"]
            .as_str()
            .context("scene needs a sprites image asset")?;
        ensure!(
            value["assets"]["sprites"]["kind"] == "image",
            "sprites asset must be an image"
        );
        let atlas_path = path
            .parent()
            .context("scene path has no directory")?
            .join(atlas);
        ensure!(
            atlas_path.is_file(),
            "scene sprite atlas is missing: {}",
            atlas_path.display()
        );
        let objects = value["objects"]
            .as_array()
            .context("scene has no objects")?;
        let mut floor = None;
        let mut deposits = Vec::new();
        let mut machines = Vec::new();
        let mut hub = None;
        for object in objects {
            if object.get("tilemap").is_some() {
                ensure!(floor.is_none(), "Bozz-torio supports one factory tilemap");
                let map = &object["tilemap"];
                ensure!(
                    map["image"] == "sprites",
                    "factory tilemap must use the sprites asset"
                );
                ensure!(
                    map["dimensions"] == serde_json::json!([PATCH_WIDTH, PATCH_HEIGHT]),
                    "factory starter tilemap must remain {PATCH_WIDTH} × {PATCH_HEIGHT} cells"
                );
                let cells = map["cells"]
                    .as_array()
                    .context("factory tilemap has no cells")?;
                ensure!(
                    cells.len() == PATCH_WIDTH * PATCH_HEIGHT,
                    "factory tilemap needs {} cells",
                    PATCH_WIDTH * PATCH_HEIGHT
                );
                let mut terrain = Vec::with_capacity(cells.len());
                for cell in cells {
                    let frame = cell.as_u64().context("tilemap cell must be an integer")?;
                    ensure!(
                        frame <= 100,
                        "tilemap cell must use the 10 × 10 sprite atlas"
                    );
                    terrain.push(if frame == 0 { 255 } else { (frame - 1) as u8 });
                }
                floor = Some(terrain);
            }
            let Some(sprite) = object.get("sprite") else {
                continue;
            };
            if sprite["image"] != "sprites" {
                continue;
            }
            let Some(frame) = sprite["frame"].as_u64() else {
                continue;
            };
            if !(6..=13).contains(&frame) && !(17..=19).contains(&frame) {
                continue;
            }
            let location = grid_location(object)?;
            let Some((x, y)) = location else {
                // Machine templates live below the board; move or duplicate them onto it.
                ensure!(
                    (6..=10).contains(&frame) || (18..=19).contains(&frame),
                    "{} lies outside the factory floor",
                    object["name"]
                );
                continue;
            };
            match frame {
                6..=10 | 18..=19 => {
                    let kind = match frame {
                        18 => Kind::Generator,
                        19 => Kind::PowerPole,
                        _ => Kind::BUILDABLE[(frame - 6) as usize],
                    };
                    let rotation = object["transform"]["rotation_degrees"][2]
                        .as_f64()
                        .unwrap_or(0.0);
                    ensure!(rotation.is_finite(), "machine rotation must be finite");
                    let quarter = ((rotation / 90.0).round() as i32).rem_euclid(4);
                    let direction = match quarter {
                        0 => Direction::East,
                        1 => Direction::North,
                        2 => Direction::West,
                        _ => Direction::South,
                    };
                    machines.push((x, y, kind, direction));
                }
                11 => {
                    ensure!(
                        hub.replace([x, y]).is_none(),
                        "scene has more than one delivery hub"
                    );
                }
                12 => deposits.push((x, y, Resource::IronOre)),
                13 => deposits.push((x, y, Resource::CopperOre)),
                17 => deposits.push((x, y, Resource::Coal)),
                _ => unreachable!(),
            }
        }
        let terrain = floor.context("scene has no factory tilemap")?;
        let hub = hub.context("scene has no delivery hub sprite")?;
        let defaults = Game::new();
        let board = &value["blackboard"];
        let mut inventory = defaults.buildings;
        for (i, key) in [
            "starter_miners",
            "starter_furnaces",
            "starter_assemblers",
            "starter_conveyors",
            "starter_splitters",
            "starter_generators",
            "starter_power_poles",
        ]
        .into_iter()
        .enumerate()
        {
            inventory[i] = setting(board, key, inventory[i] as u64, u16::MAX as u64)? as u16;
        }
        let mut stock = defaults.stock;
        for (i, key) in [
            "starter_iron_ore",
            "starter_copper_ore",
            "starter_iron_ingots",
            "starter_copper_ingots",
            "starter_gears",
            "starter_circuits",
        ]
        .into_iter()
        .enumerate()
        {
            stock[i] = setting(board, key, stock[i] as u64, u16::MAX as u64)? as u16;
        }
        let credits = setting(
            board,
            "starter_credits",
            defaults.credits as u64,
            u32::MAX as u64,
        )? as u32;
        let first_order_amount = setting(
            board,
            "first_order_ingots",
            defaults.first_order_amount as u64,
            u32::MAX as u64,
        )? as u32;
        ensure!(
            first_order_amount > 0,
            "first_order_ingots must be positive"
        );
        let first_order_reward = setting(
            board,
            "first_order_reward",
            defaults.first_order_reward as u64,
            u32::MAX as u64,
        )? as u32;
        let world_seed = setting(board, "world_seed", 0, u32::MAX as u64)?;
        let source = Self {
            path,
            name,
            terrain,
            deposits,
            machines,
            hub,
            inventory,
            stock,
            credits,
            first_order_amount,
            first_order_reward,
            world_seed,
        };
        source.new_game()?;
        Ok(source)
    }

    pub fn new_game(&self) -> Result<Game> {
        let seed = if self.world_seed == 0 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos() as u64
        } else {
            self.world_seed
        };
        let mut game = Game::from_seed(seed);
        for y in 0..PATCH_HEIGHT {
            for x in 0..PATCH_WIDTH {
                let index = (PATCH_Y + y) * WIDTH + PATCH_X + x;
                game.terrain[index] = self.terrain[y * PATCH_WIDTH + x];
                game.tiles[index].deposit = None;
            }
        }
        let old_hub = game.hub[1] * WIDTH + game.hub[0];
        game.tiles[old_hub].building = None;
        game.hub = self.hub;
        game.buildings = self.inventory.clone();
        game.stock = self.stock;
        game.credits = self.credits;
        game.first_order_amount = self.first_order_amount;
        game.first_order_reward = self.first_order_reward;
        for &(x, y, item) in &self.deposits {
            let tile = &mut game.tiles[y * WIDTH + x];
            ensure!(
                tile.deposit.is_none(),
                "two ore deposits overlap at {x}:{y}"
            );
            tile.deposit = Some(item);
        }
        let index = self.hub[1] * WIDTH + self.hub[0];
        ensure!(
            game.tiles[index].deposit.is_none(),
            "delivery hub overlaps an ore deposit"
        );
        game.tiles[index].building = Some(Building::new(Kind::Hub, Direction::West));
        for &(x, y, kind, direction) in &self.machines {
            let tile = &mut game.tiles[y * WIDTH + x];
            ensure!(tile.building.is_none(), "buildings overlap at {x}:{y}");
            ensure!(
                kind != Kind::Miner || tile.deposit.is_some_and(|r| r.ore().is_some()),
                "authored miner at {x}:{y} needs an ore deposit"
            );
            ensure!(
                kind != Kind::Generator || tile.deposit == Some(Resource::Coal),
                "authored generator at {x}:{y} needs a coal seam"
            );
            tile.building = Some(Building::new(kind, direction));
            game.placed += 1;
        }
        game.notice = format!(
            "{} loaded. Build from the starter inventory or edit the scene in Bozzard.",
            self.name
        );
        Ok(game)
    }
}

fn setting(board: &Value, key: &str, default: u64, max: u64) -> Result<u64> {
    let Some(value) = board.get(key) else {
        return Ok(default);
    };
    let number = value["scalar"]["number"]
        .as_f64()
        .with_context(|| format!("{key} must be a numeric scene blackboard value"))?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > max as f64 {
        bail!("{key} must be a whole number from 0 to {max}");
    }
    Ok(number as u64)
}

fn grid_location(object: &Value) -> Result<Option<(usize, usize)>> {
    let transform = &object["transform"]["translation"];
    let world_x = transform[0].as_f64().context("sprite has no X position")?;
    let world_y = transform[1].as_f64().context("sprite has no Y position")?;
    ensure!(
        world_x.is_finite() && world_y.is_finite(),
        "sprite position must be finite"
    );
    let x = (world_x + 10.5).round() as isize;
    let y = (7.0 - world_y).round() as isize;
    if x < 0 || y < 0 || x >= PATCH_WIDTH as isize || y >= PATCH_HEIGHT as isize {
        Ok(None)
    } else {
        Ok(Some((PATCH_X + x as usize, PATCH_Y + y as usize)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_editor_scene_drives_new_factory() {
        let path = SceneSource::default_path();
        let source = SceneSource::open(path.clone()).unwrap();
        let game = source.new_game().unwrap();
        assert_eq!(source.name, "Bozz-torio — Factory Floor");
        assert_eq!(game.hub, [PATCH_X + 18, PATCH_Y + 7]);
        assert_eq!(game.buildings, [2, 2, 2, 24, 2, 0, 0, 0]);
        assert_eq!(
            game.tiles[(PATCH_Y + 7) * WIDTH + PATCH_X + 4].deposit,
            Some(Resource::IronOre)
        );
        assert_eq!(game.terrain[PATCH_Y * WIDTH + PATCH_X], 14);
        assert!(path.is_file());
    }

    #[test]
    fn moved_hub_and_prebuilt_machine_in_scene_change_playable_layout() {
        let path = SceneSource::default_path();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["objects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|o| o["id"] == "delivery-hub")
            .unwrap()["transform"]["translation"] = serde_json::json!([8.5, 0, 0.3]);
        value["objects"].as_array_mut().unwrap().push(serde_json::json!({"id":"placed-furnace","name":"Placed furnace","transform":{"translation":[-5.5,0,0.2],"rotation_degrees":[0,0,90],"scale":[1,1,1]},"sprite":{"image":"sprites","frame":7}}));
        value["blackboard"]["starter_conveyors"]["scalar"]["number"] = serde_json::json!(30);
        value["blackboard"]["world_seed"]["scalar"]["number"] = serde_json::json!(42);
        let source = SceneSource::parse(path, &value.to_string()).unwrap();
        let game = source.new_game().unwrap();
        assert_eq!(game.hub, [PATCH_X + 19, PATCH_Y + 7]);
        assert_eq!(game.seed, 42);
        assert_eq!(game.buildings[Kind::Belt.index()], 30);
        let furnace = game.tiles[(PATCH_Y + 7) * WIDTH + PATCH_X + 5]
            .building
            .as_ref()
            .unwrap();
        assert_eq!(furnace.kind, Kind::Furnace);
        assert_eq!(furnace.direction, Direction::North);
    }
}
