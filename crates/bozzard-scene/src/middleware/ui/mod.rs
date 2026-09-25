//! Authored screen canvases, layout, localization and accessible widget interaction.
use super::registry::{self, Authored, PreviewPolicy};
mod layout;
mod menus;
mod runtime;
use crate::{
    AssetKind, Component, Field, FieldValue, GamePhase, Layer, Object, Scene, Ui, VectorRole,
};
use anyhow::{Result, ensure};
pub use layout::{Element, Frame, Rect};
pub use runtime::{Control, Input, Preferences, Runtime, ScriptEvent};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Always,
    Ready,
    Playing,
    Paused,
    GameOver,
}
impl Phase {
    pub fn matches(self, phase: Option<GamePhase>) -> bool {
        self == Self::Always
            || matches!(
                (self, phase),
                (Self::Ready, Some(GamePhase::Ready))
                    | (Self::Playing, Some(GamePhase::Playing))
                    | (Self::Paused, Some(GamePhase::Paused))
                    | (Self::GameOver, Some(GamePhase::GameOver))
            )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    #[default]
    Fit,
    Width,
    Height,
    Pixels,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Canvas {
    pub enabled: bool,
    pub layer: Layer,
    pub reference: [f32; 2],
    pub scaling: ScaleMode,
    pub phase: Phase,
    pub order: i32,
    pub text_scale: f32,
    pub high_contrast: bool,
    pub reduced_motion: bool,
}
impl Default for Canvas {
    fn default() -> Self {
        Self {
            enabled: true,
            layer: Layer::ThreeD,
            reference: [1280., 720.],
            scaling: ScaleMode::Fit,
            phase: Phase::Always,
            order: 0,
            text_scale: 1.,
            high_contrast: false,
            reduced_motion: false,
        }
    }
}
impl Component for Canvas {
    const NAME: &'static str = "ui_canvas";
    const LABEL: &'static str = "UI Canvas";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Parent widgets under this object. Anchors stretch with the viewport; sizes/offsets use reference pixels. Phase visibility creates scene-authored menus. Keyboard focus, accessible labels, text scale and high contrast apply throughout the canvas.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::options("layer", "View", &["2D", "3D"]),
            Field::vector2("reference", "Reference size", VectorRole::Scale, 1.),
            Field::options("scaling", "Scale", &["Fit", "Width", "Height", "Pixels"]),
            Field::options(
                "phase",
                "Visible during",
                &["Always", "Ready", "Playing", "Paused", "Game over"],
            ),
            Field::integer_range("order", "Draw order", 1., -10000., 10000.),
            Field::range("text_scale", "Text scale", 0.05, 1., 3.),
            Field::bool("high_contrast", "High contrast"),
            Field::bool("reduced_motion", "Reduced motion"),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "layer" => FieldValue::Index(usize::from(self.layer == Layer::ThreeD)),
            "reference" => FieldValue::Vector([self.reference[0], self.reference[1], 0.]),
            "scaling" => FieldValue::Index(self.scaling as usize),
            "phase" => FieldValue::Index(self.phase as usize),
            "order" => FieldValue::Number(self.order as f32),
            "text_scale" => FieldValue::Number(self.text_scale),
            "high_contrast" => FieldValue::Bool(self.high_contrast),
            "reduced_motion" => FieldValue::Bool(self.reduced_motion),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = v.bool()?,
            "layer" => {
                self.layer = if v.index()? == 0 {
                    Layer::TwoD
                } else {
                    Layer::ThreeD
                }
            }
            "reference" => {
                let p = v.vector()?;
                self.reference = [p[0], p[1]];
            }
            "scaling" => {
                self.scaling = *[
                    ScaleMode::Fit,
                    ScaleMode::Width,
                    ScaleMode::Height,
                    ScaleMode::Pixels,
                ]
                .get(v.index()?)
                .ok_or_else(|| anyhow::anyhow!("invalid canvas scale mode"))?
            }
            "phase" => {
                self.phase = *[
                    Phase::Always,
                    Phase::Ready,
                    Phase::Playing,
                    Phase::Paused,
                    Phase::GameOver,
                ]
                .get(v.index()?)
                .ok_or_else(|| anyhow::anyhow!("invalid canvas phase"))?
            }
            "order" => self.order = v.number()? as i32,
            "text_scale" => self.text_scale = v.number()?,
            "high_contrast" => self.high_contrast = v.bool()?,
            "reduced_motion" => self.reduced_motion = v.bool()?,
            _ => anyhow::bail!("unknown canvas field"),
        };
        Ok(())
    }
}
impl Authored for Canvas {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;

