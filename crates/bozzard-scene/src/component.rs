//! One registry for every authorable component.
//!
//! A component's scene key, editor label, availability rule, add/remove behaviour, prefab
//! three-way merge and field metadata all live in one row here, so adding a component means a
//! module next to its struct plus one row, and no consumer has to repeat the list. Components
//! marked [`Ui::Generic`] are drawn by the editor from `fields()`; the rest keep hand-written UI
//! but still use the registry for add, remove, availability and prefab merge.
use super::*;
use crate::shader_graph::ShaderGraph;
use std::sync::{OnceLock, RwLock};

/// How the editor draws a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    /// Rendered from `fields()`; no editor code per component.
    Generic,
    /// Hand-written editor section, usually because editing needs assets or derived readouts.
    Custom,
}

/// What a vector field means, so the editor can label its axes the way authors expect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorRole {
    Position,
    Offset,
    Scale,
    RotationDegrees,
    /// Linear RGB.
    Color,
}

/// Enough type information for the editor to choose a widget. Deliberately scalar: nested and
/// tagged shapes are flattened into separate fields.
#[derive(Clone, Copy, Debug)]
pub enum FieldKind {
    Bool,
    Number {
        speed: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    /// A whole number, drawn without decimals.
    Integer {
        speed: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    Vector {
        role: VectorRole,
        speed: f32,
        /// Lower bound applied to every axis, for sizes and distances that must stay positive.
        min: Option<f32>,
        /// Upper bound applied to every axis, for normalized anchors and normalized offsets.
        max: Option<f32>,
        /// 2 for XY pairs (screen anchors, UV repeats), 3 for world vectors. The value keeps three
        /// lanes either way; the editor draws the first `axes`.
        axes: u8,
    },
    Text {
        hint: &'static str,
        /// Multi-line body text, as opposed to a name or an identifier.
        multiline: bool,
    },
    /// A drawable texture: one of the built-in patterns or an image asset.
    Texture,
    /// A drawable mesh: a primitive, an imported mesh asset, or the object's own cooked surface.
    Mesh,
    /// Index into a fixed option list, for enums and tags.
    Options {
        options: &'static [&'static str],
    },
    /// Scene object ID. `filter` limits the picker to eligible objects; `activates` names the view
    /// this reference also switches to, so a follow-camera pick needs no hand-written editor code.
    Object {
        filter: Option<fn(&Object) -> bool>,
        activates: Option<Layer>,
    },
    /// A bit set drawn as labelled toggles, for collision layers and masks.
    Flags {
        labels: &'static [&'static str],
    },
    /// Asset catalog ID of the given kind.
    Asset(AssetKind),
}

/// One editable field of a component.
#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub help: &'static str,
    /// Hide the widget while this returns false. Fields are independent, so a tag field can gate
    /// the values that only exist for one variant.
    pub visible: Option<fn(&Object) -> bool>,
}

impl Field {
    pub const fn bool(key: &'static str, label: &'static str) -> Self {
        Self::new(key, label, FieldKind::Bool)
    }
    pub const fn number(key: &'static str, label: &'static str, speed: f32) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                speed,
                min: None,
                max: None,
            },
        )
    }
    /// A whole number: a count, a seed, a budget.
    pub const fn integer(key: &'static str, label: &'static str, speed: f32) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Integer {
                speed,
                min: None,
                max: None,
            },
        )
    }
    pub const fn integer_range(
        key: &'static str,
        label: &'static str,
        speed: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Integer {
                speed,
                min: Some(min),
                max: Some(max),
            },
        )
    }
    pub const fn range(
        key: &'static str,
        label: &'static str,
        speed: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                speed,
                min: Some(min),
                max: Some(max),
            },
        )
    }
    /// A two-component vector, for screen anchors, pixel offsets and UV repeats.
    pub const fn vector2(
        key: &'static str,
        label: &'static str,
        role: VectorRole,
        speed: f32,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Vector {
                role,
                speed,
                min: None,
                max: None,
                axes: 2,
            },
        )
    }
    /// A texture: a built-in pattern or an image asset.
    pub const fn texture(key: &'static str, label: &'static str) -> Self {
        Self::new(key, label, FieldKind::Texture)
    }
    /// A mesh: a primitive, an imported mesh asset, or the object's cooked surface.
    pub const fn mesh(key: &'static str, label: &'static str) -> Self {
        Self::new(key, label, FieldKind::Mesh)
    }
    /// A single-line text value, for names and identifiers.
    pub const fn text(key: &'static str, label: &'static str, hint: &'static str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Text {
                hint,
                multiline: false,
            },
        )
    }
    /// Body text, edited on several lines.
    pub const fn body_text(key: &'static str, label: &'static str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Text {
                hint: "",
                multiline: true,
            },
        )
    }
    /// Clamp a vector's axes to a range, for normalized anchors and offsets.
    pub const fn clamp(mut self, min: f32, max: f32) -> Self {
        if let FieldKind::Vector {
            role, speed, axes, ..
        } = self.kind
        {
            self.kind = FieldKind::Vector {
                role,
                speed,
                min: Some(min),
                max: Some(max),
                axes,
            };
        }
        self
    }
    pub const fn vector(
        key: &'static str,
        label: &'static str,
        role: VectorRole,
        speed: f32,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Vector {
                role,
                speed,
                min: None,
                max: None,
                axes: 3,
            },
        )
    }
    /// A vector that must stay positive on every axis.
    pub const fn vector_min(
        key: &'static str,
        label: &'static str,
        role: VectorRole,
        speed: f32,
        min: f32,
    ) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Vector {
                role,
                speed,
                min: Some(min),
                max: None,
                axes: 3,
            },
        )
    }
    /// A number with a lower bound only.
    pub const fn min(key: &'static str, label: &'static str, speed: f32, min: f32) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Number {
                speed,
                min: Some(min),
                max: None,
            },
        )
    }
    pub const fn options(
        key: &'static str,
        label: &'static str,
        options: &'static [&'static str],
    ) -> Self {
        Self::new(key, label, FieldKind::Options { options })
    }
    pub const fn object(key: &'static str, label: &'static str) -> Self {
        Self::new(
            key,
            label,
            FieldKind::Object {
                filter: None,
                activates: None,
            },
        )
    }
    /// A bit field drawn as one labelled toggle per named bit.
    pub const fn flags(
        key: &'static str,
        label: &'static str,
        labels: &'static [&'static str],
    ) -> Self {
        Self::new(key, label, FieldKind::Flags { labels })
    }
    pub const fn asset(key: &'static str, label: &'static str, kind: AssetKind) -> Self {
        Self::new(key, label, FieldKind::Asset(kind))
    }
    /// Tooltip text for this field.
    pub const fn help(mut self, help: &'static str) -> Self {
        self.help = help;
        self
    }
    /// Show this field only while `visible` holds.
    pub const fn shown_when(mut self, visible: fn(&Object) -> bool) -> Self {
        self.visible = Some(visible);
        self
    }
    /// Restrict an object picker to eligible objects.
    pub const fn filtered(mut self, filter: fn(&Object) -> bool) -> Self {
        if let FieldKind::Object { activates, .. } = self.kind {
            self.kind = FieldKind::Object {
                filter: Some(filter),
                activates,
            };
        }
        self
    }
    /// Selecting this reference also makes it the active camera for `layer`.
    pub const fn activates(mut self, layer: Layer) -> Self {
        if let FieldKind::Object { filter, .. } = self.kind {
            self.kind = FieldKind::Object {
                filter,
                activates: Some(layer),
            };
        }
        self
    }
    const fn new(key: &'static str, label: &'static str, kind: FieldKind) -> Self {
        Self {
            key,
            label,
            kind,
            help: "",
            visible: None,
        }
    }
}

/// A field's current value, independent of the component it came from.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Bool(bool),
    Number(f32),
    Vector([f32; 3]),
    Text(String),
    Index(usize),
    Object(String),
    Flags(u32),
    Asset(Option<String>),
}

impl FieldValue {
    fn expected(&self, want: &str) -> anyhow::Error {
        anyhow::anyhow!("expected {want}, got {self:?}")
    }
    pub fn bool(&self) -> Result<bool> {
        if let Self::Bool(value) = self {
            Ok(*value)
        } else {
            Err(self.expected("a boolean"))
        }
    }
    pub fn number(&self) -> Result<f32> {
        if let Self::Number(value) = self {
            Ok(*value)
        } else {
            Err(self.expected("a number"))
        }
    }
    pub fn vector(&self) -> Result<[f32; 3]> {
        if let Self::Vector(value) = self {
            Ok(*value)
        } else {
            Err(self.expected("a vector"))
        }
    }
    pub fn text(&self) -> Result<&str> {
        if let Self::Text(value) = self {
            Ok(value)
        } else {
            Err(self.expected("text"))
        }
    }
    pub fn index(&self) -> Result<usize> {
        if let Self::Index(value) = self {
            Ok(*value)
        } else {
            Err(self.expected("an option index"))
        }
    }
    pub fn object(&self) -> Result<&str> {
        if let Self::Object(value) = self {
            Ok(value)
        } else {
            Err(self.expected("an object reference"))
        }
    }
    pub fn flags(&self) -> Result<u32> {
        if let Self::Flags(value) = self {
            Ok(*value)
        } else {
            Err(self.expected("layer flags"))
        }
    }
    pub fn asset(&self) -> Result<&Option<String>> {
        if let Self::Asset(value) = self {
            Ok(value)
        } else {
            Err(self.expected("an asset reference"))
        }
    }
}

/// A component's own fields, addressed by the keys in [`Component::fields`].
pub trait Component: Clone + PartialEq + Send + Sync + 'static {
    /// Scene key, matching the `Object` field name.
    const NAME: &'static str;
    /// Editor label, which is not always the scene key (`gravity` is shown as "Rigidbody").
    const LABEL: &'static str;
    const UI: Ui = Ui::Custom;
    /// Component-level hint shown under its fields.
    const HELP: &'static str = "";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[];
        FIELDS
    }
    /// Read one field. `None` for unknown keys and for values that do not exist in the current
    /// variant.
    fn field(&self, _key: &str) -> Option<FieldValue> {
        None
    }
    /// Write one field. Enum and tag fields may change the component's variant.
    fn set_field(&mut self, key: &str, _value: FieldValue) -> Result<()> {
        anyhow::bail!("{} has no field '{key}'", Self::LABEL)
    }
}

