//! Deterministic, bounded placement over the same collider geometry used by gameplay queries.
use super::*;
use bozzard_assets::job::{Job, Progress};
use glam::Quat;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FoliageSettings {
    pub center: [f32; 2],
    pub radius: f32,
    pub count: usize,
    pub seed: u64,
    pub scale: [f32; 2],
    pub spacing: f32,
    pub max_slope_degrees: f32,
    pub align_to_surface: bool,
}
impl Default for FoliageSettings {
    fn default() -> Self {
        Self {
            center: [0.; 2],
            radius: 10.,
            count: 100,
            seed: 1,
            scale: [0.8, 1.2],
            spacing: 0.5,
            max_slope_degrees: 45.,
            align_to_surface: true,
        }
    }
}
impl FoliageSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.center
                .iter()
                .all(|n| n.is_finite() && n.abs() <= 1_000_000.)
                && self.radius.is_finite()
                && (0.01..=10_000.).contains(&self.radius),
            "Invalid foliage area"
        );
        ensure!(
            (1..=2000).contains(&self.count),
            "Foliage count must be 1..2000"
        );
        ensure!(
            self.scale
                .iter()
                .all(|n| n.is_finite() && (0.01..=100.).contains(n))
                && self.scale[0] <= self.scale[1],
            "Invalid foliage scale range"
        );
        ensure!(
            self.spacing.is_finite()
                && (self.spacing == 0. || (0.01..=10_000.).contains(&self.spacing)),
            "Spacing must be zero or 0.01..10000"
        );
        ensure!(
            self.max_slope_degrees.is_finite() && (0.0..=89.).contains(&self.max_slope_degrees),
            "Slope limit must be 0..89 degrees"
        );
        Ok(())
    }
}

pub struct PreparedFoliage {
    path: PathBuf,
    revision: u64,
    asset_revision: u64,
    progress: Progress,
    scene: Scene,
    pub group: String,
    pub placed: usize,
    pub requested: usize,
}

