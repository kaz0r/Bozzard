//! Offline diffuse irradiance bake. CPU-only; the renderer consumes its packed data.
mod bake;
mod sampling;
mod trace;
use crate::AssetStore;
use anyhow::{Context, Result, ensure};
use bozzard_scene::{BakedGi, GiVolumeSettings, Layer, Scene};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, BTreeSet};

pub use bake::bake;

/// Objects with simulation motion (and their descendants) cannot be static casters.
pub fn static_objects(scene: &Scene) -> BTreeSet<String> {
    let mut dynamic: BTreeSet<_> = scene
        .objects
        .iter()
        .filter(|o| {
            o.spin.is_some()
                || o.gravity.is_some_and(|g| g.enabled)
                || o.player_controller.is_some()
        })
        .map(|o| o.id.clone())
        .collect();
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    for object in &scene.objects {
        if let Some(parent) = &object.parent {
            children.entry(parent).or_default().push(&object.id);
        }
    }
    let mut pending: std::collections::VecDeque<_> = dynamic.iter().cloned().collect();
    while let Some(parent) = pending.pop_front() {
        if let Some(children) = children.get(parent.as_str()) {
            for child in children {
                if dynamic.insert((*child).to_owned()) {
                    pending.push_back((*child).to_owned());
                }
            }
        }
    }
    scene
        .objects
        .iter()
        .filter(|o| {
            !dynamic.contains(&o.id)
                && o.drawable
                    .as_ref()
                    .is_some_and(|d| d.gi_static && d.layer == Layer::ThreeD)
        })
        .map(|o| o.id.clone())
        .collect()
}

/// Stable source fingerprint, independent of camera/editor/display settings and paths.
/// Geometry, materials, static transforms, loaded bytes, lights and sky affect a bake.
pub fn source(scene: &Scene, assets: &AssetStore, volume: GiVolumeSettings) -> Result<String> {
    let matrices = scene.global_transforms()?;
    source_with(scene, assets, volume, &matrices)
}
fn source_with(
    scene: &Scene,
    assets: &AssetStore,
    volume: GiVolumeSettings,
    matrices: &BTreeMap<String, Mat4>,
) -> Result<String> {
    volume.validate()?;
    let mut hash = 0xcbf29ce484222325_u64;
    let mut write = |bytes: &[u8]| {
        for b in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash = (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3);
        }
    };
    write(b"bozzard-diffuse-gi-v1");
    write(&serde_json::to_vec(&volume)?);
    // Shadow map resolution/bias and ambient are display approximations, not bake inputs.
    write(&serde_json::to_vec(&(
        scene.lighting.sun_direction,
        scene.lighting.sun_color,
        scene.lighting.sun_intensity,
        scene.environment.zenith,
        scene.environment.horizon,
        scene.environment.ground,
        scene.environment.intensity,
    ))?);
    let statics = static_objects(scene);
    let mut dependencies = BTreeSet::new();
    // Stable ID order prevents a hierarchy reorder alone from expiring a bake.
    let sorted: BTreeMap<_, _> = scene.objects.iter().map(|o| (&o.id, o)).collect();
    for (id, object) in sorted {
        if statics.contains(id) {
            let drawable = object.drawable.as_ref().unwrap();
            write(id.as_bytes());
            write(&serde_json::to_vec(&matrices[id].to_cols_array())?);
            write(&serde_json::to_vec(drawable)?);
            dependencies.extend(drawable.asset_dependencies().into_iter().map(|(id, _)| id));
        }
        if let Some(light) = object.light.filter(|l| l.enabled) {
            let world = light.at(matrices[id])?;
            write(&serde_json::to_vec(&(
                light,
                world.position,
                world.direction,
            ))?);
        }
    }
    for id in dependencies {
        let entry = assets
            .handle(id)
            .and_then(|h| assets.get(h))
            .context("GI source asset missing")?;
        let fingerprint = entry
            .content_fingerprint()
            .context("GI source asset not decoded")?;
        write(id.as_bytes());
        write(&fingerprint.to_le_bytes());
    }
    Ok(format!("{hash:016x}"))
}
pub fn is_current(scene: &Scene, assets: &AssetStore) -> Result<bool> {
    let Some(baked) = &scene.gi.baked else {
        return Ok(false);
    };
    Ok(baked.volume == scene.gi.volume && baked.source == source(scene, assets, scene.gi.volume)?)
}

pub fn fit_volume(scene: &Scene, assets: &AssetStore) -> Result<GiVolumeSettings> {
    let matrices = scene.global_transforms()?;
    let statics = static_objects(scene);
    let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for object in &scene.objects {
        if !statics.contains(&object.id) {
            continue;
        }
        let local = match &object.drawable.as_ref().unwrap().mesh {
            bozzard_scene::Mesh::Cube => [Vec3::splat(-0.5), Vec3::splat(0.5)],
            bozzard_scene::Mesh::Quad => [Vec3::new(-0.5, -0.5, 0.), Vec3::new(0.5, 0.5, 0.)],
            bozzard_scene::Mesh::Asset(id) => assets
                .handle(id)
                .and_then(|h| assets.get(h))
                .and_then(|e| e.mesh_bounds())
                .context("GI mesh bounds unavailable")?,
        };
        let world = transform_bounds(local, matrices[&object.id]);
        bounds = [bounds[0].min(world[0]), bounds[1].max(world[1])];
    }
    let mut volume = scene.gi.volume;
    ensure!(
        bounds[0].is_finite() && bounds[1].is_finite(),
        "No static 3D geometry to fit"
    );
    let pad = (bounds[1] - bounds[0]).max(glam::Vec3::splat(1.)) * 0.025;
    volume.min = (bounds[0] - pad).to_array();
    volume.max = (bounds[1] + pad).to_array();
    volume.validate()?;
    Ok(volume)
}

fn transform_bounds(bounds: [Vec3; 2], model: Mat4) -> [Vec3; 2] {
    let mut result = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for i in 0..8 {
        let p = Vec3::new(
            bounds[i & 1].x,
            bounds[(i >> 1) & 1].y,
            bounds[(i >> 2) & 1].z,
        );
        let p = model.transform_point3(p);
        result = [result[0].min(p), result[1].max(p)];
    }
    result
}

#[cfg(test)]
mod tests;
