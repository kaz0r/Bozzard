//! Portable, typed gameplay graphs. Attachments embed independent copies, not file references.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinType {
    Exec,
    Text,
    Number,
    Bool,
    Vector,
    Object,
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
}
impl NodeKind {
    pub const ALL: [Self; 78] = [
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
        }
    }
    pub fn event(self) -> bool {
        matches!(
            self,
            Self::Start
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
            Self::EndGame => &[],
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
            Self::Vector
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
        Self {
            prefab: String::new(),
            id,
            position,
            kind,
            inputs: kind
                .inputs()
                .iter()
                .map(|(_, t)| match t {
                    PinType::Object => Value::Object(if kind == NodeKind::Object {
                        ObjectRef::None
                    } else {
                        ObjectRef::SelfObject
                    }),
                    PinType::Exec => Value::Exec,
                    PinType::Text => Value::Text(String::new()),
                    PinType::Number => Value::Number(0.),
                    PinType::Bool => Value::Bool(false),
                    PinType::Vector => Value::Vector(if kind == NodeKind::SetScale {
                        [1.; 3]
                    } else {
                        [0.; 3]
                    }),
                })
                .collect(),
            variable: "value".into(),
            key: InputKey::default(),
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
        graph.validate()?;
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
            self.nodes.len() <= 128 && self.wires.len() <= 512 && self.variables.len() <= 64,
            "blueprint limit: 128 nodes, 512 wires, 64 variables"
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
                n.inputs.len() == n.kind.inputs().len()
                    && n.inputs
                        .iter()
                        .zip(n.kind.inputs())
                        .all(|(v, (_, t))| v.kind() == *t && v.valid()),
                "invalid inputs on node {}",
                n.id
            );
            ensure!(n.prefab.len() <= 256, "prefab asset ID too long");
            ensure!(n.variable.len() <= 64, "variable name too long");
            if matches!(n.kind, NodeKind::GetVariable | NodeKind::SetVariable) {
                ensure!(
                    self.variables.contains_key(&n.variable),
                    "unknown variable '{}'",
                    n.variable
                );
            }
        }
        let mut incoming = BTreeSet::new();
        let mut degrees: BTreeMap<_, usize> = ids.iter().map(|id| (*id, 0)).collect();
        for w in &self.wires {
            let from = self
                .node(w.from.node)?
                .kind
                .outputs()
                .get(w.from.port)
                .context("invalid output pin")?;
            let to = self
                .node(w.to.node)?
                .kind
                .inputs()
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
            .filter_map(|v| match v {
                Value::Object(ObjectRef::Id(id)) => Some(id.as_str()),
                _ => None,
            })
    }
    pub fn remap_objects(&mut self, mapping: &BTreeMap<String, String>) {
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
        for value in self.nodes.iter_mut().flat_map(|n| &mut n.inputs) {
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
        for attachment in &mut self.blueprints {
            attachment.graph.remap_objects(mapping);
        }
    }
}
