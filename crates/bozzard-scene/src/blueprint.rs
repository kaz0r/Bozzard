//! Portable, typed gameplay graphs. Attachments embed independent copies, not file references.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Exec,
    Text,
    #[default]
    Number,
    Bool,
    Vector,
    Object,
}
impl PinType {
    pub const VALUES: [Self; 5] = [
        Self::Number,
        Self::Bool,
        Self::Vector,
        Self::Text,
        Self::Object,
    ];
    pub fn default_value(self) -> Value {
        match self {
            Self::Exec => Value::Exec,
            Self::Text => Value::Text(String::new()),
            Self::Number => Value::Number(0.),
            Self::Bool => Value::Bool(false),
            Self::Vector => Value::Vector([0.; 3]),
            Self::Object => Value::Object(ObjectRef::None),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableScope {
    #[default]
    Graph,
    Object,
    Scene,
}
/// Containers live in blackboards and expose typed scalar pins through list nodes.
/// This keeps the six pin types closed and forbids nested/unbounded containers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlackboardValue {
    Scalar(Value),
    List {
        element: PinType,
        capacity: usize,
        values: Vec<Value>,
    },
}
pub type Blackboard = BTreeMap<String, BlackboardValue>;
impl BlackboardValue {
    pub fn kind(&self) -> PinType {
        match self {
            Self::Scalar(v) => v.kind(),
            Self::List { element, .. } => *element,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.kind() != PinType::Exec,
            "Exec cannot be stored in a blackboard"
        );
        match self {
            Self::Scalar(v) => ensure!(v.valid(), "invalid blackboard value"),
            Self::List {
                element,
                capacity,
                values,
            } => ensure!(
                (1..=1024).contains(capacity)
                    && values.len() <= *capacity
                    && values.iter().all(|v| v.kind() == *element && v.valid()),
                "list needs 1–1024 capacity and matching finite scalar elements"
            ),
        }
        Ok(())
    }
    pub fn values(&self) -> &[Value] {
        match self {
            Self::Scalar(v) => std::slice::from_ref(v),
            Self::List { values, .. } => values,
        }
    }
    pub fn values_mut(&mut self) -> &mut [Value] {
        match self {
            Self::Scalar(v) => std::slice::from_mut(v),
            Self::List { values, .. } => values,
        }
    }
}
pub fn validate_blackboard(board: &Blackboard) -> Result<()> {
    ensure!(board.len() <= 64, "blackboard limit: 64 variables");
    for (name, value) in board {
        ensure!(
            !name.trim().is_empty() && name.len() <= 64,
            "variable needs a name (1–64 bytes)"
        );
        value.validate()?;
    }
    Ok(())
}
/// Persistent document IDs, never ECS handles. None is distinct from the self default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectRef {
    #[default]
    SelfObject,
    Id(String),
    None,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Exec,
    Text(String),
    Number(f32),
    Bool(bool),
    Vector([f32; 3]),
    Object(ObjectRef),
}
impl Value {
    pub fn kind(&self) -> PinType {
        match self {
            Self::Exec => PinType::Exec,
            Self::Text(_) => PinType::Text,
            Self::Number(_) => PinType::Number,
            Self::Bool(_) => PinType::Bool,
            Self::Vector(_) => PinType::Vector,
            Self::Object(_) => PinType::Object,
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Text(text) => text.len() <= 4096,
            Self::Object(ObjectRef::Id(id)) => !id.trim().is_empty(),
            Self::Number(n) => n.is_finite(),
            Self::Vector(v) => v.iter().all(|n| n.is_finite()),
            _ => true,
        }
    }
    pub fn text(&self) -> Result<&str> {
        if let Self::Text(text) = self {
            Ok(text)
        } else {
            anyhow::bail!("expected text")
        }
    }
    pub fn object(&self) -> Result<&ObjectRef> {
        if let Self::Object(value) = self {
            Ok(value)
        } else {
            anyhow::bail!("expected object reference")
        }
    }
    pub fn number(&self) -> Result<f32> {
        if let Self::Number(n) = self {
            Ok(*n)
        } else {
            anyhow::bail!("expected number")
        }
    }
    pub fn boolean(&self) -> Result<bool> {
        if let Self::Bool(b) = self {
            Ok(*b)
        } else {
            anyhow::bail!("expected boolean")
        }
    }
    pub fn vector(&self) -> Result<[f32; 3]> {
        if let Self::Vector(v) = self {
            Ok(*v)
        } else {
            anyhow::bail!("expected vector")
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputKey(String);
impl InputKey {
    /// Rejects unknown names, so a typo fails where it is authored instead of never firing.
    pub fn parse(name: &str) -> Result<Self> {
        crate::keys::canonical(name)
            .map(|name| Self(name.to_owned()))
            .with_context(|| {
                format!(
                    "unknown input key '{name}'; bind an alias ({}) or a key such as {}",
                    crate::keys::KEY_ALIASES.join("/"),
                    crate::keys::BOUND_KEYS[..8].join("/")
                )
            })
    }
    pub fn name(&self) -> &str {
        &self.0
    }
    /// Every name a scene may bind, for editor pickers.
    pub fn authorable() -> impl Iterator<Item = &'static str> {
        crate::keys::authorable()
    }
    /// The bit this binding occupies in a frame's active set. Aliases live above the keys.
    pub fn bit(&self) -> u128 {
        crate::keys::alias_index(&self.0)
            .map_or_else(|| crate::keys::bit(&self.0), crate::keys::alias_bit)
    }
    /// Whether the binding is held or queued in this frame's input.
    pub fn active(&self, input: GameplayInput) -> bool {
        match crate::keys::alias_index(&self.0) {
            Some(index) => crate::keys::alias_active(index, input),
            None => input.keys & self.bit() != 0,
        }
    }
    /// Jump and fire arrive as queued edges, so they are already one press per tick.
    pub fn instant(&self) -> bool {
        matches!(self.0.as_str(), "jump" | "fire")
    }
}
impl Default for InputKey {
    fn default() -> Self {
        Self("jump".into())
    }
}
impl Serialize for InputKey {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for InputKey {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::parse(&name).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Text,
    NumberToText,
    JoinText,
    GetText,
    SetText,
    EndGame,
    Object,
    SelfObject,
    ObjectEqual,
    IsValidObject,
    BodyEnter,
    BodyExit,
    OverlapCount,
    Start,
    Update,
    InputPressed,
    TriggerEnter,
    TriggerExit,
    Number,
    Boolean,
    Vector,
    DeltaTime,
    ElapsedTime,
    Position,
    Rotation,
    Scale,
    InputHeld,
    MoveX,
    MoveY,
    MouseX,
    MouseY,
    ForwardVector,
    BreakVector,
    IsRigidbody,
    GetVariable,
    Add,
    Subtract,
    Multiply,
    Divide,
    Sine,
    Clamp,
    Greater,
    Less,
    Equal,
    Not,
    And,
    Or,
    MakeVector,
    ScaleVector,
    AddVector,
    Branch,
    SetVariable,
    Translate,
    Rotate,
    SetPosition,
    SetRotation,
    SetScale,
    SetColor,
    SetVisible,
    SetLightIntensity,
    SetFocusDistance,
    SetAperture,
    SetFogDensity,
    SetFogLightIntensity,
    SetExposure,
    SetBloomIntensity,
    SetSaturation,
    SetHeatStrength,
    SetGrainIntensity,
    SetVignetteIntensity,

    MoveWithCollision,
    Jump,
    SetVelocity,
    LockCursor,
    UnlockCursor,
    Print,
    SpawnPrefab,
    DestroyPrefab,
    Enable,
    Disable,
    Destroy,
    CollisionEnter,
    SetGraphEnabled,
    Delay,
    Lerp,
    Min,
    Max,
    Abs,
    Length,
    Normalize,
    Dot,
    Cross,
    Distance,
    Modulo,
    Power,
    Random,
    Cosine,
    Tangent,
    ArcSine,
    ArcCosine,
    Atan2,
    ToRadians,
    ToDegrees,
    Floor,
    Ceil,
    Round,
    Sqrt,
    LerpVector,
    Raycast,
    SphereOverlap,
    BoxOverlap,
    LineOfSight,
    ListGet,
    ListPush,
    ListSet,
    ListRemove,
    ListClear,
    ListLength,
    LoadScene,
    AddScene,
    RestartScene,
    SaveGame,
    LoadGame,
    Comment,
    Reroute,
}
impl NodeKind {
    pub const ALL: [Self; 125] = [
        Self::Text,
        Self::NumberToText,
        Self::JoinText,
        Self::GetText,
        Self::SetText,
        Self::EndGame,
        Self::Object,
        Self::SelfObject,
        Self::ObjectEqual,
        Self::IsValidObject,
        Self::BodyEnter,
        Self::BodyExit,
        Self::OverlapCount,
        Self::Start,
        Self::Update,
        Self::InputPressed,
        Self::TriggerEnter,
        Self::TriggerExit,
        Self::Number,
        Self::Boolean,
        Self::Vector,
        Self::DeltaTime,
        Self::ElapsedTime,
        Self::Position,
        Self::Rotation,
        Self::Scale,
        Self::InputHeld,
        Self::MoveX,
        Self::MoveY,
        Self::MouseX,
        Self::MouseY,
        Self::ForwardVector,
        Self::BreakVector,
        Self::IsRigidbody,
        Self::GetVariable,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Sine,
        Self::Clamp,
        Self::Greater,
        Self::Less,
        Self::Equal,
        Self::Not,
        Self::And,
        Self::Or,
        Self::MakeVector,
        Self::ScaleVector,
        Self::AddVector,
        Self::Branch,
        Self::SetVariable,
        Self::Translate,
        Self::Rotate,
        Self::SetPosition,
        Self::SetRotation,
        Self::SetScale,
        Self::SetColor,
        Self::SetVisible,
        Self::SetLightIntensity,
        Self::SetFocusDistance,
        Self::SetAperture,
        Self::SetFogDensity,
        Self::SetFogLightIntensity,
        Self::SetExposure,
        Self::SetBloomIntensity,
        Self::SetSaturation,
        Self::SetHeatStrength,
        Self::SetGrainIntensity,
        Self::SetVignetteIntensity,
        Self::MoveWithCollision,
        Self::Jump,
        Self::SetVelocity,
        Self::LockCursor,
        Self::UnlockCursor,
        Self::Print,
        Self::SpawnPrefab,
        Self::DestroyPrefab,
        Self::Enable,
        Self::Disable,
        Self::Destroy,
        Self::CollisionEnter,
        Self::SetGraphEnabled,
        Self::Delay,
        Self::Lerp,
        Self::Min,
        Self::Max,
        Self::Abs,
        Self::Length,
        Self::Normalize,
        Self::Dot,
        Self::Cross,
        Self::Distance,
        Self::Modulo,
        Self::Power,
        Self::Random,
        Self::Cosine,
        Self::Tangent,
        Self::ArcSine,
        Self::ArcCosine,
        Self::Atan2,
        Self::ToRadians,
        Self::ToDegrees,
        Self::Floor,
        Self::Ceil,
        Self::Round,
        Self::Sqrt,
        Self::LerpVector,
        Self::Raycast,
        Self::SphereOverlap,
        Self::BoxOverlap,
        Self::LineOfSight,
        Self::ListGet,
        Self::ListPush,
        Self::ListSet,
        Self::ListRemove,
        Self::ListClear,
        Self::ListLength,
        Self::LoadScene,
        Self::AddScene,
        Self::RestartScene,
        Self::SaveGame,
        Self::LoadGame,
        Self::Comment,
        Self::Reroute,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::NumberToText => "Number to Text",
            Self::JoinText => "Join Text",
            Self::GetText => "Get Text",
            Self::SetText => "Set Text",
            Self::EndGame => "End Game",
            Self::Object => "Object Reference",
            Self::SelfObject => "Self",
            Self::ObjectEqual => "Same Object",
            Self::IsValidObject => "Is Valid Object",
            Self::BodyEnter => "On Object Enter",
            Self::BodyExit => "On Object Exit",
            Self::OverlapCount => "Overlap Count",
            Self::Start => "On Start",
            Self::Update => "On Update",
            Self::InputPressed => "On Input Pressed",
            Self::TriggerEnter => "On Overlap Enter",
            Self::TriggerExit => "On Overlap Exit",
            Self::Number => "Number",
            Self::Boolean => "Boolean",
            Self::Vector => "Vector",
            Self::DeltaTime => "Delta Seconds",
            Self::ElapsedTime => "Elapsed Seconds",
            Self::Position => "Get Position",
            Self::Rotation => "Get Rotation",
            Self::Scale => "Get Scale",
            Self::InputHeld => "Input Held",
            Self::MoveX => "Move Axis X (A/D)",
            Self::MoveY => "Move Axis Y (S/W)",
            Self::MouseX => "Mouse Delta X (right-drag)",
            Self::MouseY => "Mouse Delta Y (right-drag)",
            Self::ForwardVector => "Forward Vector",
            Self::BreakVector => "Break Vector",
            Self::IsRigidbody => "Is Rigidbody (dynamic body)",
            Self::GetVariable => "Get Variable",
            Self::SetVariable => "Set Variable",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Divide => "Divide",
            Self::Sine => "Sine (radians)",
            Self::Clamp => "Clamp (min–max)",
            Self::Greater => "Greater Than",
            Self::Less => "Less Than",
            Self::Equal => "Equal",
            Self::Not => "Not",
            Self::And => "And",
            Self::Or => "Or",
            Self::MakeVector => "Make Vector",
            Self::ScaleVector => "Scale Vector",
            Self::AddVector => "Add Vectors",
            Self::Branch => "Branch",
            Self::Translate => "Translate (local delta)",
            Self::Rotate => "Rotate (degrees delta)",
            Self::SetPosition => "Set Position",
            Self::SetRotation => "Set Rotation",
            Self::SetScale => "Set Scale",
            Self::SetColor => "Set Color (RGB)",
            Self::SetVisible => "Set Visible",
            Self::SetLightIntensity => "Set Light Intensity",
            Self::SetFocusDistance => "Set Focus Distance",
            Self::SetAperture => "Set Aperture (f-stop)",
            Self::SetFogDensity => "Set Volumetric Fog Density",
            Self::SetFogLightIntensity => "Set Volumetric Light Intensity",
            Self::SetExposure => "Set Exposure (EV)",
            Self::SetBloomIntensity => "Set Bloom Intensity",
            Self::SetSaturation => "Set Saturation",
            Self::SetHeatStrength => "Set Heat Strength",
            Self::SetGrainIntensity => "Set Grain Intensity",
            Self::SetVignetteIntensity => "Set Vignette Intensity",

            Self::MoveWithCollision => "Move With Collision",
            Self::Jump => "Jump",
            Self::SetVelocity => "Set Velocity",
            Self::LockCursor => "Lock Cursor",
            Self::UnlockCursor => "Unlock Cursor",
            Self::Print => "Print Number",
            Self::SpawnPrefab => "Spawn Prefab",
            Self::DestroyPrefab => "Destroy Prefab",
            Self::Enable => "On Enable",
            Self::Disable => "On Disable",
            Self::Destroy => "On Destroy",
            Self::CollisionEnter => "On Collision Enter",
            Self::SetGraphEnabled => "Set Graph Enabled",
            Self::Delay => "Delay / After",
            Self::Lerp => "Lerp",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Abs => "Abs",
            Self::Length => "Length",
            Self::Normalize => "Normalize",
            Self::Dot => "Dot",
            Self::Cross => "Cross",
            Self::Distance => "Distance",
            Self::Modulo => "Modulo",
            Self::Power => "Power",
            Self::Random => "Random (seeded)",
            Self::Cosine => "Cosine (radians)",
            Self::Tangent => "Tangent (radians)",
            Self::ArcSine => "Arc Sine",
            Self::ArcCosine => "Arc Cosine",
            Self::Atan2 => "Atan2 (Y, X)",
            Self::ToRadians => "Degrees to Radians",
            Self::ToDegrees => "Radians to Degrees",
            Self::Floor => "Floor",
            Self::Ceil => "Ceil",
            Self::Round => "Round",
            Self::Sqrt => "Square Root",
            Self::LerpVector => "Lerp Vectors",
            Self::Raycast => "Raycast",
            Self::SphereOverlap => "Sphere Overlap",
            Self::BoxOverlap => "Box Overlap",
            Self::LineOfSight => "Line of Sight",
            Self::ListGet => "List Get",
            Self::ListPush => "List Push",
            Self::ListSet => "List Set",
            Self::ListRemove => "List Remove",
            Self::ListClear => "List Clear",
            Self::ListLength => "List Length",
            Self::LoadScene => "Load Scene",
            Self::AddScene => "Load Scene Additively",
            Self::RestartScene => "Restart Scene",
            Self::SaveGame => "Save Game State",
            Self::LoadGame => "Load Game State",
            Self::Comment => "Comment",
            Self::Reroute => "Reroute",
        }
    }
    pub fn event(self) -> bool {
        matches!(
            self,
            Self::Enable
                | Self::Disable
                | Self::Destroy
                | Self::CollisionEnter
                | Self::Start
                | Self::Update
                | Self::InputPressed
                | Self::TriggerEnter
                | Self::TriggerExit
                | Self::BodyEnter
                | Self::BodyExit
        )
    }
    pub fn inputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
            Self::Delay => &[("In", Exec), ("Seconds", Number)],
            Self::SetGraphEnabled => &[
                ("In", Exec),
                ("Enabled", Bool),
                ("Target", Object),
                ("Attachment", Number),
            ],
            Self::LoadScene | Self::AddScene => &[("In", Exec), ("Scene", Text)],
            Self::RestartScene => &[("In", Exec)],
            Self::SaveGame | Self::LoadGame => &[("In", Exec), ("Slot", Text)],
            Self::Lerp => &[("A", Number), ("B", Number), ("T", Number)],
            Self::LerpVector => &[("A", Vector), ("B", Vector), ("T", Number)],
            Self::Min | Self::Max | Self::Modulo | Self::Power | Self::Atan2 => {
                &[("A", Number), ("B", Number)]
            }
            Self::Abs
            | Self::Cosine
            | Self::Tangent
            | Self::ArcSine
            | Self::ArcCosine
            | Self::ToRadians
            | Self::ToDegrees
            | Self::Floor
            | Self::Ceil
            | Self::Round
            | Self::Sqrt => &[("Value", Number)],
            Self::Length | Self::Normalize => &[("Value", Vector)],
            Self::Dot | Self::Cross | Self::Distance => &[("A", Vector), ("B", Vector)],
            Self::Random => &[("In", Exec), ("Min", Number), ("Max", Number)],
            Self::Raycast => &[
                ("In", Exec),
                ("Origin", Vector),
                ("Direction", Vector),
                ("Distance", Number),
                ("Ignore", Object),
            ],
            Self::SphereOverlap => &[
                ("In", Exec),
                ("Center", Vector),
                ("Radius", Number),
                ("Ignore", Object),
            ],
            Self::BoxOverlap => &[
                ("In", Exec),
                ("Center", Vector),
                ("Size", Vector),
                ("Ignore", Object),
            ],
            Self::LineOfSight => &[
                ("In", Exec),
                ("From", Vector),
                ("To", Vector),
                ("Ignore", Object),
            ],
            Self::ListGet => &[("Index", Number)],
            Self::ListRemove => &[("In", Exec), ("Index", Number)],
            Self::ListClear => &[("In", Exec)],
            Self::ListPush | Self::ListSet => &[("In", Exec), ("Value", Number)],
            Self::EndGame => &[("In", Exec), ("Message", Text)],
            Self::Text => &[("Value", Text)],
            Self::NumberToText => &[("Value", Number), ("Decimals (0–6)", Number)],
            Self::JoinText => &[("A", Text), ("B", Text)],
            Self::GetText => &[("Target", Object)],
            Self::SetText => &[("In", Exec), ("Text", Text), ("Target", Object)],
            Self::Object => &[("Value", Object)],
            Self::LockCursor | Self::UnlockCursor => &[("In", Exec)],
            Self::BreakVector => &[("Value", Vector)],
            Self::IsRigidbody => &[("Value", Object)],
            Self::Position | Self::Rotation | Self::Scale => &[("Target", Object)],
            Self::ForwardVector => &[("Target", Object)],
            Self::ObjectEqual => &[("A", Object), ("B", Object)],
            Self::IsValidObject => &[("Value", Object)],
            Self::Number => &[("Value", Number)],
            Self::Boolean => &[("Value", Bool)],
            Self::Vector => &[("Value", Vector)],
            Self::Add
            | Self::Subtract
            | Self::Multiply
            | Self::Divide
            | Self::Greater
            | Self::Less
            | Self::Equal => &[("A", Number), ("B", Number)],
            Self::Sine => &[("Radians", Number)],
            Self::Clamp => &[("Value", Number), ("Min", Number), ("Max", Number)],
            Self::Not => &[("Value", Bool)],
            Self::And | Self::Or => &[("A", Bool), ("B", Bool)],
            Self::MakeVector => &[("X", Number), ("Y", Number), ("Z", Number)],
            Self::ScaleVector => &[("Vector", Vector), ("Factor", Number)],
            Self::AddVector => &[("A", Vector), ("B", Vector)],
            Self::Branch => &[("In", Exec), ("Condition", Bool)],
            Self::SetFocusDistance
            | Self::SetAperture
            | Self::SetFogDensity
            | Self::SetFogLightIntensity
            | Self::SetExposure
            | Self::SetBloomIntensity
            | Self::SetSaturation
            | Self::SetHeatStrength
            | Self::SetGrainIntensity
            | Self::SetVignetteIntensity => &[("In", Exec), ("Value", Number)],
            Self::SetVariable | Self::Print => &[("In", Exec), ("Value", Number)],
            Self::Translate
            | Self::Rotate
            | Self::SetPosition
            | Self::SetRotation
            | Self::SetScale
            | Self::SetColor
            | Self::MoveWithCollision => &[("In", Exec), ("Value", Vector), ("Target", Object)],
            Self::SetVisible => &[("In", Exec), ("Visible", Bool), ("Target", Object)],
            Self::SetLightIntensity => &[("In", Exec), ("Intensity", Number), ("Target", Object)],
            Self::Jump => &[("In", Exec), ("Speed", Number), ("Target", Object)],
            Self::SetVelocity => &[("In", Exec), ("Velocity", Vector), ("Target", Object)],
            Self::SpawnPrefab => &[("In", Exec), ("Position", Vector)],
            Self::DestroyPrefab => &[("In", Exec), ("Target", Object)],
            _ => &[],
        }
    }
    pub fn target_port(self) -> Option<usize> {
        self.inputs()
            .iter()
            .position(|(label, kind)| *label == "Target" && *kind == PinType::Object)
    }
    pub fn action(self) -> bool {
        self.inputs().first().is_some_and(|p| p.1 == PinType::Exec)
    }
    pub fn outputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
            Self::Text | Self::NumberToText | Self::JoinText | Self::GetText => &[("Text", Text)],
            Self::MoveWithCollision => &[("Then", Exec), ("Grounded", Bool)],
            Self::EndGame | Self::Comment => &[],
            Self::Random => &[("Then", Exec), ("Value", Number)],
            Self::CollisionEnter => &[
                ("Then", Exec),
                ("Other", Object),
                ("Normal", Vector),
                ("Impulse", Number),
            ],
            Self::Raycast => &[
                ("Then", Exec),
                ("Hit", Bool),
                ("Object", Object),
                ("Position", Vector),
                ("Normal", Vector),
                ("Distance", Number),
            ],
            Self::SphereOverlap | Self::BoxOverlap => &[("Then", Exec), ("Count", Number)],
            Self::LineOfSight => &[("Then", Exec), ("Visible", Bool)],
            Self::BodyEnter | Self::BodyExit => &[("Then", Exec), ("Other", Object)],
            Self::SpawnPrefab => &[("Then", Exec), ("Instance", Object)],
            Self::Object | Self::SelfObject => &[("Value", Object)],
            Self::ObjectEqual | Self::IsValidObject | Self::IsRigidbody => &[("Value", Bool)],
            Self::Branch => &[("True", Exec), ("False", Exec)],
            kind if kind.event() || kind.action() => &[("Then", Exec)],
            Self::Boolean
            | Self::InputHeld
            | Self::Greater
            | Self::Less
            | Self::Equal
            | Self::Not
            | Self::And
            | Self::Or => &[("Value", Bool)],
            Self::Normalize
            | Self::Cross
            | Self::LerpVector
            | Self::Vector
            | Self::Position
            | Self::Rotation
            | Self::Scale
            | Self::ForwardVector
            | Self::MakeVector
            | Self::ScaleVector
            | Self::AddVector => &[("Value", Vector)],
            Self::BreakVector => &[("X", Number), ("Y", Number), ("Z", Number)],
            _ => &[("Value", Number)],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, from = "StoredNode")]
