//! Cached directory listings. Even a blocked filesystem call never owns the UI thread.
use super::Kind;
use anyhow::{Context, Result, ensure};
use bozzard_assets::job::{Job, Progress};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub path_text: String,
    pub label: String,
    pub folder: bool,
}

#[derive(Default)]
pub struct Browser {
    requested: Option<(PathBuf, Kind)>,
    running: Option<Job<Vec<Entry>>>,
    entries: Option<Result<Vec<Entry>, String>>,
}

impl Browser {
    pub fn clear(&mut self) {
        self.requested = None;
        self.entries = None;
        if let Some(job) = &self.running {
            job.cancel();
            // Retain a blocked worker until it exits: repeated navigation or opening
            // and closing dialogs must not start unbounded filesystem threads.
            if job.poll().is_some() {
                self.running = None;
            }
        }
    }

    pub fn request(&mut self, path: &Path, kind: Kind) {
        if !self
            .requested
            .as_ref()
            .is_some_and(|(p, k)| p == path && *k == kind)
        {
            self.clear();
            self.requested = Some((path.to_owned(), kind));
        }
        if let Some(job) = &self.running
            && let Some(result) = job.poll()
        {
            if !job.cancelled() {
                self.entries = Some(result.map_err(|error| format!("{error:#}")));
            }
            self.running = None;
        }
        if self.running.is_none() && self.entries.is_none() {
            let path = path.to_owned();
            match Job::start("Reading folder", move |progress| {
                scan(&path, kind, &progress)
            }) {
                Ok(job) => self.running = Some(job),
                Err(error) => self.entries = Some(Err(error.to_string())),
            }
        }
    }

    pub fn entries(&self) -> Option<Result<&[Entry], &str>> {
        self.entries
            .as_ref()
            .map(|result| result.as_ref().map(Vec::as_slice).map_err(String::as_str))
    }
}

fn scan(path: &Path, kind: Kind, progress: &Progress) -> Result<Vec<Entry>> {
    progress.check()?;
    let directory = std::fs::read_dir(path)
        .with_context(|| format!("Cannot read folder {}", path.display()))?;
    let mut entries = Vec::new();
    for (index, entry) in directory.enumerate() {
        progress.check()?;
        ensure!(
            index < 100_000,
            "Folder has more than 100,000 entries; enter a file path directly"
        );
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let folder = file_type.is_dir() || (file_type.is_symlink() && path.is_dir());
        if !folder && !accepts(kind, &path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push((name, path, folder));
    }
    progress.check()?;
    entries.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    Ok(entries
        .into_iter()
        .map(|(name, path, folder)| Entry {
            path_text: path.display().to_string(),
            path,
            label: format!("{} {name}", if folder { "▸" } else { "  " }),
            folder,
        })
        .collect())
}

fn accepts(kind: Kind, path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match kind {
        Kind::Import => {
            matches!(
                ext.as_str(),
                "png"
                    | "btex"
                    | "jpg"
                    | "jpeg"
                    | "obj"
                    | "gltf"
                    | "glb"
                    | "bmesh"
                    | "ttf"
                    | "otf"
                    | "wgsl"
                    | "rs"
                    | "wav"
                    | "ogg"
                    | "mp3"
                    | "flac"
            ) || name.ends_with(".prefab.json")
                || name.ends_with(".material.json")
        }
        Kind::LoadBlueprint | Kind::SaveBlueprint => name.ends_with(".blueprint.json"),
        Kind::LoadShaderGraph | Kind::SaveShaderGraph => name.ends_with(".shadergraph.json"),
        _ => ext == "json",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn wait(browser: &mut Browser, path: &Path, kind: Kind) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            browser.request(path, kind);
            if browser.entries().is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "directory worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn stalled_listing_does_not_block_navigation_or_publish_stale_results() {
        let (release, gate) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        let stalled = Path::new("stalled");
        let job = Job::start("Stalled", move |_| {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(vec![])
        })
        .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut browser = Browser {
            requested: Some((stalled.into(), Kind::Open)),
            running: Some(job),
            entries: None,
        };
        let missing =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("absent-directory-for-browser-test");
        for _ in 0..10 {
            browser.clear();
            browser.request(&missing, Kind::Import);
            assert!(browser.running.as_ref().unwrap().cancelled());
            assert!(browser.entries().is_none());
        }
        release.send(()).unwrap();
        wait(&mut browser, &missing, Kind::Import);
        assert!(
            browser
                .entries()
                .unwrap()
                .unwrap_err()
                .contains("Cannot read folder")
        );
    }

    #[test]
    fn listing_is_cached_until_refresh_and_filters_by_dialog_kind() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut browser = Browser::default();
        wait(&mut browser, &path, Kind::Import);
        let entries = browser.entries().unwrap().unwrap();
        assert!(entries.iter().any(|entry| entry.path.ends_with("main.rs")));
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].folder || !pair[1].folder)
        );
        let allocation = entries.as_ptr();
        browser.request(&path, Kind::Import);
        assert!(browser.running.is_none());
        assert_eq!(allocation, browser.entries().unwrap().unwrap().as_ptr());
        wait(&mut browser, &path, Kind::Open);
        assert!(
            browser
                .entries()
                .unwrap()
                .unwrap()
                .iter()
                .all(|entry| entry.folder)
        );
        browser.clear();
        assert!(browser.entries().is_none());
    }
}
