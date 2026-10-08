//! Kennel, Bozzard's package store, as an editor pane. It browses a registry, checks each
//! package against this editor build, installs packages into the active scene's project, and
//! adds their scripts and assets to the scene catalog. Store work runs on the pane's own
//! background job, so editing continues while a package downloads.
use super::*;
use anyhow::bail;
use bozzard_assets::job::{Job, Progress};
use bozzard_project::kennel::{
    self, Category, IndexEntry, InstallOptions, Installed, Lockfile, Manifest, Registry,
};
use bozzard_scene::AssetSource;
use eframe::egui::RichText;
use std::collections::BTreeMap;
mod readme;

/// The pane's one background operation.
enum Task {
    Open,
    Details(String),
    Install { name: String, force: bool },
    Remove { name: String, force: bool },
    Verify,
}

enum Done {
    Opened(Registry),
    Details(Box<Details>),
    Installed(Vec<Installed>),
    Removed,
    Verified(Vec<(String, String, usize)>),
}

struct Running {
    task: Task,
    job: Job<Done>,
}

struct Details {
    manifest: Manifest,
    /// The package README, read and hash-checked, or why it couldn't be.
    readme: Option<Result<String, String>>,
}

/// A forced retry offered after an install or removal refused a modified installation.
#[derive(Clone, Debug, PartialEq)]
enum Repair {
    Install(String),
    Remove(String),
}

struct Notice {
    text: String,
    error: bool,
    repair: Option<Repair>,
    /// Build variables the last install reported, as absolute folders.
    build_env: Vec<(String, PathBuf)>,
}
impl Notice {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
            repair: None,
            build_env: Vec::new(),
        }
    }
}

/// The project that holds the active scene, and what it has installed.
struct Installation {
    root: PathBuf,
    /// The scene's folder; catalog paths are relative to it.
    scene_dir: PathBuf,
    lockfile: Lockfile,
    /// Installed manifest copies; `None` when one is missing or unreadable.
    installed: BTreeMap<String, Option<Manifest>>,
}

impl Installation {
    fn read(scene: &Path) -> Result<Self> {
        let folder = bozzard_editor::root(scene);
        let scene_dir = folder
            .canonicalize()
            .with_context(|| format!("reading {}", folder.display()))?;
        let project = scene_dir
            .ancestors()
            .find(|folder| folder.join(bozzard_project::MANIFEST).is_file())
            .context(
                "This scene isn't inside a Bozzard project, so packages can't be installed. \
                 Save it in a project folder, or create one with File → New project…",
            )?;
        let root = kennel::project_root(project)?;
        let lockfile = Lockfile::load(&root)?;
        let installed = lockfile
            .packages
            .keys()
            .map(|name| {
                let path = root
                    .join(kennel::INSTALL_DIR)
                    .join(name)
                    .join(Manifest::file_name(name));
                let manifest = std::fs::read(path)
                    .ok()
                    .and_then(|bytes| Manifest::parse(&bytes).ok());
                (name.clone(), manifest)
            })
            .collect();
        Ok(Self {
            root,
            scene_dir,
            lockfile,
            installed,
        })
    }

    /// Installed packages that depend on `name`.
    fn dependents(&self, name: &str) -> Vec<&str> {
        self.installed
            .iter()
            .filter(|(_, manifest)| {
                manifest
                    .as_ref()
                    .is_some_and(|m| m.dependencies.contains_key(name))
            })
            .map(|(other, _)| other.as_str())
            .collect()
    }

    /// The `kennel/` folder relative to the scene's folder, and the catalog entries for
    /// `name` and the packages it depends on, with paths relative to the scene's folder.
    fn catalog(&self, name: &str) -> Result<(String, BTreeMap<String, AssetSource>)> {
        let base = relative_path(&self.scene_dir, &self.root.join(kennel::INSTALL_DIR))?;
        let mut entries = BTreeMap::new();
        let mut pending = vec![name.to_owned()];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let manifest = self
                .installed
                .get(&name)
                .and_then(Option::as_ref)
                .with_context(|| format!("{name} isn't installed in this project"))?;
            pending.extend(manifest.dependencies.keys().cloned());
            let path = |file: &str| format!("{base}/{name}/{file}");
            for script in &manifest.scripts {
                let source = AssetSource {
                    kind: AssetKind::Script,
                    path: path(&script.path),
                };
                entries.insert(script.id.clone(), source);
            }
            for asset in &manifest.assets {
                let source = AssetSource {
                    kind: asset.kind,
                    path: path(&asset.path),
                };
                entries.insert(asset.id.clone(), source);
            }
        }
        Ok((base, entries))
    }

    /// Whether the scene's catalog already lists every entry [`Self::catalog`] would add.
    fn in_scene(&self, scene: &bozzard_scene::Scene, name: &str) -> bool {
        self.catalog(name).is_ok_and(|(_, entries)| {
            entries
                .iter()
                .all(|(id, source)| scene.assets.get(id) == Some(source))
        })
    }
}

/// `to` relative to the folder `from`, with `/` separators. Both paths are canonical.
fn relative_path(from: &Path, to: &Path) -> Result<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    ensure!(
        from.first() == to.first(),
        "the scene and its project are on different drives"
    );
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let parts: Vec<String> = std::iter::repeat_n("..".to_owned(), from.len() - common)
        .chain(
            to[common..]
                .iter()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect();
    Ok(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
}

/// Whether this editor build has an engine Cargo feature, when the editor can tell.
fn feature_built(feature: &str) -> Option<bool> {
    match feature {
        "steam" => Some(cfg!(feature = "steam")),
        _ => None,
    }
}

fn category_label(category: Category) -> &'static str {
    match category {
        Category::Integration => "Integration",
        Category::Scripts => "Scripts",
        Category::Art => "Art",
        Category::Audio => "Audio",
        Category::Template => "Template",
        Category::Tool => "Tool",
    }
}