/// The scene state an add may consult: editor context the component cannot supply itself.
pub struct AddContext<'a> {
    pub layer: Layer,
    pub scene: &'a Scene,
    /// Bounds of the selected drawable's imported mesh, when the asset store could supply them.
    pub bounds: Option<[[f32; 3]; 2]>,
    /// Collider cooked from the selected drawable, when an asset store was available.
    pub cooked: Option<MeshCollider>,
}

impl AddContext<'_> {
    /// Default box for a new collider, sized to the selected mesh when its bounds are known.
    pub fn collider(&self) -> BoxCollider {
        self.bounds.map_or_else(BoxCollider::default, |bounds| {
            let size = Vec3::from(bounds[1]) - Vec3::from(bounds[0]);
            BoxCollider {
                size: size.max(Vec3::splat(0.001)).to_array(),
                ..Default::default()
            }
        })
    }
}

/// One registry row: everything a consumer needs to handle a component generically.
pub struct ComponentType {
    pub name: &'static str,
    pub label: &'static str,
    pub ui: Ui,
    pub help: &'static str,
    pub fields: fn() -> &'static [Field],
    pub get: fn(&Object, &str) -> Option<FieldValue>,
    pub set: fn(&mut Object, &str, FieldValue) -> Result<()>,
    pub present: fn(&Object) -> bool,
    /// Whether this object may take the component. Incompatible components stay mutually exclusive.
    pub available: fn(&Object) -> bool,
    pub add: fn(&mut Object, &AddContext) -> Result<()>,
    pub remove: fn(&mut Object, &mut Scene),
    /// Prefab refresh: take the source value only where the instance still matches its baseline.
    pub merge: fn(&mut Object, &Object, &Object),
    /// Read this component's value from a scene file into the object. The place a renamed or
    /// reshaped field would migrate its old value.
    pub load: fn(&mut Object, serde_json::Value) -> Result<()>,
    /// This component's value for a scene file, or `None` when the object has no such component.
    pub save: fn(&Object) -> Result<Option<serde_json::Value>>,
}

impl ComponentType {
    pub fn field(&self, key: &str) -> Option<Field> {
        (self.fields)().iter().copied().find(|f| f.key == key)
    }
    /// Fields visible for this object, in authoring order.
    pub fn visible_fields(&self, object: &Object) -> Vec<Field> {
        (self.fields)()
            .iter()
            .copied()
            .filter(|field| field.visible.is_none_or(|visible| visible(object)))
            .collect()
    }
}

fn merge_opt<T: Clone + PartialEq>(current: &mut Option<T>, old: &Option<T>, source: &Option<T>) {
    if current == old {
        *current = source.clone();
    }
}
fn present_or_absent<T>(value: &Option<T>) -> bool {
    value.is_some()
}

/// A component row whose `Object` field is a plain `Option<T>`.
///
/// Keeps `get`, `set`, `present` and `merge` derived from the field, so a row only states what is
/// genuinely component-specific.
macro_rules! component_row {
    ($type:ty, $field:ident, $available:expr, $add:expr, $remove:expr) => {
        ComponentType {
            name: <$type as Component>::NAME,
            label: <$type as Component>::LABEL,
            ui: <$type as Component>::UI,
            help: <$type as Component>::HELP,
            fields: <$type as Component>::fields,
            get: |object, key| object.$field.as_ref().and_then(|value| value.field(key)),
            set: |object, key, value| {
                object
                    .$field
                    .as_mut()
                    .with_context(|| format!("no {} on this object", <$type as Component>::LABEL))?
                    .set_field(key, value)
            },
            present: |object| present_or_absent(&object.$field),
            available: $available,
            add: $add,
            remove: $remove,
            merge: |current, old, source| {
                merge_opt(&mut current.$field, &old.$field, &source.$field)
            },
            load: |object, value| {
                object.$field = Some(serde_json::from_value(value)?);
                Ok(())
            },
            save: |object| {
                object
                    .$field
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(Into::into)
            },
        }
    };
}

// ---------------------------------------------------------------- Spin

impl Component for Spin {
    const NAME: &'static str = "spin";
    const LABEL: &'static str = "Spin";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Runs in Play mode only.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[Field::vector(
            "degrees_per_second",
            "Degrees/sec",
            VectorRole::RotationDegrees,
            0.5,
        )];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        (key == "degrees_per_second").then_some(FieldValue::Vector(self.0))
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        ensure!(key == "degrees_per_second", "Spin has no field '{key}'");
        self.0 = value.vector()?;
        Ok(())
    }
}

// ---------------------------------------------------------------- Gravity (Rigidbody)

