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
/// One blueprint node kind: its editor title and its default pins.
///
/// The table below is the single declaration of a node. It generates the enum, the editor's
/// add-node menu and the default pin lists, so adding a node is one row plus its runtime arm.
#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub kind: NodeKind,
    pub title: &'static str,
    /// Number is the default element type; `Node::input_pins` resolves typed declarations.
    pub inputs: &'static [(&'static str, PinType)],
    /// `Node::output_pins` resolves variable/list/reroute output types.
    pub outputs: &'static [(&'static str, PinType)],
}

macro_rules! node_kinds {
    ($($variant:ident => $title:literal { $($input_label:literal : $input_type:ident),* $(,)? } -> { $($output_label:literal : $output_type:ident),* $(,)? }),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum NodeKind {
            $($variant,)*
        }
        /// Every node kind in declaration order; the editor sorts titles for display.
        pub const NODE_SPECS: &[NodeSpec] = &[
            $(NodeSpec {
                kind: NodeKind::$variant,
                title: $title,
                inputs: &[$(($input_label, PinType::$input_type)),*],
                outputs: &[$(($output_label, PinType::$output_type)),*],
            },)*
        ];
    };
}

node_kinds! {
    Text => "Text" { "Value": Text } -> { "Text": Text },
    NumberToText => "Number to Text" { "Value": Number, "Decimals (0–6)": Number } -> { "Text": Text },
    JoinText => "Join Text" { "A": Text, "B": Text } -> { "Text": Text },
    GetText => "Get Text" { "Target": Object } -> { "Text": Text },
    SetText => "Set Text" { "In": Exec, "Text": Text, "Target": Object } -> { "Then": Exec },
    EndGame => "End Game" { "In": Exec, "Message": Text } -> {  },
    Object => "Object Reference" { "Value": Object } -> { "Value": Object },
    SelfObject => "Self" {  } -> { "Value": Object },
    ObjectEqual => "Same Object" { "A": Object, "B": Object } -> { "Value": Bool },
    IsValidObject => "Is Valid Object" { "Value": Object } -> { "Value": Bool },
    BodyEnter => "On Object Enter" {  } -> { "Then": Exec, "Other": Object },
    BodyExit => "On Object Exit" {  } -> { "Then": Exec, "Other": Object },
    OverlapCount => "Overlap Count" {  } -> { "Value": Number },
    Start => "On Start" {  } -> { "Then": Exec },
    Update => "On Update" {  } -> { "Then": Exec },
    InputPressed => "On Input Pressed" {  } -> { "Then": Exec },
    TriggerEnter => "On Overlap Enter" {  } -> { "Then": Exec },
    TriggerExit => "On Overlap Exit" {  } -> { "Then": Exec },
    Number => "Number" { "Value": Number } -> { "Value": Number },
    Boolean => "Boolean" { "Value": Bool } -> { "Value": Bool },
    Vector => "Vector" { "Value": Vector } -> { "Value": Vector },
    DeltaTime => "Delta Seconds" {  } -> { "Value": Number },
    ElapsedTime => "Elapsed Seconds" {  } -> { "Value": Number },
    Position => "Get Position" { "Target": Object } -> { "Value": Vector },
    Rotation => "Get Rotation" { "Target": Object } -> { "Value": Vector },
    Scale => "Get Scale" { "Target": Object } -> { "Value": Vector },
    InputHeld => "Input Held" {  } -> { "Value": Bool },
    MoveX => "Move Axis X (A/D)" {  } -> { "Value": Number },
    MoveY => "Move Axis Y (S/W)" {  } -> { "Value": Number },
    MouseX => "Mouse Delta X (right-drag)" {  } -> { "Value": Number },
    MouseY => "Mouse Delta Y (right-drag)" {  } -> { "Value": Number },
    ForwardVector => "Forward Vector" { "Target": Object } -> { "Value": Vector },
    BreakVector => "Break Vector" { "Value": Vector } -> { "X": Number, "Y": Number, "Z": Number },
    IsRigidbody => "Is Rigidbody (dynamic body)" { "Value": Object } -> { "Value": Bool },
    GetVariable => "Get Variable" {  } -> { "Value": Number },
    Add => "Add" { "A": Number, "B": Number } -> { "Value": Number },
    Subtract => "Subtract" { "A": Number, "B": Number } -> { "Value": Number },
    Multiply => "Multiply" { "A": Number, "B": Number } -> { "Value": Number },
    Divide => "Divide" { "A": Number, "B": Number } -> { "Value": Number },
    Sine => "Sine (radians)" { "Radians": Number } -> { "Value": Number },
    Clamp => "Clamp (min–max)" { "Value": Number, "Min": Number, "Max": Number } -> { "Value": Number },
    Greater => "Greater Than" { "A": Number, "B": Number } -> { "Value": Bool },
    Less => "Less Than" { "A": Number, "B": Number } -> { "Value": Bool },
    Equal => "Equal" { "A": Number, "B": Number } -> { "Value": Bool },
    Not => "Not" { "Value": Bool } -> { "Value": Bool },
    And => "And" { "A": Bool, "B": Bool } -> { "Value": Bool },
    Or => "Or" { "A": Bool, "B": Bool } -> { "Value": Bool },
    MakeVector => "Make Vector" { "X": Number, "Y": Number, "Z": Number } -> { "Value": Vector },
    ScaleVector => "Scale Vector" { "Vector": Vector, "Factor": Number } -> { "Value": Vector },
    AddVector => "Add Vectors" { "A": Vector, "B": Vector } -> { "Value": Vector },
    Branch => "Branch" { "In": Exec, "Condition": Bool } -> { "True": Exec, "False": Exec },
    SetVariable => "Set Variable" { "In": Exec, "Value": Number } -> { "Then": Exec },
    Translate => "Translate (local delta)" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    Rotate => "Rotate (degrees delta)" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    SetPosition => "Set Position" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    SetRotation => "Set Rotation" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    SetScale => "Set Scale" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    SetColor => "Set Color (RGB)" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec },
    SetVisible => "Set Visible" { "In": Exec, "Visible": Bool, "Target": Object } -> { "Then": Exec },
    SetLightIntensity => "Set Light Intensity" { "In": Exec, "Intensity": Number, "Target": Object } -> { "Then": Exec },
    SetFocusDistance => "Set Focus Distance" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetAperture => "Set Aperture (f-stop)" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetFogDensity => "Set Volumetric Fog Density" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetFogLightIntensity => "Set Volumetric Light Intensity" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetExposure => "Set Exposure (EV)" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetBloomIntensity => "Set Bloom Intensity" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetSaturation => "Set Saturation" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetHeatStrength => "Set Heat Strength" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetGrainIntensity => "Set Grain Intensity" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SetVignetteIntensity => "Set Vignette Intensity" { "In": Exec, "Value": Number } -> { "Then": Exec },
    MoveWithCollision => "Move With Collision" { "In": Exec, "Value": Vector, "Target": Object } -> { "Then": Exec, "Grounded": Bool },
    Jump => "Jump" { "In": Exec, "Speed": Number, "Target": Object } -> { "Then": Exec },
    SetVelocity => "Set Velocity" { "In": Exec, "Velocity": Vector, "Target": Object } -> { "Then": Exec },
    LockCursor => "Lock Cursor" { "In": Exec } -> { "Then": Exec },
    UnlockCursor => "Unlock Cursor" { "In": Exec } -> { "Then": Exec },
    Print => "Print Number" { "In": Exec, "Value": Number } -> { "Then": Exec },
    SpawnPrefab => "Spawn Prefab" { "In": Exec, "Position": Vector } -> { "Then": Exec, "Instance": Object },
    DestroyPrefab => "Destroy Prefab" { "In": Exec, "Target": Object } -> { "Then": Exec },
    Enable => "On Enable" { } -> { "Then": Exec },
    Disable => "On Disable" { } -> { "Then": Exec },
    Destroy => "On Destroy" { } -> { "Then": Exec },
    CollisionEnter => "On Collision Enter" { } -> { "Then": Exec, "Other": Object, "Normal": Vector, "Impulse": Number },
    SetGraphEnabled => "Set Graph Enabled" { "In": Exec, "Enabled": Bool, "Target": Object, "Attachment": Number } -> { "Then": Exec },
    Delay => "Delay / After" { "In": Exec, "Seconds": Number } -> { "Then": Exec },
    Lerp => "Lerp" { "A": Number, "B": Number, "T": Number } -> { "Value": Number },
    Min => "Min" { "A": Number, "B": Number } -> { "Value": Number },
    Max => "Max" { "A": Number, "B": Number } -> { "Value": Number },
    Abs => "Abs" { "Value": Number } -> { "Value": Number },
    Length => "Length" { "Value": Vector } -> { "Value": Number },
    Normalize => "Normalize" { "Value": Vector } -> { "Value": Vector },
    Dot => "Dot" { "A": Vector, "B": Vector } -> { "Value": Number },
    Cross => "Cross" { "A": Vector, "B": Vector } -> { "Value": Vector },
    Distance => "Distance" { "A": Vector, "B": Vector } -> { "Value": Number },
    Modulo => "Modulo" { "A": Number, "B": Number } -> { "Value": Number },
    Power => "Power" { "A": Number, "B": Number } -> { "Value": Number },
    Random => "Random (seeded)" { "In": Exec, "Min": Number, "Max": Number } -> { "Then": Exec, "Value": Number },
    Cosine => "Cosine (radians)" { "Value": Number } -> { "Value": Number },
    Tangent => "Tangent (radians)" { "Value": Number } -> { "Value": Number },
    ArcSine => "Arc Sine" { "Value": Number } -> { "Value": Number },
    ArcCosine => "Arc Cosine" { "Value": Number } -> { "Value": Number },
    Atan2 => "Atan2 (Y, X)" { "A": Number, "B": Number } -> { "Value": Number },
    ToRadians => "Degrees to Radians" { "Value": Number } -> { "Value": Number },
    ToDegrees => "Radians to Degrees" { "Value": Number } -> { "Value": Number },
    Floor => "Floor" { "Value": Number } -> { "Value": Number },
    Ceil => "Ceil" { "Value": Number } -> { "Value": Number },
    Round => "Round" { "Value": Number } -> { "Value": Number },
    Sqrt => "Square Root" { "Value": Number } -> { "Value": Number },
    LerpVector => "Lerp Vectors" { "A": Vector, "B": Vector, "T": Number } -> { "Value": Vector },
    Raycast => "Raycast" { "In": Exec, "Origin": Vector, "Direction": Vector, "Distance": Number, "Ignore": Object } -> { "Then": Exec, "Hit": Bool, "Object": Object, "Position": Vector, "Normal": Vector, "Distance": Number },
    SphereOverlap => "Sphere Overlap" { "In": Exec, "Center": Vector, "Radius": Number, "Ignore": Object } -> { "Then": Exec, "Count": Number },
    BoxOverlap => "Box Overlap" { "In": Exec, "Center": Vector, "Size": Vector, "Ignore": Object } -> { "Then": Exec, "Count": Number },
    LineOfSight => "Line of Sight" { "In": Exec, "From": Vector, "To": Vector, "Ignore": Object } -> { "Then": Exec, "Visible": Bool },
    ListGet => "List Get" { "Index": Number } -> { "Value": Number },
    ListPush => "List Push" { "In": Exec, "Value": Number } -> { "Then": Exec },
    ListSet => "List Set" { "In": Exec, "Value": Number, "Index": Number } -> { "Then": Exec },
    ListRemove => "List Remove" { "In": Exec, "Index": Number } -> { "Then": Exec },
    ListClear => "List Clear" { "In": Exec } -> { "Then": Exec },
    ListLength => "List Length" { } -> { "Value": Number },
    LoadScene => "Load Scene" { "In": Exec, "Scene": Text } -> { "Then": Exec },
    AddScene => "Load Scene Additively" { "In": Exec, "Scene": Text } -> { "Then": Exec },
    RestartScene => "Restart Scene" { "In": Exec } -> { "Then": Exec },
    SaveGame => "Save Game State" { "In": Exec, "Slot": Text } -> { "Then": Exec },
    LoadGame => "Load Game State" { "In": Exec, "Slot": Text } -> { "Then": Exec },
    Comment => "Comment" { } -> { },
    Reroute => "Reroute" { "Value": Number } -> { "Value": Number },
    PlayTween => "Play Tween" { "In": Exec, "Restart": Bool, "Target": Object } -> { "Then": Exec },
    PauseTween => "Pause Tween" { "In": Exec, "Target": Object } -> { "Then": Exec },
    StopTween => "Stop Tween" { "In": Exec, "Target": Object } -> { "Then": Exec },
    SeekTween => "Seek Tween" { "In": Exec, "Seconds": Number, "Target": Object } -> { "Then": Exec },
    TweenProgress => "Tween Progress" { "Target": Object } -> { "Value": Number },
    SampleCurve => "Sample Motion Curve" { "Seconds": Number, "Track": Number, "Channel": Number, "Target": Object } -> { "Value": Number },
    TweenFinished => "On Tween Finished" { } -> { "Then": Exec },
    PlayTimeline => "Play Timeline" { "In": Exec, "Restart": Bool, "Target": Object } -> { "Then": Exec },
    PauseTimeline => "Pause Timeline" { "In": Exec, "Target": Object } -> { "Then": Exec },
    StopTimeline => "Stop Timeline" { "In": Exec, "Target": Object } -> { "Then": Exec },
    SeekTimeline => "Seek Timeline" { "In": Exec, "Seconds": Number, "Target": Object } -> { "Then": Exec },
    TimelineProgress => "Timeline Progress" { "Target": Object } -> { "Value": Number },
    TimelineEvent => "On Timeline Event" { } -> { "Then": Exec, "Name": Text, "Seconds": Number },
    UiEvent => "On UI Event" {} -> { "Then": Exec, "Name": Text, "Value": Number },
    SetUiText => "Set UI Text" { "In": Exec, "Text": Text, "Target": Object } -> { "Then": Exec },
    SetUiValue => "Set UI Value" { "In": Exec, "Value": Number, "Target": Object } -> { "Then": Exec },
    SetUiVisible => "Set UI Visible" { "In": Exec, "Visible": Bool, "Target": Object } -> { "Then": Exec },
    SetUiEnabled => "Set UI Enabled" { "In": Exec, "Enabled": Bool, "Target": Object } -> { "Then": Exec },
    FocusUi => "Focus UI Widget" { "In": Exec, "Target": Object } -> { "Then": Exec },
    UiValue => "UI Value" { "Target": Object } -> { "Value": Number },
    SetUiLanguage => "Set UI Language" { "In": Exec, "Language": Text } -> { "Then": Exec },
    SetUiTextScale => "Set UI Text Scale" { "In": Exec, "Scale": Number } -> { "Then": Exec },
    SetUiContrast => "Set UI High Contrast" { "In": Exec, "Enabled": Bool } -> { "Then": Exec },
    SetUiReducedMotion => "Set UI Reduced Motion" { "In": Exec, "Enabled": Bool } -> { "Then": Exec },
    UiReducedMotion => "UI Reduced Motion" {} -> { "Value": Bool },
    StartGame => "Start Game" { "In": Exec } -> { "Then": Exec },
    PauseGame => "Pause Game" { "In": Exec } -> { "Then": Exec },
    ResumeGame => "Resume Game" { "In": Exec } -> { "Then": Exec },
    RestartGame => "Restart Game" { "In": Exec } -> { },
    QuitGame => "Quit Game" { "In": Exec } -> { },
    PlaySprite => "Play Sprite Animation" { "In": Exec, "Clip": Text, "Restart": Bool, "Target": Object } -> { "Then": Exec },
    PauseSprite => "Pause Sprite Animation" { "In": Exec, "Target": Object } -> { "Then": Exec },
    StopSprite => "Stop Sprite Animation" { "In": Exec, "Target": Object } -> { "Then": Exec },
    SetSpriteFrame => "Set Sprite Frame" { "In": Exec, "Frame": Number, "Target": Object } -> { "Then": Exec },
    SpriteFrame => "Sprite Frame" { "Target": Object } -> { "Frame": Number },
    SpriteEvent => "On Sprite Event" {} -> { "Then": Exec, "Name": Text, "Frame": Number },
    SetTile => "Set Tile" { "In": Exec, "X": Number, "Y": Number, "Tile": Number, "Target": Object } -> { "Then": Exec },
    GetTile => "Get Tile" { "X": Number, "Y": Number, "Target": Object } -> { "Tile": Number },
    SetNavDestination => "Set Navigation Destination" { "In": Exec, "Position": Vector, "Target": Object } -> { "Then": Exec },
    SetNavState => "Set Navigation State" { "In": Exec, "Name": Text, "Target": Object } -> { "Then": Exec },
    SetNavTarget => "Set Navigation Target" { "In": Exec, "Follow Object": Object, "Target": Object } -> { "Then": Exec },
    StopNavigation => "Stop Navigation" { "In": Exec, "Target": Object } -> { "Then": Exec },
    NavState => "Navigation State" { "Target": Object } -> { "Name": Text },
    NavHasPath => "Navigation Has Path" { "Target": Object } -> { "Value": Bool },
    NavSeesTarget => "Navigation Sees Target" { "Target": Object } -> { "Value": Bool },
    NavVelocity => "Navigation Velocity" { "Target": Object } -> { "Velocity": Vector },
    NavigationEvent => "On Navigation Event" {} -> { "Then": Exec, "Name": Text, "Distance": Number, "Other": Object },
    PlayAudio => "Play Audio" { "In": Exec, "Restart": Bool, "Target": Object } -> { "Then": Exec },
    PauseAudio => "Pause Audio" { "In": Exec, "Target": Object } -> { "Then": Exec },
    StopAudio => "Stop Audio" { "In": Exec, "Target": Object } -> { "Then": Exec },
    SeekAudio => "Seek Audio" { "In": Exec, "Seconds": Number, "Target": Object } -> { "Then": Exec },
    SetAudioVolume => "Set Audio Volume" { "In": Exec, "Volume": Number, "Target": Object } -> { "Then": Exec },
    SetAudioPitch => "Set Audio Pitch" { "In": Exec, "Pitch": Number, "Target": Object } -> { "Then": Exec },
    SetAudioPan => "Set Audio Pan" { "In": Exec, "Pan": Number, "Target": Object } -> { "Then": Exec },
    SetAudioBusVolume => "Set Audio Bus Volume" { "In": Exec, "Bus": Text, "Volume": Number } -> { "Then": Exec },
    AudioPosition => "Audio Position" { "Target": Object } -> { "Seconds": Number },
    AudioPlaying => "Audio Playing" { "Target": Object } -> { "Playing": Bool },
    AudioFinished => "On Audio Finished" { } -> { "Then": Exec },
    PlayAnimation => "Play Animation" { "In": Exec, "State": Text, "Fade Seconds": Number, "Target": Object } -> { "Then": Exec },
    PauseAnimation => "Pause Animation" { "In": Exec, "Target": Object } -> { "Then": Exec },
    StopAnimation => "Stop Animation" { "In": Exec, "Target": Object } -> { "Then": Exec },
    SeekAnimation => "Seek Animation" { "In": Exec, "Progress": Number, "Target": Object } -> { "Then": Exec },
    SetAnimationParameter => "Set Animation Parameter" { "In": Exec, "Name": Text, "Value": Number, "Target": Object } -> { "Then": Exec },
    AnimationProgress => "Animation Progress" { "Target": Object } -> { "Value": Number },
    AnimationState => "Animation State" { "Target": Object } -> { "Name": Text },
    AnimationEvent => "On Animation Event" { } -> { "Then": Exec, "Name": Text, "Seconds": Number },
}

