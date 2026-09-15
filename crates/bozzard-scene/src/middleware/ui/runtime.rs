use super::super::signals::{Kind, Signal, Signals};
use super::*;
use crate::{SceneInstance, World};
use anyhow::Context;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub language: Option<String>,
    pub text_scale: Option<f32>,
    pub high_contrast: Option<bool>,
    pub reduced_motion: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub scroll: f32,
    pub text: Option<String>,
    pub value: Option<f32>,
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub widgets: BTreeMap<String, State>,
    pub focus: Option<String>,
    #[serde(skip)]
    pub active: Option<String>,
    #[serde(skip)]
    pub pointer: Option<[f32; 2]>,
}
pub enum Input {
    PointerMove([f32; 2]),
    ScrollAt { point: [f32; 2], delta: f32 },
    ScrollFocused(f32),
    ScrollObject { owner: String, delta: f32 },
    PointerDown([f32; 2]),
    PointerUp([f32; 2]),
    FocusNext { reverse: bool },
    Activate,
    Adjust(f32),
    SetValue { owner: String, value: f32 },
    Key(String),
    Focus(String),
    ActivateObject(String),
    CancelPointer,
}
pub enum Control {
    Text(String),
    Value(f32),
    Visible(bool),
    Enabled(bool),
    Focus,
}
fn emit(world: &mut World, element: &Element, value: f32) -> Result<()> {
    if world.resource::<Signals>().is_none() {
        world.insert_resource(Signals::default());
    }
    world.resource_mut::<Signals>().unwrap().emit(
        &element.owner,
        Signal {
            kind: Kind::Ui,
            name: element.widget.event.clone(),
            other: None,
            value,
        },
    )
}
impl SceneInstance {
    pub fn control_ui(&self, world: &mut World, owner: &str, control: Control) -> Result<()> {
        let entity = self.entity(owner).context("UI target missing")?;
        let widget = world
            .get::<Widget>(entity)
            .context("target has no UI Widget")?;
        match &control {
            Control::Text(t) => ensure!(t.len() <= 4096, "UI text exceeds 4096 UTF-8 bytes"),
            Control::Value(v) => ensure!(
                v.is_finite() && (widget.min..=widget.max).contains(v),
                "UI value outside widget range"
            ),
            _ => {}
        }
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let runtime = world.resource_mut::<Runtime>().unwrap();
        if matches!(control, Control::Focus) {
            runtime.focus = Some(owner.into());
            return Ok(());
        }
        let state = runtime.widgets.entry(owner.into()).or_default();
        match control {
            Control::Text(t) => state.text = Some(t),
            Control::Value(v) => state.value = Some(v),
            Control::Visible(v) => state.visible = Some(v),
            Control::Enabled(v) => state.enabled = Some(v),
            Control::Focus => {}
        }
        Ok(())
    }
    pub fn set_ui_language(&self, world: &mut World, language: &str) -> Result<()> {
        ensure!(
            !language.is_empty()
                && language.len() <= 32
                && language
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "invalid UI language"
        );
        ensure!(
            self.entities
                .values()
                .filter_map(|e| world.get::<Localization>(*e))
                .any(|l| l.translations.contains_key(language)
                    || l.language == language
                    || l.fallback == language),
            "UI language is not authored in this scene"
        );
        if world.resource::<Preferences>().is_none() {
            world.insert_resource(Preferences::default());
        }
        world.resource_mut::<Preferences>().unwrap().language = Some(language.into());
        Ok(())
    }
    pub fn ui_input(
        &self,
        world: &mut World,
        layer: Layer,
        size: [f32; 2],
        input: Input,
    ) -> Result<bool> {
        let frame = self.ui_frame(world, layer, size)?;
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let previous_focus = runtime.focus.clone();
        if runtime.focus.as_deref().is_some_and(|id| {
            frame.element(id).is_none_or(|e| {
                !e.enabled
                    || !e.widget.kind.interactive()
                    || (e.clip.size.iter().any(|s| *s <= 0.) && frame.scroll_ancestor(e).is_none())
            })
        }) {
            runtime.focus = None;
        }
        let result = (|| -> Result<bool> {
            let update = |runtime: &mut Runtime,
                          world: &mut World,
                          element: &Element,
                          requested: f32|
             -> Result<()> {
                let w = &element.widget;
                let value = ((requested - w.min) / w.step).round() * w.step + w.min;
                let value = value.clamp(w.min, w.max);
                let old = runtime
                    .widgets
                    .get(&element.owner)
                    .and_then(|s| s.value)
                    .unwrap_or(w.value);
                if value != old {
                    runtime
                        .widgets
                        .entry(element.owner.clone())
                        .or_default()
                        .value = Some(value);
                    emit(world, element, value)?;
                }
                Ok(())
            };
            let activate = |runtime: &mut Runtime, world: &mut World, e: &Element| -> Result<()> {
                runtime.focus = Some(e.owner.clone());
                if e.widget.kind == WidgetKind::Toggle {
                    let old = runtime
                        .widgets
                        .get(&e.owner)
                        .and_then(|s| s.value)
                        .unwrap_or(e.widget.value);
                    let value = if old > (e.widget.min + e.widget.max) * 0.5 {
                        e.widget.min
                    } else {
                        e.widget.max
                    };
                    runtime.widgets.entry(e.owner.clone()).or_default().value = Some(value);
                    emit(world, e, value)
                } else {
                    emit(world, e, e.value)
                }
            };
            match input {
                Input::ScrollFocused(delta) => {
                    ensure!(delta.is_finite(), "invalid UI scroll");
                    let parent = runtime
                        .focus
                        .as_deref()
                        .and_then(|id| frame.element(id))
                        .and_then(|e| frame.scroll_ancestor(e))
                        .or_else(|| frame.elements.iter().rev().find(|e| e.widget.scrollable));
                    if let Some(e) = parent {
                        runtime.widgets.entry(e.owner.clone()).or_default().scroll =
                            (e.scroll + delta).clamp(0., e.scroll_max);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::ScrollAt { point, delta } => {
                    ensure!(
                        point.iter().all(|v| v.is_finite()) && delta.is_finite(),
                        "invalid UI scroll"
                    );
                    if let Some(e) = frame.elements.iter().rev().find(|e| {
                        e.widget.scrollable && e.rect.contains(point) && e.clip.contains(point)
                    }) {
                        runtime.widgets.entry(e.owner.clone()).or_default().scroll =
                            (e.scroll + delta).clamp(0., e.scroll_max);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::ScrollObject { owner, delta } => {
                    ensure!(delta.is_finite(), "invalid UI scroll");
                    if let Some(e) = frame.element(&owner).filter(|e| e.widget.scrollable) {
                        runtime.widgets.entry(owner).or_default().scroll =
                            (e.scroll + delta).clamp(0., e.scroll_max);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::PointerMove(p) | Input::PointerDown(p) | Input::PointerUp(p) => {
                    ensure!(p.iter().all(|v| v.is_finite()), "invalid UI pointer");
                    runtime.pointer = Some(p);
                    let captured = runtime.active.is_some();
                    let hit = frame.hit(p);
                    if matches!(input, Input::PointerDown(_)) {
                        runtime.active = hit.map(|e| e.owner.clone());
                        if let Some(e) = hit {
                            runtime.focus = Some(e.owner.clone());
                        }
                    }
                    if let Some(active) = runtime
                        .active
                        .as_deref()
                        .and_then(|id| frame.element(id))
                        .filter(|e| e.enabled && e.widget.kind == WidgetKind::Slider)
                    {
                        let inset = active.widget.padding;
                        let x = active.rect.min[0] + inset[0] * active.scale;
                        let width =
                            (active.rect.size[0] - (inset[0] + inset[2]) * active.scale).max(1.);
                        let value = active.widget.min
                            + ((p[0] - x) / width).clamp(0., 1.)
                                * (active.widget.max - active.widget.min);
                        update(&mut runtime, world, active, value)?;
                    }
                    if matches!(input, Input::PointerUp(_)) {
                        let active = runtime.active.take();
                        if let Some(e) = hit.filter(|e| {
                            active.as_deref() == Some(&e.owner)
                                && e.widget.kind != WidgetKind::Slider
                        }) {
                            activate(&mut runtime, world, e)?;
                        }
                    }
                    Ok(captured || frame.blocks_pointer(p))
                }
                Input::FocusNext { reverse } => {
                    let nodes = frame.focusable();
                    if nodes.is_empty() {
                        return Ok(false);
                    }
                    let current = runtime
                        .focus
                        .as_deref()
                        .and_then(|id| nodes.iter().position(|e| e.owner == id));
                    let next = match current {
                        Some(i) if reverse => (i + nodes.len() - 1) % nodes.len(),
                        Some(i) => (i + 1) % nodes.len(),
                        None if reverse => nodes.len() - 1,
                        None => 0,
                    };
                    runtime.focus = Some(nodes[next].owner.clone());
                    Ok(true)
                }
                Input::Activate => {
                    if let Some(e) = runtime
                        .focus
                        .as_deref()
                        .and_then(|id| frame.element(id))
                        .filter(|e| e.enabled && e.widget.kind.interactive())
                    {
                        activate(&mut runtime, world, e)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::Adjust(delta) => {
                    ensure!(
                        delta.is_finite() && delta.abs() <= 1000.,
                        "invalid UI adjustment"
                    );
                    if let Some(e) = runtime
                        .focus
                        .as_deref()
                        .and_then(|id| frame.element(id))
                        .filter(|e| e.enabled && e.widget.kind == WidgetKind::Slider)
                    {
                        update(&mut runtime, world, e, e.value + delta * e.widget.step)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::SetValue { owner, value } => {
                    ensure!(value.is_finite(), "invalid UI value");
                    if let Some(e) = frame
                        .element(&owner)
                        .filter(|e| e.enabled && e.widget.kind == WidgetKind::Slider)
                    {
                        update(&mut runtime, world, e, value)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::Key(key) => {
                    let canonical = crate::keys::canonical(&key).context("unknown UI key")?;
                    if let Some(e) = frame.focusable().into_iter().rev().find(|e| {
                        e.widget
                            .shortcuts
                            .iter()
                            .any(|s| crate::keys::canonical(s) == Some(canonical))
                    }) {
                        activate(&mut runtime, world, e)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::Focus(owner) => {
                    if let Some(e) = frame
                        .element(&owner)
                        .filter(|e| e.enabled && e.widget.kind.interactive())
                    {
                        runtime.focus = Some(e.owner.clone());
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::ActivateObject(owner) => {
                    if let Some(e) = frame
                        .element(&owner)
                        .filter(|e| e.enabled && e.widget.kind.interactive())
                    {
                        activate(&mut runtime, world, e)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                Input::CancelPointer => {
                    runtime.pointer = None;
                    runtime.active = None;
                    Ok(false)
                }
            }
        })();
        if runtime.focus != previous_focus
            && let Some(element) = runtime.focus.as_deref().and_then(|id| frame.element(id))
            && let Some(parent) = frame.scroll_ancestor(element)
        {
            let inside = parent
                .rect
                .inset(parent.widget.padding.map(|p| p * parent.scale));
            let top = element.rect.min[1];
            let bottom = top + element.rect.size[1];
            let shift = if top < inside.min[1] {
                top - inside.min[1]
            } else {
                (bottom - inside.min[1] - inside.size[1]).max(0.)
            };
            runtime
                .widgets
                .entry(parent.owner.clone())
                .or_default()
                .scroll = (parent.scroll + shift).clamp(0., parent.scroll_max);
        }
        world.insert_resource(runtime);
        result
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.text_scale
                .is_none_or(|v| v.is_finite() && (1.0..=3.).contains(&v)),
            "invalid UI text scale"
        );
        ensure!(
            self.language.as_ref().is_none_or(|s| !s.is_empty()
                && s.len() <= 32
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
            "invalid UI language preference"
        );
        Ok(())
    }
}
impl Runtime {
    pub fn validate(&self, scene: &Scene) -> Result<()> {
        ensure!(self.widgets.len() <= 1024, "too many saved widgets");
        for (owner, state) in &self.widgets {
            ensure!(
                state.scroll.is_finite() && (0.0..=1e6).contains(&state.scroll),
                "invalid saved UI scroll"
            );
            let object = scene
                .objects
                .iter()
                .find(|o| &o.id == owner)
                .context("saved widget owner missing")?;
            let widget = super::super::registry::get::<Widget>(object)?
                .context("saved UI Widget missing")?;
            ensure!(
                state.text.as_ref().is_none_or(|t| t.len() <= 4096)
                    && state
                        .value
                        .is_none_or(|v| v.is_finite() && (widget.min..=widget.max).contains(&v)),
                "invalid saved widget state"
            );
        }
        ensure!(
            self.focus.as_ref().is_none_or(|id| scene
                .objects
                .iter()
                .any(|o| &o.id == id && o.extras.contains_key(Widget::NAME))),
            "invalid saved UI focus"
        );
        Ok(())
    }
}
