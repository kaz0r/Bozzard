use super::*;
use bozzard_assets::{AssetStore, Handle, job::Job};

pub enum Loading {
    Play(Job<bozzard_editor::PreparedPlay>),
    Bundle(Job<bozzard_project::content::PreparedPack>),
    Lods(Job<bozzard_editor::PreparedLods>),
    Export(Job<bozzard_project::PreparedExport>),
    #[cfg(feature = "factory")]
    FactoryExport(Job<PathBuf>),
    Prefab(Job<bozzard_editor::PreparedPrefab>),
    BakeGi(Job<bozzard_editor::PreparedGi>),
    Import(Job<bozzard_editor::PreparedImport>),
    Open(Job<bozzard_editor::LoadedScene>),
    OpenAdditive(Job<bozzard_editor::LoadedScene>),
    Save(Job<bozzard_editor::PreparedSave>),
}
impl Loading {
    pub fn fraction(&self) -> f32 {
        match self {
            Self::Play(job) => job.fraction(),
            Self::Bundle(job) => job.fraction(),
            Self::Lods(job) => job.fraction(),
            Self::Export(job) => job.fraction(),
            #[cfg(feature = "factory")]
            Self::FactoryExport(job) => job.fraction(),
            Self::Prefab(job) => job.fraction(),
            Self::BakeGi(job) => job.fraction(),
            Self::Import(job) => job.fraction(),
            Self::Open(job) | Self::OpenAdditive(job) => job.fraction(),
            Self::Save(job) => job.fraction(),
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Play(job) => job.label(),
            Self::Bundle(job) => job.label(),
            Self::Lods(job) => job.label(),
            Self::Prefab(job) => job.label(),
            Self::BakeGi(job) => job.label(),
            Self::Import(job) => job.label(),
            Self::Open(job) | Self::OpenAdditive(job) => job.label(),
            Self::Save(job) => job.label(),
            Self::Export(job) => job.label(),
            #[cfg(feature = "factory")]
            Self::FactoryExport(job) => job.label(),
        }
    }
    pub fn cancel(&self) {
        match self {
            Self::Play(job) => job.cancel(),
            Self::Bundle(job) => job.cancel(),
            Self::Lods(job) => job.cancel(),
            Self::Prefab(job) => job.cancel(),
            Self::BakeGi(job) => job.cancel(),
            Self::Import(job) => job.cancel(),
            Self::Open(job) | Self::OpenAdditive(job) => job.cancel(),
            Self::Save(job) => job.cancel(),
            Self::Export(job) => job.cancel(),
            #[cfg(feature = "factory")]
            Self::FactoryExport(job) => job.cancel(),
        }
    }
    pub fn cancelled(&self) -> bool {
        match self {
            Self::Play(job) => job.cancelled(),
            Self::Bundle(job) => job.cancelled(),
            Self::Lods(job) => job.cancelled(),
            Self::Prefab(job) => job.cancelled(),
            Self::BakeGi(job) => job.cancelled(),
            Self::Import(job) => job.cancelled(),
            Self::Open(job) | Self::OpenAdditive(job) => job.cancelled(),
            Self::Save(job) => job.cancelled(),
            Self::Export(job) => job.cancelled(),
            #[cfg(feature = "factory")]
            Self::FactoryExport(job) => job.cancelled(),
        }
    }
}
pub struct Refresh {
    pub owner: bozzard_editor::SceneId,
    pub workspace: u64,
    pub revision: u64,
    pub job: Job<(AssetStore, Vec<Handle>)>,
}