fn category_color(category: Category) -> Color32 {
    match category {
        Category::Integration => theme::SKY,
        Category::Scripts => theme::GREEN,
        Category::Art => theme::CORAL,
        Category::Audio => theme::AMBER,
        Category::Template => theme::ACCENT,
        Category::Tool => Color32::from_gray(190),
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / 1_048_576.0),
    }
}

/// The host part of a URL, for "downloaded from" notes.
fn url_host(url: &str) -> &str {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest.split('/').next().unwrap_or(rest))
}

fn installed_message(installed: &[Installed]) -> String {
    let Some((package, dependencies)) = installed.split_last() else {
        return "Nothing was installed.".into();
    };
    let mut text = if package.unchanged {
        format!(
            "{} {} is already installed and intact.",
            package.name, package.version
        )
    } else {
        format!(
            "Installed {} {} into {} ({} files).",
            package.name,
            package.version,
            package.directory.display(),
            package.files
        )
    };
    if !dependencies.is_empty() {
        let list: Vec<_> = dependencies
            .iter()
            .map(|p| format!("{} {}", p.name, p.version))
            .collect();
        text += &format!(" With dependencies {}.", list.join(", "));
    }
    if !package.features.is_empty() {
        text += &format!(
            " Requires the engine features {}.",
            package.features.join(", ")
        );
    }
    text
}

enum Action {
    Install { force: bool },
    Remove,
    AddToScene,
}

#[derive(Default)]
pub struct Store {
    /// The registry folder or URL; empty means `BOZZARD_KENNEL_REGISTRY` or the public registry.
    pub location: String,
    registry: Option<Arc<Registry>>,
    opened: bool,
    search: String,
    category: Option<Category>,
    installed_only: bool,
    all_targets: bool,
    selected: Option<String>,
    details: BTreeMap<String, Result<Details, String>>,
    /// The scene path the installation was read for, and what was read.
    project: Option<(PathBuf, Result<Installation, String>)>,
    running: Option<Running>,
    notice: Option<Notice>,
}

impl Store {
    pub fn new(location: String) -> Self {
        Self {
            location,
            ..Default::default()
        }
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn installation(&self) -> Option<&Installation> {
        self.project
            .as_ref()
            .and_then(|(_, read)| read.as_ref().ok())
    }

    fn start(&mut self, task: Task, work: impl FnOnce(Progress) -> Result<Done> + Send + 'static) {
        if self.running.is_some() {
            return;
        }
        let label = match &task {
            Task::Open => "Loading the Kennel index".to_owned(),
            Task::Details(name) => format!("Reading {name}"),
            Task::Install { name, .. } => format!("Installing {name}"),
            Task::Remove { name, .. } => format!("Removing {name}"),
            Task::Verify => "Verifying installed packages".to_owned(),
        };
        match Job::start(&label, work) {
            Ok(job) => self.running = Some(Running { task, job }),
            Err(error) => {
                self.notice = Some(Notice {
                    error: true,
                    ..Notice::info(format!("{error:#}"))
                })
            }
        }
    }

    pub fn open_registry(&mut self) {
        self.opened = true;
        let location = match self.location.trim() {
            "" => kennel::default_registry(),
            location => location.to_owned(),
        };
        self.start(Task::Open, move |progress| {
            Registry::open(&location, &progress).map(Done::Opened)
        });
    }

    fn load_details(&mut self, name: String) {
        let Some(registry) = self.registry.clone() else {
            return;
        };
        self.start(Task::Details(name.clone()), move |progress| {
            let (manifest, _) = registry.manifest(&name, &progress)?;
            let readme = manifest
                .files
                .iter()
                .find(|file| file.path.eq_ignore_ascii_case("README.md"))
                .map(|file| {
                    registry
                        .package_text(&manifest, &file.path, &progress)
                        .map_err(|error| format!("{error:#}"))
                });
            Ok(Done::Details(Box::new(Details { manifest, readme })))
        });
    }

    fn install(&mut self, name: String, force: bool) {
        let (Some(registry), Some(root)) = (
            self.registry.clone(),
            self.installation().map(|i| i.root.clone()),
        ) else {
            return;
        };
        let options = InstallOptions {
            all_targets: self.all_targets,
            force,
            cache: None,
        };
        self.notice = None;
        self.start(
            Task::Install {
                name: name.clone(),
                force,
            },
            move |progress| {
                kennel::install(&root, &name, &registry, options, &progress).map(Done::Installed)
            },
        );
    }

    fn remove(&mut self, name: String, force: bool) {
        let Some(root) = self.installation().map(|i| i.root.clone()) else {
            return;
        };
        self.notice = None;
        self.start(
            Task::Remove {
                name: name.clone(),
                force,
            },
            move |progress| kennel::remove(&root, &name, force, &progress).map(|()| Done::Removed),
        );
    }

    fn verify(&mut self) {
        let Some(root) = self.installation().map(|i| i.root.clone()) else {
            return;
        };
        self.notice = None;
        self.start(Task::Verify, move |progress| {
            kennel::verify(&root, &progress).map(Done::Verified)
        });
    }

    /// Publishes a finished store job. Installs, removals and verification report back for
    /// the editor's status bar; browsing reports only inside the pane.
    pub fn poll(&mut self) -> Option<Result<String>> {
        let result = self.running.as_ref()?.job.poll()?;
        let Running { task, job } = self.running.take()?;
        if job.cancelled() {
            // A cancelled read offers Retry instead of starting again on the next frame.
            if let Task::Details(name) = task {
                self.details.insert(name, Err("Cancelled.".into()));
            }
            self.notice = Some(Notice::info("Cancelled."));
            return None;
        }
        let error = match (task, result) {
            (Task::Open, Ok(Done::Opened(registry))) => {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|name| !registry.index.packages.contains_key(name))
                {
                    self.selected = None;
                }
                self.details.clear();
                self.registry = Some(Arc::new(registry));
                return None;
            }
            (Task::Details(name), Ok(Done::Details(details))) => {
                self.details.insert(name, Ok(*details));
                return None;
            }
            (Task::Details(name), Err(error)) => {
                self.details.insert(name, Err(format!("{error:#}")));
                return None;
            }
            (Task::Install { .. }, Ok(Done::Installed(installed))) => {
                self.project = None;
                let text = installed_message(&installed);
                self.notice = Some(Notice {
                    build_env: installed
                        .last()
                        .map(|p| p.build_env.clone())
                        .unwrap_or_default(),
                    ..Notice::info(&text)
                });
                return Some(Ok(text));
            }
            (Task::Remove { name, .. }, Ok(Done::Removed)) => {
                self.project = None;
                let text = format!(
                    "Removed {name}. Scenes that still list its files need those catalog \
                     entries removed."
                );
                self.notice = Some(Notice::info(&text));
                return Some(Ok(text));
            }
            (Task::Verify, Ok(Done::Verified(packages))) => {
                let files: usize = packages.iter().map(|(_, _, files)| files).sum();
                let text = if packages.is_empty() {
                    "No Kennel packages are installed in this project.".to_owned()
                } else {
                    format!(
                        "All {} installed packages are intact: {files} files match their \
                         manifests.",
                        packages.len()
                    )
                };
                self.notice = Some(Notice::info(&text));
                return Some(Ok(text));
            }
            (Task::Open, Err(error)) => {
                self.registry = None;
                (error, None, false)
            }
            (Task::Install { name, force }, Err(error)) => {
                // Forcing helps only when an installation is already there to replace.
                let present = self.installation().is_some_and(|installation| {
                    installation
                        .root
                        .join(kennel::INSTALL_DIR)
                        .join(&name)
                        .exists()
                });
                (
                    error,
                    (!force && present).then_some(Repair::Install(name)),
                    true,
                )
            }
            (Task::Remove { name, force }, Err(error)) => {
                (error, (!force).then_some(Repair::Remove(name)), true)
            }
            (_, Err(error)) => (error, None, true),
            (_, Ok(_)) => return None,
        };
        let (error, repair, report) = error;
        self.project = None;
        self.notice = Some(Notice {
            error: true,
            repair,
            ..Notice::info(format!("{error:#}"))
        });
        report.then_some(Err(error))
    }

