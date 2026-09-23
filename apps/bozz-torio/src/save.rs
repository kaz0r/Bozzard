use crate::sim::{Game, HEIGHT, PATCH_HEIGHT, PATCH_WIDTH, PATCH_X, PATCH_Y, WIDTH};
use std::{fs, path::PathBuf};

pub struct SaveFile {
    path: PathBuf,
}

impl SaveFile {
    pub fn default_path() -> Self {
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            path: root.join("bozz-torio").join("factory.json"),
        }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load(&self) -> anyhow::Result<Game> {
        let mut game: Game = serde_json::from_slice(&fs::read(&self.path)?)?;
        if game.tiles.len() == PATCH_WIDTH * PATCH_HEIGHT {
            game.seed = game.seed.max(1);
            let old_tiles = std::mem::take(&mut game.tiles);
            let old_terrain = std::mem::take(&mut game.terrain);
            let expanded = Game::from_seed(game.seed);
            game.tiles = expanded.tiles;
            game.terrain = expanded.terrain;
            for y in 0..PATCH_HEIGHT {
                for x in 0..PATCH_WIDTH {
                    let old = y * PATCH_WIDTH + x;
                    let new = (PATCH_Y + y) * WIDTH + PATCH_X + x;
                    game.tiles[new] = old_tiles[old].clone();
                    if old_terrain.len() == PATCH_WIDTH * PATCH_HEIGHT {
                        game.terrain[new] = old_terrain[old];
                    }
                }
            }
            game.hub = [PATCH_X + game.hub[0], PATCH_Y + game.hub[1]];
            game.notice = "Your previous factory was moved into the larger world.".into();
        }
        game.buildings.resize(8, 0);
        anyhow::ensure!(
            game.tiles.len() == WIDTH * HEIGHT,
            "Save has an invalid board size"
        );
        anyhow::ensure!(
            game.terrain.len() == WIDTH * HEIGHT,
            "Save has invalid floor tiles"
        );
        let hub = Game::index(game.hub[0], game.hub[1])
            .ok_or_else(|| anyhow::anyhow!("Save hub lies outside the board"))?;
        anyhow::ensure!(
            game.tiles[hub]
                .building
                .as_ref()
                .is_some_and(|b| b.kind == crate::sim::Kind::Hub),
            "Save is missing its hub"
        );
        anyhow::ensure!(
            game.first_order_amount > 0,
            "Save has an invalid first order"
        );
        Ok(game)
    }

    pub fn archive_invalid(&self) -> anyhow::Result<PathBuf> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let archive = self.path.with_extension(format!("invalid-{stamp}.json"));
        fs::rename(&self.path, &archive)?;
        Ok(archive)
    }

    pub fn write(&self, game: &Game) -> anyhow::Result<()> {
        let parent = self.path.parent().expect("save path has a parent");
        fs::create_dir_all(parent)?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec(game)?)?;
        fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_save_is_archived_without_losing_its_contents() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("bozz-torio-invalid-{unique}.json"));
        let save = SaveFile { path };
        fs::write(&save.path, b"old, invalid data").unwrap();
        assert!(save.load().is_err());
        let archive = save.archive_invalid().unwrap();
        assert!(!save.exists());
        assert_eq!(fs::read(&archive).unwrap(), b"old, invalid data");
        fs::remove_file(archive).unwrap();
    }

    #[test]
    fn old_factory_save_is_moved_into_the_large_world() {
        let game = Game::new();
        let mut value = serde_json::to_value(&game).unwrap();
        let tiles = (0..PATCH_HEIGHT)
            .flat_map(|y| (0..PATCH_WIDTH).map(move |x| (PATCH_Y + y) * WIDTH + PATCH_X + x))
            .map(|index| serde_json::to_value(&game.tiles[index]).unwrap())
            .collect();
        let source_terrain = &game.terrain;
        let terrain = (0..PATCH_HEIGHT)
            .flat_map(|y| {
                (0..PATCH_WIDTH).map(move |x| source_terrain[(PATCH_Y + y) * WIDTH + PATCH_X + x])
            })
            .collect::<Vec<_>>();
        value["tiles"] = serde_json::Value::Array(tiles);
        value["terrain"] = serde_json::to_value(terrain).unwrap();
        value["hub"] = serde_json::json!([18, 7]);
        value["buildings"] = serde_json::json!([2, 2, 2, 24, 2]);
        let path =
            std::env::temp_dir().join(format!("bozz-torio-old-save-{}.json", std::process::id()));
        let save = SaveFile { path };
        fs::write(&save.path, serde_json::to_vec(&value).unwrap()).unwrap();
        let loaded = save.load().unwrap();
        assert_eq!(loaded.tiles.len(), WIDTH * HEIGHT);
        assert_eq!(loaded.hub, [PATCH_X + 18, PATCH_Y + 7]);
        assert_eq!(loaded.buildings.len(), 8);
        fs::remove_file(&save.path).unwrap();
    }
}
