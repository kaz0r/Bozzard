//! Rhai script attachments: the coding half of the gameplay-authoring path.
//!
//! A script is a separate authoring style over the same engine actions blueprints expose, so a
//! scene may use either or both. Attachments reference `script` catalog assets, which keeps them
//! portable (paths are relative to the scene file) and lets the prefab merge and editor picker
//! treat a script like any other asset.
use super::*;

/// Mirrors [`MAX_BLUEPRINTS`]: attachments are ordered, so the count stays bounded.
pub const MAX_SCRIPTS: usize = 16;

/// One script file bound to this object, run in attachment order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptAttachment {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Catalog ID of a `script` asset. The source is read once when the scene loads.
    pub script: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptManager {
    pub scripts: Vec<ScriptAttachment>,
}

impl ScriptManager {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.scripts.len() <= MAX_SCRIPTS,
            "at most {MAX_SCRIPTS} scripts per object"
        );
        for attachment in &self.scripts {
            ensure!(
                !attachment.script.trim().is_empty() && attachment.script.len() <= 256,
                "script attachment needs a script asset ID"
            );
        }
        Ok(())
    }
    /// Script catalog IDs, for the asset dependency check every component goes through.
    pub fn asset_dependencies(&self) -> impl Iterator<Item = (&str, AssetKind)> {
        self.scripts
            .iter()
            .filter(|attachment| !attachment.script.is_empty())
            .map(|attachment| (attachment.script.as_str(), AssetKind::Script))
    }
}

impl Component for ScriptManager {
    const NAME: &'static str = "script_manager";
    const LABEL: &'static str = "Script Manager";
    const HELP: &'static str = "Rhai scripts, run in order. Hooks: on_start, on_update(dt), on_enable, on_disable, \
         on_destroy, on_object_enter/exit(other), on_overlap_enter/exit, \
         on_collision_enter(other, normal, impulse).";
}