impl App {
    pub fn start_play(&mut self) {
        let result = (|| {
            ensure!(
                self.loading.is_none(),
                "Wait for the current operation first"
            );
            self.drag = None;
            self.gameplay_controls.reset();
            self.loading = Some(Loading::Play(self.editor.play_job()?));
            Ok(())
        })();
        self.result(result);
    }
    pub fn start_export(
        &mut self,
        destination: PathBuf,
        name: String,
        cook: bozzard_project::CookTarget,
    ) -> bool {
        let result = (|| {
            ensure!(
                self.loading.is_none(),
                "Wait for the current operation first"
            );
            self.editor.finish_gesture();
            self.drag = None;
            let scene = self.editor.scene().clone();
            let source = self.editor.path.clone();
            #[cfg(feature = "factory")]
            if self.factory_mode {
                let binary = bozz_torio::package::companion_factory(&std::env::current_exe()?)?;
                self.loading = Some(Loading::FactoryExport(Job::start(
                    "Exporting Bozz-torio",
                    move |progress| {
                        progress.report(0, 1, "Copying native factory runtime and scene")?;
                        let folder = bozz_torio::package::export_scene_with_progress(
                            &scene,
                            &source,
                            &binary,
                            &destination,
                            &progress,
                        )?;
                        Ok(folder)
                    },
                )?));
                return Ok(());
            }
            let project = bozzard_project::Project {
                version: 1,
                name,
                start_scene: "scene.json".into(),
                runtime_modules: Vec::new(),
                cook,
                view: if scene.views.contains_key(&Layer::ThreeD) {
                    Layer::ThreeD
                } else {
                    Layer::TwoD
                },
            };
            let player = bozzard_project::companion_player(&std::env::current_exe()?)?;
            self.loading = Some(Loading::Export(Job::start(
                "Preparing game export",
                move |progress| {
                    bozzard_project::prepare_export(
                        &project,
                        &scene,
                        &source,
                        &player,
                        &destination,
                        &progress,
                    )
                },
            )?));
            Ok(())
        })();
        let started = result.is_ok();
        self.result(result);
        started
    }

    pub fn start_prefab(&mut self, command: bozzard_editor::PrefabCommand) {
        let result = (|| {
            ensure!(
                self.loading.is_none(),
                "Wait for the current operation first"
            );
            self.drag = None;
            self.loading = Some(Loading::Prefab(self.editor.prefab_job(command)?));
            Ok(())
        })();
        self.result(result);
    }

