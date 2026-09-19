//! Portable, typed surface shader graphs. One graph attaches per object and
//! compiles to a WGSL `graph_material_surface` function for the scene renderer.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Surface pins. Float and Vector (linear RGB or 3D) only; texturing comes from
/// the drawable's existing five material map slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Float,
    Vector,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Float(f32),
    Vector([f32; 3]),
}
impl Value {
    pub fn kind(&self) -> PinType {
        match self {
            Self::Float(_) => PinType::Float,
            Self::Vector(_) => PinType::Vector,
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Float(n) => n.is_finite(),
            Self::Vector(v) => v.iter().all(|n| n.is_finite()),
        }
    }
}
/// Material map a Texture Sample node reads. Names match the renderer's bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureSlot {
    #[default]
    BaseColor,
    Normal,
    MetallicRoughness,
    Occlusion,
    Emissive,
}
impl TextureSlot {
    pub const ALL: [Self; 5] = [
        Self::BaseColor,
        Self::Normal,
        Self::MetallicRoughness,
        Self::Occlusion,
        Self::Emissive,
    ];
}
impl TextureSlot {
    /// Binding names provided by both scene fragment shader hosts.
    pub fn bindings(self) -> (&'static str, &'static str) {
        match self {
            Self::BaseColor => ("color_texture", "color_sampler"),
            Self::Normal => ("normal_texture", "normal_sampler"),
            Self::MetallicRoughness => ("mr_texture", "mr_sampler"),
            Self::Occlusion => ("ao_texture", "ao_sampler"),
            Self::Emissive => ("emissive_texture", "emissive_sampler"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Master,
    Time,
    UV,
    WorldNormal,
    WorldPosition,
    ViewDirection,
    Float,
    Color,
    Vector,
    TextureSample,
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Sine,
    Clamp,
    Lerp,
    OneMinus,
    AddVector,
    MultiplyVectors,
    ScaleVector,
    LerpVector,
    Dot,
    Normalize,
    Append,
    Split,
    StaticSwitch,
    StaticSwitchVector,
}
impl NodeKind {
    /// Palette order for the node editor; Master is managed automatically.
    pub const ALL: [Self; 28] = [
        Self::Time,
        Self::UV,
        Self::WorldNormal,
        Self::WorldPosition,
        Self::ViewDirection,
        Self::Float,
        Self::Color,
        Self::Vector,
        Self::TextureSample,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Power,
        Self::Sine,
        Self::Clamp,
        Self::Lerp,
        Self::OneMinus,
        Self::AddVector,
        Self::MultiplyVectors,
        Self::ScaleVector,
        Self::LerpVector,
        Self::Dot,
        Self::Normalize,
        Self::Append,
        Self::Split,
        Self::StaticSwitch,
        Self::StaticSwitchVector,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Master => "Master",
            Self::Time => "Time",
            Self::UV => "UV",
            Self::WorldNormal => "World Normal",
            Self::WorldPosition => "World Position",
            Self::ViewDirection => "View Direction",
            Self::Float => "Float",
            Self::Color => "Color",
            Self::Vector => "Vector",
            Self::TextureSample => "Texture Sample",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Divide => "Divide",
            Self::Power => "Power",
            Self::Sine => "Sine",
            Self::Clamp => "Clamp",
            Self::Lerp => "Lerp",
            Self::OneMinus => "One Minus",
            Self::AddVector => "Add Vectors",
            Self::MultiplyVectors => "Multiply Vectors",
            Self::ScaleVector => "Scale Vector",
            Self::LerpVector => "Lerp Vectors",
            Self::Dot => "Dot Product",
            Self::Normalize => "Normalize",
            Self::Append => "Make Vector",
            Self::Split => "Split Vector",
            Self::StaticSwitch => "Static Switch",
            Self::StaticSwitchVector => "Static Switch Vector",
        }
    }
    pub fn inputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
            Self::Master => &[
                ("Base Color", Vector),
                ("Metallic", Float),
                ("Roughness", Float),
                ("Emissive", Vector),
                ("Alpha", Float),
                ("Normal", Vector),
            ],
            Self::Float => &[("Value", Float)],
            Self::Color | Self::Vector => &[("Value", Vector)],
            Self::TextureSample => &[("UV", Vector)],
            Self::Add | Self::Subtract | Self::Multiply | Self::Divide => {
                &[("A", Float), ("B", Float)]
            }
            Self::Power => &[("Base", Float), ("Exponent", Float)],
            Self::Sine | Self::OneMinus => &[("Value", Float)],
            Self::Clamp => &[("Value", Float), ("Min", Float), ("Max", Float)],
            Self::Lerp => &[("A", Float), ("B", Float), ("Factor", Float)],
            Self::AddVector | Self::MultiplyVectors => &[("A", Vector), ("B", Vector)],
            Self::ScaleVector => &[("Vector", Vector), ("Factor", Float)],
            Self::LerpVector => &[("A", Vector), ("B", Vector), ("Factor", Float)],
            Self::Dot => &[("A", Vector), ("B", Vector)],
            Self::Normalize => &[("Value", Vector)],
            Self::Append => &[("X", Float), ("Y", Float), ("Z", Float)],
            Self::Split => &[("Value", Vector)],
            Self::StaticSwitch => &[("Off", Float), ("On", Float)],
            Self::StaticSwitchVector => &[("Off", Vector), ("On", Vector)],
            _ => &[],
        }
    }
    pub fn outputs(self) -> &'static [(&'static str, PinType)] {
        use PinType::*;
        match self {
            Self::Master => &[],
            Self::Time
            | Self::Float
            | Self::Add
            | Self::Subtract
            | Self::Multiply
            | Self::Divide
            | Self::Power
            | Self::Sine
            | Self::Clamp
            | Self::Lerp
            | Self::OneMinus
            | Self::Dot
            | Self::StaticSwitch => &[("Value", Float)],
            Self::UV
            | Self::WorldNormal
            | Self::WorldPosition
            | Self::ViewDirection
            | Self::Color
            | Self::Vector
            | Self::AddVector
            | Self::MultiplyVectors
            | Self::ScaleVector
            | Self::LerpVector
            | Self::Normalize
            | Self::Append
            | Self::StaticSwitchVector => &[("Value", Vector)],
            Self::TextureSample => &[("Color", Vector), ("Alpha", Float)],
            Self::Split => &[("X", Float), ("Y", Float), ("Z", Float)],
        }
    }
    /// WGSL expression for each output port of a node whose value was bound to `let v{id}`.
    /// Textures sample into `v{id}` (Color = rgb, Alpha = a); Split picks components.
    fn output_expr(self, id: u32, port: usize) -> String {
        match self {
            Self::TextureSample => format!("v{id}.{}", ["rgb", "a"][port]),
            Self::Split => format!("v{id}.{}", ["x", "y", "z"][port]),
            _ => format!("v{id}"),
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
    pub slot: TextureSlot,
    /// Named compile-time keyword used by a static switch.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keyword: String,
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
                .map(|(label, t)| match t {
                    PinType::Float => {
                        Value::Float(if kind == NodeKind::Lerp && *label == "Factor" {
                            0.5
                        } else {
                            0.
                        })
                    }
                    PinType::Vector => Value::Vector(if kind == NodeKind::Color {
                        [1.; 3]
                    } else {
                        [0.; 3]
                    }),
                })
                .collect(),
            slot: TextureSlot::BaseColor,
            keyword: String::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub node: u32,
    pub port: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub from: Socket,
    pub to: Socket,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderGraph {
    pub version: u32,
    pub name: String,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    /// At most eight boolean defaults: a graph has at most 256 specializations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keywords: BTreeMap<String, bool>,
}
impl Default for ShaderGraph {
    fn default() -> Self {
        Self {
            version: 1,
            name: "New Shader".into(),
            nodes: vec![Node::new(1, NodeKind::Master, [300., 40.])],
            wires: vec![],
            keywords: BTreeMap::new(),
        }
    }
}
/// Master channel: (input port, SurfaceParams field, clamped assignment).
type MasterField = (usize, &'static str, Option<(&'static str, &'static str)>);
impl ShaderGraph {
    pub fn from_json(json: &str) -> Result<Self> {
        ensure!(json.len() <= 1024 * 1024, "shader graph exceeds 1 MiB");
        let graph: Self = serde_json::from_str(json).context("parsing shader graph JSON")?;
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
            .context("missing shader graph node")
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
        self.validate_presentation()?;
        ensure!(self.version == 1, "unsupported shader graph version");
        self.keyword_mask(&BTreeMap::new())?;
        ensure!(
            self.nodes.len() <= 128 && self.wires.len() <= 512,
            "shader graph limit: 128 nodes, 512 wires"
        );
        let mut ids = BTreeSet::new();
        let mut masters = 0;
        for n in &self.nodes {
            ensure!(ids.insert(n.id), "duplicate shader graph node ID");
            if matches!(
                n.kind,
                NodeKind::StaticSwitch | NodeKind::StaticSwitchVector
            ) {
                ensure!(
                    self.keywords.contains_key(&n.keyword),
                    "static switch {} requires a declared keyword",
                    n.id
                );
            } else {
                ensure!(
                    n.keyword.is_empty(),
                    "only static switches accept a keyword"
                );
            }
            ensure!(
                n.inputs.len() == n.kind.inputs().len()
                    && n.inputs
                        .iter()
                        .zip(n.kind.inputs())
                        .all(|(v, (_, t))| v.kind() == *t && v.valid()),
                "invalid inputs on node {}",
                n.id
            );
            if n.kind == NodeKind::Master {
                masters += 1;
            }
        }
        ensure!(masters == 1, "shader graph needs exactly one Master node");
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
        ensure!(visited == self.nodes.len(), "shader graphs cannot cycle");
        Ok(())
    }
    /// Presentation is excluded from compiled-source identities, but must still
    /// be checked when an edited layout reuses a previously validated program.
    pub fn validate_presentation(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "shader graph needs a name (1–128 bytes)"
        );
        ensure!(
            self.nodes.iter().all(|n| n
                .position
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1_000_000.)),
            "invalid node position"
        );
        Ok(())
    }
    pub fn keyword_mask(&self, overrides: &BTreeMap<String, bool>) -> Result<u8> {
        self.layered_keyword_mask(std::iter::once(overrides))
    }
    /// Later layers override earlier ones without allocating a combined map.
    pub fn layered_keyword_mask<'a>(
        &self,
        layers: impl IntoIterator<Item = &'a BTreeMap<String, bool>>,
    ) -> Result<u8> {
        ensure!(
            self.keywords.len() <= 8,
            "shader graph limit: eight keywords"
        );
        for name in self.keywords.keys() {
            ensure!(
                valid_keyword(name),
                "invalid shader keyword '{name}': use 1–32 ASCII letters, digits or underscores, starting with a letter or underscore"
            );
        }
        let mut mask = self
            .keywords
            .iter()
            .enumerate()
            .fold(0, |mask, (index, (_, default))| {
                mask | (u8::from(*default) << index)
            });
        for overrides in layers {
            for (name, enabled) in overrides {
                let index = self
                    .keywords
                    .keys()
                    .position(|key| key == name)
                    .with_context(|| format!("material uses undeclared shader keyword '{name}'"))?;
                let bit = 1 << index;
                mask = if *enabled { mask | bit } else { mask & !bit };
            }
        }
        Ok(mask)
    }
    /// Exact program comparison avoids hash collisions and ignores editor layout.
    pub fn same_program(&self, other: &Self) -> bool {
        self.version == other.version
            && self.keywords == other.keywords
            && self.wires == other.wires
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().zip(&other.nodes).all(|(a, b)| {
                a.id == b.id
                    && a.kind == b.kind
                    && a.inputs.len() == b.inputs.len()
                    && a.inputs.iter().zip(&b.inputs).all(|(a, b)| match (a, b) {
                        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
                        (Value::Vector(a), Value::Vector(b)) => {
                            a.map(f32::to_bits) == b.map(f32::to_bits)
                        }
                        _ => false,
                    })
                    && a.slot == b.slot
                    && a.keyword == b.keyword
            })
    }
    /// Fast lookup hint; callers must also compare the actual program on a hit.
    pub fn program_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.version.hash(&mut hash);
        self.keywords.hash(&mut hash);
        self.wires.hash(&mut hash);
        for node in &self.nodes {
            node.id.hash(&mut hash);
            node.kind.hash(&mut hash);
            node.slot.hash(&mut hash);
            node.keyword.hash(&mut hash);
            for value in &node.inputs {
                std::mem::discriminant(value).hash(&mut hash);
                match value {
                    Value::Float(v) => v.to_bits().hash(&mut hash),
                    Value::Vector(v) => v.map(f32::to_bits).hash(&mut hash),
                }
            }
        }
        hash.finish()
    }
    /// Topologically ordered evaluation plan: (node, per-input resolved expressions).
    fn plan(&self, mask: u8) -> Result<Vec<(&Node, Vec<String>)>> {
        let selected_port = |node: &Node| {
            matches!(
                node.kind,
                NodeKind::StaticSwitch | NodeKind::StaticSwitchVector
            )
            .then(|| {
                let bit = self
                    .keywords
                    .keys()
                    .position(|name| *name == node.keyword)
                    .unwrap();
                usize::from(mask & (1 << bit) != 0)
            })
        };
        // Traverse only connected Master inputs and selected static branches.
        // Unselected texture samples never reach WGSL or a GPU compiler.
        let nodes: BTreeMap<_, _> = self.nodes.iter().map(|n| (n.id, n)).collect();
        let master = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Master)
            .unwrap()
            .id;
        let active_wires: Vec<_> = self
            .wires
            .iter()
            .filter(|w| selected_port(nodes[&w.to.node]).is_none_or(|port| w.to.port == port))
            .collect();
        let mut needed = BTreeSet::new();
        let mut stack = vec![master];
        while let Some(id) = stack.pop() {
            if needed.insert(id) {
                stack.extend(
                    active_wires
                        .iter()
                        .filter(|w| w.to.node == id)
                        .map(|w| w.from.node),
                );
            }
        }
        let wires: Vec<_> = active_wires
            .into_iter()
            .filter(|w| needed.contains(&w.to.node))
            .collect();
        let mut source_of: BTreeMap<Socket, (u32, usize)> = BTreeMap::new();
        for w in &wires {
            source_of.insert(w.to, (w.from.node, w.from.port));
        }
        let mut degrees: BTreeMap<u32, usize> = needed.iter().map(|id| (*id, 0)).collect();
        for w in &wires {
            *degrees.get_mut(&w.to.node).unwrap() += 1;
        }
        let mut order = Vec::new();
        let mut queue: VecDeque<u32> = degrees
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        while let Some(id) = queue.pop_front() {
            order.push(id);
            for w in wires.iter().filter(|w| w.from.node == id) {
                let degree = degrees.get_mut(&w.to.node).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(w.to.node);
                }
            }
        }
        debug_assert_eq!(order.len(), needed.len());
        Ok(order
            .iter()
            .map(|id| {
                let node = self.node(*id).unwrap();
                let inputs = (0..node.kind.inputs().len())
                    .map(|port| match source_of.get(&Socket { node: *id, port }) {
                        Some((source, source_port)) => self
                            .node(*source)
                            .unwrap()
                            .kind
                            .output_expr(*source, *source_port),
                        None => match &node.inputs[port] {
                            Value::Float(v) => wgsl_float(*v),
                            Value::Vector(v) => format!(
                                "vec3<f32>({}, {}, {})",
                                wgsl_float(v[0]),
                                wgsl_float(v[1]),
                                wgsl_float(v[2])
                            ),
                        },
                    })
                    .collect();
                (node, inputs)
            })
            .collect())
    }
    /// Statement for one node in topological order; inputs are already WGSL expressions.
    fn statement(
        kind: NodeKind,
        id: u32,
        slot: TextureSlot,
        inputs: &[String],
        enabled: bool,
    ) -> String {
        let [a, b, c] = match inputs {
            [a] => [a.as_str(), "0", "0"],
            [a, b] => [a.as_str(), b.as_str(), "0"],
            [a, b, c] => [a.as_str(), b.as_str(), c.as_str()],
            _ => ["", "", ""],
        };
        let expr = match kind {
            NodeKind::Time => "time".to_owned(),
            NodeKind::UV => "vec3<f32>(uv, 0.0)".to_owned(),
            NodeKind::WorldNormal => "world_normal".to_owned(),
            NodeKind::WorldPosition => "world".to_owned(),
            NodeKind::ViewDirection => "view".to_owned(),
            NodeKind::TextureSample => {
                let (texture, sampler) = slot.bindings();
                format!("textureSample({texture}, {sampler}, uv)")
            }
            NodeKind::Add => format!("({a} + {b})"),
            NodeKind::Subtract => format!("({a} - {b})"),
            NodeKind::Multiply => format!("({a} * {b})"),
            NodeKind::Divide => format!("({a} / max(abs({b}), 0.0001) * sign({b}))"),
            NodeKind::Power => format!("pow(max({a}, 0.0), {b})"),
            NodeKind::Sine => format!("sin({a})"),
            NodeKind::Clamp => format!("clamp({a}, {b}, {c})"),
            NodeKind::Lerp => format!("mix({a}, {b}, {c})"),
            NodeKind::OneMinus => format!("(1.0 - {a})"),
            NodeKind::AddVector => format!("({a} + {b})"),
            NodeKind::MultiplyVectors => format!("({a} * {b})"),
            NodeKind::ScaleVector => format!("({a} * {b})"),
            NodeKind::LerpVector => format!("mix({a}, {b}, {c})"),
            NodeKind::Dot => format!("dot({a}, {b})"),
            NodeKind::Normalize => format!("normalize({a})"),
            NodeKind::Append => format!("vec3<f32>({a}, {b}, {c})"),
            NodeKind::Split | NodeKind::Float | NodeKind::Color | NodeKind::Vector => a.to_owned(),
            NodeKind::StaticSwitch | NodeKind::StaticSwitchVector => {
                if enabled { b } else { a }.to_owned()
            }
            NodeKind::Master => unreachable!("master has no outputs"),
        };
        format!("    let v{id} = {expr};")
    }
    /// Master channel: (input port, SurfaceParams field, clamped assignment).
    const MASTER_FIELDS: [MasterField; 6] = [
        (0, "base", None),
        (1, "metallic", Some(("0.0", "1.0"))),
        (2, "roughness", Some(("0.045", "1.0"))),
        (3, "emissive", None),
        (4, "alpha", Some(("0.0", "1.0"))),
        (5, "normal", None),
    ];
    /// WGSL body of `graph_material_surface`, overriding only connected Master inputs.
    /// The host provides `default_material_surface` with the same signature and the
    /// `SurfaceParams` struct, `time`, and the material map bindings.
    pub fn surface_function(&self) -> Result<String> {
        self.surface_function_variant(&BTreeMap::new())
    }
    pub fn surface_function_variant(&self, overrides: &BTreeMap<String, bool>) -> Result<String> {
        self.surface_function_mask(self.keyword_mask(overrides)?)
    }
    pub fn surface_function_mask(&self, mask: u8) -> Result<String> {
        self.validate()?;
        ensure!(
            (mask as u16) < (1u16 << self.keywords.len()),
            "invalid shader keyword mask"
        );
        let connected: BTreeSet<Socket> = self.wires.iter().map(|w| w.to).collect();
        let mut code = String::from(
            "fn graph_material_surface(uv: vec2<f32>, normal_uv: vec2<f32>, mr_uv: vec2<f32>, ao_uv: vec2<f32>, emissive_uv: vec2<f32>, world_normal: vec3<f32>, tangent: vec4<f32>, world: vec3<f32>, view: vec3<f32>, front: bool, time: f32) -> SurfaceParams {\n    var params = default_material_surface(uv, normal_uv, mr_uv, ao_uv, emissive_uv, world_normal, tangent, world, view, front, time);\n",
        );
        let plan = self.plan(mask)?;
        for (node, inputs) in &plan {
            if node.kind != NodeKind::Master {
                let enabled = self
                    .keywords
                    .keys()
                    .position(|name| *name == node.keyword)
                    .is_some_and(|bit| mask & (1 << bit) != 0);
                code.push_str(&Self::statement(
                    node.kind, node.id, node.slot, inputs, enabled,
                ));
                code.push('\n');
            }
        }
        let master = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Master)
            .unwrap();
        let (_, master_inputs) = plan.iter().find(|(n, _)| n.id == master.id).unwrap();
        for (port, field, clamp) in Self::MASTER_FIELDS {
            if connected.contains(&Socket {
                node: master.id,
                port,
            }) {
                let value = &master_inputs[port];
                code.push_str(&match clamp {
                    Some((lo, hi)) => format!("    params.{field} = clamp({value}, {lo}, {hi});\n"),
                    None => format!("    params.{field} = {value};\n"),
                });
            }
        }
        code.push_str("    return params;\n}\n");
        Ok(code)
    }
}
pub fn valid_keyword(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn wgsl_float(v: f32) -> String {
    let mut text = format!("{v}");
    if !text.contains('.') && !text.contains('e') {
        text.push_str(".0");
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_variants_remove_unused_branches_and_validate_both_sides() {
        let mut graph = graph(
            vec![
                Node::new(1, NodeKind::Master, [0.; 2]),
                Node::new(2, NodeKind::TextureSample, [0.; 2]),
                Node::new(3, NodeKind::StaticSwitchVector, [0.; 2]),
                Node::new(4, NodeKind::Color, [0.; 2]),
                Node::new(5, NodeKind::Time, [0.; 2]),
            ],
            vec![wire(2, 0, 3, 1), wire(4, 0, 3, 0), wire(3, 0, 1, 0)],
        );
        graph.keywords.insert("DETAIL".into(), false);
        graph.nodes[2].keyword = "DETAIL".into();
        let off = graph.surface_function().unwrap();
        let on = graph
            .surface_function_variant(&BTreeMap::from([("DETAIL".into(), true)]))
            .unwrap();
        assert!(!off.contains("textureSample("));
        assert!(!off.contains("let v2") && off.contains("let v4"));
        assert!(on.contains("textureSample(") && !on.contains("let v4"));
        assert!(!off.contains("let v5") && !on.contains("let v5"));
        assert_eq!(
            graph,
            ShaderGraph::from_json(&graph.to_json().unwrap()).unwrap()
        );
        assert!(
            graph
                .surface_function_variant(&BTreeMap::from([("MISSING".into(), true)]))
                .is_err()
        );
        // Invalid inactive branches still fail authoring validation.
        graph.nodes[1].inputs[0] = Value::Vector([f32::NAN; 3]);
        assert!(graph.surface_function().is_err());
    }
    #[test]
    fn keyword_names_and_variant_count_are_bounded() {
        let mut graph = ShaderGraph::default();
        for index in 0..8 {
            graph.keywords.insert(format!("FEATURE_{index}"), true);
        }
        assert_eq!(graph.keyword_mask(&BTreeMap::new()).unwrap(), 255);
        graph.keywords.insert("NINTH".into(), false);
        assert!(graph.validate().is_err());
        for bad in [
            "",
            "9X",
            "a b",
            "x;return",
            "é",
            "abcdefghijklmnopqrstuvwxyz0123456789",
        ] {
            graph.keywords = BTreeMap::from([(bad.into(), false)]);
            assert!(graph.validate().is_err(), "{bad}");
        }
    }
    fn graph(nodes: Vec<Node>, wires: Vec<Wire>) -> ShaderGraph {
        ShaderGraph {
            version: 1,
            name: "Test".into(),
            nodes,
            wires,
            keywords: BTreeMap::new(),
        }
    }
    fn wire(from: u32, from_port: usize, to: u32, to_port: usize) -> Wire {
        Wire {
            from: Socket {
                node: from,
                port: from_port,
            },
            to: Socket {
                node: to,
                port: to_port,
            },
        }
    }
    #[test]
    fn defaults_roundtrip_and_require_master() {
        let default = ShaderGraph::default();
        assert_eq!(
            ShaderGraph::from_json(&default.to_json().unwrap()).unwrap(),
            default
        );
        let masterless = graph(vec![Node::new(1, NodeKind::Time, [0., 0.])], vec![]);
        assert!(masterless.validate().is_err());
        let two = graph(
            vec![
                Node::new(1, NodeKind::Master, [0., 0.]),
                Node::new(2, NodeKind::Master, [0., 100.]),
            ],
            vec![],
        );
        assert!(two.validate().is_err());
    }
    #[test]
    fn master_channels_write_existing_surface_params_fields() {
        // Every connected Master channel must assign a real SurfaceParams field;
        // a typo here only fails at wgpu module creation in the editor.
        for (port, kind, field) in [
            (0, NodeKind::Vector, "base"),
            (1, NodeKind::Float, "metallic"),
            (2, NodeKind::Float, "roughness"),
            (3, NodeKind::Vector, "emissive"),
            (4, NodeKind::Float, "alpha"),
            (5, NodeKind::Vector, "normal"),
        ] {
            let g = graph(
                vec![
                    Node::new(1, NodeKind::Master, [0., 0.]),
                    Node::new(2, kind, [200., 0.]),
                ],
                vec![wire(2, 0, 1, port)],
            );
            assert!(
                g.surface_function()
                    .unwrap()
                    .contains(&format!("params.{field} ="))
            );
        }
    }
    #[test]
    fn rejects_cycles_type_mismatches_and_double_wires() {
        let cycle = graph(
            vec![
                Node::new(1, NodeKind::Master, [0., 0.]),
                Node::new(2, NodeKind::Add, [100., 0.]),
            ],
            vec![wire(1, 1, 2, 0), wire(2, 0, 1, 1)],
        );
        assert!(cycle.validate().is_err());
        let mismatch = graph(
            vec![
                Node::new(1, NodeKind::Master, [0., 0.]),
                Node::new(2, NodeKind::Time, [100., 0.]),
            ],
            vec![wire(2, 0, 1, 0)], // Float into Base Color (Vector)
        );
        assert!(mismatch.validate().is_err());
        let mut duplicate = graph(
            vec![
                Node::new(1, NodeKind::Master, [0., 0.]),
                Node::new(2, NodeKind::Time, [100., 0.]),
                Node::new(3, NodeKind::Time, [100., 100.]),
            ],
            vec![wire(2, 0, 1, 1), wire(3, 0, 1, 1)],
        );
        assert!(duplicate.validate().is_err());
        duplicate.wires.pop();
        assert!(duplicate.validate().is_ok());
    }
    #[test]
    fn codegen_overrides_only_connected_channels() {
        let mut g = graph(
            vec![
                Node::new(1, NodeKind::Master, [400., 0.]),
                Node::new(2, NodeKind::Color, [40., 40.]),
                Node::new(3, NodeKind::Time, [40., 200.]),
                Node::new(4, NodeKind::Multiply, [220., 160.]),
            ],
            vec![wire(2, 0, 1, 0), wire(3, 0, 4, 0), wire(4, 0, 1, 1)],
        );
        g.nodes[3].inputs[1] = Value::Float(0.5);
        let code = g.surface_function().unwrap();
        assert!(code.starts_with("fn graph_material_surface("));
        assert!(code.contains("var params = default_material_surface("));
        assert!(code.contains("let v2 = vec3<f32>(1.0, 1.0, 1.0);"));
        assert!(code.contains("let v3 = time;"));
        assert!(code.contains("let v4 = (v3 * 0.5);"));
        assert!(code.contains("params.base = v2;"));
        assert!(code.contains("params.metallic = clamp(v4, 0.0, 1.0);"));
        // Unconnected channels keep stock behavior and constants format as WGSL floats.
        assert!(!code.contains("params.roughness"));
        assert!(!code.contains(" = 0.5;"));
        assert!(code.ends_with("    return params;\n}\n"));
        let roundtrip = ShaderGraph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(roundtrip.surface_function().unwrap(), code);
    }
    #[test]
    fn texture_split_and_reject_invalid_constants() {
        let mut g = graph(
            vec![
                Node::new(1, NodeKind::Master, [500., 0.]),
                Node::new(2, NodeKind::TextureSample, [40., 40.]),
                Node::new(3, NodeKind::Split, [260., 40.]),
            ],
            vec![wire(2, 0, 1, 0), wire(2, 1, 1, 4)],
        );
        g.nodes[1].slot = TextureSlot::MetallicRoughness;
        g.nodes[2].inputs[0] = Value::Vector([0., 0., 0.]);
        let code = g.surface_function().unwrap();
        assert!(code.contains("textureSample(mr_texture, mr_sampler, uv)"));
        assert!(code.contains("params.base = v2.rgb;"));
        assert!(code.contains("params.alpha = clamp(v2.a, 0.0, 1.0);"));
        g.nodes[2].inputs[0] = Value::Vector([f32::NAN, 0., 0.]);
        assert!(g.surface_function().is_err());
        g.nodes[2].inputs[0] = Value::Vector([0.; 3]);
        assert!(g.surface_function().is_ok());
        // Div-by-zero is guarded in codegen.
        let mut divide = graph(
            vec![
                Node::new(1, NodeKind::Master, [400., 0.]),
                Node::new(2, NodeKind::Divide, [100., 0.]),
            ],
            vec![],
        );
        divide.nodes[1].inputs = vec![Value::Float(1.), Value::Float(0.)];
        divide.wires.push(wire(2, 0, 1, 1));
        assert!(divide.surface_function().unwrap().contains("sign(0.0)"));
    }
}
