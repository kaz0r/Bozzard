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
/// One blueprint node kind: its editor title and its pins.
///
/// The table below is the single declaration of a node. It generates the enum, the editor's
/// add-node menu and every pin list, so adding a node is one row plus its runtime arm.
#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub kind: NodeKind,
    pub title: &'static str,
    pub inputs: &'static [(&'static str, PinType)],
    pub outputs: &'static [(&'static str, PinType)],
}

macro_rules! node_kinds {
    ($($variant:ident => $title:literal { $($input_label:literal : $input_type:ident),* $(,)? } -> { $($output_label:literal : $output_type:ident),* $(,)? }),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum NodeKind {
            $($variant,)*
        }
        /// Every node kind in declaration order, which is also the editor's menu order.
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
            ],
            "the runtime starts chains from exactly these"
        );
        // An action consumes an Exec wire; a value node does neither.
        assert!(NodeKind::Print.action() && !NodeKind::Print.event());
        assert!(NodeKind::SelfObject.outputs()[0].1 == PinType::Object);
        assert!(!NodeKind::SelfObject.event() && !NodeKind::SelfObject.action());
    }
}
