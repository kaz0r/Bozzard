//! Versioned scene documents and ECS instances, with no graphics dependencies.
//! IDs are document-local persistent strings, never runtime entity handles.
pub mod game_flow;
pub use game_flow::{GameAction, GameFlowSettings, GamePhase, GameSession};
mod gi;
pub use gi::{BakedGi, GI_PROBE_STRIDE, GI_VISIBILITY_SIZE, GiSettings, GiVolumeSettings};
mod text;
pub use text::{ScreenText, TextAlignment, TextFont, TextRendering};
mod surface;
pub use surface::SurfaceMaterialOverride;
pub mod blueprint;
mod blueprint_runtime;
pub mod middleware;
mod runtime_prefabs;
pub mod scene_control;
pub mod shader_graph;
pub mod spatial;
pub use blueprint::{Blueprint, BlueprintAttachment};
pub use blueprint_runtime::{
    BlueprintDebugger, BlueprintHidden, BlueprintRuntime, Breakpoint, DebugCommand, DebugPause,
    NodeSnapshot, PinWatch, VariableWatch, WatchSnapshot,
};
pub mod script;
pub use bozzard_compute as compute;
mod compute_runtime;
pub use compute_runtime::{SceneCompute, load_compute_kernels};
pub use script::{MAX_SCRIPTS, ScriptAttachment, ScriptManager};
mod script_runtime;
pub use script_runtime::{ScriptRuntime, ScriptRuntimeStats, load_sources};
mod fog;
pub use fog::FogSettings;
mod environment;
pub use environment::EnvironmentSettings;
mod temporal;
pub use temporal::{MotionBlur, ScreenSpaceReflections, TemporalAntiAliasing};
mod particles;
pub use particles::{MAX_PARTICLES, Particle, ParticleEmitter, ParticleKind, ParticleSimulation};
mod volumetric;
pub use volumetric::VolumetricFog;
mod optics;
pub use optics::{AutoExposure, DepthOfField};
mod display;
pub use display::{
    AmbientOcclusion, BloomSettings, ColorGrading, DisplayPreset, DisplaySettings, FilmGrain,
    HeatDistortion, PostProcessVolume, ToneMapper, Vignette,
};
mod light;
pub use light::{
    Light, LightKind, MAX_LOCAL_LIGHTS, MAX_SHADOWED_POINT_LIGHTS, MAX_SHADOWED_SPOT_LIGHTS,
    WorldLight,
};
mod lighting;
pub use lighting::Lighting;

use anyhow::{Context, Result, ensure};
use bozzard_ecs::{Entity, World};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::result::Result as StdResult;

mod collision;
mod component;
pub use component::{
    AddContext, COMPONENTS, Component, ComponentType, Field, FieldKind, FieldValue, Ui, VectorRole,
    available_components, component_type, component_type_by_label, components,
    eligible_follow_camera, register_component,
};
mod gameplay;
pub mod keys;
mod prefab;
pub use prefab::{Prefab, PrefabInstance};
pub mod bvh;
mod gravity;
mod joint;
mod physics;
pub use collision::{
    BoxCollider, CollisionBox, CollisionMesh, CollisionSnapshot, Contact, DEFAULT_LAYERS,
    DEFAULT_MASK, LAYER_NAMES, MeshCollider, MoveResult, QueryHit, TriangleMesh, layers_interact,
};
pub use gameplay::{
    CursorCapture, GameplayInput, GameplayState, PlayerController, PlayerMotion, Trigger,
    TriggerAction,
};
pub use gravity::{Gravity, GravityState};
pub use joint::{Joint, JointKind};

/// Current scene schema. A component this build does not know is preserved rather than rejected,
/// so files stay version 1: their shape never changed.
pub const SCENE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub translation: [f32; 3],
    /// Euler angles in degrees, composed Y then X then Z (local-to-parent).
    pub rotation_degrees: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation_degrees: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        let [x, y, z] = self.rotation_degrees.map(f32::to_radians);
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            Quat::from_euler(EulerRot::YXZ, y, x, z),
            Vec3::from(self.translation),
        )
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.translation
                .iter()
                .chain(&self.rotation_degrees)
                .chain(&self.scale)
                .all(|v| v.is_finite()),
            "transform contains non-finite values"
        );
        ensure!(
            self.scale.iter().all(|s| s.abs() >= 0.0001),
            "scale is zero or too close to zero"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Layer {
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "3d")]
    ThreeD,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "projection", rename_all = "snake_case", deny_unknown_fields)]
pub enum Camera {
    Orthographic {
        vertical_size: f32,
        near: f32,
        far: f32,
    },
    Perspective {
        vertical_fov_degrees: f32,
        near: f32,
        far: f32,
    },
}

