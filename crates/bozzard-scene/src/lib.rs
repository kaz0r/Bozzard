//! Versioned scene documents and ECS instances, with no graphics dependencies.
//! IDs are document-local persistent strings, never runtime entity handles.
use anyhow::{Context, Result, ensure};
use bozzard_ecs::{Entity, World};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

mod collision;
pub use collision::{BoxCollider, CollisionBox, CollisionSnapshot};

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Texture {
    White,
    Checker,
    Asset(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drawable {
    pub layer: Layer,
    pub mesh: Mesh,
    pub texture: Texture,
    /// Linear RGB tint. This initial pass supports opaque materials only.
    pub color: [f32; 3],
    pub uv_scale: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spin(pub [f32; 3]);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
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
    pub fn validate(&self) -> Result<()> {
        self.order().map(|_| ())
    }

    /// Iterative topological sort: arbitrary document order, no recursive stack limit.
    fn order(&self) -> Result<Vec<usize>> {
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
        let mut ids = BTreeMap::new();
        for (index, object) in self.objects.iter().enumerate() {
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
            if let Some(collider) = object.collider {
                collider.validate()?;
            }
            if let Some(camera) = object.camera {
                camera.validate()?;
            }
            if let Some(drawable) = &object.drawable {
                for (id, kind) in drawable.asset_dependencies() {
                    let source = self
                        .assets
                        .get(id)
                        .with_context(|| format!("missing asset '{id}' on '{}'", object.id))?;
                    ensure!(
                        source.kind == kind,
                        "wrong asset kind for '{id}' on '{}'",
                        object.id
                    );
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
            matrices.insert(object.id.as_str(), global);
        }
        Ok(order)
    }

    /// Validates the entire document before making any changes to the destination world.
    pub fn spawn(&self, world: &mut World) -> Result<SceneInstance> {
        let order = self.order()?;
        let mut entities = BTreeMap::new();
        for object in &self.objects {
            let entity = world.spawn();
            world.insert(entity, object.transform)?;
            if let Some(value) = object.camera {
                world.insert(entity, value)?;
            }
            if let Some(value) = &object.drawable {
                world.insert(entity, value.clone())?;
            }
            if let Some(value) = object.collider {
                world.insert(entity, value)?;
            }
            if let Some(value) = object.spin {
                world.insert(entity, value)?;
            }
            entities.insert(object.id.clone(), entity);
        }
        Ok(SceneInstance {
            document: self.clone(),
            entities,
            order,
        })
    }
}

/// Structural membership/parentage is fixed for this first scene instance API.
/// Transform and optional component values remain live ECS data.
pub struct SceneInstance {
    document: Scene,
    entities: BTreeMap<String, Entity>,
    order: Vec<usize>,
}

impl SceneInstance {
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
            {
                objects.push((matrices[id], drawable.clone()));
            }
        }
        Ok(SceneView {
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
            object.camera = world.get::<Camera>(entity).copied();
            object.drawable = world.get::<Drawable>(entity).cloned();
            object.spin = world.get::<Spin>(entity).copied();
            object.collider = world.get::<BoxCollider>(entity).copied();
        }
        scene.validate()?;
        Ok(scene)
    }
}

pub struct SceneView {
    pub view_projection: Mat4,
    pub objects: Vec<(Mat4, Drawable)>,
}

impl Drawable {
    pub fn asset_dependencies(&self) -> Vec<(&str, AssetKind)> {
        let mut result = Vec::new();
        if let Mesh::Asset(id) = &self.mesh {
            result.push((id.as_str(), AssetKind::Mesh));
        }
        if let Texture::Asset(id) = &self.texture {
            result.push((id.as_str(), AssetKind::Image));
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
            if let Some(drawable) = &object.drawable {
                for (id, _) in drawable.asset_dependencies() {
                    users.entry(id.into()).or_default().push(object.id.clone());
                }
            }
        }
        users
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn object(id: &str) -> Object {
        Object {
            id: id.into(),
            name: id.into(),
            parent: None,
            transform: Transform::default(),
            camera: None,
            drawable: None,
            spin: None,
            collider: None,
        }
    }
    fn scene() -> Scene {
        Scene {
            version: 1,
            name: "test".into(),
            views: BTreeMap::new(),
            objects: vec![object("child"), object("parent")],
            assets: BTreeMap::new(),
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
