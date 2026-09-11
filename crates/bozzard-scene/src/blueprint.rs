//! Portable, typed gameplay graphs. Attachments embed independent copies, not file references.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinType {
    Exec,
    Number,
    Bool,
    Vector,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Exec,
    Number(f32),
    Bool(bool),
    Vector([f32; 3]),
}
impl Value {
    pub fn kind(&self) -> PinType {
        match self {
            Self::Exec => PinType::Exec,
            Self::Number(_) => PinType::Number,
            Self::Bool(_) => PinType::Bool,
            Self::Vector(_) => PinType::Vector,
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Number(n) => n.is_finite(),
            Self::Vector(v) => v.iter().all(|n| n.is_finite()),
            _ => true,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKey {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
}
impl InputKey {
    pub const ALL: [Self; 5] = [
        Self::Forward,
        Self::Backward,
        Self::Left,
        Self::Right,
        Self::Jump,
    ];
    pub fn active(self, input: GameplayInput) -> bool {
        match self {
            Self::Forward => input.movement[1] > 0.,
            Self::Backward => input.movement[1] < 0.,
            Self::Left => input.movement[0] < 0.,
            Self::Right => input.movement[0] > 0.,
            Self::Jump => input.jump,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
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
    GetVariable,
    Add,
    Subtract,
    Multiply,
    Divide,
    Sine,
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
    MoveWithCollision,
    Jump,
    Print,
}
impl NodeKind {
    pub const ALL: [Self; 44] = [
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
        Self::GetVariable,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Sine,
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
        Self::MoveWithCollision,
        Self::Jump,
        Self::Print,
    ];
    pub fn title(self) -> &'static str {
        match self {
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
            Self::GetVariable => "Get Variable",
            Self::SetVariable => "Set Variable",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Divide => "Divide",
            Self::Sine => "Sine (radians)",
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
            Self::MoveWithCollision => "Move With Collision",
            Self::Jump => "Jump",
            Self::Print => "Print Number",
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
        )
    }
    pub fn inputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
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
            Self::Not => &[("Value", Bool)],
            Self::And | Self::Or => &[("A", Bool), ("B", Bool)],
            Self::MakeVector => &[("X", Number), ("Y", Number), ("Z", Number)],
            Self::ScaleVector => &[("Vector", Vector), ("Factor", Number)],
            Self::AddVector => &[("A", Vector), ("B", Vector)],
            Self::Branch => &[("In", Exec), ("Condition", Bool)],
            Self::SetVariable | Self::Print => &[("In", Exec), ("Value", Number)],
            Self::Translate
            | Self::Rotate
            | Self::SetPosition
            | Self::SetRotation
            | Self::SetScale
            | Self::SetColor
            | Self::MoveWithCollision => &[("In", Exec), ("Value", Vector)],
            Self::SetVisible => &[("In", Exec), ("Visible", Bool)],
            Self::SetLightIntensity => &[("In", Exec), ("Intensity", Number)],
            Self::Jump => &[("In", Exec), ("Speed", Number)],
            _ => &[],
        }
    }
    pub fn action(self) -> bool {
        self.inputs().first().is_some_and(|p| p.1 == PinType::Exec)
    }
    pub fn outputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
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
            | Self::MakeVector
            | Self::ScaleVector
            | Self::AddVector => &[("Value", Vector)],
            _ => &[("Value", Number)],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: u32,
    pub position: [f32; 2],
    pub kind: NodeKind,
    pub inputs: Vec<Value>,
    #[serde(default)]
    pub variable: String,
    #[serde(default = "jump_key")]
    pub key: InputKey,
}
fn jump_key() -> InputKey {
    InputKey::Jump
}
impl Node {
    pub fn new(id: u32, kind: NodeKind, position: [f32; 2]) -> Self {
        Self {
            id,
            position,
            kind,
            inputs: kind
                .inputs()
                .iter()
                .map(|(_, t)| match t {
                    PinType::Exec => Value::Exec,
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
            key: InputKey::Jump,
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
            ensure!(from.1 == to.1, "pin types do not match");
            ensure!(incoming.insert(w.to), "input already connected");
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