impl Component for Gravity {
    const NAME: &'static str = "gravity";
    const LABEL: &'static str = "Rigidbody";
    const UI: Ui = Ui::Generic;
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::range("acceleration", "Acceleration (m/s²)", 0.1, 0.0001, 10000.0)
                .help("Positive is downward."),
            F::range("max_speed", "Max speed (m/s)", 0.5, 0.0001, 10000.0),
            F::range("jump_speed", "Jump speed (m/s)", 0.1, 0.0001, 10000.0)
                .help("Editor Play launch speed for the selected object."),
            F::range("mass", "Mass (kg)", 0.1, 0.0001, 1000000.0)
                .help("Dynamic body: contacts can rotate, topple and push it.")
                .shown_when(|object| object.player_controller.is_none()),
            F::range("friction", "Friction", 0.01, 0.0, 10.0)
                .shown_when(|object| object.player_controller.is_none()),
            F::range("restitution", "Restitution", 0.01, 0.0, 1.0)
                .shown_when(|object| object.player_controller.is_none()),
            F::range("linear_damping", "Linear drag", 0.01, 0.0, 100.0)
                .help("Per second. 0 keeps velocity; 1 removes it in one second.")
                .shown_when(|object| object.player_controller.is_none()),
            F::range("angular_damping", "Angular damping", 0.01, 0.0, 100.0)
                .shown_when(|object| object.player_controller.is_none()),
            F::range("gravity_scale", "Gravity scale", 0.01, -100.0, 100.0)
                .help("Multiplier on Acceleration. 0 floats, negative falls upward.")
                .shown_when(|object| object.player_controller.is_none()),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(FieldValue::Number(match key {
            "enabled" => return Some(FieldValue::Bool(self.enabled)),
            "acceleration" => self.acceleration,
            "max_speed" => self.max_speed,
            "jump_speed" => self.jump_speed,
            "mass" => self.mass,
            "friction" => self.friction,
            "restitution" => self.restitution,
            "linear_damping" => self.linear_damping,
            "angular_damping" => self.angular_damping,
            "gravity_scale" => self.gravity_scale,
            _ => return None,
        }))
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "acceleration" => self.acceleration = value.number()?,
            "max_speed" => self.max_speed = value.number()?,
            "jump_speed" => self.jump_speed = value.number()?,
            "mass" => self.mass = value.number()?,
            "friction" => self.friction = value.number()?,
            "restitution" => self.restitution = value.number()?,
            "linear_damping" => self.linear_damping = value.number()?,
            "angular_damping" => self.angular_damping = value.number()?,
            "gravity_scale" => self.gravity_scale = value.number()?,
            other => anyhow::bail!("Rigidbody has no field '{other}'"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Box collider

impl Component for BoxCollider {
    const NAME: &'static str = "collider";
    const LABEL: &'static str = "Box Collider";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Blocks swept box movement. Add Rigidbody to make it fall.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::vector("center", "Center", VectorRole::Offset, 0.05),
            F::vector_min("size", "Size", VectorRole::Scale, 0.05, 0.0001)
                .help("Full local dimensions, independent of the rendered mesh."),
            F::flags("layers", "Layers", LAYER_NAMES),
            F::flags("mask", "Collides with", LAYER_NAMES).help(
                "Two colliders meet only when each one's Layers intersect the other's Collides with.",
            ),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        match key {
            "enabled" => Some(FieldValue::Bool(self.enabled)),
            "center" => Some(FieldValue::Vector(self.center)),
            "size" => Some(FieldValue::Vector(self.size)),
            "layers" => Some(FieldValue::Flags(self.layers)),
            "mask" => Some(FieldValue::Flags(self.mask)),
            _ => None,
        }
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "center" => self.center = value.vector()?,
            "size" => self.size = value.vector()?,
            "layers" => self.layers = value.flags()?,
            "mask" => self.mask = value.flags()?,
            other => anyhow::bail!("Box Collider has no field '{other}'"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Mesh collider

impl Component for MeshCollider {
    const NAME: &'static str = "mesh_collider";
    const LABEL: &'static str = "Mesh Collider";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str =
        "Baked geometry follows Transform, but not later renderer or source edits.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::flags("layers", "Layers", LAYER_NAMES),
            F::flags("mask", "Collides with", LAYER_NAMES).help(
                "Two colliders meet only when each one's Layers intersect the other's Collides with.",
            ),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        match key {
            "enabled" => Some(FieldValue::Bool(self.enabled)),
            "layers" => Some(FieldValue::Flags(self.layers)),
            "mask" => Some(FieldValue::Flags(self.mask)),
            _ => None,
        }
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "layers" => self.layers = value.flags()?,
            "mask" => self.mask = value.flags()?,
            other => anyhow::bail!("Mesh Collider has no field '{other}'"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Player controller

impl Component for PlayerController {
    const NAME: &'static str = "player_controller";
    const LABEL: &'static str = "Player Controller";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "One root player per scene. Enabled collider + Rigidbody required. Selection does not control gameplay.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::object("camera", "Follow camera")
                .filtered(eligible_follow_camera)
                .activates(Layer::ThreeD)
                .help("Choosing a root perspective camera also activates it for 3D (one Undo)."),
            F::range("move_speed", "Move speed", 0.1, 0.0001, 10000.0),
            F::range("jump_speed", "Controller jump speed", 0.1, 0.0001, 10000.0)
                .help("Overrides Rigidbody's legacy selected-box jump speed."),
            F::range("capsule_radius", "Capsule radius", 0.01, 0.01, 100.0),
            F::range("capsule_height", "Capsule height", 0.05, 0.02, 1000.0)
                .help("Total height, caps included. At least twice the radius."),
            F::range("step_height", "Step height", 0.01, 0.0, 100.0)
                .help("Tallest obstacle auto-stepped, and the ground-snap distance."),
            F::range("slope_limit_degrees", "Slope limit °", 0.5, 0.0, 89.0)
                .help("Steeper floors slide the controller down instead of being climbed."),
            F::bool("snap_to_ground", "Snap to ground"),
            F::range("camera_distance", "Follow distance", 0.1, 0.0001, 10000.0),
            F::range("camera_height", "Follow height", 0.1, 0.0001, 10000.0),
            F::range("camera_radius", "Camera clearance", 0.05, 0.0001, 100.0),
            F::range(
                "orbit_sensitivity",
                "Orbit sensitivity",
                0.01,
                0.0001,
                100.0,
            ),
            F::number("fall_height", "Fall / respawn Y", 0.1),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "camera" => FieldValue::Object(self.camera.clone()),
            "move_speed" => FieldValue::Number(self.move_speed),
            "jump_speed" => FieldValue::Number(self.jump_speed),
            "capsule_radius" => FieldValue::Number(self.capsule_radius),
            "capsule_height" => FieldValue::Number(self.capsule_height),
            "step_height" => FieldValue::Number(self.step_height),
            "slope_limit_degrees" => FieldValue::Number(self.slope_limit_degrees),
            "snap_to_ground" => FieldValue::Bool(self.snap_to_ground),
            "camera_distance" => FieldValue::Number(self.camera_distance),
            "camera_height" => FieldValue::Number(self.camera_height),
            "camera_radius" => FieldValue::Number(self.camera_radius),
            "orbit_sensitivity" => FieldValue::Number(self.orbit_sensitivity),
            "fall_height" => FieldValue::Number(self.fall_height),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "camera" => self.camera = value.object()?.to_owned(),
            "move_speed" => self.move_speed = value.number()?,
            "jump_speed" => self.jump_speed = value.number()?,
            "capsule_radius" => self.capsule_radius = value.number()?,
            "capsule_height" => self.capsule_height = value.number()?,
            "step_height" => self.step_height = value.number()?,
            "slope_limit_degrees" => self.slope_limit_degrees = value.number()?,
            "snap_to_ground" => self.snap_to_ground = value.bool()?,
            "camera_distance" => self.camera_distance = value.number()?,
            "camera_height" => self.camera_height = value.number()?,
            "camera_radius" => self.camera_radius = value.number()?,
            "orbit_sensitivity" => self.orbit_sensitivity = value.number()?,
            "fall_height" => self.fall_height = value.number()?,
            other => anyhow::bail!("Player Controller has no field '{other}'"),
        }
        Ok(())
    }
}

/// A root perspective camera with no physics or follow behaviour of its own.
pub fn eligible_follow_camera(object: &Object) -> bool {
    object.parent.is_none()
        && object.spin.is_none()
        && object.gravity.is_none()
        && object.collider.is_none()
        && object.mesh_collider.is_none()
        && object.trigger.is_none()
        && object.player_controller.is_none()
        && matches!(object.camera, Some(Camera::Perspective { .. }))
}

// ---------------------------------------------------------------- Trigger

const TRIGGER_ACTIONS: &[&str] = &[
    "Sensor (Blueprints)",
    "Collectible",
    "Checkpoint",
    "Goal (all collectibles)",
];

fn trigger_action_index(action: &TriggerAction) -> usize {
    match action {
        TriggerAction::Sensor => 0,
        TriggerAction::Collectible => 1,
        TriggerAction::Checkpoint { .. } => 2,
        TriggerAction::Goal => 3,
    }
}
fn trigger_from_index(index: usize) -> Result<TriggerAction> {
    Ok(match index {
        0 => TriggerAction::Sensor,
        1 => TriggerAction::Collectible,
        2 => TriggerAction::Checkpoint {
            respawn: [0.0, 1.0, 0.0],
        },
        3 => TriggerAction::Goal,
        other => anyhow::bail!("unknown trigger action {other}"),
    })
}
fn trigger_is_checkpoint(object: &Object) -> bool {
    object
        .trigger
        .as_ref()
        .is_some_and(|t| matches!(t.action, TriggerAction::Checkpoint { .. }))
}

impl Component for Trigger {
    const NAME: &'static str = "trigger";
    const LABEL: &'static str = "Trigger";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Non-solid box. Collectibles hide once per run; progress survives falls, resets on Stop / Play or player R.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Trigger enabled"),
            F::vector("center", "Trigger center", VectorRole::Offset, 0.05),
            F::vector("size", "Trigger size", VectorRole::Scale, 0.05),
            F::flags("layers", "Layers", LAYER_NAMES),
            F::flags("mask", "Detects", LAYER_NAMES).help(
                "The volume only sees objects whose Layers intersect this. The player is on Default.",
            ),
            F::options("action", "Action", TRIGGER_ACTIONS),
            F::vector("respawn", "Respawn (world)", VectorRole::Position, 0.1)
                .shown_when(trigger_is_checkpoint)
                .help("Place above a safe floor, clear of solids and above Fall Y."),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.volume.enabled),
            "center" => FieldValue::Vector(self.volume.center),
            "size" => FieldValue::Vector(self.volume.size),
            "layers" => FieldValue::Flags(self.volume.layers),
            "mask" => FieldValue::Flags(self.volume.mask),
            "action" => FieldValue::Index(trigger_action_index(&self.action)),
            "respawn" => match self.action {
                TriggerAction::Checkpoint { respawn } => FieldValue::Vector(respawn),
                _ => return None,
            },
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.volume.enabled = value.bool()?,
            "center" => self.volume.center = value.vector()?,
            "size" => self.volume.size = value.vector()?,
            "layers" => self.volume.layers = value.flags()?,
            "mask" => self.volume.mask = value.flags()?,
            "action" => {
                let index = value.index()?;
                // Re-selecting Checkpoint keeps the authored respawn point.
                self.action = match (index, &self.action) {
                    (2, TriggerAction::Checkpoint { respawn }) => {
                        TriggerAction::Checkpoint { respawn: *respawn }
                    }
                    _ => trigger_from_index(index)?,
                };
            }
            "respawn" => {
                ensure!(
                    matches!(self.action, TriggerAction::Checkpoint { .. }),
                    "respawn only applies to a Checkpoint trigger"
                );
                if let TriggerAction::Checkpoint { respawn } = &mut self.action {
                    *respawn = value.vector()?;
                }
            }
            other => anyhow::bail!("Trigger has no field '{other}'"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Joint

const JOINT_KINDS: &[&str] = &["Fixed", "Hinge", "Ball socket", "Slider", "Rope"];

fn joint_kind_index(kind: JointKind) -> usize {
    match kind {
        JointKind::Fixed => 0,
        JointKind::Revolute => 1,
        JointKind::Spherical => 2,
        JointKind::Prismatic => 3,
        JointKind::Rope => 4,
    }
}
fn joint_from_index(index: usize) -> Result<JointKind> {
    Ok(match index {
        0 => JointKind::Fixed,
        1 => JointKind::Revolute,
        2 => JointKind::Spherical,
        3 => JointKind::Prismatic,
        4 => JointKind::Rope,
        other => anyhow::bail!("unknown joint kind {other}"),
    })
}
/// Objects a joint may name: an enabled collider or a Rigidbody root.
pub fn joint_endpoint(object: &Object) -> bool {
    object.collider.is_some_and(|c| c.enabled)
        || object.mesh_collider.as_ref().is_some_and(|c| c.enabled)
        || object.gravity.is_some()
}
fn joint_has_axis(object: &Object) -> bool {
    object
        .joint
        .as_ref()
        .is_some_and(|joint| matches!(joint.kind, JointKind::Revolute | JointKind::Prismatic))
}
fn joint_has_limits(object: &Object) -> bool {
    object.joint.as_ref().is_some_and(|joint| {
        joint.limits && matches!(joint.kind, JointKind::Revolute | JointKind::Prismatic)
    })
}
fn joint_is_rope(object: &Object) -> bool {
    object
        .joint
        .as_ref()
        .is_some_and(|joint| joint.kind == JointKind::Rope)
}

impl Component for Joint {
    const NAME: &'static str = "joint";
    const LABEL: &'static str = "Joint";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str =
        "Both endpoints need colliders. Anchors and axes are local to each body.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::object("other", "Other body").filtered(joint_endpoint),
            F::options("kind", "Kind", JOINT_KINDS),
            F::vector("anchor", "Anchor (self)", VectorRole::Offset, 0.05),
            F::vector("other_anchor", "Anchor (other)", VectorRole::Offset, 0.05),
            F::vector("axis", "Axis (self)", VectorRole::Offset, 0.05).shown_when(joint_has_axis),
            F::vector("other_axis", "Axis (other)", VectorRole::Offset, 0.05)
                .shown_when(joint_has_axis),
            F::bool("limits", "Use limits").shown_when(|object| {
                object.joint.as_ref().is_some_and(|joint| {
                    matches!(joint.kind, JointKind::Revolute | JointKind::Prismatic)
                })
            }),
            F::number("min_limit", "Min limit", 0.5).shown_when(joint_has_limits),
            F::number("max_limit", "Max limit", 0.5)
                .shown_when(|object| joint_has_limits(object) || joint_is_rope(object)),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "other" => FieldValue::Object(self.other.clone()),
            "kind" => FieldValue::Index(joint_kind_index(self.kind)),
            "anchor" => FieldValue::Vector(self.anchor),
            "other_anchor" => FieldValue::Vector(self.other_anchor),
            "axis" => FieldValue::Vector(self.axis),
            "other_axis" => FieldValue::Vector(self.other_axis),
            "limits" => FieldValue::Bool(self.limits),
            "min_limit" => FieldValue::Number(self.min_limit),
            "max_limit" => FieldValue::Number(self.max_limit),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "other" => self.other = value.object()?.to_owned(),
            "kind" => self.kind = joint_from_index(value.index()?)?,
            "anchor" => self.anchor = value.vector()?,
            "other_anchor" => self.other_anchor = value.vector()?,
            "axis" => self.axis = value.vector()?,
            "other_axis" => self.other_axis = value.vector()?,
            "limits" => self.limits = value.bool()?,
            "min_limit" => self.min_limit = value.number()?,
            "max_limit" => self.max_limit = value.number()?,
            other => anyhow::bail!("Joint has no field '{other}'"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Light

impl Component for Light {
    const NAME: &'static str = "light";
    const LABEL: &'static str = "Light";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Spot and directional lights shine along local -Z. Range is in world units, independent of scale.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::options("kind", "Kind", LIGHT_KINDS),
            F::vector("color", "Color", VectorRole::Color, 0.01),
            F::range("intensity", "Intensity", 1.0, 0.0, 100000.0).help(
                "Candela for point/spot; illuminance in lux for directional.",
            ),
            F::range("range", "Range", 0.1, 0.001, 100000.0)
                .help("Range ignores scale.")
                .shown_when(|object| light_kind(object) != Some(LightKind::Directional)),
            F::range("inner_angle_degrees", "Inner angle °", 0.2, 0.0, 89.9)
                .shown_when(|object| light_kind(object) == Some(LightKind::Spot)),
            F::range("outer_angle_degrees", "Outer angle °", 0.2, 0.0, 89.9)
                .shown_when(|object| light_kind(object) == Some(LightKind::Spot)),
            F::bool("shadows", "Cast shadows")
                .help("1024 px spot map, up to 8 shadowed spotlights; 6 × 512 px point map, up to 4 per scene. Disabled lights still reserve their kind's authored budget.")
                .shown_when(|object| light_kind(object) != Some(LightKind::Directional)),
            F::range("shadow_bias", "Depth bias", 0.001, 0.0, 1.0)
                .help("World units. Increase slightly to remove surface shadow speckling.")
                .shown_when(|object| light_kind(object) != Some(LightKind::Directional)),
            F::range("shadow_normal_bias", "Normal bias", 0.001, 0.0, 1.0)
                .help("World units. Large offsets can detach shadows from objects.")
                .shown_when(|object| light_kind(object) != Some(LightKind::Directional)),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "kind" => FieldValue::Index(match self.kind {
                LightKind::Point => 0,
                LightKind::Spot => 1,
                LightKind::Directional => 2,
            }),
            "color" => FieldValue::Vector(self.color),
            "intensity" => FieldValue::Number(self.intensity),
            "range" => FieldValue::Number(self.range),
            "inner_angle_degrees" => FieldValue::Number(self.inner_angle_degrees),
            "outer_angle_degrees" => FieldValue::Number(self.outer_angle_degrees),
            "shadows" => FieldValue::Bool(self.shadows),
            "shadow_bias" => FieldValue::Number(self.shadow_bias),
            "shadow_normal_bias" => FieldValue::Number(self.shadow_normal_bias),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "kind" => {
                self.kind = match value.index()? {
                    0 => LightKind::Point,
                    1 => LightKind::Spot,
                    2 => LightKind::Directional,
                    other => anyhow::bail!("unknown light kind {other}"),
                }
            }
            "color" => self.color = value.vector()?,
            "intensity" => self.intensity = value.number()?,
            "range" => self.range = value.number()?,
            "inner_angle_degrees" => {
                self.inner_angle_degrees = value.number()?;
                self.inner_angle_degrees = self.inner_angle_degrees.min(self.outer_angle_degrees);
            }
            "outer_angle_degrees" => {
                self.outer_angle_degrees = value.number()?;
                self.inner_angle_degrees = self.inner_angle_degrees.min(self.outer_angle_degrees);
            }
            "shadows" => self.shadows = value.bool()?,
            "shadow_bias" => self.shadow_bias = value.number()?,
            "shadow_normal_bias" => self.shadow_normal_bias = value.number()?,
            other => anyhow::bail!("Light has no field '{other}'"),
        }
        Ok(())
    }
}

const LIGHT_KINDS: &[&str] = &["Point", "Spot", "Directional"];

fn light_kind(object: &Object) -> Option<LightKind> {
    object.light.as_ref().map(|light| light.kind)
}

// ---------------------------------------------------------------- Camera

impl Component for Camera {
    const NAME: &'static str = "camera";
    const LABEL: &'static str = "Camera";
    const UI: Ui = Ui::Generic;
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::options("projection", "Projection", &["Orthographic", "Perspective"]),
            F::range("vertical_size", "Vertical size", 0.1, 0.001, 10000.0)
                .shown_when(|object| camera_projection(object) == Some(0)),
            F::range("vertical_fov_degrees", "Vertical FOV", 0.2, 1.0, 179.0)
                .shown_when(|object| camera_projection(object) == Some(1)),
            F::range("near", "Near", 0.01, 0.0001, 1000.0),
            F::range("far", "Far", 1.0, 0.0001, 1000000.0),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match (key, self) {
            ("projection", Self::Orthographic { .. }) => FieldValue::Index(0),
            ("projection", Self::Perspective { .. }) => FieldValue::Index(1),
            ("vertical_size", Self::Orthographic { vertical_size, .. }) => {
                FieldValue::Number(*vertical_size)
            }
            (
                "vertical_fov_degrees",
                Self::Perspective {
                    vertical_fov_degrees,
                    ..
                },
            ) => FieldValue::Number(*vertical_fov_degrees),
            ("near", Self::Orthographic { near, .. } | Self::Perspective { near, .. }) => {
                FieldValue::Number(*near)
            }
            ("far", Self::Orthographic { far, .. } | Self::Perspective { far, .. }) => {
                FieldValue::Number(*far)
            }
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        if key == "projection" {
            let (vertical_size, vertical_fov_degrees, near, far) = match self {
                Self::Orthographic {
                    vertical_size,
                    near,
                    far,
                } => (*vertical_size, 60.0, *near, *far),
                Self::Perspective {
                    vertical_fov_degrees,
                    near,
                    far,
                } => (7.0, *vertical_fov_degrees, *near, *far),
            };
            *self = match value.index()? {
                0 => Self::Orthographic {
                    vertical_size,
                    near,
                    far,
                },
                1 => Self::Perspective {
                    vertical_fov_degrees,
                    near,
                    far,
                },
                other => anyhow::bail!("unknown camera projection {other}"),
            };
            return Ok(());
        }
        match (key, self) {
            ("vertical_size", Self::Orthographic { vertical_size, .. }) => {
                *vertical_size = value.number()?
            }
            (
                "vertical_fov_degrees",
                Self::Perspective {
                    vertical_fov_degrees,
                    ..
                },
            ) => *vertical_fov_degrees = value.number()?,
            ("near", Self::Orthographic { near, .. } | Self::Perspective { near, .. }) => {
                *near = value.number()?
            }
            ("far", Self::Orthographic { far, .. } | Self::Perspective { far, .. }) => {
                *far = value.number()?
            }
            (other, _) => anyhow::bail!("Camera has no field '{other}'"),
        }
        Ok(())
    }
}

fn camera_projection(object: &Object) -> Option<usize> {
    object.camera.as_ref().map(|camera| match camera {
        Camera::Orthographic { .. } => 0,
        Camera::Perspective { .. } => 1,
    })
}

// ---------------------------------------------------------------- Remaining components

impl Component for Drawable {
    const NAME: &'static str = "drawable";
    const LABEL: &'static str = "Mesh Renderer";
    const UI: Ui = Ui::Generic;
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::options("layer", "Layer", &["3D", "2D"]),
            F::mesh("mesh", "Mesh"),
            F::texture("texture", "Texture"),
            F::vector("color", "Tint", VectorRole::Color, 0.0),
            F::vector2("uv_scale", "UV repeat", VectorRole::Scale, 0.05).clamp(0.001, 1000.0),
            F::bool("gi_static", "Contribute to GI bake (static)")
                .help("Known moving objects and their ancestors are excluded."),
            F::bool("metallic_override", "Override metallic"),
            F::range("metallic", "Metallic", 0.005, 0.0, 1.0).shown_when(drawable_metallic),
            F::bool("roughness_override", "Override roughness"),
            F::range("roughness", "Roughness", 0.005, 0.0, 1.0).shown_when(drawable_roughness),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        let number = |value: f32| Some(FieldValue::Number(value));
        Some(match key {
            "layer" => FieldValue::Index(if self.layer == Layer::ThreeD { 0 } else { 1 }),
            "mesh" => FieldValue::Text(mesh_id(&self.mesh)),
            "texture" => FieldValue::Text(texture_id(&self.texture)),
            "color" => FieldValue::Vector(self.color),
            "uv_scale" => FieldValue::Vector([self.uv_scale[0], self.uv_scale[1], 0.0]),
            "gi_static" => FieldValue::Bool(self.gi_static),
            "metallic_override" => FieldValue::Bool(self.metallic.is_some()),
            "metallic" => return number(self.metallic?),
            "roughness_override" => FieldValue::Bool(self.roughness.is_some()),
            "roughness" => return number(self.roughness?),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "layer" => {
                self.layer = if value.index()? == 0 {
                    Layer::ThreeD
                } else {
                    Layer::TwoD
                }
            }
            "mesh" => {
                let mesh = mesh_from_id(value.text()?, &self.mesh)?;
                // Overrides are keyed to the previous surface, so they cannot survive a swap.
                if mesh != self.mesh {
                    self.material_overrides.clear();
                }
                self.mesh = mesh;
            }
            "texture" => self.texture = texture_from_id(value.text()?),
            "color" => {
                let vector = value.vector()?;
                self.color = [vector[0], vector[1], vector[2]];
            }
            "uv_scale" => {
                let vector = value.vector()?;
                self.uv_scale = [vector[0], vector[1]];
            }
            "gi_static" => self.gi_static = value.bool()?,
            "metallic_override" => {
                self.metallic = value.bool()?.then_some(self.metallic.unwrap_or(0.0));
            }
            "metallic" => self.metallic = Some(value.number()?),
            "roughness_override" => {
                self.roughness = value.bool()?.then_some(self.roughness.unwrap_or(1.0));
            }
            "roughness" => self.roughness = Some(value.number()?),
            _ => anyhow::bail!("Mesh Renderer has no field '{key}'"),
        }
        Ok(())
    }
}

impl Component for Material {
    const NAME: &'static str = "material";
    const LABEL: &'static str = "Material";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str =
        "Per-object surface values; unchecked factors inherit the source material.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("texture_override", "Override texture / effect"),
            F::texture("texture", "Texture").shown_when(material_texture),
            F::vector("color", "Tint", VectorRole::Color, 0.0),
            F::vector2("uv_scale", "UV repeat", VectorRole::Scale, 0.05).clamp(0.001, 1000.0),
            F::bool("metallic_override", "Override metallic"),
            F::range("metallic", "Metallic", 0.005, 0.0, 1.0).shown_when(material_metallic),
            F::bool("roughness_override", "Override roughness"),
            F::range("roughness", "Roughness", 0.005, 0.0, 1.0).shown_when(material_roughness),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        let number = |value: f32| Some(FieldValue::Number(value));
        Some(match key {
            "texture_override" => FieldValue::Bool(self.texture.is_some()),
            "texture" => FieldValue::Text(texture_id(self.texture.as_ref()?)),
            "color" => FieldValue::Vector(self.color),
            "uv_scale" => FieldValue::Vector([self.uv_scale[0], self.uv_scale[1], 0.0]),
            "metallic_override" => FieldValue::Bool(self.metallic.is_some()),
            "metallic" => return number(self.metallic?),
            "roughness_override" => FieldValue::Bool(self.roughness.is_some()),
            "roughness" => return number(self.roughness?),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "texture_override" => {
                self.texture = value.bool()?.then_some(Texture::White);
            }
            "texture" => self.texture = Some(texture_from_id(value.text()?)),
            "color" => {
                let vector = value.vector()?;
                self.color = [vector[0], vector[1], vector[2]];
            }
            "uv_scale" => {
                let vector = value.vector()?;
                self.uv_scale = [vector[0], vector[1]];
            }
            "metallic_override" => {
                self.metallic = value.bool()?.then_some(self.metallic.unwrap_or(0.0));
            }
            "metallic" => self.metallic = Some(value.number()?),
            "roughness_override" => {
                self.roughness = value.bool()?.then_some(self.roughness.unwrap_or(1.0));
            }
            "roughness" => self.roughness = Some(value.number()?),
            _ => anyhow::bail!("Material has no field '{key}'"),
        }
        Ok(())
    }
}

fn drawable_metallic(object: &Object) -> bool {
    object
        .drawable
        .as_ref()
        .is_some_and(|drawable| drawable.metallic.is_some())
}

fn drawable_roughness(object: &Object) -> bool {
    object
        .drawable
        .as_ref()
        .is_some_and(|drawable| drawable.roughness.is_some())
}

fn material_texture(object: &Object) -> bool {
    object
        .material
        .as_ref()
        .is_some_and(|material| material.texture.is_some())
}

fn material_metallic(object: &Object) -> bool {
    object
        .material
        .as_ref()
        .is_some_and(|material| material.metallic.is_some())
}

fn material_roughness(object: &Object) -> bool {
    object
        .material
        .as_ref()
        .is_some_and(|material| material.roughness.is_some())
}

/// Texture and mesh fields carry a stable id string, so a field stays a plain value.
fn texture_id(texture: &Texture) -> String {
    match texture {
        Texture::White => "white".into(),
        Texture::Checker => "checker".into(),
        Texture::Normals => "normals".into(),
        Texture::ProceduralChecker => "procedural_checker".into(),
        Texture::Toon => "toon".into(),
        Texture::Asset(id) => id.clone(),
    }
}

fn texture_from_id(id: &str) -> Texture {
    match id {
        "white" => Texture::White,
        "checker" => Texture::Checker,
        "normals" => Texture::Normals,
        "procedural_checker" => Texture::ProceduralChecker,
        "toon" => Texture::Toon,
        other => Texture::Asset(other.to_owned()),
    }
}

fn mesh_id(mesh: &Mesh) -> String {
    match mesh {
        Mesh::Cube => "cube".into(),
        Mesh::Quad => "quad".into(),
        Mesh::Asset(id) => id.clone(),
        Mesh::Surface { .. } => "surface".into(),
    }
}

/// A cooked surface can only stay; choosing a primitive or asset replaces it (and drops the
/// surface-bound material overrides in `set_field`).
fn mesh_from_id(id: &str, current: &Mesh) -> Result<Mesh> {
    Ok(match id {
        "cube" => Mesh::Cube,
        "quad" => Mesh::Quad,
        "surface" => current.clone(),
        other => Mesh::Asset(other.to_owned()),
    })
}
impl Component for TextRendering {
    const NAME: &'static str = "text_rendering";
    const LABEL: &'static str = "Text Rendering";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str =
        "Top anchor · local XY plane · unlit · Transform controls position, rotation and scale";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Enabled"),
            F::bool("screen_hud", "Screen HUD").help(
                "Pinned to the viewport. Position and size use pixels; Transform stops moving it.",
            ),
            F::vector2("anchor", "Anchor X/Y", VectorRole::Offset, 0.01)
                .clamp(0.0, 1.0)
                .shown_when(is_screen_text),
            F::vector2("offset", "Offset X/Y", VectorRole::Offset, 1.0)
                .clamp(-10_000.0, 10_000.0)
                .shown_when(is_screen_text),
            F::options("layer", "Layer", &["3D", "2D"]),
            F::body_text("text", "Text").help("Plain text · max 4096 UTF-8 bytes"),
            F::options("font", "Font", &["Sans", "Monospace"]),
            F::range("font_size", "Font size", 0.01, 0.001, 1000.0)
                .help("Pixels in a screen HUD, local units otherwise."),
            F::bool("word_wrap", "Word wrap"),
            F::range("max_width", "Wrap width", 0.05, 0.001, 10_000.0)
                .help("Pixels in a screen HUD, local units otherwise.")
                .shown_when(is_wrapped_text),
            F::options("alignment", "Alignment", &["Left", "Center", "Right"]),
            F::vector("color", "Color", VectorRole::Color, 0.0),
            F::range("opacity", "Opacity", 0.01, 0.0, 1.0),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        let screen = self.screen.unwrap_or_default();
        let number = |value: f32| Some(FieldValue::Number(value));
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "screen_hud" => FieldValue::Bool(self.screen.is_some()),
            "anchor" => FieldValue::Vector([screen.anchor[0], screen.anchor[1], 0.0]),
            "offset" => FieldValue::Vector([screen.offset[0], screen.offset[1], 0.0]),
            "layer" => FieldValue::Index(if self.layer == Layer::ThreeD { 0 } else { 1 }),
            "text" => FieldValue::Text(self.text.clone()),
            "font" => FieldValue::Index(if self.font == TextFont::Sans { 0 } else { 1 }),
            "font_size" => return number(self.font_size),
            "word_wrap" => FieldValue::Bool(self.max_width.is_some()),
            "max_width" => return number(self.max_width?),
            "alignment" => FieldValue::Index(match self.alignment {
                TextAlignment::Left => 0,
                TextAlignment::Center => 1,
                TextAlignment::Right => 2,
            }),
            "color" => FieldValue::Vector([self.color[0], self.color[1], self.color[2]]),
            "opacity" => return number(self.color[3]),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "screen_hud" => {
                let screen = value.bool()?;
                // The two modes size text in different units, so a switch resets the size rather
                // than reinterpreting the old number in the new unit.
                self.screen = screen.then(ScreenText::default);
                self.font_size = if screen { 24.0 } else { 0.5 };
                self.max_width = None;
            }
            "anchor" => {
                let vector = value.vector()?;
                self.screen.get_or_insert_with(ScreenText::default).anchor = [vector[0], vector[1]];
            }
            "offset" => {
                let vector = value.vector()?;
                self.screen.get_or_insert_with(ScreenText::default).offset = [vector[0], vector[1]];
            }
            "layer" => {
                self.layer = if value.index()? == 0 {
                    Layer::ThreeD
                } else {
                    Layer::TwoD
                }
            }
            "text" => self.text = value.text()?.to_owned(),
            "font" => {
                self.font = if value.index()? == 0 {
                    TextFont::Sans
                } else {
                    TextFont::Monospace
                }
            }
            "font_size" => self.font_size = value.number()?,
            "word_wrap" => {
                self.max_width = value.bool()?.then_some(self.font_size * 8.0);
            }
            "max_width" => self.max_width = Some(value.number()?),
            "alignment" => {
                self.alignment = match value.index()? {
                    0 => TextAlignment::Left,
                    1 => TextAlignment::Center,
                    _ => TextAlignment::Right,
                }
            }
            "color" => {
                let vector = value.vector()?;
                self.color[..3].copy_from_slice(&vector[..3]);
            }
            "opacity" => self.color[3] = value.number()?,
            _ => anyhow::bail!("Text Rendering has no field '{key}'"),
        }
        Ok(())
    }
}

fn is_screen_text(object: &Object) -> bool {
    object
        .text_rendering
        .as_ref()
        .is_some_and(|text| text.screen.is_some())
}

fn is_wrapped_text(object: &Object) -> bool {
    object
        .text_rendering
        .as_ref()
        .is_some_and(|text| text.max_width.is_some())
}

impl Component for ParticleEmitter {
    const NAME: &'static str = "particle_emitter";
    const LABEL: &'static str = "Particle Emitter";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Live preview is in Effects. Play runs the full scene.";
    fn fields() -> &'static [Field] {
        use Field as F;
        const FIELDS: &[Field] = &[
            F::bool("enabled", "Emit particles"),
            F::options("kind", "Preset look", &["Smoke", "Ash", "Sparks & trails"])
                .help("Apply a preset for this look; this alone only retags the emitter."),
            F::range("rate", "Particles / second", 0.5, 0.0, 500.0),
            F::range("lifetime", "Lifetime seconds", 0.1, 0.1, 30.0),
            F::range("radius", "Emission radius", 0.05, 0.0, 20.0),
            F::range("start_size", "Start size", 0.01, 0.001, 20.0),
            F::range("end_size", "End size", 0.02, 0.001, 40.0),
            F::vector("color", "Color", VectorRole::Color, 0.0),
            F::range("opacity", "Opacity", 0.01, 0.0, 1.0),
            F::vector2("wind", "Wind X/Y", VectorRole::Offset, 0.02).clamp(-100.0, 100.0),
            F::range("turbulence", "Curl / turbulence", 0.05, 0.0, 10.0),
            F::range("trail_length", "Trail seconds", 0.01, 0.0, 1.0).shown_when(is_sparks),
            F::range("speed", "Launch speed", 0.1, 0.0, 50.0),
            F::range("spread", "Spread", 0.1, 0.0, 20.0),
            F::range("gravity", "Vertical acceleration", 0.1, -30.0, 30.0),
            F::range("drag", "Drag", 0.05, 0.0, 10.0),
            F::range("softness", "Soft intersections", 0.01, 0.001, 10.0),
            F::integer_range("max_particles", "Particle budget", 1.0, 1.0, 2048.0),
            F::integer("seed", "Random seed", 1.0),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        let number = |value: f32| Some(FieldValue::Number(value));
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "kind" => FieldValue::Index(
                ParticleKind::ALL
                    .iter()
                    .position(|kind| *kind == self.kind)
                    .unwrap_or_default(),
            ),
            "rate" => return number(self.rate),
            "lifetime" => return number(self.lifetime),
            "radius" => return number(self.radius),
            "start_size" => return number(self.start_size),
            "end_size" => return number(self.end_size),
            "color" => FieldValue::Vector(self.color),
            "opacity" => return number(self.opacity),
            "wind" => FieldValue::Vector([self.wind[0], self.wind[1], 0.0]),
            "turbulence" => return number(self.turbulence),
            "trail_length" => return number(self.trail_length),
            "speed" => return number(self.speed),
            "spread" => return number(self.spread),
            "gravity" => return number(self.gravity),
            "drag" => return number(self.drag),
            "softness" => return number(self.softness),
            "max_particles" => return number(self.max_particles as f32),
            "seed" => return number(self.seed as f32),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, value: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = value.bool()?,
            "kind" => {
                self.kind = *ParticleKind::ALL
                    .get(value.index()?)
                    .context("no such particle preset")?;
            }
            "rate" => self.rate = value.number()?,
            "lifetime" => self.lifetime = value.number()?,
            "radius" => self.radius = value.number()?,
            "start_size" => self.start_size = value.number()?,
            "end_size" => self.end_size = value.number()?,
            "color" => {
                let vector = value.vector()?;
                self.color = [vector[0], vector[1], vector[2]];
            }
            "opacity" => self.opacity = value.number()?,
            "wind" => {
                let vector = value.vector()?;
                self.wind = [vector[0], vector[1], self.wind[2]];
            }
            "turbulence" => self.turbulence = value.number()?,
            "trail_length" => self.trail_length = value.number()?,
            "speed" => self.speed = value.number()?,
            "spread" => self.spread = value.number()?,
            "gravity" => self.gravity = value.number()?,
            "drag" => self.drag = value.number()?,
            "softness" => self.softness = value.number()?,
            "max_particles" => self.max_particles = value.number()?.round() as u32,
            "seed" => self.seed = value.number()?.round() as u32,
            _ => anyhow::bail!("Particle Emitter has no field '{key}'"),
        }
        Ok(())
    }
}

fn is_sparks(object: &Object) -> bool {
    object
        .particle_emitter
        .as_ref()
        .is_some_and(|emitter| emitter.kind == ParticleKind::Sparks)
}
impl Component for BlueprintAttachment {
    const NAME: &'static str = "blueprints";
    const LABEL: &'static str = "Blueprint";
    const HELP: &'static str =
        "Attachment order defines execution order; variables use Graph, Object or Scene scope.";
}
impl Component for blueprint::Blackboard {
    const NAME: &'static str = "blackboard";
    const LABEL: &'static str = "Object Blackboard";
    const HELP: &'static str = "Edit shared declarations in Blueprint → Blackboards → Object.";
}
impl Component for ShaderGraph {
    const NAME: &'static str = "shader_graph";
    const LABEL: &'static str = "Shader Graph";
    const HELP: &'static str = "Compiles to a WGSL surface override over the material maps.";
}

/// Every authorable component, in inspector order.
pub const COMPONENTS: &[ComponentType] = &[
    component_row!(
        Drawable,
        drawable,
        |object| object.drawable.is_none(),
        |object, context| {
            object.drawable = Some(Drawable {
                metallic: None,
                roughness: None,
                gi_static: true,
                material_overrides: Vec::new(),
                layer: context.layer,
                mesh: Mesh::Cube,
                texture: Texture::White,
                color: [1.0; 3],
                uv_scale: [1.0; 2],
            });
            Ok(())
        },
        |object, _scene| {
            object.drawable = None;
            object.material = None;
        }
    ),
    component_row!(
        Material,
        material,
        |object| object.drawable.is_some() && object.material.is_none(),
        |object, _context| {
            // Loud rather than a silent no-op: the row is offered only for a mesh, but a caller
            // that ignores availability should not end up with nothing.
            let drawable = object
                .drawable
                .as_ref()
                .context("Material needs a Mesh Renderer")?;
            object.material = Some(Material::from_drawable(drawable));
            Ok(())
        },
        |object, _scene| object.material = None
    ),
    component_row!(
        MeshCollider,
        mesh_collider,
        |object| object.drawable.is_some()
            && object.mesh_collider.is_none()
            && object.player_controller.is_none()
            && object.trigger.is_none(),
        |object, context| {
            let collider = context
                .cooked
                .clone()
                .context("Mesh Collider needs the selected mesh; cooking needs loaded assets")?;
            if object.gravity.is_some_and(|gravity| gravity.enabled) {
                collider.mesh.convex_hull()?;
            }
            object.collider = None;
            object.mesh_collider = Some(collider);
            Ok(())
        },
        |object, _scene| {
            object.mesh_collider = None;
            object.gravity = None;
        }
    ),
    component_row!(
        BoxCollider,
        collider,
        |object| object.collider.is_none()
            && object.trigger.is_none()
            && object.mesh_collider.is_none(),
        |object, context| {
            object.collider = Some(context.collider());
            Ok(())
        },
        |object, _scene| {
            object.collider = None;
            object.gravity = None;
            object.player_controller = None;
        }
    ),
    component_row!(
        Gravity,
        gravity,
        |object| object.gravity.is_none() && object.trigger.is_none(),
        |object, context| {
            if let Some(mesh) = &object.mesh_collider {
                mesh.mesh.convex_hull()?;
            } else {
                object.collider.get_or_insert_with(|| context.collider());
            }
            object.gravity = Some(Gravity::default());
            Ok(())
        },
        |object, _scene| {
            object.gravity = None;
            object.player_controller = None;
        }
    ),
    component_row!(
        PlayerController,
        player_controller,
        |object| object.player_controller.is_none()
            && object.mesh_collider.is_none()
            && object.trigger.is_none()
            && object.parent.is_none()
            && object.spin.is_none(),
        |object, context| {
            object
                .collider
                .get_or_insert_with(|| context.collider())
                .enabled = true;
            object.gravity.get_or_insert_with(Gravity::default).enabled = true;
            object.player_controller = Some(PlayerController {
                camera: context
                    .scene
                    .views
                    .get(&Layer::ThreeD)
                    .cloned()
                    .unwrap_or_default(),
                ..Default::default()
            });
            Ok(())
        },
        |object, _scene| object.player_controller = None
    ),
    component_row!(
        Trigger,
        trigger,
        |object| object.trigger.is_none()
            && object.collider.is_none()
            && object.gravity.is_none()
            && object.mesh_collider.is_none(),
        |object, _context| {
            object.trigger = Some(Trigger {
                action: TriggerAction::Sensor,
                ..Default::default()
            });
            Ok(())
        },
        |object, _scene| object.trigger = None
    ),
    component_row!(
        Joint,
        joint,
        |object| object.joint.is_none(),
        |object, _context| {
            object.joint = Some(Joint::default());
            Ok(())
        },
        |object, _scene| object.joint = None
    ),
    component_row!(
        Light,
        light,
        |object| object.light.is_none(),
        |object, _context| {
            object.light = Some(Light::default());
            Ok(())
        },
        |object, _scene| object.light = None
    ),
    component_row!(
        ParticleEmitter,
        particle_emitter,
        |object| object.particle_emitter.is_none(),
        |object, _context| {
            object.particle_emitter = Some(ParticleEmitter::default());
            Ok(())
        },
        |object, _scene| object.particle_emitter = None
    ),
    component_row!(
        Spin,
        spin,
        |object| object.spin.is_none() && object.player_controller.is_none(),
        |object, _context| {
            object.spin = Some(Spin([0.0, 45.0, 0.0]));
            Ok(())
        },
        |object, _scene| object.spin = None
    ),
    component_row!(
        Camera,
        camera,
        |object| object.camera.is_none(),
        |object, _context| {
            object.camera = Some(Camera::Perspective {
                vertical_fov_degrees: 60.0,
                near: 0.1,
                far: 1000.0,
            });
            Ok(())
        },
        |object, scene| {
            object.camera = None;
            scene.views.retain(|_, id| id != &object.id);
        }
    ),
    component_row!(
        TextRendering,
        text_rendering,
        |object| object.text_rendering.is_none(),
        |object, context| {
            object.text_rendering = Some(TextRendering {
                layer: context.layer,
                ..Default::default()
            });
            Ok(())
        },
        |object, _scene| object.text_rendering = None
    ),
    ComponentType {
        name: ScriptManager::NAME,
        label: ScriptManager::LABEL,
        ui: ScriptManager::UI,
        help: ScriptManager::HELP,
        fields: ScriptManager::fields,
        // A script list is not field-shaped: it is an ordered set of asset references, drawn by
        // the editor's Script Manager section like the blueprint attachment list.
        get: |_, _| None,
        set: |_, key, _| anyhow::bail!("Script Manager has no field '{key}'"),
        present: |object| object.script_manager.is_some(),
        available: |object| {
            object
                .script_manager
                .as_ref()
                .is_none_or(|manager| manager.scripts.len() < MAX_SCRIPTS)
        },
        add: |object, _context| {
            let manager = object.script_manager.get_or_insert_default();
            manager.scripts.push(ScriptAttachment {
                enabled: true,
                script: String::new(),
            });
            Ok(())
        },
        remove: |object, _scene| object.script_manager = None,
        merge: |current, old, source| {
            merge_opt(
                &mut current.script_manager,
                &old.script_manager,
                &source.script_manager,
            )
        },
        load: |object, value| {
            object.script_manager = Some(serde_json::from_value(value)?);
            Ok(())
        },
        save: |object| {
            object
                .script_manager
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(Into::into)
        },
    },
    ComponentType {
        name: BlueprintAttachment::NAME,
        label: BlueprintAttachment::LABEL,
        ui: BlueprintAttachment::UI,
        help: BlueprintAttachment::HELP,
        fields: BlueprintAttachment::fields,
        get: |object, key| {
            object
                .blueprints
                .first()
                .and_then(|attachment| attachment.field(key))
        },
        set: |object, key, value| {
            object
                .blueprints
                .first_mut()
                .context("no Blueprint on this object")?
                .set_field(key, value)
        },
        present: |object| !object.blueprints.is_empty(),
        available: |object| object.blueprints.len() < MAX_BLUEPRINTS,
        add: |object, _context| {
            object.blueprints.push(BlueprintAttachment {
                enabled: true,
                graph: Blueprint::default(),
            });
            Ok(())
        },
        remove: |object, _scene| object.blueprints.clear(),
        merge: |current, old, source| {
            if current.blueprints == old.blueprints {
                current.blueprints = source.blueprints.clone();
            }
        },
        load: |object, value| {
            object.blueprints = serde_json::from_value(value)?;
            Ok(())
        },
        save: |object| {
            (!object.blueprints.is_empty())
                .then(|| serde_json::to_value(&object.blueprints))
                .transpose()
                .map_err(Into::into)
        },
    },
    ComponentType {
        name: blueprint::Blackboard::NAME,
        label: blueprint::Blackboard::LABEL,
        ui: blueprint::Blackboard::UI,
        help: blueprint::Blackboard::HELP,
        fields: blueprint::Blackboard::fields,
        get: |_, _| None,
        set: |object, key, value| object.blackboard.set_field(key, value),
        present: |object| !object.blackboard.is_empty(),
        available: |object| !object.blueprints.is_empty() && object.blackboard.is_empty(),
        add: |object, _| {
            object.blackboard.insert(
                "shared".into(),
                blueprint::BlackboardValue::Scalar(blueprint::Value::Number(0.)),
            );
            Ok(())
        },
        remove: |object, _| object.blackboard.clear(),
        merge: |current, old, source| {
            if current.blackboard == old.blackboard {
                current.blackboard = source.blackboard.clone();
            }
        },
        load: |object, value| {
            object.blackboard = serde_json::from_value(value)?;
            blueprint::validate_blackboard(&object.blackboard)
        },
        save: |object| {
            (!object.blackboard.is_empty())
                .then(|| serde_json::to_value(&object.blackboard))
                .transpose()
                .map_err(Into::into)
        },
    },
    component_row!(
        ShaderGraph,
        shader_graph,
        |object| object.shader_graph.is_none(),
        |object, _context| {
            object.shader_graph = Some(ShaderGraph::default());
            Ok(())
        },
        |object, _scene| object.shader_graph = None
    ),
];

/// Attachments per object, matching the editor's limit.
pub const MAX_BLUEPRINTS: usize = 16;

/// Components registered by the embedding game or project, so a gameplay type does not have to be
/// compiled into this crate.
///
/// Registration happens at startup and leaks the row to hand out `&'static` entries like the
/// built-in table; a game registers a handful of components, not thousands. A registered row is
/// stored in `Object::extras`, since only the built-in rows have typed fields here.
static REGISTERED: OnceLock<RwLock<BTreeMap<String, &'static ComponentType>>> = OnceLock::new();

fn registered() -> &'static RwLock<BTreeMap<String, &'static ComponentType>> {
    REGISTERED.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Add a component to the registry. Fails on a name that shadows an existing component.
pub fn register_component(entry: ComponentType) -> Result<()> {
    ensure!(
        !entry.name.is_empty()
            && entry
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
        "component names are snake_case: '{}'",
        entry.name
    );
    let mut components = registered().write().unwrap();
    // Built-ins are checked directly: taking a read lock here would deadlock against our own write.
    ensure!(
        COMPONENTS
            .iter()
            .all(|built_in| built_in.name != entry.name)
            && !components.contains_key(entry.name),
        "component '{}' is already registered",
        entry.name
    );
    components.insert(entry.name.into(), Box::leak(Box::new(entry)));
    Ok(())
}

/// Every authorable component: the built-in table first, then registered ones.
pub fn components() -> impl Iterator<Item = &'static ComponentType> {
    let registered: Vec<_> = registered().read().unwrap().values().copied().collect();
    COMPONENTS.iter().chain(registered)
}

/// The registry row for a scene key.
pub fn component_type(name: &str) -> Option<&'static ComponentType> {
    COMPONENTS
        .iter()
        .find(|entry| entry.name == name)
        .or_else(|| registered().read().unwrap().get(name).copied())
}

/// The row that owns an editor label, so menus can work in labels and callers in keys.
pub fn component_type_by_label(label: &str) -> Option<&'static ComponentType> {
    components().find(|entry| entry.label.eq_ignore_ascii_case(label))
}