impl Camera {
    pub fn projection(&self, aspect: f32) -> Result<Mat4> {
        ensure!(
            aspect.is_finite() && aspect > 0.0,
            "camera aspect must be positive and finite"
        );
        self.validate()?;
        Ok(match *self {
            Self::Orthographic {
                vertical_size,
                near,
                far,
            } => {
                let h = vertical_size * 0.5;
                glam::camera::rh::proj::directx::orthographic(
                    -h * aspect,
                    h * aspect,
                    -h,
                    h,
                    near,
                    far,
                )
            }
            Self::Perspective {
                vertical_fov_degrees,
                near,
                far,
            } => glam::camera::rh::proj::directx::perspective(
                vertical_fov_degrees.to_radians(),
                aspect,
                near,
                far,
            ),
        })
    }
    fn validate(&self) -> Result<()> {
        let (near, far) = match *self {
            Self::Orthographic {
                vertical_size,
                near,
                far,
            } => {
                ensure!(
                    vertical_size.is_finite() && vertical_size > 0.0,
                    "orthographic size must be positive"
                );
                (near, far)
            }
            Self::Perspective {
                vertical_fov_degrees,
                near,
                far,
            } => {
                ensure!(
                    vertical_fov_degrees.is_finite()
                        && (1.0..179.0).contains(&vertical_fov_degrees),
                    "perspective FOV must be between 1 and 179 degrees"
                );
                (near, far)
            }
        };
        ensure!(
            near.is_finite() && far.is_finite() && near > 0.0 && far > near,
            "camera requires 0 < near < far"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mesh {
    Quad,
    Cube,
    Asset(String),
    /// One imported primitive, with its local origin at the source bounds center.
    Surface {
        asset: String,
        index: u32,
        source: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Texture {
    White,
    Checker,
    /// Unlit world-space normal visualization.
    Normals,
    /// UV checker shader; UV repeat controls the number of cells.
    ProceduralChecker,
    /// Three-band sun shading, multiplied by the drawable tint.
    Toon,
    Asset(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drawable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    /// Contribute to a static GI bake. Known moving objects/ancestors are excluded.
    #[serde(default = "default_true")]
    pub gi_static: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_overrides: Vec<SurfaceMaterialOverride>,
    pub layer: Layer,
    pub mesh: Mesh,
    pub texture: Texture,
    /// Linear RGB tint. This initial pass supports opaque materials only.
    pub color: [f32; 3],
    pub uv_scale: [f32; 2],
}

/// Optional per-object material; without it, meshes use their source appearance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    /// None inherits the mesh's source texture, including imported material maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Texture>,
    pub color: [f32; 3],
    pub uv_scale: [f32; 2],
}
impl Material {
    pub fn from_drawable(drawable: &Drawable) -> Self {
        Self {
            metallic: None,
            roughness: None,
            texture: None,
            color: drawable.color,
            uv_scale: drawable.uv_scale,
        }
    }
    pub fn apply(&self, drawable: &mut Drawable) {
        if let Some(texture) = &self.texture {
            drawable.texture = texture.clone();
        }
        drawable.metallic = self.metallic.or(drawable.metallic);
        drawable.roughness = self.roughness.or(drawable.roughness);
        drawable.color = self.color;
        drawable.uv_scale = self.uv_scale;
        if let Mesh::Surface { index, source, .. } = &drawable.mesh
            && (self.texture.is_some() || self.metallic.is_some() || self.roughness.is_some())
        {
            let index = *index;
            let source = source.clone();
            let position = drawable
                .material_overrides
                .iter()
                .position(|v| v.surface == index && v.source == source)
                .unwrap_or_else(|| {
                    drawable
                        .material_overrides
                        .push(SurfaceMaterialOverride::inherited(index, source));
                    drawable.material_overrides.len() - 1
                });
            let value = &mut drawable.material_overrides[position];
            if let Some(texture) = &self.texture {
                value.texture = Some(texture.clone());
            }
            if let Some(metallic) = self.metallic {
                value.metallic = Some(metallic);
            }
            if let Some(roughness) = self.roughness {
                value.roughness = Some(roughness);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spin(pub [f32; 3]);

/// An object is its identity (`id`, `name`, `parent`, `transform`) plus any number of components.
///
/// Components are siblings of the identity fields, and the [registry](component::components) owns
/// every one of them. A component this build does not recognize is kept in [`Object::extras`] and
/// written back unchanged, so a scene from a newer build survives a round trip through an older
/// one. A typo in a *known* component's fields still fails loudly, because each component struct
/// keeps `deny_unknown_fields`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(try_from = "ObjectWire")]
pub struct Object {
    pub blackboard: blueprint::Blackboard,
    pub particle_emitter: Option<ParticleEmitter>,
    pub text_rendering: Option<TextRendering>,
    pub material: Option<Material>,
    pub blueprints: Vec<BlueprintAttachment>,
    /// Embedded surface shader graph; applies to the drawable's whole material.
    pub shader_graph: Option<shader_graph::ShaderGraph>,
    pub light: Option<Light>,
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub transform: Transform,
    pub camera: Option<Camera>,
    pub drawable: Option<Drawable>,
    pub spin: Option<Spin>,
    pub collider: Option<BoxCollider>,
    pub mesh_collider: Option<MeshCollider>,
    pub gravity: Option<Gravity>,
    pub player_controller: Option<PlayerController>,
    pub trigger: Option<Trigger>,
    pub joint: Option<Joint>,
    pub script_manager: Option<ScriptManager>,
    /// Components this build does not recognize, kept verbatim. See [`Object::extra`].
    #[serde(skip)]
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// Wire form of an object: identity fields plus a flat map of components.
///
/// A flattened map cannot be combined with `deny_unknown_fields`, which is the point: an unknown
/// *component* is data to preserve, while an unknown *field* inside a known component is a
/// mistake. The component structs themselves stay strict.
#[derive(Deserialize)]
struct ObjectWire {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub transform: Transform,
    #[serde(flatten)]
    pub components: BTreeMap<String, serde_json::Value>,
}

impl Object {
    /// An unrecognized component's value, exactly as it was loaded.
    pub fn extra(&self, name: &str) -> Option<&serde_json::Value> {
        self.extras.get(name)
    }
    /// Store a component value verbatim. Registered components use this for storage when their
    /// types are not compiled into this crate.
    pub fn set_extra(&mut self, name: impl Into<String>, value: serde_json::Value) {
        self.extras.insert(name.into(), value);
    }
}

impl TryFrom<ObjectWire> for Object {
    type Error = anyhow::Error;
    fn try_from(wire: ObjectWire) -> Result<Self> {
        let mut object = Object {
            id: wire.id,
            name: wire.name,
            parent: wire.parent,
            transform: wire.transform,
            ..Default::default()
        };
        for (name, value) in wire.components {
            match component::component_type(&name) {
                // serde can only carry a Display string out of `try_from`, so the cause chain is
                // folded in here: a typo inside a component must name its component *and* field.
                Some(entry) => (entry.load)(&mut object, value)
                    .map_err(|error| anyhow::anyhow!("reading component '{name}': {error:#}"))?,
                None => {
                    object.extras.insert(name, value);
                }
            }
        }
        Ok(object)
    }
}

impl Serialize for Object {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        use serde::ser::{Error as _, SerializeMap as _};
        let mut components = serde_json::Map::new();
        for entry in component::components() {
            let saved = (entry.save)(self).map_err(|error| {
                S::Error::custom(format!("writing component '{}': {error:#}", entry.name))
            })?;
            if let Some(value) = saved {
                components.insert(entry.name.into(), value);
            }
        }
        components.extend(self.extras.clone());
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("name", &self.name)?;
        if let Some(parent) = &self.parent {
            map.serialize_entry("parent", parent)?;
        }
        map.serialize_entry("transform", &self.transform)?;
        for (name, value) in &components {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Embedded levels share this document's preloaded asset catalog. Nested libraries are forbidden.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_scenes: BTreeMap<String, std::sync::Arc<Scene>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blackboard: blueprint::Blackboard,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_flow: Option<GameFlowSettings>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prefabs: BTreeMap<String, PrefabInstance>,
    #[serde(default)]
    pub fog: FogSettings,
    #[serde(default)]
    pub gi: GiSettings,
    #[serde(default)]
    pub environment: EnvironmentSettings,
    #[serde(default)]
    pub display: DisplaySettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_process_volumes: Vec<PostProcessVolume>,
    #[serde(default)]
    pub lighting: Lighting,
    pub version: u32,
    pub name: String,
    /// Active camera object ID per view. Scenes may provide either or both views.
    pub views: BTreeMap<Layer, String>,
    pub objects: Vec<Object>,
    /// Stable asset IDs mapped to source paths relative to this scene file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, AssetSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    ComputeShader,
    Audio,
    Prefab,
    Image,
    Mesh,
    /// A TrueType/OpenType font file (`.ttf`, `.otf`).
    Font,
    /// A Rhai script file (`.rs` by project convention).
    Script,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSource {
    pub kind: AssetKind,
    pub path: String,
}

impl Scene {
    pub fn from_json(json: &str) -> Result<Self> {
        let scene: Self = serde_json::from_str(json).context("parsing scene JSON")?;
        scene.validate()?;
        Ok(scene)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
    pub fn global_transforms(&self) -> Result<BTreeMap<String, Mat4>> {
        let order = self.order()?;
        let mut result = BTreeMap::new();
        for index in order {
            let object = &self.objects[index];
            let parent = object
                .parent
                .as_ref()
                .map(|p| result[p])
                .unwrap_or(Mat4::IDENTITY);
            result.insert(object.id.clone(), parent * object.transform.matrix());
        }
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        self.order().map(|_| ())
    }

    /// Iterative topological sort: arbitrary document order, no recursive stack limit.
    fn order(&self) -> Result<Vec<usize>> {
        if let Some(flow) = &self.game_flow {
            flow.validate()?;
        }
        self.fog.validate()?;
        self.gi.validate()?;
        self.lighting.validate()?;
        self.display.validate()?;
        ensure!(
            self.post_process_volumes.len() <= 32,
            "at most 32 post-process volumes"
        );
        for volume in &self.post_process_volumes {
            volume.validate()?;
        }
        self.environment.validate()?;
        ensure!(
            self.version == SCENE_VERSION,
            "unsupported scene version {} (expected {SCENE_VERSION})",
            self.version
        );
        ensure!(
            self.objects.len() <= 100_000,
            "scene exceeds the initial object limit"
        );
        for (id, source) in &self.assets {
            ensure!(!id.trim().is_empty(), "asset ID is empty");
            ensure!(
                !source.path.is_empty()
                    && !source.path.contains('\\')
                    && !source.path.contains(':')
                    && !source.path.starts_with('/'),
                "asset '{id}' needs a relative path using forward slashes"
            );
        }
        ensure!(
            self.objects.iter().filter(|o| o.light.is_some()).count() <= MAX_LOCAL_LIGHTS,
            "scene supports at most {MAX_LOCAL_LIGHTS} local lights"
        );
        middleware::registry::validate_all(self)?;
        gameplay::validate(self)?;
        prefab::validate(self)?;
        for (kind, limit) in [
            (LightKind::Spot, MAX_SHADOWED_SPOT_LIGHTS),
            (LightKind::Point, MAX_SHADOWED_POINT_LIGHTS),
        ] {
            ensure!(
                self.objects
                    .iter()
                    .filter_map(|o| o.light)
                    .filter(|l| l.kind == kind && l.requests_shadow_map())
                    .count()
                    <= limit,
                "scene supports at most {limit} shadowed {kind:?} lights (including disabled lights)"
            );
        }
        scene_control::validate_library(self)?;
        blueprint::validate_blackboard(&self.blackboard)?;
        let mut ids = BTreeMap::new();
        for (index, object) in self.objects.iter().enumerate() {
            ensure!(!object.id.trim().is_empty(), "object ID is empty");
            ensure!(
                ids.insert(object.id.as_str(), index).is_none(),
                "duplicate object ID '{}'",
                object.id
            );
        }
        // Every Rigidbody root that owns at least one authored collider, including compound
        // children. Presence is enough here: a disabled collider is a paused body, not a typo.
        let mut bodies_with_shapes = BTreeSet::new();
        for object in &self.objects {
            if object.collider.is_none() && object.mesh_collider.is_none() {
                continue;
            }
            bodies_with_shapes.insert(body_root(self, &ids, &object.id).to_owned());
        }
        for object in &self.objects {
            ensure!(
                object.blueprints.len() <= 16,
                "at most 16 blueprints per object"
            );
            blueprint::validate_blackboard(&object.blackboard)?;
            for attachment in &object.blueprints {
                for node in attachment.graph.nodes.iter().filter(|n| n.uses_variable()) {
                    match node.scope {
                        blueprint::VariableScope::Graph => {}
                        blueprint::VariableScope::Object => attachment
                            .graph
                            .validate_variable(node, &object.blackboard)?,
                        blueprint::VariableScope::Scene => {
                            attachment.graph.validate_variable(node, &self.blackboard)?
                        }
                    }
                }
                attachment
                    .graph
                    .validate()
                    .with_context(|| format!("blueprint on '{}'", object.id))?;
            }
            if let Some(graph) = &object.shader_graph {
                graph
                    .validate()
                    .with_context(|| format!("shader graph on '{}'", object.id))?;
            }
            ensure!(!object.id.trim().is_empty(), "object ID is empty");
            object
                .transform
                .validate()
                .with_context(|| format!("object '{}'", object.id))?;
            if let Some(gravity) = object.gravity {
                gravity.validate()?;
                ensure!(
                    !gravity.enabled
                        || bodies_with_shapes.contains(body_root(self, &ids, &object.id)),
                    "Rigidbody needs a Box or Mesh Collider on '{}' or a colliding child",
                    object.id
                );
            }
            if let Some(collider) = object.collider {
                collider.validate()?;
            }
            if let Some(emitter) = object.particle_emitter {
                emitter.validate()?;
            }
            if object.mesh_collider.is_some() {
                ensure!(
                    object.collider.is_none()
                        && object.player_controller.is_none()
                        && object.trigger.is_none(),
                    "Mesh Collider cannot also have Box Collider, Player Controller or Trigger on '{}'",
                    object.id
                );
            }
            if let Some(text) = &object.text_rendering {
                text.validate()?;
            }
            if let Some(manager) = &object.script_manager {
                manager.validate()?;
            }
            if let Some(light) = object.light {
                light.validate()?;
            }
            if let Some(camera) = object.camera {
                camera.validate()?;
            }
            if let Some(joint) = &object.joint {
                joint.validate()?;
                ensure!(
                    !joint.other.is_empty() && ids.contains_key(joint.other.as_str()),
                    "Joint on '{}' needs an existing Other object",
                    object.id
                );
                let owner = body_root(self, &ids, &object.id);
                let other = body_root(self, &ids, &joint.other);
                ensure!(
                    owner != other,
                    "Joint on '{}' must connect two different bodies, not one body to itself",
                    object.id
                );
                ensure!(
                    bodies_with_shapes.contains(owner) && bodies_with_shapes.contains(other),
                    "Joint on '{}' needs colliders on both bodies",
                    object.id
                );
            }
            for (id, kind) in object.asset_dependencies() {
                ensure!(
                    self.assets.get(id).is_some_and(|a| a.kind == kind),
                    "missing or wrong-kind asset '{id}' on '{}'",
                    object.id
                );
            }
            if let Some(drawable) = &object.drawable {
                ensure!(
                    drawable
                        .metallic
                        .iter()
                        .chain(drawable.roughness.iter())
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                    "invalid mesh surface factors"
                );
            }
            if let Some(material) = &object.material {
                ensure!(
                    object.drawable.is_some(),
                    "Material needs a mesh on '{}'",
                    object.id
                );
                ensure!(
                    material
                        .color
                        .iter()
                        .chain(material.metallic.iter())
                        .chain(material.roughness.iter())
                        .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
                        && material.uv_scale.iter().all(|v| v.is_finite() && *v > 0.0),
                    "invalid Material on '{}'",
                    object.id
                );
            }
            if let Some(drawable) = &object.drawable {
                if let Mesh::Surface { index, source, .. } = &drawable.mesh {
                    ensure!(
                        *index < 4096
                            && source.len() == 16
                            && source
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                        "invalid surface binding"
                    );
                    ensure!(
                        drawable
                            .material_overrides
                            .iter()
                            .all(|v| v.surface == *index
                                && v.source == *source
                                && v.transform == Transform::default()),
                        "surface entities use their own Transform and material binding"
                    );
                }
                ensure!(
                    drawable.material_overrides.is_empty()
                        || matches!(drawable.mesh, Mesh::Asset(_) | Mesh::Surface { .. }),
                    "surface overrides need an imported model"
                );
                ensure!(
                    drawable.material_overrides.len() <= 4096,
                    "too many surface overrides"
                );
                let mut surfaces = std::collections::BTreeSet::new();
                for value in &drawable.material_overrides {
                    value.validate()?;
                    ensure!(surfaces.insert(value.surface), "duplicate surface override");
                }
                ensure!(
                    drawable
                        .color
                        .iter()
                        .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                    "invalid color on '{}'",
                    object.id
                );
                ensure!(
                    drawable.uv_scale.iter().all(|v| v.is_finite() && *v > 0.0),
                    "invalid UV scale on '{}'",
                    object.id
                );
            }
            if let Some(spin) = object.spin {
                ensure!(
                    spin.0.iter().all(|v| v.is_finite()),
                    "invalid spin on '{}'",
                    object.id
                );
            }
        }
        for object in &self.objects {
            for attachment in &object.blueprints {
                for target in attachment.graph.object_references() {
                    ensure!(
                        ids.contains_key(target),
                        "blueprint '{}' on '{}' references missing object '{}'; clear or reassign the reference first",
                        attachment.graph.name,
                        object.id,
                        target
                    );
                }
            }
        }
        for target in blueprint::board_references(&self.blackboard).chain(
            self.objects
                .iter()
                .flat_map(|o| blueprint::board_references(&o.blackboard)),
        ) {
            ensure!(
                ids.contains_key(target),
                "blackboard references missing object '{target}'"
            );
        }
        for camera in self.views.values() {
            let index = *ids
                .get(camera.as_str())
                .with_context(|| format!("missing view camera '{camera}'"))?;
            ensure!(
                self.objects[index].camera.is_some(),
                "view object '{camera}' has no camera component"
            );
        }
        let mut children = vec![Vec::new(); self.objects.len()];
        let mut roots = VecDeque::new();
        for (index, object) in self.objects.iter().enumerate() {
            if let Some(parent) = &object.parent {
                let parent = *ids
                    .get(parent.as_str())
                    .with_context(|| format!("missing parent '{parent}' on '{}'", object.id))?;
                children[parent].push(index);
            } else {
                roots.push_back(index);
            }
        }
        let mut order = Vec::with_capacity(self.objects.len());
        while let Some(index) = roots.pop_front() {
            order.push(index);
            roots.extend(children[index].iter().copied());
        }
        ensure!(
            order.len() == self.objects.len(),
            "scene transform hierarchy contains a cycle"
        );
        // Detect overflow/singular composition before spawning anything.
        let mut matrices = BTreeMap::new();
        for &index in &order {
            let object = &self.objects[index];
            let parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p.as_str()])
                .unwrap_or(Mat4::IDENTITY);
            let global = parent * object.transform.matrix();
            if object.collider.is_some_and(|c| c.enabled)
                || object.mesh_collider.as_ref().is_some_and(|c| c.enabled)
            {
                let mut ancestor = object.parent.as_deref();
                while let Some(id) = ancestor {
                    let body = &self.objects[ids[id]];
                    if body.player_controller.is_none() && body.gravity.is_some() {
                        // A compound shape rides the body pose, so a dynamic body needs the
                        // child's relative transform to be a rigid, uniformly scaled one.
                        if body.gravity.is_some_and(|g| g.enabled) {
                            physics::parent_pose(matrices[id].inverse() * global).with_context(
                                || format!("compound collider '{}' on Rigidbody '{id}'", object.id),
                            )?;
                        }
                        break;
                    }
                    ancestor = body.parent.as_deref();
                }
            }
            ensure!(
                global.is_finite() && global.inverse().is_finite(),
                "invalid composed transform on '{}'",
                object.id
            );
            if let Some(collider) = object.collider {
                collider.geometry(global)?;
            }
            if let Some(collider) = &object.mesh_collider {
                collider.geometry(global)?;
                if object.gravity.is_some_and(|g| g.enabled) {
                    collider.mesh.convex_hull()?;
                }
            }
            if object.gravity.is_some_and(|g| g.enabled) && object.player_controller.is_none() {
                physics::parent_pose(parent)
                    .with_context(|| format!("Rigidbody '{}'", object.id))?;
            }
            if let Some(trigger) = &object.trigger {
                trigger.volume.geometry(global)?;
            }
            if let Some(emitter) = object.particle_emitter {
                emitter.validate()?;
            }
            if let Some(light) = object.light {
                light.at(global)?;
            }
            matrices.insert(object.id.as_str(), global);
        }
        gameplay::validate_respawns(self, &matrices)?;
        Ok(order)
    }

    /// Validates the entire document before making any changes to the destination world.
    pub fn spawn(&self, world: &mut World) -> Result<SceneInstance> {
        let order = self.order()?;
        let mut entities = BTreeMap::new();
        for object in &self.objects {
            entities.insert(object.id.clone(), object.spawn_in(world)?);
        }
        let templates = self
            .prefabs
            .iter()
            .map(|(root, link)| {
                (
                    link.asset.clone(),
                    Prefab {
                        version: 1,
                        name: link.asset.clone(),
                        root: root.clone(),
                        objects: link.baseline.clone(),
                        assets: self
                            .assets
                            .iter()
                            .filter(|(_, a)| a.kind != AssetKind::Prefab)
                            .map(|(id, a)| (id.clone(), a.clone()))
                            .collect(),
                    },
                )
            })
            .collect();
        let instance = SceneInstance {
            document: self.clone(),
            entities,
            order,
            templates,
            next_spawn: 0,
            restart_document: std::sync::Arc::new(self.clone()),
            scene_serial: 0,
            hierarchy_objects: self
                .objects
                .iter()
                .flat_map(|o| o.parent.iter().flat_map(|p| [p.clone(), o.id.clone()]))
                .collect(),
            particle_state: Default::default(),
            display_time: 0.,
            display_overrides: Default::default(),
            script_engine: std::sync::OnceLock::new(),
            scripts: BTreeMap::new(),
            compute_kernels: BTreeMap::new(),
            compute_state: std::sync::OnceLock::new(),
            compute_capabilities: Default::default(),
        };
        instance.initialize_gameplay(world);
        Ok(instance)
    }
}

/// Runtime scene membership and live ECS components. Authored documents remain independent.
#[derive(Clone)]
pub struct SceneInstance {
    particle_state: particles::ParticleSystem,
    display_time: f32,
    display_overrides: display::DisplayOverrides,
    templates: BTreeMap<String, Prefab>,
    next_spawn: u64,
    restart_document: std::sync::Arc<Scene>,
    scene_serial: u64,
    hierarchy_objects: std::collections::BTreeSet<String>,
    document: Scene,
    entities: BTreeMap<String, Entity>,
    order: Vec<usize>,
    /// Built on the first script registration, so a scene without scripts never pays for it.
    script_engine: std::sync::OnceLock<std::sync::Arc<script_runtime::ScriptEngine>>,
    scripts: BTreeMap<String, std::sync::Arc<script_runtime::CompiledScript>>,
    compute_kernels: BTreeMap<String, std::sync::Arc<compute::Kernel>>,
    compute_state:
        std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<compute_runtime::SceneCompute>>>,
    compute_capabilities: compute::Capabilities,
}

impl SceneInstance {
    fn rebuild_hierarchy_index(&mut self) {
        self.hierarchy_objects = self
            .document
            .objects
            .iter()
            .flat_map(|o| o.parent.iter().flat_map(|p| [p.clone(), o.id.clone()]))
            .collect();
    }
    pub fn document(&self) -> &Scene {
        &self.document
    }
    pub fn entity(&self, id: &str) -> Option<Entity> {
        self.entities.get(id).copied()
    }
    pub fn camera_entity(&self, layer: Layer) -> Result<Entity> {
        let id = self
            .document
            .views
            .get(&layer)
            .context("scene does not provide this view")?;
        Ok(self.entities[id])
    }
    pub fn has_view(&self, layer: Layer) -> bool {
        self.document.views.contains_key(&layer)
    }

    pub fn global_transforms(&self, world: &World) -> Result<BTreeMap<String, Mat4>> {
        let mut matrices = BTreeMap::new();
        for &index in &self.order {
            let object = &self.document.objects[index];
            let local = world
                .get::<Transform>(self.entities[&object.id])
                .context("scene object/transform was removed")?;
            local.validate()?;
            let parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p])
                .unwrap_or(Mat4::IDENTITY);
            let global = parent * local.matrix();
            ensure!(
                global.is_finite() && global.inverse().is_finite(),
                "invalid runtime transform on '{}'",
                object.id
            );
            matrices.insert(object.id.clone(), global);
        }
        Ok(matrices)
    }

    /// An isolated root cannot affect any other object's composed transform.
    fn validate_transform_change(&self, world: &World, id: &str) -> Result<()> {
        if self.hierarchy_objects.contains(id) {
            // ponytail: full checks for hierarchy edits; validate dirty subtrees if these become hot.
            self.global_transforms(world)?;
        } else {
            let local = world
                .get::<Transform>(self.entities[id])
                .context("scene object/transform was removed")?;
            local.validate()?;
            let matrix = local.matrix();
            ensure!(
                matrix.is_finite() && matrix.inverse().is_finite(),
                "invalid runtime transform on '{id}'"
            );
        }
        Ok(())
    }

    pub fn view(&self, world: &World, layer: Layer, aspect: f32) -> Result<SceneView> {
        let matrices = self.global_transforms(world)?;
        let camera_id = world
            .resource::<middleware::timeline::Runtime>()
            .and_then(|r| r.cameras.get(&layer))
            .filter(|id| {
                self.entities
                    .get(*id)
                    .is_some_and(|e| world.get::<Camera>(*e).is_some())
            })
            .or_else(|| self.document.views.get(&layer))
            .context("scene does not provide this view")?;
        let camera = *self
            .entities
            .get(camera_id)
            .context("view camera was removed")?;
        let projection = world
            .get::<Camera>(camera)
            .context("view camera component was removed")?
            .projection(aspect)?;
        let view_projection = projection * matrices[camera_id].inverse();
        let mut objects = Vec::new();
        let mut object_ids = Vec::new();
        let mut compute_textures = BTreeMap::new();
        let compute_state = self.compute_if_initialized();
        let mut skin_poses = BTreeMap::new();
        let mut shader_graphs = Vec::new();
        let mut texts = Vec::new();
        for (id, entity) in &self.entities {
            if let Some(text) = world.get::<TextRendering>(*entity)
                && text.enabled
                && text.layer == layer
                && !world.get::<BlueprintHidden>(*entity).is_some_and(|h| h.0)
                && !world
                    .resource::<GameplayState>()
                    .is_some_and(|s| s.collected.contains(id))
            {
                text.validate()?;
                texts.push((matrices[id], text.clone()));
            }
            if let Some(drawable) = world.get::<Drawable>(*entity)
                && drawable.layer == layer
                && !world.get::<BlueprintHidden>(*entity).is_some_and(|h| h.0)
                && !world
                    .resource::<GameplayState>()
                    .is_some_and(|s| s.collected.contains(id))
            {
                let mut drawable = drawable.clone();
                if let Some(material) = world.get::<Material>(*entity) {
                    material.apply(&mut drawable);
                }
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                entity.hash(&mut hasher);
                let motion_id = hasher.finish().max(1);
                object_ids.push(motion_id);
                if let Some(handle) = compute_state
                    .as_ref()
                    .and_then(|state| state.material_texture(id))
                {
                    compute_textures.insert(motion_id, handle);
                }
                if let Some(animator) = world.get::<middleware::animation::Animator>(*entity)
                    && !animator.rig.bindings.is_empty()
                {
                    let (signature, matrices) = match world
                        .resource::<middleware::animation::Runtime>()
                        .and_then(|r| r.players.get(id))
                        .filter(|p| !p.palette.is_empty())
                    {
                        Some(player) => (player.signature, player.palette.clone()),
                        None => (
                            animator.rig.signature(),
                            std::sync::Arc::new(animator.rig.palette(&animator.rig.rest_pose())?),
                        ),
                    };
                    skin_poses.insert(
                        motion_id,
                        middleware::animation::Palette {
                            signature,
                            matrices,
                        },
                    );
                }
                shader_graphs.push(
                    world
                        .get::<shader_graph::ShaderGraph>(*entity)
                        .map(|g| std::sync::Arc::new(g.clone())),
                );
                objects.push((matrices[id], drawable));
            }
        }
        let mut lights = Vec::new();
        let mut shadowed_spots = 0;
        let mut shadowed_points = 0;
        if layer == Layer::ThreeD {
            for (id, entity) in &self.entities {
                if let Some(light) = world.get::<Light>(*entity) {
                    light.validate()?;
                    if light.requests_shadow_map() {
                        match light.kind {
                            LightKind::Spot => shadowed_spots += 1,
                            LightKind::Point => shadowed_points += 1,
                            LightKind::Directional => {}
                        }
                    }
                    if light.enabled {
                        lights.push(light.at(matrices[id])?);
                    }
                }
            }
        }
        ensure!(lights.len() <= MAX_LOCAL_LIGHTS, "too many runtime lights");
        ensure!(
            shadowed_spots <= MAX_SHADOWED_SPOT_LIGHTS,
            "too many runtime shadowed spotlights"
        );
        ensure!(
            shadowed_points <= MAX_SHADOWED_POINT_LIGHTS,
            "too many runtime shadowed point lights"
        );
        Ok(SceneView {
            sprites: self.sprite_frame_with_matrices(world, layer, &matrices)?,
            skin_poses,
            particles: if layer == Layer::ThreeD {
                self.particle_state.frame()
            } else {
                Vec::new()
            },
            fog: self.document.fog,
            lights,
            environment: self.document.environment,
            display: self.display_at(
                matrices[camera_id].transform_point3(glam::Vec3::ZERO),
                layer,
            ),
            display_time: self.display_time,
            lighting: self.document.lighting,
            view_projection,
            objects,
            object_ids,
            compute_textures,
            shader_graphs,
            texts,
        })
    }

    /// Captures this instance's known components, preserving persistent IDs and parents.
    /// Runtime entities added outside the instance are intentionally not serialized.
    pub fn capture(&self, world: &World) -> Result<Scene> {
        let mut scene = self.document.clone();
        for object in &mut scene.objects {
            let entity = self.entities[&object.id];
            middleware::registry::capture_all(object, world, entity)?;
            object.transform = *world
                .get::<Transform>(entity)
                .context("cannot save a removed scene object/transform")?;
            object.particle_emitter = world.get::<ParticleEmitter>(entity).copied();
            object.text_rendering = world.get::<TextRendering>(entity).cloned();
            object.material = world.get::<Material>(entity).cloned();
            object.light = world.get::<Light>(entity).copied();
            object.camera = world.get::<Camera>(entity).copied();
            object.drawable = world.get::<Drawable>(entity).cloned();
            object.spin = world.get::<Spin>(entity).copied();
            object.collider = world.get::<BoxCollider>(entity).copied();
            object.mesh_collider = world.get::<MeshCollider>(entity).cloned();
            object.gravity = world.get::<Gravity>(entity).copied();
            object.player_controller = world.get::<PlayerController>(entity).cloned();
            object.trigger = world.get::<Trigger>(entity).cloned();
            object.joint = world.get::<Joint>(entity).cloned();
            object.shader_graph = world.get::<shader_graph::ShaderGraph>(entity).cloned();
        }
        scene.validate()?;
        Ok(scene)
    }
}

pub struct SceneView {
    pub sprites: Vec<middleware::sprite::Visual>,
    pub skin_poses: BTreeMap<u64, middleware::animation::Palette>,
    /// Runtime identities in the same order as objects; never serialized.
    pub object_ids: Vec<u64>,
    /// GPU-free generated texture identities, separate from imported image asset IDs.
    pub compute_textures: BTreeMap<u64, compute::Handle>,
    /// Surface shader graph per object, same order as `objects`.
    pub shader_graphs: Vec<Option<std::sync::Arc<shader_graph::ShaderGraph>>>,
    pub particles: Vec<Particle>,
    pub display_time: f32,
    pub texts: Vec<(Mat4, TextRendering)>,
    pub fog: FogSettings,
    pub lights: Vec<WorldLight>,
    pub environment: EnvironmentSettings,
    pub display: DisplaySettings,
    pub lighting: Lighting,
    pub view_projection: Mat4,
    pub objects: Vec<(Mat4, Drawable)>,
}

impl Object {
    fn spawn_in(&self, world: &mut World) -> Result<Entity> {
        let entity = world.spawn();
        world.insert(entity, self.transform)?;
        middleware::registry::spawn_all(self, world, entity)?;
        macro_rules! insert { ($($field:ident),*) => { $(if let Some(value) = &self.$field { world.insert(entity, value.clone())?; })* }; }
        insert!(
            particle_emitter,
            text_rendering,
            material,
            light,
            camera,
            drawable,
            gravity,
            collider,
            mesh_collider,
            player_controller,
            trigger,
            joint,
            spin,
            shader_graph
        );
        if self.gravity.is_some() {
            world.insert(entity, GravityState::default())?;
        }
        Ok(entity)
    }
    pub fn asset_dependencies(&self) -> Vec<(&str, AssetKind)> {
        let mut dependencies = self
            .drawable
            .as_ref()
            .map(Drawable::asset_dependencies)
            .unwrap_or_default();
        if let Some(Material {
            texture: Some(Texture::Asset(id)),
            ..
        }) = &self.material
        {
            dependencies.push((id, AssetKind::Image));
        }
        for node in self.blueprints.iter().flat_map(|b| &b.graph.nodes) {
            if node.kind == blueprint::NodeKind::SpawnPrefab && !node.prefab.is_empty() {
                dependencies.push((&node.prefab, AssetKind::Prefab));
            }
        }
        if let Some(manager) = &self.script_manager {
            dependencies.extend(manager.asset_dependencies());
        }
        if let Some(TextRendering {
            font: TextFont::Custom(id),
            ..
        }) = &self.text_rendering
        {
            dependencies.push((id, AssetKind::Font));
        }
        dependencies.extend(middleware::registry::dependencies(self));
        dependencies
    }
    pub fn remap_assets(&mut self, mapping: &BTreeMap<String, String>) {
        middleware::registry::remap_assets(self, mapping);
        let remap = |id: &mut String| {
            if let Some(new) = mapping.get(id) {
                *id = new.clone();
            }
        };
        if let Some(TextRendering {
            font: TextFont::Custom(id),
            ..
        }) = &mut self.text_rendering
        {
            remap(id);
        }
        if let Some(material) = &mut self.material
            && let Some(Texture::Asset(id)) = &mut material.texture
        {
            remap(id);
        }
        if let Some(drawable) = &mut self.drawable {
            if let Mesh::Asset(id) | Mesh::Surface { asset: id, .. } = &mut drawable.mesh {
                remap(id);
            }
            if let Texture::Asset(id) = &mut drawable.texture {
                remap(id);
            }
            for surface in &mut drawable.material_overrides {
                if let Some(Texture::Asset(id)) = &mut surface.texture {
                    remap(id);
                }
            }
        }
        for node in self.blueprints.iter_mut().flat_map(|b| &mut b.graph.nodes) {
            if node.kind == blueprint::NodeKind::SpawnPrefab {
                remap(&mut node.prefab);
            }
        }
        if let Some(manager) = &mut self.script_manager {
            for attachment in &mut manager.scripts {
                remap(&mut attachment.script);
            }
        }
    }
    pub fn effective_drawable(&self) -> Option<Drawable> {
        let mut drawable = self.drawable.clone()?;
        if let Some(material) = &self.material {
            material.apply(&mut drawable);
        }
        Some(drawable)
    }
}

impl Scene {
    /// Whether any object carries a script, which makes the scene a gameplay scene.
    pub fn has_scripts(&self) -> bool {
        self.objects.iter().any(|object| {
            object
                .script_manager
                .as_ref()
                .is_some_and(|manager| !manager.scripts.is_empty())
        })
    }
    /// Spawnable prefab asset IDs a runtime has to have ready before the first tick: every enabled
    /// `Spawn Prefab` node, plus every prefab in the catalog of a scene that runs scripts.
    ///
    /// A script names its prefab in source, which is opaque here, so the catalog is the
    /// declaration: a prefab a script can spawn has to be in `assets`, and the runtime loads the
    /// lot once instead of guessing from the text.
    pub fn spawn_asset_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self
            .objects
            .iter()
            .flat_map(|object| &object.blueprints)
            .filter(|attachment| attachment.enabled)
            .flat_map(|attachment| &attachment.graph.nodes)
            .filter(|node| node.kind == blueprint::NodeKind::SpawnPrefab && !node.prefab.is_empty())
            .map(|node| node.prefab.clone())
            .collect();
        if self.has_scripts() {
            ids.extend(
                self.assets
                    .iter()
                    .filter(|(_, source)| source.kind == AssetKind::Prefab)
                    .map(|(id, _)| id.clone()),
            );
        }
        ids
    }
}

impl Drawable {
    pub fn asset_dependencies(&self) -> Vec<(&str, AssetKind)> {
        let mut result = Vec::new();
        if let Mesh::Asset(id) | Mesh::Surface { asset: id, .. } = &self.mesh {
            result.push((id.as_str(), AssetKind::Mesh));
        }
        if let Texture::Asset(id) = &self.texture {
            result.push((id.as_str(), AssetKind::Image));
        }
        for value in &self.material_overrides {
            if let Some(Texture::Asset(id)) = &value.texture {
                result.push((id.as_str(), AssetKind::Image));
            }
        }
        result
    }
}

impl Scene {
    /// Reverse dependencies used by reload diagnostics and future editor tooling.
    pub fn asset_users(&self) -> BTreeMap<String, Vec<String>> {
        let mut users: BTreeMap<String, Vec<String>> = self
            .assets
            .keys()
            .map(|id| (id.clone(), Vec::new()))
            .collect();
        for object in &self.objects {
            for (id, _) in object.asset_dependencies() {
                let objects = users.entry(id.into()).or_default();
                if objects.last() != Some(&object.id) {
                    objects.push(object.id.clone());
                }
            }
        }
        for (root, link) in &self.prefabs {
            users
                .entry(link.asset.clone())
                .or_default()
                .push(root.clone());
            for object in &link.baseline {
                for (id, _) in object.asset_dependencies() {
                    let users = users.entry(id.into()).or_default();
                    if !users.contains(root) {
                        users.push(root.clone());
                    }
                }
            }
        }
        users
    }
}

fn default_true() -> bool {
    true
}

fn default_layers() -> u32 {
    DEFAULT_LAYERS
}

fn default_mask() -> u32 {
    DEFAULT_MASK
}

/// The Rigidbody that owns an object's collider: itself or its nearest ancestor with a `Gravity`
/// component that is not a Player Controller. Without one, the object is its own body.
fn body_root<'a>(scene: &'a Scene, ids: &BTreeMap<&'a str, usize>, id: &'a str) -> &'a str {
    let mut root = id;
    let mut current = Some(id);
    // Bounded walk: a cyclic or dangling hierarchy fails elsewhere instead of looping here.
    for _ in 0..=scene.objects.len() {
        let Some(c) = current else { break };
        let Some(&index) = ids.get(c) else { break };
        let object = &scene.objects[index];
        root = &object.id;
        if object.player_controller.is_none() && object.gravity.is_some() {
            return root;
        }
        current = object.parent.as_deref();
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    fn object(id: &str) -> Object {
        Object {
            blackboard: Default::default(),
            particle_emitter: None,
            material: None,
            blueprints: Vec::new(),
            script_manager: None,
            shader_graph: None,
            light: None,
            id: id.into(),
            name: id.into(),
            parent: None,
            transform: Transform::default(),
            camera: None,
            drawable: None,
            spin: None,
            collider: None,
            mesh_collider: None,
            text_rendering: None,
            gravity: None,
            player_controller: None,
            trigger: None,
            joint: None,
            extras: BTreeMap::new(),
        }
    }
    fn scene() -> Scene {
        Scene {
            blackboard: Default::default(),
            runtime_scenes: Default::default(),
            game_flow: None,
            fog: Default::default(),
            gi: Default::default(),
            environment: EnvironmentSettings::default(),
            display: DisplaySettings::default(),
            post_process_volumes: Vec::new(),
            lighting: Lighting::default(),
            version: SCENE_VERSION,
            name: "test".into(),
            views: BTreeMap::new(),
            objects: vec![object("child"), object("parent")],
            assets: BTreeMap::new(),
            prefabs: BTreeMap::new(),
        }
    }

    #[test]
    fn hierarchy_and_runtime_roundtrip_keep_ids_across_worlds() {
        let mut scene = scene();
        scene.objects[0].parent = Some("parent".into());
        scene.objects[0].transform.translation = [1.0, 0.0, 0.0];
        scene.objects[1].transform.translation = [3.0, 0.0, 0.0];
        scene.objects[1].transform.scale = [2.0; 3];
        let mut a = World::new();
        let instance = scene.spawn(&mut a).unwrap();
        assert_eq!(
            instance.global_transforms(&a).unwrap()["child"].transform_point3(Vec3::ZERO),
            Vec3::new(5.0, 0.0, 0.0)
        );
        a.get_mut::<Transform>(instance.entity("child").unwrap())
            .unwrap()
            .translation[1] = 4.0;
        let saved = instance.capture(&a).unwrap();
        let loaded = Scene::from_json(&saved.to_json().unwrap()).unwrap();
        assert_eq!(saved, loaded);
        let mut b = World::new();
        let second = loaded.spawn(&mut b).unwrap();
        assert_ne!(instance.entity("child"), second.entity("child"));
        assert_eq!(
            instance.global_transforms(&a).unwrap(),
            second.global_transforms(&b).unwrap()
        );
    }

    #[test]
    fn malformed_scenes_do_not_partially_spawn() {
        for kind in 0..7 {
            let mut scene = scene();
            match kind {
                0 => scene.version = 999,
                1 => scene.objects[1].id = "child".into(),
                2 => scene.objects[0].parent = Some("missing".into()),
                3 => {
                    scene.objects[0].parent = Some("parent".into());
                    scene.objects[1].parent = Some("child".into());
                }
                4 => scene.objects[0].transform.scale[0] = 0.0,
                5 => scene.objects[0].transform.translation[0] = f32::NAN,
                _ => {
                    scene.views.insert(Layer::TwoD, "child".into());
                }
            }
            let mut world = World::new();
            assert!(scene.spawn(&mut world).is_err(), "case {kind}");
            assert!(world.is_empty());
        }
        assert!(
            Scene::from_json(r#"{"version":1,"name":"x","views":{},"objects":[],"typo":1}"#)
                .is_err()
        );
    }

    #[test]
    fn cameras_use_webgpu_depth_and_preserve_aspect() {
        for camera in [
            Camera::Perspective {
                vertical_fov_degrees: 60.0,
                near: 0.1,
                far: 100.0,
            },
            Camera::Orthographic {
                vertical_size: 4.0,
                near: 0.1,
                far: 100.0,
            },
        ] {
            let m = camera.projection(2.0).unwrap();
            assert!(m.project_point3(Vec3::new(0.0, 0.0, -0.1)).z.abs() < 1e-5);
            assert!((m.project_point3(Vec3::new(0.0, 0.0, -100.0)).z - 1.0).abs() < 1e-5);
            assert!((m.y_axis.y / m.x_axis.x - 2.0).abs() < 1e-5);
            assert!(camera.projection(0.0).is_err());
        }
        assert!(
            Camera::Perspective {
                vertical_fov_degrees: 180.0,
                near: 0.1,
                far: 100.0
            }
            .projection(1.0)
            .is_err()
        );
    }
}
