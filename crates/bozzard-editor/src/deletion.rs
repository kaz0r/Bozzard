//! Project deletion removes a source and its scene usages; Undo restores both.
use super::*;

// Same-filesystem move without overwriting a file created since deletion/Undo.
pub(super) fn move_file(from: &Path, to: &Path) -> Result<()> {
    ensure!(
        std::fs::symlink_metadata(from)?.is_file(),
        "source is no longer a regular file"
    );
    std::fs::hard_link(from, to).with_context(|| {
        format!(
            "cannot move {} to {} (destination must be unused)",
            from.display(),
            to.display()
        )
    })?;
    if let Err(error) = std::fs::remove_file(from) {
        let _ = std::fs::remove_file(to);
        return Err(error.into());
    }
    Ok(())
}

impl Editor {
    pub fn delete_project_asset(&mut self, id: &str) -> Result<usize> {
        let source = self
            .scene
            .assets
            .get(id)
            .context("asset no longer exists")?;
        let path = root(&self.path).join(&source.path);
        self.delete_project_file(&path)
    }

    /// Standalone Blueprint files contain independent copies, not live attachment links.
    pub fn delete_project_file(&mut self, path: &Path) -> Result<usize> {
        ensure!(
            self.play.is_none(),
            "Stop Play before deleting project assets"
        );
        let project = root(&self.path).canonicalize()?;
        ensure!(
            std::fs::symlink_metadata(path)?.is_file(),
            "only regular project files can be deleted"
        );
        let source = path.canonicalize()?;
        ensure!(
            source.starts_with(project.join("assets")),
            "External source files are protected. Copy/import this file into the scene's assets folder first; Remove from scene library keeps an external source intact."
        );
        let ids: BTreeSet<_> = self
            .scene
            .assets
            .iter()
            .filter(|(_, a)| {
                root(&self.path).join(&a.path).canonicalize().ok().as_ref() == Some(&source)
            })
            .map(|(id, _)| id.clone())
            .collect();
        ensure!(
            !ids.is_empty()
                || source
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".blueprint.json")),
            "file is not a project asset or Blueprint"
        );
        let users = self.scene.asset_users();
        let mut objects = BTreeSet::new();
        for id in &ids {
            for user in &users[id] {
                objects.extend(subtree(&self.scene, user));
            }
        }
        ensure!(
            !self.scene.views.values().any(|id| objects.contains(id)),
            "Deletion includes an active camera; assign another camera first"
        );
        let mut scene = self.scene.clone();
        scene.objects.retain(|o| !objects.contains(&o.id));
        scene.prefabs.retain(|id, _| !objects.contains(id));
        scene.assets.retain(|id, _| !ids.contains(id));
        scene.validate().context("Asset deletion would leave protected references or a partial prefab; clear references or unpack first")?;
        let mut assets = self.cached_assets(&scene, &self.path)?;
        // Refresh observed dependencies before checking shared source-file ownership.
        assets.refresh();
        for entry in assets.entries() {
            ensure!(
                !entry
                    .source_dependencies()
                    .any(|p| p.canonicalize().ok().as_ref() == Some(&source)),
                "File is also a dependency of '{}'; remove that asset first",
                entry.id
            );
            if let Some(AssetData::Prefab(prefab)) = entry.data() {
                let directory = root(&self.path).join(&scene.assets[&entry.id].path);
                ensure!(
                    !prefab.assets.values().any(|a| directory
                        .parent()
                        .unwrap()
                        .join(&a.path)
                        .canonicalize()
                        .ok()
                        .as_ref()
                        == Some(&source)),
                    "File is referenced by prefab '{}'; update that source first",
                    entry.id
                );
            }
        }
        let trash = project.join(".bozzard-trash");
        std::fs::create_dir_all(&trash)?;
        ensure!(
            !std::fs::symlink_metadata(&trash)?.file_type().is_symlink(),
            "project trash must not be a symlink"
        );
        let mut index = 1_u64;
        let destination = loop {
            let candidate = trash.join(format!(
                "{index}-{}",
                source.file_name().unwrap().to_string_lossy()
            ));
            if !candidate.try_exists()? {
                break candidate;
            }
            index = index.checked_add(1).context("trash name space exhausted")?;
        };
        self.finish_gesture();
        move_file(&source, &destination)?;
        self.record(Change {
            label: "Delete project asset".into(),
            scene: self.scene.clone(),
            assets: Some(self.assets.clone()),
            restore_file: Some((destination, source)),
        });
        self.scene = scene;
        self.assets = assets;
        self.revision += 1;
        self.asset_revision += 1;
        self.repair_selection();
        Ok(objects.len())
    }
}
