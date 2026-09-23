use crate::sim::{Game, HEIGHT, WIDTH};
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
        let game: Game = serde_json::from_slice(&fs::read(&self.path)?)?;
        anyhow::ensure!(
            game.tiles.len() == WIDTH * HEIGHT,
            "Save has an invalid board size"
        );
        anyhow::ensure!(
            game.tiles[7 * WIDTH + 18]
                .building
                .as_ref()
                .is_some_and(|b| b.kind == crate::sim::Kind::Hub),
            "Save is missing its hub"
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
        fs::write(&temporary, serde_json::to_vec_pretty(game)?)?;
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
}
