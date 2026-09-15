//! Optional particle modules sample normalized age; existing emitter presets retain their defaults.
use super::{curve::Curve, registry::Authored};
use crate::{Component, Field, FieldValue, Ui};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Curves {
    pub size: Curve,
    pub opacity: Curve,
    pub color: [Curve; 3],
    pub speed: Curve,
}
impl Default for Curves {
    fn default() -> Self {
        Self {
            size: Curve::constant(1.),
            opacity: Curve::constant(1.),
            color: std::array::from_fn(|_| Curve::constant(1.)),
            speed: Curve::constant(1.),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modules {
    pub enabled: bool,
    pub curves: Arc<Curves>,
}
impl Default for Modules {
    fn default() -> Self {
        Self {
            enabled: true,
            curves: Arc::new(Curves::default()),
        }
    }
}
impl Component for Modules {
    const NAME: &'static str = "particle_modules";
    const LABEL: &'static str = "Particle Curves";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Add alongside Particle Emitter. Curves multiply size, opacity, RGB color and speed over normalized lifetime (0–1). New particles capture the curves; existing particles finish with their original settings. Native playback integrates motion on the GPU; headless playback uses the CPU reference.";
    fn fields() -> &'static [Field] {
        {
            const FIELDS: &[Field] = &[Field::bool("enabled", "Enable curves")];
            FIELDS
        }
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        (key == "enabled").then_some(FieldValue::Bool(self.enabled))
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        ensure!(key == "enabled", "unknown particle module field");
        self.enabled = value.bool()?;
        Ok(())
    }
}
impl Authored for Modules {
    fn validate(&self) -> Result<()> {
        for curve in [&self.curves.size, &self.curves.opacity, &self.curves.speed]
            .into_iter()
            .chain(&self.curves.color)
        {
            curve.validate()?;
            ensure!(
                curve.keys.len() <= 64
                    && curve.duration() <= 1.
                    && curve.keys.iter().all(|k| k.value >= 0.
                        && k.value <= 8.
                        && k.incoming.abs() <= 100.
                        && k.outgoing.abs() <= 100.),
                "particle curves need 1–64 keys over age 0–1, values 0–8 and bounded tangents"
            );
        }
        Ok(())
    }
}