    fn hide_in_preview(object: &mut Object) -> Result<()> {
        if let Some(mut canvas) = registry::get::<Self>(object)? {
            // Keep the authored-menu marker so legacy menu migration does not
            // recreate menus when the last visible canvas is hidden.
            canvas.enabled = false;
            registry::set(object, &canvas)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.reference
                .iter()
                .all(|v| v.is_finite() && (16.0..=16384.).contains(v))
                && (-10000..=10000).contains(&self.order)
                && self.text_scale.is_finite()
                && (1.0..=3.).contains(&self.text_scale),
            "invalid canvas reference size, order or text scale"
        );
        Ok(())
    }
    fn validate_scene(&self, owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(owner.parent.is_none(), "UI Canvas must be a root object");
        ensure!(
            !owner.extras.contains_key(Widget::NAME),
            "place UI Widgets under their Canvas, not on it"
        );
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 32,
            "scene exceeds 32 canvases"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    #[default]
    Panel,
    Label,
    Image,
    Button,
    Toggle,
    Slider,
}
impl WidgetKind {
    pub fn interactive(self) -> bool {
        matches!(self, Self::Button | Self::Toggle | Self::Slider)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Absolute,
    Row,
    Column,
    Grid,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Anchors {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub pivot: [f32; 2],
    pub offset: [f32; 2],
    pub size: [f32; 2],
}
impl Default for Anchors {
    fn default() -> Self {
        Self {
            min: [0.5; 2],
            max: [0.5; 2],
            pivot: [0.5; 2],
            offset: [0.; 2],
            size: [240., 48.],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Widget {
    pub enabled: bool,
    pub visible: bool,
    pub kind: WidgetKind,
    pub anchors: Anchors,
    pub order: i32,
    pub layout: Layout,
    pub columns: u32,
    pub gap: f32,
    pub padding: [f32; 4],
    pub grow: f32,
    pub clip_children: bool,
    pub auto_text_height: bool,
    pub scrollable: bool,
    pub text: String,
    pub locale_key: String,
    pub binding: String,
    pub font_size: f32,
    pub text_color: [f32; 4],
    pub background: [f32; 4],
    pub image: String,
    pub uv: [f32; 4],
    pub border: [f32; 4],
    pub event: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub focus_order: i32,
    pub accessible_name: String,
    pub description: String,
    pub shortcuts: Vec<String>,
}
impl Default for Widget {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: true,
            kind: WidgetKind::Panel,
            anchors: Default::default(),
            order: 0,
            layout: Layout::Absolute,
            columns: 2,
            gap: 12.,
            padding: [12.; 4],
            grow: 0.,
            clip_children: true,
            auto_text_height: true,
            scrollable: false,
            text: String::new(),
            locale_key: String::new(),
            binding: String::new(),
            font_size: 20.,
            text_color: [0.95, 0.97, 1., 1.],
            background: [0.035, 0.055, 0.09, 0.95],
            image: String::new(),
            uv: [0., 0., 1., 1.],
            border: [0.; 4],
            event: "activate".into(),
            value: 0.,
            min: 0.,
            max: 1.,
            step: 0.1,
            focus_order: 0,
            accessible_name: String::new(),
            description: String::new(),
            shortcuts: vec![],
        }
    }
}
impl Component for Widget {
    const NAME: &'static str = "ui_widget";
    const LABEL: &'static str = "UI Widget";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Place under a UI Canvas or another widget. Button/toggle/slider actions emit On UI Event on this object. Tab/Shift-Tab move focus; Enter/Space activate; arrow keys adjust sliders. Borders use left/top/right/bottom source pixels for nine-slice images.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("visible", "Visible"),
            Field::bool("enabled", "Enabled"),
            Field::options(
                "kind",
                "Widget",
                &["Panel", "Label", "Image", "Button", "Toggle", "Slider"],
            ),
            Field::vector2("anchor_min", "Anchor minimum", VectorRole::Position, 0.01)
                .clamp(0., 1.),
            Field::vector2("anchor_max", "Anchor maximum", VectorRole::Position, 0.01)
                .clamp(0., 1.),
            Field::vector2("pivot", "Pivot", VectorRole::Position, 0.01).clamp(0., 1.),
            Field::vector2("offset", "Offset", VectorRole::Position, 1.),
            Field::vector2("size", "Size / stretch offset", VectorRole::Scale, 1.),
            Field::options(
                "layout",
                "Child layout",
                &["Absolute", "Row", "Column", "Grid"],
            ),
            Field::integer_range("columns", "Grid columns", 1., 1., 64.),
            Field::range("gap", "Child gap", 1., 0., 1000.),
            Field::range("grow", "Flex weight", 0.1, 0., 100.),
            Field::bool("clip_children", "Clip children"),
            Field::bool("auto_text_height", "Fit text height"),
            Field::bool("scrollable", "Scroll overflowing content"),
            Field::integer_range("order", "Draw order", 1., -10000., 10000.),
            Field::body_text("text", "Text"),
            Field::text("locale_key", "Localization key", "menu.start"),
            Field::options(
                "binding",
                "Live text",
                &["None", "Game message", "Game title", "Game instructions"],
            ),
            Field::range("font_size", "Font size", 1., 6., 200.),
            Field::vector("text_color", "Text color", VectorRole::Color, 0.01).clamp(0., 1.),
            Field::range("text_opacity", "Text opacity", 0.01, 0., 1.),
            Field::vector("background", "Background", VectorRole::Color, 0.01).clamp(0., 1.),
            Field::range("opacity", "Background opacity", 0.01, 0., 1.),
            Field::asset("image", "Image", AssetKind::Image),
            Field::text("event", "Event name", "activate"),
            Field::range("value", "Value", 0.01, -100000., 100000.),
            Field::range("min", "Minimum", 0.01, -100000., 100000.),
            Field::range("max", "Maximum", 0.01, -100000., 100000.),
            Field::range("step", "Step", 0.01, 0.0001, 100000.),
            Field::integer_range("focus_order", "Tab order", 1., -10000., 10000.),
            Field::text("accessible_name", "Accessible label", "Uses text if empty"),
            Field::body_text("description", "Accessible description"),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        let pair = |p: [f32; 2]| FieldValue::Vector([p[0], p[1], 0.]);
        Some(match key {
            "visible" => FieldValue::Bool(self.visible),
            "enabled" => FieldValue::Bool(self.enabled),
            "kind" => FieldValue::Index(self.kind as usize),
            "anchor_min" => pair(self.anchors.min),
            "anchor_max" => pair(self.anchors.max),
            "pivot" => pair(self.anchors.pivot),
            "offset" => pair(self.anchors.offset),
            "size" => pair(self.anchors.size),
            "layout" => FieldValue::Index(self.layout as usize),
            "columns" => FieldValue::Number(self.columns as f32),
            "gap" => FieldValue::Number(self.gap),
            "grow" => FieldValue::Number(self.grow),
            "clip_children" => FieldValue::Bool(self.clip_children),
            "auto_text_height" => FieldValue::Bool(self.auto_text_height),
            "scrollable" => FieldValue::Bool(self.scrollable),
            "order" => FieldValue::Number(self.order as f32),
            "text" => FieldValue::Text(self.text.clone()),
            "locale_key" => FieldValue::Text(self.locale_key.clone()),
            "binding" => FieldValue::Index(
                ["", "game.message", "game.title", "game.instructions"]
                    .iter()
                    .position(|s| *s == self.binding)
                    .unwrap_or(0),
            ),
            "font_size" => FieldValue::Number(self.font_size),
            "text_color" => {
                FieldValue::Vector([self.text_color[0], self.text_color[1], self.text_color[2]])
            }
            "text_opacity" => FieldValue::Number(self.text_color[3]),
            "background" => {
                FieldValue::Vector([self.background[0], self.background[1], self.background[2]])
            }
            "opacity" => FieldValue::Number(self.background[3]),
            "image" => FieldValue::Text(self.image.clone()),
            "event" => FieldValue::Text(self.event.clone()),
            "value" => FieldValue::Number(self.value),
            "min" => FieldValue::Number(self.min),
            "max" => FieldValue::Number(self.max),
            "step" => FieldValue::Number(self.step),
            "focus_order" => FieldValue::Number(self.focus_order as f32),
            "accessible_name" => FieldValue::Text(self.accessible_name.clone()),
            "description" => FieldValue::Text(self.description.clone()),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        let pair = |v: &FieldValue| -> Result<[f32; 2]> {
            let p = v.vector()?;
            Ok([p[0], p[1]])
        };
        match key {
            "visible" => self.visible = v.bool()?,
            "enabled" => self.enabled = v.bool()?,
            "kind" => {
                self.kind = *[
                    WidgetKind::Panel,
                    WidgetKind::Label,
                    WidgetKind::Image,
                    WidgetKind::Button,
                    WidgetKind::Toggle,
                    WidgetKind::Slider,
                ]
                .get(v.index()?)
                .ok_or_else(|| anyhow::anyhow!("invalid widget kind"))?
            }
            "anchor_min" => self.anchors.min = pair(&v)?,
            "anchor_max" => self.anchors.max = pair(&v)?,
            "pivot" => self.anchors.pivot = pair(&v)?,
            "offset" => self.anchors.offset = pair(&v)?,
            "size" => self.anchors.size = pair(&v)?,
            "layout" => {
                self.layout = *[Layout::Absolute, Layout::Row, Layout::Column, Layout::Grid]
                    .get(v.index()?)
                    .ok_or_else(|| anyhow::anyhow!("invalid widget layout"))?
            }
            "columns" => self.columns = v.number()? as u32,
            "gap" => self.gap = v.number()?,
            "grow" => self.grow = v.number()?,
            "clip_children" => self.clip_children = v.bool()?,
            "auto_text_height" => self.auto_text_height = v.bool()?,
            "scrollable" => self.scrollable = v.bool()?,
            "order" => self.order = v.number()? as i32,
            "text" => self.text = v.text()?.into(),
            "locale_key" => self.locale_key = v.text()?.into(),
            "binding" => {
                self.binding = ["", "game.message", "game.title", "game.instructions"]
                    .get(v.index()?)
                    .ok_or_else(|| anyhow::anyhow!("invalid widget text binding"))?
                    .to_string()
            }
            "font_size" => self.font_size = v.number()?,
            "text_color" => self.text_color[..3].copy_from_slice(&v.vector()?),
            "text_opacity" => self.text_color[3] = v.number()?,
            "background" => self.background[..3].copy_from_slice(&v.vector()?),
            "opacity" => self.background[3] = v.number()?,
            "image" => self.image = v.text()?.into(),
            "event" => self.event = v.text()?.into(),
            "value" => self.value = v.number()?,
            "min" => {
                self.min = v.number()?;
                self.value = self.value.max(self.min);
            }
            "max" => {
                self.max = v.number()?;
                self.value = self.value.min(self.max);
            }
            "step" => self.step = v.number()?,
            "focus_order" => self.focus_order = v.number()? as i32,
            "accessible_name" => self.accessible_name = v.text()?.into(),
            "description" => self.description = v.text()?.into(),
            _ => anyhow::bail!("unknown widget field"),
        };
        Ok(())
    }
}
impl Authored for Widget {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;

    fn validate(&self) -> Result<()> {
        let a = self.anchors;
        ensure!(
            a.min
                .iter()
                .chain(&a.max)
                .chain(&a.pivot)
                .all(|v| v.is_finite() && (0.0..=1.).contains(v))
                && (0..2).all(|i| a.min[i] <= a.max[i])
                && a.offset
                    .iter()
                    .chain(&a.size)
                    .all(|v| v.is_finite() && v.abs() <= 10000.),
            "invalid widget anchors or size"
        );
        ensure!(
            self.columns > 0
                && self.columns <= 64
                && self.gap.is_finite()
                && (0.0..=1000.).contains(&self.gap)
                && self.grow.is_finite()
                && (0.0..=100.).contains(&self.grow)
                && self
                    .padding
                    .iter()
                    .chain(&self.border)
                    .all(|v| v.is_finite() && (0.0..=1000.).contains(v))
                && (-10000..=10000).contains(&self.order)
                && (-10000..=10000).contains(&self.focus_order),
            "invalid widget layout"
        );
        ensure!(
            self.text.len() <= 4096
                && self.locale_key.len() <= 128
                && self.accessible_name.len() <= 512
                && self.description.len() <= 2048
                && !self.event.is_empty()
                && self.event.len() <= 256
                && ["", "game.message", "game.title", "game.instructions"]
                    .contains(&self.binding.as_str()),
            "invalid widget text or event"
        );
        ensure!(
            self.font_size.is_finite()
                && (6.0..=200.).contains(&self.font_size)
                && self
                    .text_color
                    .iter()
                    .chain(&self.background)
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid widget appearance"
        );
        ensure!(
            self.uv
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v))
                && self.uv[2] > 0.
                && self.uv[3] > 0.
                && self.uv[0] + self.uv[2] <= 1.000001
                && self.uv[1] + self.uv[3] <= 1.000001,
            "invalid widget image region"
        );
        ensure!(
            [self.min, self.max, self.step, self.value]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100000.)
                && self.max > self.min
                && self.step > 0.
                && (self.min..=self.max).contains(&self.value),
            "invalid widget value range"
        );
        ensure!(
            self.shortcuts.len() <= 8
                && self
                    .shortcuts
                    .iter()
                    .all(|s| crate::keys::canonical(s).is_some()),
            "invalid widget keyboard shortcut"
        );
        Ok(())
    }
    fn validate_scene(&self, _owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 1024,
            "scene exceeds 1024 UI widgets"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Localization {
    pub language: String,
    pub fallback: String,
    pub translations: Arc<BTreeMap<String, BTreeMap<String, String>>>,
}
impl Default for Localization {
    fn default() -> Self {
        Self {
            language: "en".into(),
            fallback: "en".into(),
            translations: Arc::new(BTreeMap::new()),
        }
    }
}
impl Localization {
    pub fn text<'a>(&'a self, language: Option<&str>, key: &str) -> Option<&'a str> {
        self.translations
            .get(language.unwrap_or(&self.language))
            .and_then(|table| table.get(key))
            .or_else(|| {
                self.translations
                    .get(&self.fallback)
                    .and_then(|table| table.get(key))
            })
            .map(String::as_str)
    }
}
impl Component for Localization {
    const NAME: &'static str = "localization";
    const LABEL: &'static str = "Localization";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Widget localization keys select a translation. Missing keys use the fallback language, then the widget's literal text. Blueprint Set UI Language changes the active language.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::text("language", "Language", "en"),
            Field::text("fallback", "Fallback", "en"),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(FieldValue::Text(match key {
            "language" => self.language.clone(),
            "fallback" => self.fallback.clone(),
            _ => return None,
        }))
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        match key {
            "language" => self.language = v.text()?.into(),
            "fallback" => self.fallback = v.text()?.into(),
            _ => anyhow::bail!("unknown localization field"),
        };
        Ok(())
    }
}
impl Authored for Localization {
    const PREVIEW: PreviewPolicy = PreviewPolicy::Retain;

    fn validate(&self) -> Result<()> {
        let valid_language = |s: &str| {
            !s.is_empty()
                && s.len() <= 32
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        };
        ensure!(
            valid_language(&self.language)
                && valid_language(&self.fallback)
                && self.translations.len() <= 64,
            "invalid localization language"
        );
        let mut bytes = 0;
        for (lang, table) in self.translations.iter() {
            ensure!(
                valid_language(lang) && table.len() <= 4096,
                "invalid localization table"
            );
            for (key, text) in table {
                ensure!(
                    !key.is_empty() && key.len() <= 128 && text.len() <= 4096,
                    "invalid localized string"
                );
                bytes += key.len() + text.len();
            }
        }
        ensure!(bytes <= 4 * 1024 * 1024, "localized text exceeds 4 MiB");
        Ok(())
    }
    fn validate_scene(&self, _owner: &Object, scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            scene
                .objects
                .iter()
                .filter(|o| o.extras.contains_key(Self::NAME))
                .count()
                <= 1,
            "scene has more than one Localization table"
        );
        Ok(())
    }
}