/// Components this object does not already have and may take, in inspector order.
pub fn available_components(object: &Object) -> impl Iterator<Item = &'static ComponentType> {
    components().filter(move |entry| !(entry.present)(object) && (entry.available)(object))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    fn scene() -> Scene {
        Scene::from_json(
            r#"{
              "version": 1,
              "name": "Components",
              "views": {},
              "objects": [{ "id": "thing", "name": "Thing", "transform": { "translation": [0,0,0], "rotation_degrees": [0,0,0], "scale": [1,1,1] } }]
            }"#,
        )
        .unwrap()
    }

    fn object(scene: &Scene) -> Object {
        scene.objects[0].clone()
    }

    fn context<'a>(scene: &'a Scene) -> AddContext<'a> {
        AddContext {
            layer: Layer::ThreeD,
            scene,
            bounds: None,
            cooked: None,
        }
    }

    #[test]
    fn every_row_is_unique_and_complete() {
        let mut names = BTreeSet::new();
        let mut labels = BTreeSet::new();
        for entry in COMPONENTS {
            assert!(names.insert(entry.name), "duplicate name {}", entry.name);
            assert!(
                labels.insert(entry.label),
                "duplicate label {}",
                entry.label
            );
            assert!(
                component_type(entry.name).is_some_and(|found| found.label == entry.label),
                "{} is not reachable by name",
                entry.label
            );
            assert!(
                component_type_by_label(entry.label).is_some_and(|found| found.name == entry.name),
                "{} is not reachable by label",
                entry.label
            );
        }
    }

    #[test]
    fn the_registry_claims_every_component_of_an_object() {
        // Object also has id, name, parent and transform, which are identity rather than
        // components: they exist on every object and have no add/remove.
        let names: Vec<_> = COMPONENTS.iter().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            [
                "drawable",
                "material",
                "mesh_collider",
                "collider",
                "gravity",
                "player_controller",
                "trigger",
                "joint",
                "light",
                "particle_emitter",
                "spin",
                "camera",
                "text_rendering",
                "script_manager",
                "blueprints",
                "blackboard",
                "shader_graph",
            ]
        );
    }

    #[test]
    fn generic_components_round_trip_every_field_through_the_registry() {
        let scene = scene();
        // Rows are offered where they are available, so a probe object either stays bare or takes
        // the mesh some of them need first. Targets are remembered so a row that stops being
        // exercised fails here instead of quietly dropping out of the loop.
        let bare = object(&scene);
        let mut meshed = object(&scene);
        (component_type("drawable").expect("drawable row").add)(&mut meshed, &context(&scene))
            .unwrap();
        let mut covered = BTreeSet::new();
        for entry in COMPONENTS.iter().filter(|e| e.ui == Ui::Generic) {
            let Some(mut object) = [bare.clone(), meshed.clone()]
                .into_iter()
                .find(|object| (entry.available)(object))
            else {
                continue;
            };
            if (entry.add)(&mut object, &context(&scene)).is_err() {
                // Mesh Collider needs cooked geometry, which only a loaded asset store has.
                continue;
            }
            covered.insert(entry.name);
            assert!((entry.present)(&object), "{} did not appear", entry.label);
            for field in (entry.fields)() {
                if field.visible.is_some_and(|visible| !visible(&object)) {
                    continue;
                }
                let probe = match field.kind {
                    FieldKind::Bool => FieldValue::Bool(true),
                    FieldKind::Number { min, .. } => {
                        FieldValue::Number(min.unwrap_or(1.0).clamp(2.5, 1_000.0))
                    }
                    FieldKind::Integer { min, .. } => {
                        FieldValue::Number(min.unwrap_or(1.0).clamp(2.5, 1_000.0).round())
                    }
                    FieldKind::Vector { axes, .. } => FieldValue::Vector(if axes == 2 {
                        // A two-component field owns two lanes; the third is not part of its value.
                        [1.0, 2.0, 0.0]
                    } else {
                        [1.0, 2.0, 3.0]
                    }),
                    FieldKind::Text { .. } => FieldValue::Text("probe".into()),
                    FieldKind::Texture => FieldValue::Text("checker".into()),
                    FieldKind::Mesh => FieldValue::Text("cube".into()),
                    FieldKind::Options { .. } => FieldValue::Index(0),
                    FieldKind::Object { .. } => FieldValue::Object("other".into()),
                    FieldKind::Flags { labels } => {
                        FieldValue::Flags((1u32 << labels.len().max(1)) - 1)
                    }
                    FieldKind::Asset(_) => FieldValue::Asset(Some("asset".into())),
                };
                (entry.set)(&mut object, field.key, probe.clone())
                    .unwrap_or_else(|e| panic!("{}.{}: {e}", entry.label, field.key));
                assert_eq!(
                    (entry.get)(&object, field.key),
                    Some(probe.clone()),
                    "{}.{} did not survive a write",
                    entry.label,
                    field.key
                );
            }
            // Every generic component declares only fields it can read and write.
            for field in (entry.fields)() {
                assert!(
                    (entry.get)(&object, field.key).is_some() || field.visible.is_some(),
                    "{}.{} never reads back",
                    entry.label,
                    field.key
                );
            }
            (entry.add)(&mut object, &context(&scene)).unwrap_or(());
            let mut scene = scene.clone();
            (entry.remove)(&mut object, &mut scene);
            assert!(
                !(entry.present)(&object),
                "{} did not disappear",
                entry.label
            );
        }
        assert_eq!(
            covered,
            BTreeSet::from([
                "drawable",
                "material",
                "collider",
                "gravity",
                "player_controller",
                "light",
                "particle_emitter",
                "spin",
                "camera",
                "text_rendering",
                "trigger",
                "joint",
            ]),
            "every generic component except Mesh Collider is covered here"
        );
    }

    #[test]
    fn a_component_is_only_offered_where_it_is_compatible() {
        let scene = scene();
        let mut object = object(&scene);
        let trigger = component_type("trigger").unwrap();
        (trigger.add)(&mut object, &context(&scene)).unwrap();
        assert!(!(trigger.available)(&object), "Trigger must stay single");
        for entry in available_components(&object) {
            assert_ne!(entry.name, "collider", "a Trigger owns the volume");
            assert_ne!(entry.name, "gravity", "a Trigger is not a body");
            assert_ne!(entry.name, "player_controller", "a Trigger is not a player");
        }
        assert!(
            available_components(&object).any(|entry| entry.name == "text_rendering"),
            "unrelated components stay available"
        );
        // Removing the Trigger frees its slot again.
        (trigger.remove)(&mut object, &mut scene.clone());
        assert!(available_components(&object).any(|entry| entry.name == "collider"));
    }

    #[test]
    fn every_component_survives_a_round_trip_through_a_scene_file() {
        let scene = scene();
        let origin = object(&scene);
        let mut covered = 0;
        for entry in COMPONENTS {
            let mut object = origin.clone();
            if !(entry.available)(&object) || (entry.add)(&mut object, &context(&scene)).is_err() {
                continue;
            }
            covered += 1;
            let value = (entry.save)(&object)
                .unwrap()
                .unwrap_or_else(|| panic!("{} saves nothing after being added", entry.name));
            let mut loaded = origin.clone();
            (entry.load)(&mut loaded, value).unwrap_or_else(|error| {
                panic!("{} does not load its own value: {error}", entry.name)
            });
            assert!(
                (entry.present)(&loaded),
                "{} is absent after loading",
                entry.name
            );
            // Compare the fields, not the whole object: adding some components cascades (a
            // Rigidbody brings a Box Collider), and that is not the wire's business.
            for field in (entry.fields)() {
                assert_eq!(
                    (entry.get)(&loaded, field.key),
                    (entry.get)(&object, field.key),
                    "{} lost '{}' on the way out and back",
                    entry.name,
                    field.key
                );
            }
        }
        assert!(
            covered >= 12,
            "only {covered} components were reachable from a bare object"
        );
    }

    #[test]
    fn adding_twice_is_rejected_and_removal_is_reversible() {
        let scene = scene();
        let entry = component_type("light").unwrap();
        let mut object = object(&scene);
        (entry.add)(&mut object, &context(&scene)).unwrap();
        assert!(!(entry.available)(&object));
        object.light.as_mut().unwrap().intensity = 42.0;
        (entry.remove)(&mut object, &mut scene.clone());
        assert!((entry.get)(&object, "intensity").is_none());
        (entry.add)(&mut object, &context(&scene)).unwrap();
        assert_eq!(
            (entry.get)(&object, "intensity"),
            Some(FieldValue::Number(100.0)),
            "a re-added component starts from its defaults"
        );
    }

    #[test]
    fn prefab_merge_keeps_edits_and_takes_untouched_source_values() {
        let scene = scene();
        let mut baseline = object(&scene);
        let mut source = baseline.clone();
        let mut current = baseline.clone();
        let spin = component_type("spin").unwrap();
        (spin.add)(&mut baseline, &context(&scene)).unwrap();
        (spin.add)(&mut source, &context(&scene)).unwrap();
        (spin.add)(&mut current, &context(&scene)).unwrap();
        source.spin = Some(Spin([0.0, 90.0, 0.0]));
        baseline.spin = Some(Spin([0.0, 45.0, 0.0]));
        current.spin = Some(Spin([0.0, 45.0, 0.0]));
        (spin.merge)(&mut current, &baseline, &source);
        assert_eq!(
            current.spin,
            Some(Spin([0.0, 90.0, 0.0])),
            "an untouched field takes the source value"
        );
        current.spin = Some(Spin([0.0, 5.0, 0.0]));
        (spin.merge)(&mut current, &baseline, &source);
        assert_eq!(
            current.spin,
            Some(Spin([0.0, 5.0, 0.0])),
            "a locally edited component stays"
        );
        // A component the source removed disappears from untouched instances.
        source.spin = None;
        (spin.merge)(&mut current, &baseline, &source);
        assert_eq!(current.spin, Some(Spin([0.0, 5.0, 0.0])));
        current.spin = baseline.spin;
        (spin.merge)(&mut current, &baseline, &source);
        assert_eq!(current.spin, None);
    }

    #[test]
    fn every_component_merges_including_on_es_and_graphs() {
        let scene = scene();
        let context = context(&scene);
        for entry in COMPONENTS {
            let mut baseline = object(&scene);
            match (entry.add)(&mut baseline, &context) {
                Ok(()) => {}
                // A Mesh Collider needs a cooked mesh; every other component is self-sufficient.
                Err(_) => continue,
            }
            let mut source = baseline.clone();
            let mut current = baseline.clone();
            (entry.remove)(&mut source, &mut scene.clone());
            (entry.merge)(&mut current, &baseline, &source);
            assert!(
                !(entry.present)(&current),
                "{} did not follow the source's removal",
                entry.label
            );
        }
    }

    #[test]
    fn triggers_keep_their_respawn_point_across_variants() {
        let mut trigger = Trigger::default();
        trigger
            .set_field("action", FieldValue::Index(2))
            .expect("checkpoint");
        trigger
            .set_field("respawn", FieldValue::Vector([1.0, 2.0, 3.0]))
            .unwrap();
        trigger.set_field("action", FieldValue::Index(2)).unwrap();
        assert_eq!(
            trigger.field("respawn"),
            Some(FieldValue::Vector([1.0, 2.0, 3.0])),
            "re-selecting Checkpoint keeps the authored point"
        );
        trigger.set_field("action", FieldValue::Index(3)).unwrap();
        assert_eq!(
            trigger.field("respawn"),
            None,
            "a Goal has no respawn point"
        );
        assert!(
            trigger
                .set_field("respawn", FieldValue::Bool(true))
                .is_err()
        );
        assert!(trigger.set_field("nope", FieldValue::Bool(true)).is_err());
    }

    #[test]
    fn a_field_list_follows_the_component_variant() {
        let mut object = scene().objects[0].clone();
        let entry = component_type("light").unwrap();
        object.light = Some(Light::default());
        let kind = |object: &mut Object, kind| object.light.as_mut().unwrap().kind = kind;
        let visible = |object: &Object| -> Vec<&'static str> {
            entry
                .visible_fields(object)
                .into_iter()
                .map(|field| field.key)
                .collect()
        };
        assert_eq!(
            visible(&object),
            [
                "enabled",
                "kind",
                "color",
                "intensity",
                "range",
                "shadows",
                "shadow_bias",
                "shadow_normal_bias",
            ],
            "a point light keeps its range and shadow fields"
        );
        kind(&mut object, LightKind::Spot);
        assert!(visible(&object).contains(&"inner_angle_degrees"));
        kind(&mut object, LightKind::Directional);
        assert_eq!(
            visible(&object),
            ["enabled", "kind", "color", "intensity"],
            "a directional light owns neither a cone nor a local shadow map"
        );
        // Every visible field is readable, so the editor never draws an empty widget.
        for field in entry.visible_fields(&object) {
            assert!(
                (entry.get)(&object, field.key).is_some(),
                "{} is visible but unreadable",
                field.key
            );
        }
    }

    #[test]
    fn a_light_cone_stays_valid_while_editing() {
        let mut light = Light::default();
        light
            .set_field("outer_angle_degrees", FieldValue::Number(20.0))
            .unwrap();
        light
            .set_field("inner_angle_degrees", FieldValue::Number(40.0))
            .unwrap();
        assert_eq!(
            light.field("inner_angle_degrees"),
            Some(FieldValue::Number(20.0)),
            "the inner angle follows the outer one instead of failing validation"
        );
        light
            .set_field("outer_angle_degrees", FieldValue::Number(10.0))
            .unwrap();
        assert_eq!(
            light.field("inner_angle_degrees"),
            Some(FieldValue::Number(10.0))
        );
        light.validate().unwrap();
    }

    #[test]
    fn camera_projection_switches_and_keeps_the_lens() {
        let mut camera = Camera::Perspective {
            vertical_fov_degrees: 55.0,
            near: 0.5,
            far: 250.0,
        };
        camera
            .set_field("projection", FieldValue::Index(0))
            .unwrap();
        assert_eq!(camera.field("vertical_size"), Some(FieldValue::Number(7.0)));
        assert_eq!(camera.field("near"), Some(FieldValue::Number(0.5)));
        assert_eq!(camera.field("far"), Some(FieldValue::Number(250.0)));
        assert_eq!(
            camera.field("vertical_fov_degrees"),
            None,
            "an orthographic camera has no field of view"
        );
        camera
            .set_field("projection", FieldValue::Index(1))
            .unwrap();
        assert_eq!(
            camera.field("vertical_fov_degrees"),
            Some(FieldValue::Number(60.0)),
            "a projection switch falls back to the default lens, like the editor toggle"
        );
        assert_eq!(camera.field("near"), Some(FieldValue::Number(0.5)));
        assert_eq!(camera.field("far"), Some(FieldValue::Number(250.0)));
    }
}