    /// Adds `name`'s scripts and assets, and its dependencies', to the scene catalog as one
    /// undoable change. Entries already pointing into the project's `kennel/` folder are
    /// updated, so this also follows an upgrade.
    fn add_to_scene(&self, editor: &mut Editor, name: &str) -> Result<usize> {
        let installation = self
            .installation()
            .context("Open a scene inside a Bozzard project first")?;
        let (base, entries) = installation.catalog(name)?;
        let mut scene = editor.scene().clone();
        let mut changed = 0;
        for (id, source) in entries {
            match scene.assets.get(&id) {
                Some(existing) if *existing == source => continue,
                Some(existing) if !existing.path.starts_with(&format!("{base}/")) => bail!(
                    "The scene's catalog already uses {id} for {}. Rename or remove that \
                     entry first.",
                    existing.path
                ),
                _ => {}
            }
            scene.assets.insert(id, source);
            changed += 1;
        }
        if changed > 0 {
            editor.finish_gesture();
            editor.apply(&format!("Add {name} from Kennel"), scene)?;
        }
        Ok(changed)
    }

    /// Draws the store. Adding a package to the scene reports back for the status bar.
    pub fn ui(&mut self, ui: &mut egui::Ui, editor: &mut Editor) -> Option<Result<String>> {
        if self
            .project
            .as_ref()
            .is_none_or(|(scene, _)| *scene != editor.path)
        {
            let read = Installation::read(&editor.path).map_err(|error| format!("{error:#}"));
            self.project = Some((editor.path.clone(), read));
        }
        if !self.opened {
            self.open_registry();
        }
        if let Some(name) = self.selected.clone()
            && self.registry.is_some()
            && self.running.is_none()
            && !self.details.contains_key(&name)
        {
            self.load_details(name);
        }
        self.header(ui);
        self.status(ui);
        ui.separator();
        let Some(registry) = self.registry.clone() else {
            if self.running.is_none() {
                ui.add_space(12.0);
                ui.label("The registry couldn't be loaded.");
                if ui.button("Retry").clicked() {
                    self.open_registry();
                }
            }
            return None;
        };
        let height = ui.available_height();
        if ui.available_width() >= 720.0 {
            let list_width = (ui.available_width() * 0.4).clamp(280.0, 440.0);
            let mut report = None;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(list_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("kennel-list")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.list(ui, &registry));
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("kennel-detail")
                            .auto_shrink([false, false])
                            .show(ui, |ui| report = self.detail(ui, &registry, editor));
                    },
                );
            });
            report
        } else {
            egui::ScrollArea::vertical()
                .id_salt("kennel-narrow")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.selected.is_none() {
                        self.list(ui, &registry);
                        None
                    } else if ui.button("← All packages").clicked() {
                        self.selected = None;
                        None
                    } else {
                        self.detail(ui, &registry, editor)
                    }
                })
                .inner
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        // Rows wrap or truncate: an overlong row would widen the whole pane past its clip.
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Kennel")
                    .font(theme::bold(ui.ctx(), 20.0))
                    .color(theme::GREEN),
            );
            ui.weak("Packages for Bozzard, checked against this editor before they install.");
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search packages…")
                    .desired_width(200.0),
            );
            if ui
                .selectable_label(self.category.is_none(), "All")
                .clicked()
            {
                self.category = None;
            }
            for category in Category::ALL {
                let selected = self.category == Some(category);
                if ui
                    .selectable_label(selected, category_label(category))
                    .clicked()
                {
                    self.category = (!selected).then_some(category);
                }
            }
            ui.checkbox(&mut self.installed_only, "Installed");
        });
        ui.horizontal(|ui| {
            ui.label("Registry");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.location)
                    .hint_text(kennel::default_registry())
                    .desired_width((ui.available_width() - 80.0).max(120.0)),
            );
            let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let refresh = ui
                .add_enabled(self.running.is_none(), egui::Button::new("Refresh"))
                .on_hover_text(
                    "Reload the registry's index. Leave the field empty for the public registry.",
                );
            if (refresh.clicked() || submitted) && self.running.is_none() {
                self.open_registry();
            }
        });
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        if let Some(running) = &self.running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add(egui::Label::new(running.job.label()).truncate());
                let fraction = running.job.fraction();
                if fraction > 0.0 && fraction < 1.0 {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(140.0)
                            .show_percentage(),
                    );
                }
                if ui
                    .add_enabled(!running.job.cancelled(), egui::Button::new("Cancel"))
                    .clicked()
                {
                    running.job.cancel();
                }
            });
        }
        let mut repair = None;
        let mut dismiss = false;
        if let Some(notice) = &self.notice {
            let tint = if notice.error {
                theme::CORAL
            } else {
                theme::GREEN
            };
            egui::Frame::new()
                .fill(tint.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0, tint.gamma_multiply(0.4)))
                .corner_radius(8)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&notice.text).color(tint)).wrap());
                    for (variable, path) in &notice.build_env {
                        ui.horizontal(|ui| {
                            // The button goes first: a truncated label takes the rest of the row.
                            let line = format!("{variable}={}", path.display());
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(line.clone());
                            }
                            ui.add(egui::Label::new(RichText::new(line).monospace()).truncate());
                        });
                    }
                    ui.horizontal(|ui| {
                        if let Some(offer) = &notice.repair {
                            let label = match offer {
                                Repair::Install(_) => "Replace the installed files",
                                Repair::Remove(_) => "Remove anyway",
                            };
                            if ui
                                .add_enabled(self.running.is_none(), egui::Button::new(label))
                                .clicked()
                            {
                                repair = Some(offer.clone());
                            }
                        }
                        dismiss = ui.small_button("Dismiss").clicked();
                    });
                });
        }
        if dismiss {
            self.notice = None;
        }
        match repair {
            Some(Repair::Install(name)) => self.install(name, true),
            Some(Repair::Remove(name)) => self.remove(name, true),
            None => {}
        }
        let mut verify = false;
        match self.project.as_ref().map(|(_, read)| read) {
            Some(Ok(installation)) => {
                ui.horizontal(|ui| {
                    ui.weak("Project");
                    let count = installation.lockfile.packages.len();
                    let verify_button = ui
                        .add_enabled(
                            self.running.is_none() && count > 0,
                            egui::Button::new("Verify"),
                        )
                        .on_hover_text("Re-hash every installed file against its manifest");
                    verify = verify_button.clicked();
                    ui.weak(match count {
                        1 => "1 package installed ·".to_owned(),
                        count => format!("{count} packages installed ·"),
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(installation.root.display().to_string()).monospace(),
                        )
                        .truncate(),
                    );
                });
            }
            Some(Err(error)) => {
                ui.add(egui::Label::new(RichText::new(error).color(theme::AMBER)).wrap());
            }
            None => {}
        }
        if verify {
            self.verify();
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, registry: &Registry) {
        let installed: BTreeMap<String, String> = self
            .installation()
            .map(|installation| {
                installation
                    .lockfile
                    .packages
                    .iter()
                    .map(|(name, locked)| (name.clone(), locked.version.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut shown = 0;
        for (name, entry) in &registry.index.packages {
            if self
                .category
                .is_some_and(|category| category != entry.category)
                || (self.installed_only && !installed.contains_key(name))
                || !entry.matches(name, &self.search)
            {
                continue;
            }
            shown += 1;
            let selected = self.selected.as_deref() == Some(name);
            if card(ui, name, entry, installed.get(name), selected).clicked() {
                self.selected = Some(name.clone());
            }
            ui.add_space(6.0);
        }
        if shown == 0 {
            ui.add_space(8.0);
            ui.weak(if registry.index.packages.is_empty() {
                "This registry has no packages yet."
            } else {
                "No packages match."
            });
        }
    }

    fn detail(
        &mut self,
        ui: &mut egui::Ui,
        registry: &Registry,
        editor: &mut Editor,
    ) -> Option<Result<String>> {
        let Some(name) = self.selected.clone() else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.weak(
                    "Select a package to see what it contains, what it needs and how it installs.",
                );
            });
            return None;
        };
        let entry = registry.index.packages.get(&name)?;
        ui.label(RichText::new(&entry.title).font(theme::bold(ui.ctx(), 22.0)));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&name).monospace());
            ui.weak(format!("· {} ·", entry.version));
            theme::chip(
                ui,
                category_label(entry.category),
                category_color(entry.category),
            );
            for tag in &entry.tags {
                ui.weak(format!("#{tag}"));
            }
        });
        ui.add(egui::Label::new(&entry.summary).wrap());
        ui.add_space(8.0);
        let details = match self.details.get(&name) {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Reading the manifest…");
                });
                return None;
            }
            Some(Err(error)) => {
                ui.add(egui::Label::new(RichText::new(error).color(theme::CORAL)).wrap());
                if ui
                    .add_enabled(self.running.is_none(), egui::Button::new("Retry"))
                    .clicked()
                {
                    self.details.remove(&name);
                }
                return None;
            }
            Some(Ok(details)) => details,
        };
        let manifest = &details.manifest;
        let installation = self
            .project
            .as_ref()
            .and_then(|(_, read)| read.as_ref().ok());
        let locked = installation.and_then(|i| i.lockfile.packages.get(&name));
        let idle = self.running.is_none();
        let host = kennel::host_target();
        let mut all_targets = self.all_targets;
        let mut action = None;

        let compatible =
            manifest.supports_engine().unwrap_or(false) && manifest.supports_script_api();
        let has_host = manifest.bins.is_empty() || manifest.targets().contains(&host);
        let font = theme::bold(ui.ctx(), 13.0);
        ui.horizontal_wrapped(|ui| {
            let primary = |text: String| {
                egui::Button::new(
                    RichText::new(text)
                        .font(font.clone())
                        .color(Color32::from_rgb(16, 32, 26)),
                )
                .fill(theme::GREEN)
                .min_size(Vec2::new(110.0, 28.0))
            };
            let blocked = if installation.is_none() {
                Some("Open a scene inside a Bozzard project to install packages.".to_owned())
            } else if !compatible {
                Some("This package doesn't support this editor. See Compatibility.".to_owned())
            } else if !has_host && !all_targets {
                Some(format!(
                    "No binaries for {host}. Choose every platform to install them anyway."
                ))
            } else {
                None
            };
            let label = match locked {
                None => Some("Install".to_owned()),
                Some(locked) if locked.version != entry.version => {
                    Some(format!("Update {} → {}", locked.version, entry.version))
                }
                Some(locked) if all_targets && locked.targets != manifest.targets() => {
                    Some("Add every platform's binaries".to_owned())
                }
                Some(locked) => {
                    theme::chip(ui, &format!("✓ Installed {}", locked.version), theme::GREEN);
                    None
                }
            };
            if let Some(label) = label {
                let button = ui
                    .add_enabled(idle && blocked.is_none(), primary(label))
                    .on_disabled_hover_text(blocked.unwrap_or_default());
                if button.clicked() {
                    action = Some(Action::Install { force: false });
                }
            }
            if let (Some(installation), Some(_)) = (installation, locked)
                && (!manifest.scripts.is_empty() || !manifest.assets.is_empty())
            {
                if installation.in_scene(editor.scene(), &name) {
                    theme::chip(ui, "✓ In scene", theme::ACCENT);
                } else {
                    let ids = installation
                        .catalog(&name)
                        .map(|(_, entries)| entries.into_keys().collect::<Vec<_>>().join(", "))
                        .unwrap_or_else(|error| format!("{error:#}"));
                    let button = ui
                        .add_enabled(editor.play.is_none(), egui::Button::new("Add to scene"))
                        .on_hover_text(format!("Adds {ids} to this scene's asset catalog"))
                        .on_disabled_hover_text("Stop Play to change the scene");
                    if button.clicked() {
                        action = Some(Action::AddToScene);
                    }
                }
            }
            if let (Some(installation), Some(_)) = (installation, locked) {
                let dependents = installation.dependents(&name);
                let button = ui
                    .add_enabled(idle && dependents.is_empty(), egui::Button::new("Remove"))
                    .on_hover_text(format!("Delete kennel/{name} from the project"))
                    .on_disabled_hover_text(if dependents.is_empty() {
                        "Wait for the current operation".to_owned()
                    } else {
                        format!("{} depends on it", dependents.join(", "))
                    });
                if button.clicked() {
                    action = Some(Action::Remove);
                }
            }
        });
        if !manifest.bins.is_empty() {
            ui.checkbox(&mut all_targets, "Binaries for every platform")
                .on_hover_text(format!(
                    "For exporting to other platforms. Otherwise only {host}'s binaries install."
                ));
        }
        ui.add_space(8.0);

        theme::section(ui, "Compatibility", |ui| {
            compatibility(ui, manifest, &host);
        });
        theme::section(ui, "Contents", |ui| contents(ui, manifest, &host));
        if !manifest.build_env.is_empty() {
            theme::section(ui, "Build environment", |ui| {
                for (variable, folder) in &manifest.build_env {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(variable).monospace().strong());
                        match installation.filter(|_| locked.is_some()) {
                            Some(installation) => {
                                let path = installation
                                    .root
                                    .join(kennel::INSTALL_DIR)
                                    .join(&name)
                                    .join(folder);
                                let line = format!("{variable}={}", path.display());
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(line);
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(path.display().to_string()).monospace(),
                                    )
                                    .truncate(),
                                );
                            }
                            None => {
                                ui.weak(format!("kennel/{name}/{folder}, once installed"));
                            }
                        }
                    });
                }
                ui.weak(
                    "Engine builds read these variables. Set them to build the editor or \
                     player against exactly the binaries Kennel verified.",
                );
            });
        }
        theme::section(ui, "Package", |ui| {
            package(ui, manifest, entry, registry, installation);
        });
        if let Some(readme) = &details.readme {
            theme::section(ui, "Readme", |ui| match readme {
                Ok(text) => readme::show(ui, text),
                Err(error) => {
                    ui.add(egui::Label::new(RichText::new(error).color(theme::CORAL)).wrap());
                }
            });
        }

        self.all_targets = all_targets;
        match action? {
            Action::Install { force } => {
                self.install(name, force);
                None
            }
            Action::Remove => {
                self.remove(name, false);
                None
            }
            Action::AddToScene => Some(self.add_to_scene(editor, &name).map(|changed| {
                let text = format!(
                    "Added {name} to the scene catalog ({changed} entries). Save the scene to \
                     keep them."
                );
                self.notice = Some(Notice::info(&text));
                text
            })),
        }
    }
}

