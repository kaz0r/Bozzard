use super::{sampling, static_objects};
use crate::{
    AssetData, AssetStore, Filter, MeshData, MeshPart, Sampler, job::Progress, picking::MeshIndex,
};
use anyhow::{Context, Result, ensure};
use bozzard_scene::{Drawable, LightKind, Mesh, Scene, Texture, WorldLight};
use glam::{Mat4, Vec2, Vec3, Vec4};
use std::sync::Arc;

struct Geometry {
    data: Arc<AssetData>,
    index: Arc<MeshIndex>,
    blended: Vec<bool>,
}
impl Geometry {
    fn new(data: Arc<AssetData>, index: Arc<MeshIndex>) -> Self {
        let AssetData::Mesh(mesh) = data.as_ref() else {
            unreachable!()
        };
        let blended = mesh
            .parts
            .iter()
            .map(|p| {
                p.alpha_cutoff.is_none()
                    && (p.color[3] < 1.
                        || p.image
                            .as_ref()
                            .is_some_and(|i| i.rgba.chunks_exact(4).any(|p| p[3] < 255)))
            })
            .collect();
        Self {
            data,
            index,
            blended,
        }
    }
    fn mesh(&self) -> &MeshData {
        let AssetData::Mesh(mesh) = self.data.as_ref() else {
            unreachable!()
        };
        mesh
    }
}
struct Instance {
    geometry: Arc<Geometry>,
    inverse: Mat4,
    normal: Mat4,
    drawable: Drawable,
    texture: Option<Arc<AssetData>>,
    texture_blended: bool,
    bounds: [Vec3; 2],
}
#[derive(Clone, Copy)]
pub(super) struct Hit {
    pub distance: f32,
    pub position: Vec3,
    pub normal: Vec3,
    pub front: bool,
    pub albedo: Vec3,
    pub emission: Vec3,
    pub double_sided: bool,
}
pub(super) struct TraceScene {
    instances: Vec<Instance>,
    pub lights: Vec<WorldLight>,
    pub sun: Vec3,
    pub sun_radiance: Vec3,
    environment: bozzard_scene::EnvironmentSettings,
    pub epsilon: f32,
    pub distance_limit: f32,
}
impl TraceScene {
    pub fn new(scene: &Scene, assets: &AssetStore, progress: &Progress) -> Result<Self> {
        let matrices = scene.global_transforms()?;
        let statics = static_objects(scene);
        ensure!(
            statics.len() <= 1024,
            "GI bake supports at most1024 static mesh instances"
        );
        let mut cache = std::collections::BTreeMap::new();
        let mut instances = Vec::new();
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        let mut triangles = 0usize;
        for object in &scene.objects {
            if !statics.contains(&object.id) {
                continue;
            }
            progress.stage(format!("Preparing GI geometry: {}", object.name))?;
            let drawable = object.drawable.as_ref().unwrap();
            let key = match &drawable.mesh {
                Mesh::Cube => "builtin:cube".into(),
                Mesh::Quad => "builtin:quad".into(),
                Mesh::Asset(id) => format!("asset:{id}"),
            };
            if !cache.contains_key(&key) {
                let geometry = match &drawable.mesh {
                    Mesh::Asset(id) => {
                        let entry = assets
                            .handle(id)
                            .and_then(|h| assets.get(h))
                            .context("GI mesh missing")?;
                        Geometry::new(
                            entry.shared_data().context("GI mesh not decoded")?,
                            entry
                                .mesh_index
                                .clone()
                                .context("GI mesh index unavailable")?,
                        )
                    }
                    mesh => {
                        let data = builtin(*mesh == Mesh::Cube);
                        let index = Arc::new(MeshIndex::build(&data, progress)?);
                        Geometry::new(Arc::new(AssetData::Mesh(data)), index)
                    }
                };
                cache.insert(key.clone(), Arc::new(geometry));
            }
            let geometry = cache[&key].clone();
            triangles += geometry.mesh().indices.len() / 3;
            ensure!(
                triangles <= 1_000_000,
                "GI bake exceeds one million static triangles including instances"
            );
            let model = matrices[&object.id];
            let local = super::transform_bounds(
                geometry
                    .index
                    .bounds()
                    .context("GI mesh has no triangles")?,
                model,
            );
            bounds = [bounds[0].min(local[0]), bounds[1].max(local[1])];
            let texture = if let Texture::Asset(id) = &drawable.texture {
                Some(
                    assets
                        .handle(id)
                        .and_then(|h| assets.get(h))
                        .and_then(|e| e.shared_data())
                        .context("GI texture missing")?,
                )
            } else {
                None
            };
            let texture_blended = texture.as_deref().is_some_and(|data| match data {
                AssetData::Image(image) => image.rgba.chunks_exact(4).any(|p| p[3] < 255),
                _ => false,
            });
            instances.push(Instance {
                texture_blended,
                geometry,
                inverse: model.inverse(),
                normal: model.inverse().transpose(),
                drawable: drawable.clone(),
                texture,
                bounds: local,
            });
        }
        ensure!(!instances.is_empty(), "No static 3D geometry to bake");
        let extent = (bounds[1] - bounds[0]).length();
        ensure!(
            extent.is_finite() && extent <= 100_000.,
            "GI geometry bounds exceed supported scale"
        );
        let lights = scene
            .objects
            .iter()
            .filter_map(|o| o.light.filter(|l| l.enabled).map(|l| l.at(matrices[&o.id])))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            instances,
            lights,
            sun: Vec3::from(scene.lighting.sun_direction).normalize(),
            sun_radiance: Vec3::from(scene.lighting.sun_color) * scene.lighting.sun_intensity,
            environment: scene.environment,
            epsilon: (extent * 1e-6).clamp(0.00001, 0.01),
            distance_limit: ((Vec3::from(scene.gi.volume.max) - Vec3::from(scene.gi.volume.min))
                .length()
                * 2.
                + extent * 2.)
                .clamp(10., 1_000_000.),
        })
    }
    pub fn environment(&self, d: Vec3) -> Vec3 {
        let end = if d.y >= 0. {
            self.environment.zenith
        } else {
            self.environment.ground
        };
        Vec3::from(self.environment.horizon).lerp(Vec3::from(end), d.y.abs().sqrt())
            * self.environment.intensity
    }
    /// Alpha-tested geometry; alpha-blended surfaces do not occlude or bounce light.
    pub fn hit(&self, origin: Vec3, direction: Vec3, limit: f32) -> Option<Hit> {
        let mut start = origin;
        let mut travelled = 0.;
        for _ in 0..128 {
            let mut best = None;
            let mut nearest = limit - travelled;
            for instance in &self.instances {
                if crate::picking::box_entry(instance.bounds, start, direction, nearest).is_none() {
                    continue;
                }
                let local = instance.inverse.transform_point3(start);
                let ray = instance.inverse.transform_vector3(direction);
                if let Some(hit) =
                    instance
                        .geometry
                        .index
                        .cast(instance.geometry.mesh(), local, ray)
                    && hit.distance < nearest
                {
                    nearest = hit.distance;
                    best = Some((instance, hit));
                }
            }
            let (instance, hit) = best?;
            let sample = instance.sample(hit.triangle, start + direction * nearest, direction);
            if let Some(mut sample) = sample {
                sample.distance = travelled + nearest;
                return Some(sample);
            }
            let advance = nearest + self.epsilon;
            travelled += advance;
            if travelled >= limit {
                return None;
            }
            start += direction * advance;
        }
        // Conservatively stop transport after too many cutout layers rather than
        // treating an unexamined remainder as open sky and leaking light through it.
        Some(Hit {
            distance: travelled,
            position: start,
            normal: -direction,
            front: true,
            albedo: Vec3::ZERO,
            emission: Vec3::ZERO,
            double_sided: false,
        })
    }
    pub fn radiance(&self, origin: Vec3, direction: Vec3, bounces: u32, seed: &mut u64) -> Vec3 {
        let mut o = origin;
        let mut d = direction;
        let mut throughput = Vec3::ONE;
        let mut total = Vec3::ZERO;
        for bounce in 0..=bounces {
            let Some(hit) = self.hit(o, d, self.distance_limit) else {
                total += throughput * self.environment(d);
                break;
            };
            if !hit.front && !hit.double_sided {
                break;
            }
            total += throughput * hit.emission;
            if bounce == bounces {
                break;
            }
            let mut direct = Vec3::ZERO;
            let offset = hit.position + hit.normal * self.epsilon;
            let cosine = hit.normal.dot(self.sun).max(0.);
            if cosine > 0.
                && self.sun_radiance.max_element() > 0.
                && self.hit(offset, self.sun, self.distance_limit).is_none()
            {
                direct += self.sun_radiance * cosine;
            }
            for world in &self.lights {
                if world.light.kind == LightKind::Directional {
                    let l = -Vec3::from(world.direction);
                    let nl = hit.normal.dot(l).max(0.);
                    if nl > 0. && self.hit(offset, l, self.distance_limit).is_none() {
                        direct += Vec3::from(world.light.color) * world.light.intensity * nl;
                    }
                    continue;
                }
                let to = Vec3::from(world.position) - hit.position;
                let distance = to.length();
                if distance <= self.epsilon || distance >= world.light.range {
                    continue;
                }
                let l = to / distance;
                let nl = hit.normal.dot(l).max(0.);
                if nl == 0.
                    || self
                        .hit(offset, l, (distance - self.epsilon * 2.).max(0.))
                        .is_some()
                {
                    continue;
                }
                let window = (1. - (distance / world.light.range).powi(4)).max(0.);
                let mut attenuation = window * window / (distance * distance).max(0.0001);
                if world.light.kind == LightKind::Spot {
                    let outer = world.light.outer_angle_degrees.to_radians().cos();
                    let inner = world.light.inner_angle_degrees.to_radians().cos();
                    let angle = Vec3::from(world.direction).dot(-l);
                    let cone = if inner - outer > 0.000001 {
                        ((angle - outer) / (inner - outer)).clamp(0., 1.)
                    } else if angle >= outer {
                        1.
                    } else {
                        0.
                    };
                    attenuation *= cone * cone;
                }
                direct += Vec3::from(world.light.color) * world.light.intensity * attenuation * nl;
            }
            total += throughput * hit.albedo * direct / std::f32::consts::PI;
            throughput *= hit.albedo;
            if throughput.max_element() < 0.00001 {
                break;
            }
            o = offset;
            d = sampling::cosine(hit.normal, seed);
        }
        total.min(Vec3::splat(60_000.))
    }
}
impl Instance {
    fn sample(&self, triangle: u32, world: Vec3, ray: Vec3) -> Option<Hit> {
        let mesh = self.geometry.mesh();
        let indices = &mesh.indices[triangle as usize * 3..triangle as usize * 3 + 3];
        let vertices = [
            mesh.vertices[indices[0] as usize],
            mesh.vertices[indices[1] as usize],
            mesh.vertices[indices[2] as usize],
        ];
        let a = Vec3::from_slice(&vertices[0][..3]);
        let e1 = Vec3::from_slice(&vertices[1][..3]) - a;
        let e2 = Vec3::from_slice(&vertices[2][..3]) - a;
        let p = self.inverse.transform_point3(world) - a;
        let d00 = e1.dot(e1);
        let d01 = e1.dot(e2);
        let d11 = e2.dot(e2);
        let denom = d00 * d11 - d01 * d01;
        if denom <= 0. {
            return None;
        }
        let v = (d11 * p.dot(e1) - d01 * p.dot(e2)) / denom;
        let w = (d00 * p.dot(e2) - d01 * p.dot(e1)) / denom;
        let bary = [1. - v - w, v, w];
        let uv = vertices
            .iter()
            .zip(bary)
            .map(|(v, w)| Vec2::from_slice(&v[6..]) * w)
            .sum::<Vec2>()
            * Vec2::from(self.drawable.uv_scale);
        let geometric = self
            .normal
            .transform_vector3(e1.cross(e2))
            .try_normalize()?;
        let mut normal = self
            .normal
            .transform_vector3(
                vertices
                    .iter()
                    .zip(bary)
                    .map(|(v, w)| Vec3::from_slice(&v[3..6]) * w)
                    .sum::<Vec3>(),
            )
            .try_normalize()
            .unwrap_or(geometric);
        let front = geometric.dot(ray) < 0.;
        let part = mesh
            .parts
            .iter()
            .enumerate()
            .find(|(_, p)| triangle * 3 >= p.start && triangle * 3 < p.start + p.count);
        let mut color;
        let mut metallic = 0.;
        let mut emission = Vec3::ZERO;
        let mut double_sided = false;
        if let Some((part_index, part)) = part {
            color = Vec4::from(part.color);
            let override_ = self
                .drawable
                .material_overrides
                .iter()
                .find(|o| o.surface as usize == part_index && o.source == part.source_key);
            if let Some(o) = override_ {
                color *= Vec3::from(o.tint).extend(1.);
            }
            if let Some(shading) = &part.shading {
                double_sided = shading.material.double_sided;
                metallic = override_
                    .and_then(|o| o.metallic)
                    .unwrap_or(shading.material.metallic);
                let extra_uv = |start: usize| {
                    indices
                        .iter()
                        .zip(bary)
                        .map(|(i, w)| {
                            Vec2::from_slice(
                                &shading.vertices[(*i - shading.vertex_start) as usize]
                                    [start..start + 2],
                            ) * w
                        })
                        .sum::<Vec2>()
                        * Vec2::from(self.drawable.uv_scale)
                };
                if let Some(map) = &shading.material.metallic_roughness {
                    metallic *= sampling::image(&map.image, map.sampler, extra_uv(6), false).z;
                }
                emission = Vec3::from(shading.material.emissive_factor);
                if let Some(map) = &shading.material.emissive {
                    emission *=
                        sampling::image(&map.image, map.sampler, extra_uv(10), true).truncate();
                }
            }
            if let Some(image) = &part.image
                && self.drawable.texture == Texture::White
            {
                color *= sampling::image(
                    image,
                    part.shading
                        .as_ref()
                        .map(|s| s.material.base_color_sampler)
                        .unwrap_or_default(),
                    uv,
                    true,
                );
            } else {
                color *= self.fallback_texture(uv);
            }
            let blended = if self.drawable.texture == Texture::White {
                self.geometry.blended[part_index]
            } else {
                part.alpha_cutoff.is_none() && (part.color[3] < 1. || self.texture_blended)
            };
            if blended || !opaque(part, color.w) {
                return None;
            }
        } else {
            color = self.fallback_texture(uv);
            if self.texture_blended || color.w < 1. - 1e-5 {
                return None;
            }
        }
        color *= Vec3::from(self.drawable.color).extend(1.);
        if !front && double_sided {
            normal = -normal;
        }
        let mut albedo = color.truncate().clamp(Vec3::ZERO, Vec3::ONE);
        if part.is_some_and(|(_, p)| p.shading.is_some()) {
            let f0 = Vec3::splat(0.04).lerp(albedo, metallic);
            albedo *= (Vec3::ONE - f0) * (1. - metallic);
        }
        Some(Hit {
            distance: 0.,
            position: world,
            normal,
            front,
            albedo,
            emission,
            double_sided,
        })
    }
    fn fallback_texture(&self, uv: Vec2) -> Vec4 {
        if let Some(data) = &self.texture
            && let AssetData::Image(image) = data.as_ref()
        {
            return sampling::image(
                image,
                Sampler {
                    mag: Filter::Nearest,
                    ..Default::default()
                },
                uv,
                true,
            );
        }
        if self.drawable.texture == Texture::Checker {
            let light =
                ((uv.x * 2.).floor() as i64 + (uv.y * 2.).floor() as i64).rem_euclid(2) == 0;
            return if light {
                Vec4::new(240., 180., 70., 255.) / 255.
            } else {
                Vec4::new(20., 90., 105., 255.) / 255.
            };
        }
        Vec4::ONE
    }
}
fn opaque(part: &MeshPart, alpha: f32) -> bool {
    if let Some(cutoff) = part.alpha_cutoff {
        alpha > 0.00001 && alpha >= cutoff
    } else {
        alpha >= 1. - 1e-5
    }
}
fn builtin(cube: bool) -> MeshData {
    let mut mesh = MeshData {
        vertices: Vec::new(),
        indices: Vec::new(),
        parts: Vec::new(),
        warnings: Vec::new(),
    };
    let normals = if cube {
        vec![
            Vec3::Z,
            Vec3::NEG_Z,
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
        ]
    } else {
        vec![Vec3::Z]
    };
    for n in normals {
        let u = if n.y.abs() < 0.99 {
            Vec3::Y.cross(n).normalize()
        } else {
            Vec3::X
        };
        let v = n.cross(u);
        let center = if cube { n * 0.5 } else { Vec3::ZERO };
        let first = mesh.vertices.len() as u32;
        for ((x, y), uv) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
            .into_iter()
            .zip([[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        {
            let p = center + u * x + v * y;
            mesh.vertices
                .push([p.x, p.y, p.z, n.x, n.y, n.z, uv[0], uv[1]]);
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageData, PbrMaterial, SurfaceShading, TextureMap};
    fn picture(rgba: [u8; 4]) -> Arc<ImageData> {
        Arc::new(ImageData {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
        })
    }
    fn surface(mesh: MeshData, drawable: Drawable) -> Instance {
        let index = Arc::new(MeshIndex::build(&mesh, &Progress::default()).unwrap());
        Instance {
            geometry: Arc::new(Geometry::new(Arc::new(AssetData::Mesh(mesh)), index)),
            inverse: Mat4::IDENTITY,
            normal: Mat4::IDENTITY,
            drawable,
            texture: None,
            texture_blended: false,
            bounds: [Vec3::splat(-0.5), Vec3::splat(0.5)],
        }
    }
    fn drawable() -> Drawable {
        Drawable {
            gi_static: true,
            material_overrides: vec![],
            layer: bozzard_scene::Layer::ThreeD,
            mesh: Mesh::Quad,
            texture: Texture::White,
            color: [1.; 3],
            uv_scale: [1.; 2],
        }
    }
    fn mesh() -> MeshData {
        let mut mesh = builtin(false);
        mesh.parts.push(MeshPart {
            source_key: "0123456789abcdef".into(),
            name: "test".into(),
            material_name: None,
            start: 0,
            count: 6,
            color: [1.; 4],
            image: Some(picture([128, 64, 32, 255])),
            alpha_cutoff: None,
            shading: None,
        });
        mesh
    }
    fn sample(s: &Instance) -> Option<Hit> {
        s.sample(0, Vec3::new(0.2, -0.1, 0.), Vec3::NEG_Z)
    }
    #[test]
    fn directional_bounce_ignores_position_and_range_but_respects_direction() {
        let light = bozzard_scene::Light {
            kind: LightKind::Directional,
            intensity: 2.,
            ..Default::default()
        }
        .at(Mat4::IDENTITY)
        .unwrap();
        let mut scene = TraceScene {
            instances: vec![surface(mesh(), drawable())],
            lights: vec![light],
            sun: Vec3::Z,
            sun_radiance: Vec3::ZERO,
            environment: bozzard_scene::EnvironmentSettings {
                intensity: 0.,
                ..Default::default()
            },
            epsilon: 0.001,
            distance_limit: 100.,
        };
        let sample = |scene: &TraceScene| scene.radiance(Vec3::Z, Vec3::NEG_Z, 1, &mut 42);
        let lit = sample(&scene);
        assert!(lit.max_element() > 0.);
        scene.lights[0].position = [1000.; 3];
        scene.lights[0].light.range = 0.001;
        assert_eq!(sample(&scene), lit);
        scene.lights[0].direction = Vec3::Z.to_array();
        assert_eq!(sample(&scene), Vec3::ZERO);
    }

    #[test]
    fn cpu_transport_uses_srgb_tints_cutouts_and_explicit_texture_override() {
        let m = mesh();
        let mut d = drawable();
        d.color = [0.5, 1., 1.];
        d.material_overrides
            .push(bozzard_scene::SurfaceMaterialOverride {
                surface: 0,
                source: "0123456789abcdef".into(),
                tint: [1., 0.5, 1.],
                metallic: None,
                roughness: None,
            });
        let s = surface(m.clone(), d.clone());
        let hit = sample(&s).unwrap();
        assert!(
            hit.albedo.distance(Vec3::new(
                sampling::srgb(128. / 255.) * 0.5,
                sampling::srgb(64. / 255.) * 0.5,
                sampling::srgb(32. / 255.)
            )) < 1e-6
        );
        let mut masked = m.clone();
        masked.parts[0].image = Some(picture([255, 255, 255, 100]));
        masked.parts[0].alpha_cutoff = Some(0.5);
        assert!(sample(&surface(masked.clone(), d.clone())).is_none());
        masked.parts[0].alpha_cutoff = Some(0.3);
        assert!(sample(&surface(masked.clone(), d.clone())).is_some());
        masked.parts[0].alpha_cutoff = None;
        assert!(sample(&surface(masked, d.clone())).is_none());
        d.texture = Texture::Checker;
        let hit = sample(&surface(m, d)).unwrap();
        assert!(
            hit.albedo
                .distance(Vec3::new(240. / 255. * 0.5, 180. / 255. * 0.5, 70. / 255.))
                < 1e-6
        );
    }
    #[test]
    fn metallic_map_removes_diffuse_emission_uses_its_uv_and_double_sided_flips_normal() {
        let mut m = mesh();
        m.parts[0].shading = Some(SurfaceShading {
            vertex_start: 0,
            vertices: vec![[1., 0., 0., 1., 0., 0., 0.75, 0.5, 0., 0., 0.75, 0.5]; 4],
            material: PbrMaterial {
                metallic: 1.,
                roughness: 0.5,
                normal_scale: 1.,
                occlusion_strength: 1.,
                emissive_factor: [0.5, 1., 0.5],
                double_sided: true,
                base_color_sampler: Default::default(),
                metallic_roughness: Some(TextureMap {
                    image: picture([0, 255, 255, 255]),
                    sampler: Default::default(),
                }),
                normal: None,
                occlusion: None,
                emissive: Some(TextureMap {
                    image: Arc::new(ImageData {
                        width: 2,
                        height: 1,
                        rgba: vec![255, 0, 0, 255, 0, 128, 0, 255],
                    }),
                    sampler: Sampler {
                        mag: Filter::Nearest,
                        ..Default::default()
                    },
                }),
            },
        });
        let s = surface(m, drawable());
        let front = sample(&s).unwrap();
        assert!(front.albedo.length() < 1e-6);
        assert!(
            front
                .emission
                .distance(Vec3::new(0., sampling::srgb(128. / 255.), 0.))
                < 1e-6
        );
        let back = s.sample(0, Vec3::new(0.2, -0.1, 0.), Vec3::Z).unwrap();
        assert!(!back.front && back.double_sided);
        assert!(back.normal.distance(Vec3::NEG_Z) < 1e-6);
    }
}