impl Editor {
    /// Copy a complete scene subtree, including nested prefab links and internal references.
    /// The prototype's root origin is the planting point; original assets remain shared.
    pub fn scatter_foliage_job(
        &self,
        prototype: &str,
        surface: &str,
        settings: FoliageSettings,
    ) -> Result<Job<PreparedFoliage>> {
        ensure!(self.play.is_none(), "Stop Play before scattering foliage");
        settings.validate()?;
        ensure!(
            prototype != surface,
            "Choose different foliage and ground objects"
        );
        let root_object = self
            .scene
            .objects
            .iter()
            .find(|o| o.id == prototype)
            .context("Foliage prototype is missing")?;
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for object in &self.scene.objects {
            if let Some(parent) = object.parent.as_deref() {
                children.entry(parent).or_default().push(&object.id);
            }
        }
        let mut members = BTreeSet::new();
        let mut pending = vec![prototype];
        while let Some(id) = pending.pop() {
            if members.insert(id.to_owned())
                && let Some(children) = children.get(id)
            {
                pending.extend(children);
            }
        }
        ensure!(
            !members.contains(surface),
            "Ground cannot belong to the foliage prototype"
        );
        ensure!(
            members.len() <= 128 && members.len() * settings.count <= 20_000,
            "Scatter at most 20000 objects per operation, with at most 128 objects per prototype"
        );
        ensure!(
            self.scene.objects.len() + members.len() * settings.count < 100_000,
            "Scatter exceeds the scene object limit"
        );
        let prototype: Vec<_> = self
            .scene
            .objects
            .iter()
            .filter(|o| members.contains(&o.id))
            .cloned()
            .collect();
        // A partial linked prefab would leave missing members after remapping.
        let links: BTreeMap<_, _> = self
            .scene
            .prefabs
            .iter()
            .filter(|(root, _)| members.contains(*root))
            .map(|(root, link)| (root.clone(), link.clone()))
            .collect();
        ensure!(
            links
                .values()
                .all(|link| link.members.values().all(|id| members.contains(id))),
            "Select the complete prefab hierarchy before scattering"
        );
        let matrix = {
            let demo = self.edit_demo()?;
            demo.instance().global_transforms(&demo.app.world)?[&root_object.id]
        };
        let (scale, rotation, _) = matrix.to_scale_rotation_translation();
        let reconstructed =
            Mat4::from_scale_rotation_translation(scale, rotation, matrix.w_axis.truncate());
        ensure!(
            matrix.is_finite() && matrix.abs_diff_eq(reconstructed, 1e-4),
            "Foliage prototype has a sheared world transform; use an unscaled parent"
        );
        let mut collision = self.collisions()?;
        collision.boxes.retain(|b| b.id == surface);
        collision.meshes.retain(|m| m.id == surface);
        collision.overlaps.clear();
        ensure!(
            !collision.boxes.is_empty() || !collision.meshes.is_empty(),
            "Ground needs an enabled Box or Mesh Collider"
        );
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for b in &collision.boxes {
            for p in &b.corners {
                min_y = min_y.min(p.y);
                max_y = max_y.max(p.y);
            }
        }
        for m in &collision.meshes {
            let corners = bozzard_scene::MeshCollider {
                enabled: true,
                layers: 1,
                mask: u32::MAX,
                mesh: m.mesh.clone(),
            }
            .geometry(m.matrix)?;
            for p in corners {
                min_y = min_y.min(p.y);
                max_y = max_y.max(p.y);
            }
        }
        ensure!(
            min_y.is_finite() && max_y.is_finite() && min_y.abs().max(max_y.abs()) <= 1_000_000.,
            "Ground is outside the foliage placement range"
        );
        let mut prepared = PreparedFoliage {
            path: self.path.clone(),
            revision: self.revision,
            asset_revision: self.asset_revision,
            progress: Default::default(),
            scene: self.scene.clone(),
            group: unique_id(&self.scene, "foliage"),
            placed: 0,
            requested: settings.count,
        };
        let root = root_object.id.clone();
        Job::start("Scattering foliage", move |progress| {
            prepared.progress = progress.clone();
            prepared.scene.objects.push(Object {
                id: prepared.group.clone(),
                name: "Foliage scatter".into(),
                ..Default::default()
            });
            let mut occupied: BTreeSet<String> = prepared
                .scene
                .objects
                .iter()
                .map(|o| o.id.clone())
                .collect();
            let mut next_id = 1_u64;
            let mut random = Random(settings.seed);
            let mut cells: BTreeMap<(i64, i64), Vec<Vec3>> = BTreeMap::new();
            let min_up = settings.max_slope_degrees.to_radians().cos();
            let attempts = (settings.count * 24).min(40_000);
            for attempt in 0..attempts {
                if prepared.placed == settings.count {
                    break;
                }
                if attempt % 64 == 0 {
                    progress.report(
                        attempt,
                        attempts,
                        format!("Placed {} of {}", prepared.placed, settings.count),
                    )?;
                }
                let angle = random.next() * std::f32::consts::TAU;
                let radius = random.next().sqrt() * settings.radius;
                let origin = Vec3::new(
                    settings.center[0] + angle.cos() * radius,
                    max_y + 1.,
                    settings.center[1] + angle.sin() * radius,
                );
                let Some(hit) = collision.raycast(origin, Vec3::NEG_Y, max_y - min_y + 2., None)?
                else {
                    continue;
                };
                if hit.normal.y < min_up {
                    continue;
                }
                let cell = if settings.spacing > 0. {
                    (
                        (hit.position.x / settings.spacing).floor() as i64,
                        (hit.position.z / settings.spacing).floor() as i64,
                    )
                } else {
                    (0, 0)
                };
                if settings.spacing > 0.
                    && (-1..=1).any(|x| {
                        (-1..=1).any(|z| {
                            cells.get(&(cell.0 + x, cell.1 + z)).is_some_and(|points| {
                                points.iter().any(|p| {
                                    let offset = *p - hit.position;
                                    offset.x * offset.x + offset.z * offset.z
                                        < settings.spacing * settings.spacing
                                })
                            })
                        })
                    })
                {
                    continue;
                }
                if settings.spacing > 0. {
                    cells.entry(cell).or_default().push(hit.position);
                }
                let multiplier =
                    settings.scale[0] + random.next() * (settings.scale[1] - settings.scale[0]);
                let yaw = Quat::from_rotation_y(random.next() * std::f32::consts::TAU);
                let alignment = if settings.align_to_surface {
                    Quat::from_rotation_arc(Vec3::Y, hit.normal)
                } else {
                    Quat::IDENTITY
                };
                let orientation = alignment * yaw * rotation;
                let (y, x, z) = orientation.to_euler(glam::EulerRot::YXZ);
                let mut mapping = BTreeMap::new();
                for object in &prototype {
                    let id = loop {
                        let id = format!("foliage-instance-{next_id}");
                        next_id += 1;
                        if occupied.insert(id.clone()) {
                            break id;
                        }
                    };
                    mapping.insert(object.id.clone(), id);
                }
                for original in &prototype {
                    let mut object = original.clone();
                    object.remap_ids(&mapping);
                    if original.id == root {
                        object.parent = Some(prepared.group.clone());
                        object.transform = Transform {
                            translation: hit.position.to_array(),
                            rotation_degrees: [x, y, z].map(f32::to_degrees),
                            scale: (scale * multiplier).to_array(),
                        };
                    }
                    prepared.scene.objects.push(object);
                }
                for (root, link) in &links {
                    let mut copy = link.clone();
                    for id in copy.members.values_mut() {
                        *id = mapping[id].clone();
                    }
                    for object in &mut copy.baseline {
                        object.remap_ids(&mapping);
                    }
                    prepared.scene.prefabs.insert(mapping[root].clone(), copy);
                }
                prepared.placed += 1;
            }
            ensure!(
                prepared.placed > 0,
                "No suitable ground within the area, slope and spacing limits"
            );
            progress.stage("Validating foliage placement")?;
            prepared.scene.validate()?;
            Ok(prepared)
        })
    }

    pub fn accept_foliage(&mut self, prepared: PreparedFoliage) -> Result<(String, usize, usize)> {
        prepared.progress.check()?;
        ensure!(
            self.play.is_none()
                && self.path == prepared.path
                && self.revision == prepared.revision
                && self.asset_revision == prepared.asset_revision,
            "Scene or assets changed while scattering; retry"
        );
        self.finish_gesture();
        self.apply("Scatter foliage", prepared.scene)?;
        self.select_object(Some(prepared.group.clone()));
        Ok((prepared.group, prepared.placed, prepared.requested))
    }
}

struct Random(u64);
impl Random {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^= x >> 31;
        (x >> 40) as f32 / 16_777_216.
    }
}
