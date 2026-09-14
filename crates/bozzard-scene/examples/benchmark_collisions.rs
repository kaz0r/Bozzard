//! Live queries (no editor cache), with an exhaustive SAT oracle outside timing.
use anyhow::{Result, ensure};
use bozzard_ecs::World;
use bozzard_scene::{Scene, Transform};
use glam::Vec3;
use std::{hint::black_box, time::Instant};

fn measure<T>(label: &str, mut operation: impl FnMut(usize) -> Result<T>) -> Result<()> {
    let mut samples = Vec::new();
    for i in 0..110 {
        let start = Instant::now();
        black_box(operation(i)?);
        if i >= 10 {
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "collision_benchmark path={label} median_ms={:.6} p95_ms={:.6}",
        (samples[49] + samples[50]) * 0.5,
        samples[94]
    );
    Ok(())
}

fn main() -> Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(384);
    ensure!((2..=4096).contains(&count), "count must be 2..4096");
    let objects: Vec<_> = (0..count).map(|i| serde_json::json!({
        "id": format!("box-{i:04}"), "name": "Box",
        "transform": {"translation": [(i % 32) as f32 * 3., (i / 1024) as f32 * 3., (i / 32 % 32) as f32 * 3.],
            "rotation_degrees": [0., if i % 2 == 0 {15.} else {-15.}, 0.], "scale": [1., 1., 1.]},
        "collider": {}
    })).collect();
    let scene = Scene::from_json(
        &serde_json::json!({
            "version": 1, "name": "Live collision benchmark", "views": {}, "objects": objects
        })
        .to_string(),
    )?;
    let mut world = World::new();
    let instance = scene.spawn(&mut world)?;
    let mover = instance.entity("box-0000").unwrap();
    let original = *world.get::<Transform>(mover).unwrap();
    println!("collision_benchmark boxes={count}");
    measure("live_overlaps", |i| {
        world.get_mut::<Transform>(mover).unwrap().translation[0] = (i % 9) as f32 * 0.05;
        instance.collisions(&world)
    })?;
    let snapshot = instance.collisions(&world)?;
    let mut reference = Vec::new();
    for (i, a) in snapshot.boxes.iter().enumerate() {
        for b in &snapshot.boxes[i + 1..] {
            if a.intersects(b) {
                reference.push((a.id.clone(), b.id.clone()));
            }
        }
    }
    ensure!(
        snapshot.overlaps == reference,
        "broad phase differs from exhaustive SAT"
    );
    measure("move_box", |_| {
        *world.get_mut::<Transform>(mover).unwrap() = original;
        let result = instance.move_box(&mut world, "box-0000", Vec3::new(0.1, 0., 0.1))?;
        ensure!(
            result.contacts.is_empty() && result.applied.distance(Vec3::new(0.1, 0., 0.1)) < 1e-6,
            "unexpected motion result"
        );
        Ok(result)
    })?;
    Ok(())
}
