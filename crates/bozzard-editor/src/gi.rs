use super::*;
use bozzard_assets::job::Job;
use bozzard_scene::BakedGi;

pub(super) struct Freshness {
    revision: u64,
    assets: Vec<(String, Option<u64>)>,
    current: bool,
}

pub struct PreparedGi {
    baked: BakedGi,
    revision: u64,
    path: PathBuf,
}
impl Editor {
    pub fn gi_current(&self) -> bool {
        if self.scene.gi.baked.is_none() {
            return false;
        }
        // AssetStore is public: include actual resident content fingerprints, not
        // just Editor's publication counter. Same-ID replacement and Undo are safe.
        let assets: Vec<_> = self
            .assets
            .entries()
            .map(|e| (e.id.clone(), e.content_fingerprint()))
            .collect();
        let mut cached = self.gi_freshness.borrow_mut();
        if let Some(previous) = cached.as_ref()
            && previous.revision == self.revision
            && previous.assets == assets
        {
            return previous.current;
        }
        let current = bozzard_assets::gi::is_current(&self.scene, &self.assets).unwrap_or(false);
        *cached = Some(Freshness {
            revision: self.revision,
            assets,
            current,
        });
        current
    }
    pub fn fit_gi_volume(&mut self) -> Result<()> {
        let volume = bozzard_assets::gi::fit_volume(&self.scene, &self.assets)?;
        let mut scene = self.scene.clone();
        scene.gi.volume = volume;
        self.finish_gesture();
        self.apply("Fit GI volume", scene)
    }
    pub fn bake_gi_job(&mut self) -> Result<Job<PreparedGi>> {
        ensure!(self.play.is_none(), "Stop Play before baking GI");
        self.finish_gesture();
        let scene = self.scene.clone();
        let assets = self.assets.clone();
        let revision = self.revision;
        let path = self.path.clone();
        assets.require_ready()?;
        Job::start("Preparing GI bake", move |progress| {
            let baked = bozzard_assets::gi::bake(&scene, &assets, scene.gi.volume, &progress)?;
            Ok(PreparedGi {
                baked,
                revision,
                path,
            })
        })
    }
    pub fn accept_gi(&mut self, prepared: PreparedGi) -> Result<()> {
        ensure!(
            self.play.is_none() && self.path == prepared.path && self.revision == prepared.revision,
            "Scene changed during GI bake; bake again"
        );
        ensure!(
            prepared.baked.source
                == bozzard_assets::gi::source(&self.scene, &self.assets, self.scene.gi.volume)?,
            "GI source assets changed during bake; bake again"
        );
        let mut scene = self.scene.clone();
        scene.gi.baked = Some(std::sync::Arc::new(prepared.baked));
        scene.gi.enabled = true;
        self.apply("Bake global illumination", scene)
    }
}