impl App {
    pub fn kennel_content(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        if let Some(report) = self.kennel.ui(ui, &mut self.editor) {
            self.kennel_report(report);
        }
    }

    pub fn kennel_report(&mut self, report: Result<String>) {
        match report {
            Ok(message) => {
                self.status = message;
                self.error = false;
            }
            Err(error) => self.result(Err(error)),
        }
    }
}

/// A package tile in the list. Clicking anywhere on it selects the package.
fn card(
    ui: &mut egui::Ui,
    name: &str,
    entry: &IndexEntry,
    installed: Option<&String>,
    selected: bool,
) -> egui::Response {
    let frame = egui::Frame::new()
        .fill(if selected {
            theme::ACCENT.gamma_multiply(0.14)
        } else {
            theme::glass(8)
        })
        .stroke(egui::Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::glass(18)
            },
        ))
        .corner_radius(10)
        .inner_margin(egui::Margin::same(10));
    let shown = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add(
            egui::Label::new(RichText::new(&entry.title).font(theme::bold(ui.ctx(), 14.0)))
                .truncate(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.weak(&entry.version);
            theme::chip(
                ui,
                category_label(entry.category),
                category_color(entry.category),
            );
            match installed {
                Some(version) if *version == entry.version => {
                    theme::chip(ui, "Installed", theme::GREEN);
                }
                Some(version) => {
                    theme::chip(
                        ui,
                        &format!("Update {version} → {}", entry.version),
                        theme::AMBER,
                    );
                }
                None => {}
            }
        });
        ui.add(egui::Label::new(RichText::new(&entry.summary).weak()).wrap());
        if !entry.tags.is_empty() {
            let tags: Vec<_> = entry.tags.iter().map(|tag| format!("#{tag}")).collect();
            ui.small(tags.join("  "));
        }
    });
    ui.interact(
        shown.response.rect,
        ui.id().with(("kennel-card", name)),
        egui::Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn check(ui: &mut egui::Ui, ok: Option<bool>, text: String, note: String) {
    ui.horizontal_wrapped(|ui| {
        let (mark, color) = match ok {
            Some(true) => ("✓", theme::GREEN),
            Some(false) => ("✕", theme::CORAL),
            None => ("?", theme::AMBER),
        };
        ui.colored_label(color, mark);
        ui.label(text);
        ui.weak(note);
    });
}

fn compatibility(ui: &mut egui::Ui, manifest: &Manifest, host: &str) {
    let engine = &manifest.engine;
    check(
        ui,
        manifest.supports_engine().ok(),
        format!("Bozzard {}", engine.bozzard),
        format!("this editor is {}", kennel::ENGINE_VERSION),
    );
    check(
        ui,
        Some(manifest.supports_script_api()),
        format!("Script API {}", engine.script_api),
        format!(
            "this editor provides {}",
            bozzard_project::runtime::SCRIPT_API_VERSION
        ),
    );
    for feature in &engine.features {
        let built = feature_built(feature);
        let note = match built {
            Some(true) => "built into this editor".to_owned(),
            Some(false) => {
                format!("not built into this editor; build it with --features {feature}")
            }
            None => "this editor can't check it".to_owned(),
        };
        check(ui, built, format!("Engine feature {feature}"), note);
    }
    if manifest.bins.is_empty() {
        check(ui, Some(true), "No native binaries".into(), String::new());
    } else {
        let targets = manifest.targets();
        let available = targets.iter().any(|target| target == host);
        let note = if available {
            "this machine".to_owned()
        } else {
            format!("only {}", targets.join(", "))
        };
        check(ui, Some(available), format!("Binaries for {host}"), note);
    }
}

fn contents(ui: &mut egui::Ui, manifest: &Manifest, host: &str) {
    let heading = |ui: &mut egui::Ui, text: String| {
        ui.label(RichText::new(text).strong());
    };
    if !manifest.scripts.is_empty() {
        heading(ui, format!("Scripts · {}", manifest.scripts.len()));
        for script in &manifest.scripts {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&script.id).monospace().color(theme::GREEN));
                ui.weak(format!("{} · {}", script.path, size(script.bytes)));
            });
        }
        ui.add_space(4.0);
    }
    if !manifest.assets.is_empty() {
        heading(ui, format!("Assets · {}", manifest.assets.len()));
        for asset in &manifest.assets {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&asset.id).monospace().color(theme::SKY));
                ui.weak(format!(
                    "{:?} · {} · {}",
                    asset.kind,
                    asset.path,
                    size(asset.bytes)
                ));
            });
        }
        ui.add_space(4.0);
    }
    if !manifest.bins.is_empty() {
        heading(ui, format!("Binaries · {}", manifest.bins.len()));
        for bin in &manifest.bins {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&bin.path).monospace());
                for target in &bin.targets {
                    if target == host {
                        ui.colored_label(theme::ACCENT, target);
                    } else {
                        ui.weak(target);
                    }
                }
                let mut note = size(bin.bytes);
                if let Some(source) = &bin.source {
                    note += &format!(" · downloads from {}", url_host(&source.url));
                }
                ui.weak(note);
            });
        }
        ui.add_space(4.0);
    }
    if !manifest.files.is_empty() {
        heading(ui, format!("Files · {}", manifest.files.len()));
        for file in &manifest.files {
            ui.weak(format!("{} · {}", file.path, size(file.bytes)));
        }
    }
}

