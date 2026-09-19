//! Self-contained starter projects. Creation never overwrites an existing directory.
use super::*;
use std::{fs, io::Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectTemplate {
    ThirdPerson,
    Collect2d,
}
impl ProjectTemplate {
    pub const ALL: [Self; 2] = [Self::ThirdPerson, Self::Collect2d];
    pub fn id(self) -> &'static str {
        match self {
            Self::ThirdPerson => "third-person",
            Self::Collect2d => "collect-2d",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ThirdPerson => "3D exploration",
            Self::Collect2d => "2D collect game",
        }
    }
    pub fn scene(self) -> Result<Scene> {
        Scene::from_json(match self {
            Self::ThirdPerson => include_str!("../templates/third-person.json"),
            Self::Collect2d => include_str!("../templates/collect-2d.json"),
        })
    }
}
impl std::str::FromStr for ProjectTemplate {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.id() == value)
            .with_context(|| format!("unknown template '{value}'; use third-person or collect-2d"))
    }
}

pub fn create_project(
    destination: &Path,
    name: &str,
    template: ProjectTemplate,
) -> Result<PathBuf> {
    let mut scene = template.scene()?;
    scene.name = name.into();
    if let Some(flow) = &mut scene.game_flow {
        flow.title = name.into();
    }
    let project = Project {
        version: 1,
        name: name.into(),
        start_scene: "scenes/main.json".into(),
        cook: CookTarget::Universal,
        view: match template {
            ProjectTemplate::ThirdPerson => Layer::ThreeD,
            ProjectTemplate::Collect2d => Layer::TwoD,
        },
    };
    project.validate_scene(&scene)?;
    let scene_json = scene.to_json()?;
    let manifest = serde_json::to_string_pretty(&project)?;
    // Reserve the destination exclusively; an existing file, directory or symlink is an error.
    fs::create_dir(destination).with_context(|| {
        format!(
            "create new project folder {} (must not already exist)",
            destination.display()
        )
    })?;
    let result = (|| {
        fs::create_dir(destination.join("scenes"))?;
        fs::create_dir(destination.join("scenes/assets"))?;
        let write = |relative: &str, bytes: &[u8]| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination.join(relative))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(())
        };
        write("scenes/main.json", scene_json.as_bytes())?;
        if template == ProjectTemplate::Collect2d {
            write(
                "scenes/assets/controller.rs",
                include_bytes!("../templates/controller.rs"),
            )?;
        }
        write("README.md", b"# Starter project\n\nOpen scenes/main.json in Bozzard. Press Play and Start to try the game.\nThe opening menu explains the controls. Stop restores the authored scene.\n\nUse File > Export game to create a standalone native game.\nbozzard.project.json selects the starting scene and view.\nKeep scene assets alongside the scene when moving this project.\n")?;
        write(".gitignore", b"/dist/\n/work/\n.DS_Store\n")?;
        write(MANIFEST, manifest.as_bytes())?;
        Ok(destination.join(MANIFEST))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}