    pub fn start_import(&mut self, path: PathBuf) {
        let result = (|| {
            if let Some(Loading::Import(job)) = &self.loading {
                ensure!(!job.cancelled(), "Wait for cancellation to finish");
                ensure!(
                    self.import_queue.len() < 31,
                    "Import queue is full (32 files); try remaining files after loading finishes"
                );
                self.import_queue.push_back(path);
                return Ok(());
            }
            ensure!(
                self.loading.is_none(),
                "Wait for the current import or cancel it first"
            );
            self.editor.finish_gesture();
            self.drag = None;
            self.loading = Some(Loading::Import(self.editor.import_job(path)?));
            self.workspace.assets_visible = true;
            Ok(())
        })();
        self.result(result);
    }
    pub fn poll_loading(&mut self) {
        let cancelled = self.loading.as_ref().is_some_and(Loading::cancelled);
        let completion = match self.loading.as_ref() {
            Some(Loading::Play(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    self.editor.accept_play(prepared)?;
                    #[cfg(feature = "factory")]
                    if self.factory_mode {
                        let module = match bozz_torio::module::FactoryModule::from_scene(
                            self.editor.scene(), &self.editor.path,
                        ) {
                            Ok(module) => module,
                            Err(error) => {
                                self.stop_play();
                                return Err(error);
                            }
                        };
                        if let Err(error) = self.editor.play.as_mut().unwrap().app
                            .install_modules(vec![Box::new(module.clone())]) {
                            self.stop_play();
                            return Err(error.into());
                        }
                        self.factory_module = Some(module);
                    }
                    if let Some(id) = self.pending_lobby.take()
                        && let Err(error) = self.editor.play.as_mut().unwrap().join_multiplayer(id) {
                        self.stop_play();
                        return Err(error);
                    }
                    self.status = "Play started".into();
                    Ok(())
                })
            }),
            Some(Loading::Bundle(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let report = prepared.report();
                    let path = prepared.commit()?;
                    self.status = format!("Content pack ready: {}. Cooked {}, reused {} cached assets.", path.display(), report.built, report.reused);
                    Ok(())
                })
            }),
            Some(Loading::Lods(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let levels = self.editor.accept_lods(prepared)?;
                    self.status = format!(
                        "Generated LODs: {}. Save the scene to keep them.",
                        levels
                            .iter()
                            .map(|level| format!(
                                "{} → {} triangles",
                                level.source_triangles, level.triangles
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    self.workspace.assets_visible = true;
                    Ok(())
                })
            }),
            Some(Loading::Export(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let report = prepared.report();
                    let folder = prepared.commit()?;
                    self.status =
                        format!("Game exported to {}. Cooked {}, reused {} cached assets. Open Game to play.", folder.display(), report.built, report.reused);
                    self.dialog = Some(files::Dialog::new(files::Kind::Exported, &folder));
                    Ok(())
                })
            }),
            #[cfg(feature = "factory")]
            Some(Loading::FactoryExport(job)) => job.poll().map(|result| {
                result.map(|folder| {
                    self.status = format!("Bozz-torio exported to {}. Open Game to play.", folder.display());
                    self.dialog = Some(files::Dialog::new(files::Kind::Exported, &folder));
                })
            }),
            Some(Loading::Prefab(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let label = prepared.label.clone();
                    let layer = prepared.layer;
                    let asset = self.editor.accept_prefab(prepared)?;
                    if let Some(layer) = layer {
                        self.workspace.layer_2d = layer == Layer::TwoD;
                    }
                    self.status = format!("{label}: {asset}. Save the scene to keep its links.");
                    self.asset_browser.reveal(asset);
                    self.workspace.assets_visible = true;
                    self.hierarchy_search.clear();
                    Ok(())
                })
            }),
            Some(Loading::BakeGi(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    self.editor.accept_gi(prepared)?;
                    self.status = "Global illumination baked. Save the scene to keep it.".into();
                    Ok(())
                })
            }),
            Some(Loading::Save(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    self.editor.accept_save(prepared)?;
                    self.status = format!("Saved {}", self.editor.path.display());
                    Ok(())
                })
            }),
            Some(Loading::Import(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let id = self.editor.accept_import(prepared)?;
                    self.status = format!("Imported {id}");
                    self.asset_browser.reveal(id);
                    Ok(())
                })
            }),
            Some(Loading::Open(job)) => job.poll().map(|result| {
                result.and_then(|loaded| {
                    let incoming = loaded.into_editor();
                    if let Some(id) = self.open_scenes.find_path(&self.editor, &incoming.path)
                        && id != self.open_scenes.active() {
                        self.open_scenes.activate(&mut self.editor, id)?;
                    } else {
                        self.open_scenes.replace(&mut self.editor, incoming)?;
                    }
                    self.scene_activated(false);
                    self.status = format!("Opened {}", self.editor.path.display());
                    Ok(())
                })
            }),
            Some(Loading::OpenAdditive(job)) => job.poll().map(|result| {
                result.and_then(|loaded| {
                    self.open_scenes.add(&mut self.editor, loaded.into_editor())?;
                    self.scene_activated(true);
                    self.status = format!("Opened {} scenes · Editing {}", self.open_scenes.len(), self.editor.scene().name);
                    Ok(())
                })
            }),
            None => None,
        };
        if let Some(result) = completion {
            self.loading = None;
            let success = result.is_ok();
            self.result(result);
            if cancelled {
                self.status = "Loading cancelled".into();
                self.error = false;
            }
            if !success {
                self.import_queue.clear();
            }
            if self.continue_after_save {
                self.continue_after_save = false;
                if success {
                    self.perform_pending();
                } else {
                    self.confirm_discard = true;
                }
            }
            if self.close_after_loading {
                self.close_after_loading = false;
                self.request(Pending::Close);
            } else if self.loading.is_none()
                && let Some(path) = self.import_queue.pop_front()
            {
                self.start_import(path);
            }
        }
    }
}