fn package(
    ui: &mut egui::Ui,
    manifest: &Manifest,
    entry: &IndexEntry,
    registry: &Registry,
    installation: Option<&Installation>,
) {
    // Wrapped rows rather than a grid: long values never widen a narrow pane.
    let row = |ui: &mut egui::Ui, key: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.horizontal_wrapped(|ui| {
            ui.weak(key);
            add(ui);
        });
    };
    row(ui, "Authors", &mut |ui| {
        ui.label(manifest.authors.join(", "));
    });
    let mut licenses = vec![manifest.license.clone()];
    for license in manifest.bins.iter().filter_map(|bin| bin.license.as_ref()) {
        if !licenses.contains(license) {
            licenses.push(license.clone());
        }
    }
    row(ui, "License", &mut |ui| {
        ui.label(licenses.join(" · "));
    });
    if let Some(repository) = &manifest.repository {
        row(ui, "Repository", &mut |ui| {
            ui.hyperlink(repository);
        });
    }
    row(ui, "Dependencies", &mut |ui| {
        if manifest.dependencies.is_empty() {
            ui.label("None");
        }
        for (dependency, requirement) in &manifest.dependencies {
            let installed = installation
                .and_then(|i| i.lockfile.packages.get(dependency))
                .map_or("installs with it".to_owned(), |locked| {
                    format!("installed {}", locked.version)
                });
            ui.label(RichText::new(format!("{dependency} {requirement}")).monospace());
            ui.weak(installed);
        }
    });
    row(ui, "Manifest", &mut |ui| {
        let short = format!("sha256 {}…", &entry.sha256[..16]);
        if ui
            .add(egui::Label::new(RichText::new(short).monospace()).sense(egui::Sense::click()))
            .on_hover_text(format!("{}\nClick to copy", entry.sha256))
            .clicked()
        {
            ui.ctx().copy_text(entry.sha256.clone());
        }
    });
    row(ui, "Registry", &mut |ui| {
        ui.label(RichText::new(registry.location()).monospace());
    });
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            RichText::new(
                "The index pins this manifest and the manifest pins every file by size and \
                 SHA-256. Upstream binaries download from their source and are checked against \
                 their own pinned hashes. Checksums aren't signatures, and package scripts run \
                 as game code, so install packages from registries you trust.",
            )
            .weak(),
        )
        .wrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "bozzard-kennel-pane-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A registry with `kit`, a script library, and `game-rules`, which imports it.
    fn registry(root: &Path) -> PathBuf {
        let registry = root.join("registry");
        let package = |name: &str, dependencies: &str, scripts: &[(&str, &str, &str)]| {
            let folder = registry.join("packages").join(name);
            std::fs::create_dir_all(folder.join("scripts")).unwrap();
            let readme = format!("# {name}\n\nA **test** package with `scripts/`.\n");
            std::fs::write(folder.join("README.md"), &readme).unwrap();
            let mut listed = Vec::new();
            for (id, path, text) in scripts {
                std::fs::write(folder.join(path), text).unwrap();
                listed.push(format!(
                    r#"{{"id":"{id}","path":"{path}","bytes":{},"sha256":"{}"}}"#,
                    text.len(),
                    sha(text.as_bytes())
                ));
            }
            let manifest = format!(
                r#"{{"kennel":1,"name":"{name}","version":"1.0.0","title":"Test {name}",
                "summary":"Scripts for Kennel pane tests.","category":"scripts","tags":["test"],
                "authors":["Bozzard tests"],"license":"MIT OR Apache-2.0",
                "engine":{{"bozzard":">=0.1.0","script_api":1}},"dependencies":{{{dependencies}}},
                "scripts":[{}],
                "files":[{{"path":"README.md","bytes":{},"sha256":"{}"}}]}}"#,
                listed.join(","),
                readme.len(),
                sha(readme.as_bytes())
            );
            std::fs::write(folder.join(format!("{name}.pkg.json")), manifest).unwrap();
        };
        package(
            "kit",
            "",
            &[("kit/math", "scripts/math.rhai", "fn twice(x) { x * 2.0 }\n")],
        );
        package(
            "game-rules",
            r#""kit":"^1""#,
            &[(
                "game-rules/player",
                "scripts/player.rhai",
                "import \"kit/math\" as math;\nfn on_update(me, dt) { math::twice(dt); }\n",
            )],
        );
        kennel::build_index(&registry).unwrap();
        kennel::check(&registry, None, &Progress::default()).unwrap();
        registry
    }

    fn finish(store: &mut Store) -> Option<Result<String>> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while store.busy() {
            if let Some(report) = store.poll() {
                return Some(report);
            }
            assert!(Instant::now() < deadline, "Kennel job timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        None
    }

    fn draw(store: &mut Store, editor: &mut Editor, width: f32) -> Option<Result<String>> {
        let ctx = egui::Context::default();
        let mut report = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 900.0),
                )),
                ..Default::default()
            },
            |ui| report = store.ui(ui, editor),
        );
        output.textures_delta.clear();
        report
    }

    #[test]
    fn store_installs_a_package_with_its_dependency_and_adds_both_to_the_scene() {
        let temp = Temp::new();
        let registry = registry(&temp.0);
        let manifest = bozzard_project::create_project(
            &temp.0.join("game"),
            "Kennel Game",
            bozzard_project::ProjectTemplate::Collect2d,
        )
        .unwrap();
        let (_, scene) = bozzard_project::Project::load(&manifest).unwrap();
        let mut editor = Editor::open(&scene).unwrap();
        let mut store = Store::new(registry.display().to_string());

        // The first frame reads the project and opens the registry.
        draw(&mut store, &mut editor, 1200.0);
        assert!(finish(&mut store).is_none());
        let installation = store.installation().unwrap();
        assert_eq!(installation.root, temp.0.join("game"));
        assert!(installation.lockfile.packages.is_empty());
        store.search = "PANE tests".into();
        store.category = Some(Category::Scripts);
        store.selected = Some("game-rules".into());
        draw(&mut store, &mut editor, 1200.0);
        assert!(finish(&mut store).is_none());
        let details = store.details["game-rules"].as_ref().unwrap();
        assert_eq!(details.manifest.scripts[0].id, "game-rules/player");
        assert!(
            details
                .readme
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .contains("**test**")
        );

        store.install("game-rules".into(), false);
        let report = finish(&mut store).unwrap().unwrap();
        assert!(report.contains("Installed game-rules 1.0.0"), "{report}");
        assert!(report.contains("With dependencies kit 1.0.0"), "{report}");
        // Narrow panes stack the list and the details.
        draw(&mut store, &mut editor, 520.0);
        let installation = store.installation().unwrap();
        assert_eq!(
            installation.lockfile.packages.keys().collect::<Vec<_>>(),
            ["game-rules", "kit"]
        );
        assert!(!installation.in_scene(editor.scene(), "game-rules"));
        assert_eq!(installation.dependents("kit"), ["game-rules"]);

        assert_eq!(store.add_to_scene(&mut editor, "game-rules").unwrap(), 2);
        let assets = &editor.scene().assets;
        assert_eq!(
            assets["game-rules/player"].path,
            "../kennel/game-rules/scripts/player.rhai"
        );
        assert_eq!(assets["kit/math"].path, "../kennel/kit/scripts/math.rhai");
        assert!(
            store
                .installation()
                .unwrap()
                .in_scene(editor.scene(), "game-rules")
        );
        assert_eq!(store.add_to_scene(&mut editor, "game-rules").unwrap(), 0);
        editor.undo().unwrap();
        assert!(!editor.scene().assets.contains_key("kit/math"));
        draw(&mut store, &mut editor, 1200.0);

        store.verify();
        let report = finish(&mut store).unwrap().unwrap();
        assert!(
            report.contains("All 2 installed packages are intact"),
            "{report}"
        );
        std::fs::write(
            temp.0.join("game/kennel/kit/scripts/math.rhai"),
            "fn twice(x) { x }\n",
        )
        .unwrap();
        store.verify();
        assert!(finish(&mut store).unwrap().is_err());
        draw(&mut store, &mut editor, 1200.0);
        store.remove("game-rules".into(), false);
        assert!(finish(&mut store).unwrap().is_ok());
        draw(&mut store, &mut editor, 1200.0);
        store.remove("kit".into(), false);
        let error = finish(&mut store).unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("was modified"), "{error:#}");
        assert_eq!(
            store.notice.as_ref().unwrap().repair,
            Some(Repair::Remove("kit".into()))
        );
        draw(&mut store, &mut editor, 1200.0);
        store.remove("kit".into(), true);
        assert!(finish(&mut store).unwrap().is_ok());
        draw(&mut store, &mut editor, 1200.0);
        assert!(store.installation().unwrap().lockfile.packages.is_empty());
        assert!(!temp.0.join("game/kennel/kit").exists());
    }

    #[test]
    fn scenes_outside_a_project_explain_why_nothing_installs() {
        let temp = Temp::new();
        let registry = registry(&temp.0);
        let scene = bozzard_scene::Scene::from_json(
            r#"{"version":1,"name":"Loose","views":{},"objects":[]}"#,
        )
        .unwrap();
        let mut editor = Editor::new(scene, &temp.0.join("loose.json")).unwrap();
        let mut store = Store::new(registry.display().to_string());
        draw(&mut store, &mut editor, 1200.0);
        finish(&mut store);
        let error = store.project.as_ref().unwrap().1.as_ref().err().unwrap();
        assert!(error.contains("isn't inside a Bozzard project"), "{error}");
        store.install("kit".into(), false);
        assert!(!store.busy());
        let mut missing = Store::new(temp.0.join("absent").display().to_string());
        draw(&mut missing, &mut editor, 1200.0);
        finish(&mut missing);
        assert!(missing.registry.is_none());
        assert!(missing.notice.as_ref().unwrap().error);
        draw(&mut missing, &mut editor, 1200.0);
    }

    #[test]
    fn catalog_paths_are_relative_to_the_scene_folder() {
        let path = |p: &str| PathBuf::from(p);
        assert_eq!(
            relative_path(&path("/game/scenes"), &path("/game/kennel")).unwrap(),
            "../kennel"
        );
        assert_eq!(
            relative_path(&path("/game"), &path("/game/kennel")).unwrap(),
            "kennel"
        );
        assert_eq!(
            relative_path(&path("/game/a/b"), &path("/game/kennel")).unwrap(),
            "../../kennel"
        );
        assert_eq!(
            url_host("https://static.crates.io/crates/x.crate"),
            "static.crates.io"
        );
        assert_eq!(size(383_432), "374 KiB");
    }
}
