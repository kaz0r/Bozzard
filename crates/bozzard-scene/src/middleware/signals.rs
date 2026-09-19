//! Transient middleware notifications delivered through typed Blueprint event nodes.
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Timeline,
    Animation,
    Ui,
    Navigation,
    Sprite,
}
#[derive(Clone, Debug)]
pub struct Signal {
    pub kind: Kind,
    pub name: String,
    pub other: Option<String>,
    pub value: f32,
}
#[derive(Default)]
pub struct Signals {
    owners: BTreeMap<String, Vec<Signal>>,
    count: usize,
}
impl Signals {
    pub(crate) fn remove_objects(&mut self, ids: &std::collections::BTreeSet<String>) {
        self.owners.retain(|id, _| !ids.contains(id));
        for signals in self.owners.values_mut() {
            for signal in signals {
                if signal.other.as_ref().is_some_and(|id| ids.contains(id)) {
                    signal.other = None;
                }
            }
        }
        self.count = self.owners.values().map(Vec::len).sum();
    }
    pub const LIMIT: usize = 4096;
    pub fn begin(&mut self, kind: Kind) {
        self.count = 0;
        self.owners.retain(|_, signals| {
            signals.retain(|s| s.kind != kind);
            self.count += signals.len();
            !signals.is_empty()
        });
    }
    pub fn emit(&mut self, owner: &str, signal: Signal) -> Result<()> {
        ensure!(self.count < Self::LIMIT, "middleware event budget exceeded");
        ensure!(
            signal.name.len() <= 256 && signal.value.is_finite(),
            "invalid middleware event"
        );
        self.owners.entry(owner.into()).or_default().push(signal);
        self.count += 1;
        Ok(())
    }
    pub fn for_owner(&self, owner: &str, kind: Kind) -> impl Iterator<Item = &Signal> {
        self.owners
            .get(owner)
            .into_iter()
            .flatten()
            .filter(move |s| s.kind == kind)
    }
}