pub struct Node {
    pub scope: VariableScope,
    pub value_type: PinType,
    pub comment: String,
    #[serde(default)]
    pub prefab: String,
    pub id: u32,
    pub position: [f32; 2],
    pub kind: NodeKind,
    pub inputs: Vec<Value>,
    #[serde(default)]
    pub variable: String,
    #[serde(default = "jump_key")]
    pub key: InputKey,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNode {
    #[serde(default)]
    scope: VariableScope,
    #[serde(default)]
    value_type: PinType,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    prefab: String,
    id: u32,
    position: [f32; 2],
    kind: NodeKind,
    inputs: Vec<Value>,
    #[serde(default)]
    variable: String,
    #[serde(default = "jump_key")]
    key: InputKey,
}
impl From<StoredNode> for Node {
    fn from(mut n: StoredNode) -> Self {
        if let Some(port) = n.kind.target_port()
            && n.inputs.len() == port
        {
            n.inputs.push(Value::Object(ObjectRef::SelfObject));
        }
        Self {
            scope: n.scope,
            value_type: n.value_type,
            comment: n.comment,
            prefab: n.prefab,
            id: n.id,
            position: n.position,
            kind: n.kind,
            inputs: n.inputs,
            variable: n.variable,
            key: n.key,
        }
    }
}
fn jump_key() -> InputKey {
    InputKey::default()
}
impl Node {
    pub fn new(id: u32, kind: NodeKind, position: [f32; 2]) -> Self {
        let mut node = Self {
            scope: VariableScope::Graph,
            value_type: PinType::Number,
            comment: String::new(),
            prefab: String::new(),
            id,
            position,
            kind,
            inputs: Vec::new(),
            variable: "value".into(),
            key: InputKey::default(),
        };
        node.reset_inputs();
        node
    }
    pub fn reset_inputs(&mut self) {
        self.inputs = self
            .input_pins()
            .iter()
            .map(|(_, t)| t.default_value())
            .collect();
        if let Some(p) = self.kind.target_port() {
            self.inputs[p] = Value::Object(ObjectRef::SelfObject);
        }
        if self.kind == NodeKind::SetScale {
            self.inputs[1] = Value::Vector([1.; 3]);
        }
    }
    pub fn uses_variable(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::GetVariable
                | NodeKind::SetVariable
                | NodeKind::ListGet
                | NodeKind::ListPush
                | NodeKind::ListSet
                | NodeKind::ListRemove
                | NodeKind::ListClear
                | NodeKind::ListLength
                | NodeKind::SphereOverlap
                | NodeKind::BoxOverlap
        )
    }
    pub fn uses_list(&self) -> bool {
        self.uses_variable() && !matches!(self.kind, NodeKind::GetVariable | NodeKind::SetVariable)
    }
    pub fn input_pins(&self) -> &'static [(&'static str, PinType)] {
        use NodeKind as K;
        macro_rules! pins {
            ($t:ident) => {
                match self.kind {
                    K::SetVariable | K::ListPush => {
                        &[("In", PinType::Exec), ("Value", PinType::$t)]
                    }
                    K::ListSet => &[
                        ("In", PinType::Exec),
                        ("Value", PinType::$t),
                        ("Index", PinType::Number),
                    ],
                    K::Reroute => &[("Value", PinType::$t)],
                    _ => self.kind.inputs(),
                }
            };
        }
        match self.value_type {
            PinType::Exec => pins!(Exec),
            PinType::Text => pins!(Text),
            PinType::Number => pins!(Number),
            PinType::Bool => pins!(Bool),
            PinType::Vector => pins!(Vector),
            PinType::Object => pins!(Object),
        }
    }
    pub fn output_pins(&self) -> &'static [(&'static str, PinType)] {
        use NodeKind as K;
        macro_rules! pins {
            ($t:ident) => {
                match self.kind {
                    K::GetVariable | K::ListGet | K::Reroute => &[("Value", PinType::$t)],
                    K::ListPush | K::ListSet => &[("Then", PinType::Exec)],
                    _ => self.kind.outputs(),
                }
            };
        }
        match self.value_type {
            PinType::Exec => pins!(Exec),
            PinType::Text => pins!(Text),
            PinType::Number => pins!(Number),
            PinType::Bool => pins!(Bool),
            PinType::Vector => pins!(Vector),
            PinType::Object => pins!(Object),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub node: u32,
    pub port: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub from: Socket,
    pub to: Socket,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    pub version: u32,
    pub name: String,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    #[serde(default)]
    pub variables: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blackboard: Blackboard,
}
impl Default for Blueprint {
    fn default() -> Self {
        Self {
            version: 1,
            name: "New Blueprint".into(),
            nodes: vec![
                Node::new(1, NodeKind::Start, [40., 40.]),
                Node::new(2, NodeKind::Update, [40., 160.]),
            ],
            wires: vec![],
            blackboard: Blackboard::new(),
            variables: BTreeMap::from([("value".into(), 0.)]),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintAttachment {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub graph: Blueprint,
}
impl Blueprint {
    pub fn from_json(json: &str) -> Result<Self> {
        ensure!(json.len() <= 1024 * 1024, "blueprint exceeds 1 MiB");
        let graph: Self = serde_json::from_str(json).context("parsing blueprint JSON")?;
        if let Err(error) = graph.validate() {
            let diff = graph
                .stale_wires()
                .iter()
                .map(|i| {
                    format!(
                        "\nwire {}:{} -> {}:{}: {}",
                        i.wire.from.node,
                        i.wire.from.port,
                        i.wire.to.node,
                        i.wire.to.port,
                        i.reason
                    )
                })
                .collect::<String>();
            anyhow::bail!("{error:#}{diff}");
        }
        Ok(graph)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
    pub fn node(&self, id: u32) -> Result<&Node> {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .context("missing blueprint node")
    }
    pub fn connect(&mut self, wire: Wire) -> Result<()> {
        let mut next = self.clone();
        next.wires.retain(|w| w.to != wire.to);
        next.wires.push(wire);
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn remove_node(&mut self, id: u32) {
        self.nodes.retain(|n| n.id != id);
        self.wires.retain(|w| w.from.node != id && w.to.node != id);
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported blueprint version");
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "blueprint needs a name (1–128 bytes)"
        );
        ensure!(
            self.nodes.len() <= 128
                && self.wires.len() <= 512
                && self.variables.len() + self.blackboard.len() <= 64,
            "blueprint limit: 128 nodes, 512 wires, 64 variables"
        );
        validate_blackboard(&self.blackboard)?;
        ensure!(
            self.blackboard
                .keys()
                .all(|k| !self.variables.contains_key(k)),
            "duplicate local variable"
        );
        for (name, value) in &self.variables {
            ensure!(
                !name.trim().is_empty() && name.len() <= 64 && value.is_finite(),
                "invalid blueprint variable"
            );
        }
        let mut ids = BTreeSet::new();
        for n in &self.nodes {
            ensure!(ids.insert(n.id), "duplicate blueprint node ID");
            ensure!(
                n.position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.),
                "invalid node position"
            );
            ensure!(
                n.inputs.len() == n.input_pins().len()
                    && n.inputs
                        .iter()
                        .zip(n.input_pins())
                        .all(|(v, (_, t))| v.kind() == *t && v.valid()),
                "invalid inputs on node {}",
                n.id
            );
            ensure!(n.prefab.len() <= 256, "prefab asset ID too long");
            ensure!(n.variable.len() <= 64, "variable name too long");
            ensure!(n.comment.len() <= 4096, "node comment exceeds 4096 bytes");
            if n.uses_variable() {
                ensure!(
                    n.value_type != PinType::Exec,
                    "variables cannot have execution type"
                );
                if n.scope == VariableScope::Graph {
                    self.validate_variable(n, &self.blackboard)?;
                }
            }
        }
        let mut incoming = BTreeSet::new();
        let mut degrees: BTreeMap<_, usize> = ids.iter().map(|id| (*id, 0)).collect();
        for w in &self.wires {
            let from = self
                .node(w.from.node)?
                .output_pins()
                .get(w.from.port)
                .context("invalid output pin")?;
            let to = self
                .node(w.to.node)?
                .input_pins()
                .get(w.to.port)
                .context("invalid input pin")?;
            ensure!(
                from.1 == to.1,
                "wire {}:{:?} -> {}:{:?} mixes {:?} with {:?}",
                w.from.node,
                from.0,
                w.to.node,
                to.0,
                from.1,
                to.1
            );
            ensure!(
                incoming.insert(w.to),
                "wire {}:{:?} -> {}:{:?} doubles an already connected input",
                w.from.node,
                from.0,
                w.to.node,
                to.0
            );
            *degrees.get_mut(&w.to.node).unwrap() += 1;
        }
        let mut queue: VecDeque<_> = degrees
            .iter()
            .filter_map(|(id, d)| (*d == 0).then_some(*id))
            .collect();
        let mut visited = 0;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            for w in self.wires.iter().filter(|w| w.from.node == id) {
                let degree = degrees.get_mut(&w.to.node).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(w.to.node);
                }
            }
        }
        ensure!(
            visited == self.nodes.len(),
            "cycles are not supported; use On Update with variables"
        );
        Ok(())
    }
    pub fn spinning() -> Self {
        let mut graph = Self {
            name: "Spin".into(),
            nodes: vec![
                Node::new(1, NodeKind::Update, [40., 40.]),
                Node::new(2, NodeKind::DeltaTime, [40., 240.]),
                Node::new(3, NodeKind::ScaleVector, [360., 210.]),
                Node::new(4, NodeKind::Rotate, [680., 40.]),
            ],
            ..Self::default()
        };
        graph.nodes[2].inputs[0] = Value::Vector([0., 45., 0.]);
        graph.wires = vec![
            Wire {
                from: Socket { node: 1, port: 0 },
                to: Socket { node: 4, port: 0 },
            },
            Wire {
                from: Socket { node: 2, port: 0 },
                to: Socket { node: 3, port: 1 },
            },
            Wire {
                from: Socket { node: 3, port: 0 },
                to: Socket { node: 4, port: 1 },
            },
        ];
        graph
    }
}

impl Blueprint {
    pub fn object_references(&self) -> impl Iterator<Item = &str> {
        self.nodes
            .iter()
            .flat_map(|n| &n.inputs)
            .chain(self.blackboard.values().flat_map(BlackboardValue::values))
            .filter_map(|v| match v {
                Value::Object(ObjectRef::Id(id)) => Some(id.as_str()),
                _ => None,
            })
    }
    pub fn remap_objects(&mut self, mapping: &BTreeMap<String, String>) {
        remap_board(&mut self.blackboard, mapping);
        for value in self.nodes.iter_mut().flat_map(|n| &mut n.inputs) {
            if let Value::Object(ObjectRef::Id(id)) = value
                && let Some(new) = mapping.get(id)
            {
                *id = new.clone();
            }
        }
    }
    /// Portable graph imports must be rebound rather than accidentally targeting matching IDs.
    pub fn clear_object_bindings(&mut self) {
        for node in &mut self.nodes {
            node.prefab.clear();
        }
        for value in self.nodes.iter_mut().flat_map(|n| &mut n.inputs).chain(
            self.blackboard
                .values_mut()
                .flat_map(BlackboardValue::values_mut),
        ) {
            if matches!(value, Value::Object(ObjectRef::Id(_))) {
                *value = Value::Object(ObjectRef::None);
            }
        }
    }
    /// Known write targets for static GI. An event-dependent target may address any scene object.
    pub fn write_targets(&self, owner: &str) -> Option<BTreeSet<String>> {
        fn resolve(g: &Blueprint, socket: Socket, depth: usize) -> Option<ObjectRef> {
            if depth > 128 {
                return None;
            }
            if let Some(w) = g.wires.iter().find(|w| w.to == socket) {
                let node = g.node(w.from.node).ok()?;
                match node.kind {
                    NodeKind::SelfObject => Some(ObjectRef::SelfObject),
                    NodeKind::Object => resolve(
                        g,
                        Socket {
                            node: node.id,
                            port: 0,
                        },
                        depth + 1,
                    ),
                    _ => None,
                }
            } else {
                g.node(socket.node)
                    .ok()?
                    .inputs
                    .get(socket.port)?
                    .object()
                    .ok()
                    .cloned()
            }
        }
        let mut ids = BTreeSet::new();
        for node in self.nodes.iter().filter(|n| n.kind.action()) {
            if let Some(port) = node.kind.target_port() {
                match resolve(
                    self,
                    Socket {
                        node: node.id,
                        port,
                    },
                    0,
                )? {
                    ObjectRef::SelfObject => {
                        ids.insert(owner.to_owned());
                    }
                    ObjectRef::Id(id) => {
                        ids.insert(id);
                    }
                    ObjectRef::None => {}
                }
            }
        }
        Some(ids)
    }
}
impl Object {
    pub fn remap_blueprint_objects(&mut self, mapping: &BTreeMap<String, String>) {
        remap_board(&mut self.blackboard, mapping);
        for attachment in &mut self.blueprints {
            attachment.graph.remap_objects(mapping);
        }
    }
}

impl Blueprint {
    pub fn validate_variable(&self, node: &Node, board: &Blackboard) -> Result<()> {
        if node.scope == VariableScope::Graph
            && !node.uses_list()
            && node.value_type == PinType::Number
            && self.variables.contains_key(&node.variable)
        {
            return Ok(());
        }
        let entry = board.get(&node.variable).with_context(|| {
            format!(
                "unknown {:?} variable '{}' at node {}",
                node.scope, node.variable, node.id
            )
        })?;
        let expected = if matches!(node.kind, NodeKind::SphereOverlap | NodeKind::BoxOverlap) {
            PinType::Object
        } else {
            node.value_type
        };
        ensure!(
            entry.kind() == expected
                && matches!(entry, BlackboardValue::List { .. }) == node.uses_list(),
            "variable '{}' has a different type at node {}",
            node.variable,
            node.id
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireIssue {
    pub index: usize,
    pub wire: Wire,
    pub reason: String,
}
impl Blueprint {
    /// Every obsolete endpoint/type is reported, so a failed edit/import is repairable in one pass.
    pub fn stale_wires(&self) -> Vec<WireIssue> {
        let mut incoming = BTreeSet::new();
        self.wires
            .iter()
            .enumerate()
            .filter_map(|(index, &wire)| {
                let a = self
                    .node(wire.from.node)
                    .ok()
                    .and_then(|n| n.output_pins().get(wire.from.port));
                let b = self
                    .node(wire.to.node)
                    .ok()
                    .and_then(|n| n.input_pins().get(wire.to.port));
                let reason = match (a, b) {
                    (None, _) => Some("source node or output no longer exists".into()),
                    (_, None) => Some("destination node or input no longer exists".into()),
                    (Some(a), Some(b)) if a.1 != b.1 => Some(format!(
                        "{} ({:?}) → {} ({:?}): type changed",
                        a.0, a.1, b.0, b.1
                    )),
                    _ if !incoming.insert(wire.to) => Some("input already has a wire".into()),
                    _ => None,
                };
                reason.map(|reason| WireIssue {
                    index,
                    wire,
                    reason,
                })
            })
            .collect()
    }
    pub fn copy_subgraph(&self, selected: &BTreeSet<u32>) -> Result<Self> {
        let mut graph = self.clone();
        graph.nodes.retain(|n| selected.contains(&n.id));
        ensure!(!graph.nodes.is_empty(), "select nodes to copy");
        graph
            .wires
            .retain(|w| selected.contains(&w.from.node) && selected.contains(&w.to.node));
        let variables: BTreeSet<_> = graph
            .nodes
            .iter()
            .filter(|n| n.uses_variable() && n.scope == VariableScope::Graph)
            .map(|n| n.variable.clone())
            .collect();
        graph.variables.retain(|n, _| variables.contains(n));
        graph.blackboard.retain(|n, _| variables.contains(n));
        graph.validate()?;
        Ok(graph)
    }
    /// Paste is atomic, with fresh IDs and preserved internal wiring; conflicting locals fail.
    pub fn paste_subgraph(&mut self, copied: &Self, offset: [f32; 2]) -> Result<BTreeSet<u32>> {
        copied.validate()?;
        let mut next = self.clone();
        for (name, value) in &copied.variables {
            ensure!(
                next.variables.get(name).is_none_or(|v| v == value),
                "conflicting local variable '{name}'"
            );
            next.variables.insert(name.clone(), *value);
        }
        for (name, value) in &copied.blackboard {
            ensure!(
                next.blackboard.get(name).is_none_or(|v| v == value),
                "conflicting local variable '{name}'"
            );
            next.blackboard.insert(name.clone(), value.clone());
        }
        let mut id = next.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        let mut mapping = BTreeMap::new();
        for n in &copied.nodes {
            id = id.checked_add(1).context("node ID space exhausted")?;
            mapping.insert(n.id, id);
            let mut node = n.clone();
            node.id = id;
            node.position[0] += offset[0];
            node.position[1] += offset[1];
            next.nodes.push(node);
        }
        next.wires.extend(copied.wires.iter().map(|w| Wire {
            from: Socket {
                node: mapping[&w.from.node],
                port: w.from.port,
            },
            to: Socket {
                node: mapping[&w.to.node],
                port: w.to.port,
            },
        }));
        next.validate()?;
        *self = next;
        Ok(mapping.into_values().collect())
    }
}
pub fn board_references(board: &Blackboard) -> impl Iterator<Item = &str> {
    board
        .values()
        .flat_map(BlackboardValue::values)
        .filter_map(|v| match v {
            Value::Object(ObjectRef::Id(id)) => Some(id.as_str()),
            _ => None,
        })
}
pub fn remap_board(board: &mut Blackboard, mapping: &BTreeMap<String, String>) {
    for v in board.values_mut().flat_map(BlackboardValue::values_mut) {
        if let Value::Object(ObjectRef::Id(id)) = v
            && let Some(new) = mapping.get(id)
        {
            *id = new.clone();
        }
    }
}