impl NodeKind {
    /// The whole table, for menus and validation over every kind.
    pub fn specs() -> &'static [NodeSpec] {
        NODE_SPECS
    }
    /// This kind's row. Declaration order makes the lookup an index.
    pub fn spec(self) -> &'static NodeSpec {
        &NODE_SPECS[self as usize]
    }
    pub fn title(self) -> &'static str {
        self.spec().title
    }
    pub fn inputs(self) -> &'static [(&'static str, PinType)] {
        self.spec().inputs
    }
    pub fn outputs(self) -> &'static [(&'static str, PinType)] {
        self.spec().outputs
    }
    /// Events start a chain from the simulation or input instead of an incoming wire.
    pub fn event(self) -> bool {
        self.inputs().is_empty()
            && self
                .outputs()
                .first()
                .is_some_and(|port| port.1 == PinType::Exec)
    }
    pub fn target_port(self) -> Option<usize> {
        self.inputs()
            .iter()
            .position(|(label, kind)| *label == "Target" && *kind == PinType::Object)
    }
    pub fn action(self) -> bool {
        self.inputs()
            .first()
            .is_some_and(|port| port.1 == PinType::Exec)
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
        crate::middleware::registry::remap_all(self, mapping);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_node_table_is_the_single_declaration_of_every_kind() {
        let mut titles = BTreeSet::new();
        for (index, spec) in NODE_SPECS.iter().enumerate() {
            assert_eq!(
                spec.kind as usize, index,
                "{} is out of declaration order, so spec() would return the wrong row",
                spec.title
            );
            assert_eq!(spec.kind.spec().title, spec.title);
            assert!(titles.insert(spec.title), "duplicate title {}", spec.title);
            // The snake_case name is the wire format saved graphs depend on.
            let name = serde_json::to_string(&spec.kind).unwrap();
            assert_eq!(name, format!("\"{}\"", name.trim_matches('"')));
            assert_eq!(serde_json::from_str::<NodeKind>(&name).unwrap(), spec.kind);
            // A target port is only meaningful where the pin exists.
            assert!(
                spec.kind.target_port().is_none()
                    || spec.inputs[spec.kind.target_port().unwrap()].1 == PinType::Object
            );
        }
        assert_eq!(titles.len(), NODE_SPECS.len());
    }

    #[test]
    fn events_stay_the_nodes_the_simulation_starts() {
        let events: Vec<_> = NodeKind::specs()
            .iter()
            .filter(|spec| spec.kind.event())
            .map(|spec| spec.title)
            .collect();
        assert_eq!(
            events,
            [
                "On Object Enter",
                "On Object Exit",
                "On Start",
                "On Update",
                "On Input Pressed",
                "On Overlap Enter",
                "On Overlap Exit",
                "On Enable",
                "On Disable",
                "On Destroy",
                "On Collision Enter",
                "On Tween Finished",
                "On Timeline Event",
                "On UI Event",
                "On Sprite Event",
                "On Navigation Event",
                "On Audio Finished",
                "On Animation Event",
            ],
            "the runtime starts chains from exactly these"
        );
        // An action consumes an Exec wire; a value node does neither.
        assert!(NodeKind::Print.action() && !NodeKind::Print.event());
        assert!(NodeKind::SelfObject.outputs()[0].1 == PinType::Object);
        assert!(!NodeKind::SelfObject.event() && !NodeKind::SelfObject.action());
    }
}
