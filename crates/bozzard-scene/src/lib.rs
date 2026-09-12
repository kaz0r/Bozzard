//! Versioned scene documents and ECS instances, with no graphics dependencies.
//! IDs are document-local persistent strings, never runtime entity handles.
mod gi;
pub use gi::{BakedGi, GI_PROBE_STRIDE, GI_VISIBILITY_SIZE, GiSettings, GiVolumeSettings};
mod surface;
pub use surface::SurfaceMaterialOverride;
pub mod blueprint;
mod blueprint_runtime;
mod runtime_prefabs;
pub use blueprint::{Blueprint, BlueprintAttachment};
pub use blueprint_runtime::{BlueprintHidden, BlueprintRuntime};
mod fog;
pub use fog::FogSettings;
mod environment;
pub use environment::EnvironmentSettings;
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
use std::collections::{BTreeMap, VecDeque};

mod collision;
mod gameplay;
mod prefab;
pub use prefab::{Prefab, PrefabInstance};
mod gravity;
pub use collision::{BoxCollider, CollisionBox, CollisionSnapshot, MoveResult};
pub use gameplay::{GameplayInput, GameplayState, PlayerController, Trigger, TriggerAction};
pub use gravity::{Gravity, GravityState};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blueprints: Vec<BlueprintAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub transform: Transform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<Camera>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawable: Option<Drawable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spin: Option<Spin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collider: Option<BoxCollider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<Gravity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_controller: Option<PlayerController>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
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
    Prefab,
    Image,
    Mesh,
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
        let mut ids = BTreeMap::new();
        for (index, object) in self.objects.iter().enumerate() {
            ensure!(
                object.blueprints.len() <= 16,
                "at most 16 blueprints per object"
            );
            for attachment in &object.blueprints {
                attachment
                    .graph
                    .validate()
                    .with_context(|| format!("blueprint on '{}'", object.id))?;
            }
            ensure!(!object.id.trim().is_empty(), "object ID is empty");
            ensure!(
                ids.insert(object.id.as_str(), index).is_none(),
                "duplicate object ID '{}'",
                object.id
            );
            object
                .transform
                .validate()
                .with_context(|| format!("object '{}'", object.id))?;
            if let Some(gravity) = object.gravity {
                gravity.validate()?;
                ensure!(
                    !gravity.enabled || object.collider.is_some(),
                    "gravity needs a box collider on '{}'",
                    object.id
                );
            }
            if let Some(collider) = object.collider {
                collider.validate()?;
            }
            if let Some(light) = object.light {
                light.validate()?;
            }
            if let Some(camera) = object.camera {
                camera.validate()?;
            }
            for (id, kind) in object.asset_dependencies() {
                ensure!(
                    self.assets.get(id).is_some_and(|a| a.kind == kind),
                    "missing or wrong-kind asset '{id}' on '{}'",
                    object.id
                );
            }
            if let Some(material) = &object.material {
                ensure!(
                    object.drawable.is_some(),
                    "Material needs a mesh on '{}'",
                    object.id
                );
                ensure!(
                    (material.metallic.is_none() && material.roughness.is_none())
                        || object
                            .drawable
                            .as_ref()
                            .is_some_and(|d| matches!(d.mesh, Mesh::Surface { .. })),
                    "PBR factor overrides need an imported surface entity"
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
            ensure!(
                global.is_finite() && global.inverse().is_finite(),
                "invalid composed transform on '{}'",
                object.id
            );
            if let Some(collider) = object.collider {
                collider.geometry(global)?;
            }
            if let Some(trigger) = &object.trigger {
                trigger.volume.geometry(global)?;
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
            display_time: 0.,
            display_overrides: Default::default(),
        };
        instance.initialize_gameplay(world);
        Ok(instance)
    }
}

/// Runtime scene membership and live ECS components. Authored documents remain independent.
#[derive(Clone)]
pub struct SceneInstance {
    display_time: f32,
    display_overrides: display::DisplayOverrides,
    templates: BTreeMap<String, Prefab>,
    next_spawn: u64,
    document: Scene,
    entities: BTreeMap<String, Entity>,
    order: Vec<usize>,
}

impl SceneInstance {
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
        if self
            .document
            .objects
            .iter()
            .any(|o| o.parent.as_deref() == Some(id) || (o.id == id && o.parent.is_some()))
        {
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
        let camera = self.camera_entity(layer)?;
        let projection = world
            .get::<Camera>(camera)
            .context("view camera component was removed")?
            .projection(aspect)?;
        let camera_id = &self.document.views[&layer];
        let view_projection = projection * matrices[camera_id].inverse();
        let mut objects = Vec::new();
        for (id, entity) in &self.entities {
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
        })
    }

    /// Captures this instance's known components, preserving persistent IDs and parents.
    /// Runtime entities added outside the instance are intentionally not serialized.
    pub fn capture(&self, world: &World) -> Result<Scene> {
        let mut scene = self.document.clone();
        for object in &mut scene.objects {
            let entity = self.entities[&object.id];
            object.transform = *world
                .get::<Transform>(entity)
                .context("cannot save a removed scene object/transform")?;
            object.material = world.get::<Material>(entity).cloned();
            object.light = world.get::<Light>(entity).copied();
            object.camera = world.get::<Camera>(entity).copied();
            object.drawable = world.get::<Drawable>(entity).cloned();
            object.spin = world.get::<Spin>(entity).copied();
            object.collider = world.get::<BoxCollider>(entity).copied();
            object.gravity = world.get::<Gravity>(entity).copied();
            object.player_controller = world.get::<PlayerController>(entity).cloned();
            object.trigger = world.get::<Trigger>(entity).cloned();
        }
        scene.validate()?;
        Ok(scene)
    }
}

pub struct SceneView {
    pub display_time: f32,
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
        macro_rules! insert { ($($field:ident),*) => { $(if let Some(value) = &self.$field { world.insert(entity, value.clone())?; })* }; }
        insert!(
            material,
            light,
            camera,
            drawable,
            gravity,
            collider,
            player_controller,
            trigger,
            spin
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
        dependencies
    }
    pub fn remap_assets(&mut self, mapping: &BTreeMap<String, String>) {
        let remap = |id: &mut String| {
            if let Some(new) = mapping.get(id) {
                *id = new.clone();
            }
        };
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
    }
    pub fn effective_drawable(&self) -> Option<Drawable> {
        let mut drawable = self.drawable.clone()?;
        if let Some(material) = &self.material {
            material.apply(&mut drawable);
        }
        Some(drawable)
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

#[cfg(test)]
mod tests {
    use super::*;
    fn object(id: &str) -> Object {
        Object {
            material: None,
            blueprints: Vec::new(),
            light: None,
            id: id.into(),
            name: id.into(),
            parent: None,
            transform: Transform::default(),
            camera: None,
            drawable: None,
            spin: None,
            collider: None,
            gravity: None,
            player_controller: None,
            trigger: None,
        }
    }
    fn scene() -> Scene {
        Scene {
            fog: Default::default(),
            gi: Default::default(),
            environment: EnvironmentSettings::default(),
            display: DisplaySettings::default(),
            post_process_volumes: Vec::new(),
            lighting: Lighting::default(),
            version: 1,
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
