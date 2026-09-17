use anyhow::Result;
use bozzard_compute::{
    BindingKind, Dispatch, JobState, Kernel, Owner, Runtime, Scope, TextureFormat,
};
use bozzard_render::{Backend, Gpu, compute::Executor, instance, wgpu};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const SOURCE: &str = r#"
struct Params { scale: f32, add: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(2) var<storage, read_write> values: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x < arrayLength(&values) { values[id.x] = values[id.x] * params.scale + params.add; }
}
"#;
fn complete(gpu: &Gpu, executor: &mut Executor, runtime: &mut Runtime) -> Result<()> {
    // Blocking is confined to the offscreen test. Production poll only uses PollType::Poll.
    gpu.wait()?;
    executor.poll(gpu)?;
    runtime.begin_tick();
    Ok(())
}

#[test]
fn readback_saturation_cancellation_and_world_replacement_remain_bounded() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut executor = Executor::new(&gpu);
    let mut runtime = Runtime::new(executor.capabilities().clone());
    let owner = Owner::new("pool-test", 0);
    let kernel = Kernel::parse(SOURCE)?;
    let BindingKind::Storage { layout, .. } = &kernel.entry("main")?.binding("values")?.kind else {
        panic!()
    };
    let layout = Arc::new(layout.clone());
    let buffer = runtime.create_buffer(&owner, Scope::Attachment, "values", layout.clone(), 4)?;
    runtime.write(&owner, buffer, &json!([1., 2., 3., 4.]))?;
    let tickets = (0..bozzard_compute::MAX_READBACKS)
        .map(|_| runtime.readback(&owner, buffer))
        .collect::<Result<Vec<_>>>()?;
    assert!(runtime.readback(&owner, buffer).is_err());
    assert!(executor.submit(&gpu, &mut runtime)?);
    for ticket in tickets {
        runtime.cancel(&owner, ticket)?;
        runtime.forget(&owner, ticket)?;
    }
    assert_eq!(
        runtime.statistics().pending_readbacks,
        8,
        "cancelling submitted work cannot prematurely recycle staging slots"
    );
    runtime.reset();
    let new = runtime.create_buffer(&owner, Scope::Attachment, "new", layout, 4)?;
    runtime.write(&owner, new, &json!([5., 6., 7., 8.]))?;
    let answer = runtime.readback(&owner, new)?;
    assert!(
        !executor.submit(&gpu, &mut runtime)?,
        "old-world mappings still occupy the pool even though CPU handles are invalidated"
    );
    complete(&gpu, &mut executor, &mut runtime)?;
    assert_eq!(runtime.job(&owner, answer)?.state, JobState::Queued);
    assert!(executor.submit(&gpu, &mut runtime)?);
    complete(&gpu, &mut executor, &mut runtime)?;
    assert_eq!(
        runtime.take_result(&owner, answer, 4)?,
        json!([5., 6., 7., 8.])
    );
    let warm = executor.statistics();
    for count in [8, 32, 8, 32] {
        runtime.reset();
        let texture = runtime.create_texture(
            &owner,
            Scope::Attachment,
            "resize",
            count,
            count,
            TextureFormat::Rgba8Unorm,
        )?;
        assert!(executor.submit(&gpu, &mut runtime)?);
        complete(&gpu, &mut executor, &mut runtime)?;
        assert!(executor.texture_view(texture).is_some());
        assert_eq!(executor.statistics().allocations, 1);
    }
    assert_eq!(
        executor.statistics().readback_allocations,
        warm.readback_allocations
    );
    executor.clear_world();
    assert_eq!(executor.statistics().allocations, 0);
    assert!(!executor.has_pending());
    Ok(())
}

#[test]
fn offscreen_compute_matches_cpu_ordering_readback_texture_reload_and_reuse() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut executor = Executor::new(&gpu);
    let mut runtime = Runtime::new(executor.capabilities().clone());
    assert!(!executor.submit(&gpu, &mut runtime)?);
    assert_eq!(executor.statistics().staging_bytes, 0);
    assert_eq!(executor.statistics().allocations, 0);
    executor.set_profiling(true);
    let owner = Owner::new("offscreen", 0);
    let kernel = Arc::new(Kernel::parse(SOURCE)?);
    let BindingKind::Storage { layout, .. } = &kernel.entry("main")?.binding("values")?.kind else {
        panic!()
    };
    let buffer = runtime.create_buffer(
        &owner,
        Scope::Attachment,
        "numbers",
        Arc::new(layout.clone()),
        257,
    )?;
    let mut expected: Vec<f32> = (0..257).map(|n| n as f32 * 0.125).collect();
    runtime.write(&owner, buffer, &json!(expected))?;
    let bindings = BTreeMap::from([("values".into(), buffer)]);
    runtime.dispatch(
        &owner,
        Dispatch {
            asset: "numbers",
            kernel: kernel.clone(),
            entry: "main",
            bindings: &bindings,
            params: &json!({"scale": 2., "add": 1.}),
            groups: kernel.entry("main")?.groups_for_extent([257, 1, 1])?,
        },
    )?;
    for value in &mut expected {
        *value = *value * 2. + 1.;
    }
    runtime.begin_tick();
    runtime.write_range(&owner, buffer, 253, &json!([-3., 4.]))?;
    expected[253] = -3.;
    expected[254] = 4.;
    let dispatch = runtime.dispatch(
        &owner,
        Dispatch {
            asset: "numbers",
            kernel: kernel.clone(),
            entry: "main",
            bindings: &bindings,
            params: &json!({"scale": 3., "add": 2.}),
            groups: [5, 1, 1],
        },
    )?;
    for value in &mut expected {
        *value = *value * 3. + 2.;
    }
    let result = runtime.readback(&owner, buffer)?;
    assert!(executor.submit(&gpu, &mut runtime)?);
    assert_eq!(runtime.job(&owner, dispatch)?.state, JobState::Submitted);
    assert!(
        !executor.submit(&gpu, &mut runtime)?,
        "second viewport cannot replay a submitted batch"
    );
    complete(&gpu, &mut executor, &mut runtime)?;
    assert_eq!(runtime.job(&owner, dispatch)?.state, JobState::Complete);
    let actual = runtime.take_result(&owner, result, 257)?;
    for (a, e) in actual.as_array().unwrap().iter().zip(&expected) {
        assert!((a.as_f64().unwrap() - f64::from(*e)).abs() < 1e-5);
    }
    let warm = executor.statistics();
    assert_eq!(warm.pipeline_compilations, 1);
    assert_eq!(warm.bind_group_creations, 1);
    // Warm dispatches retain pipeline/bind groups, uniform storage, buffers, upload and readback pools.
    for _ in 0..4 {
        runtime.dispatch(
            &owner,
            Dispatch {
                asset: "numbers",
                kernel: kernel.clone(),
                entry: "main",
                bindings: &bindings,
                params: &json!({"scale": 1., "add": 0.}),
                groups: [5, 1, 1],
            },
        )?;
        let result = runtime.readback(&owner, buffer)?;
        assert!(executor.submit(&gpu, &mut runtime)?);
        complete(&gpu, &mut executor, &mut runtime)?;
        runtime.take_result(&owner, result, 257)?;
    }
    let after = executor.statistics();
    assert_eq!(after.pipeline_compilations, warm.pipeline_compilations);
    assert_eq!(after.bind_group_creations, warm.bind_group_creations);
    assert_eq!(after.resource_creations, warm.resource_creations);
    assert_eq!(after.upload_allocations, warm.upload_allocations);
    assert_eq!(after.readback_allocations, warm.readback_allocations);
    // Queued jobs pin the old revision while a compatible edit takes effect later in the batch.
    runtime.dispatch(
        &owner,
        Dispatch {
            asset: "numbers",
            kernel: kernel.clone(),
            entry: "main",
            bindings: &bindings,
            params: &json!({"scale": 2., "add": 1.}),
            groups: [5, 1, 1],
        },
    )?;
    let replacement = Arc::new(Kernel::parse(
        SOURCE.replace("+ params.add", "- params.add"),
    )?);
    runtime.dispatch(
        &owner,
        Dispatch {
            asset: "numbers",
            kernel: replacement,
            entry: "main",
            bindings: &bindings,
            params: &json!({"scale": 1., "add": 1.}),
            groups: [5, 1, 1],
        },
    )?;
    let result = runtime.readback_range(&owner, buffer, 253, 4)?;
    assert!(executor.submit(&gpu, &mut runtime)?);
    complete(&gpu, &mut executor, &mut runtime)?;
    assert_eq!(
        runtime.take_result(&owner, result, 4)?,
        json!(expected[253..].iter().map(|v| *v * 2.).collect::<Vec<_>>())
    );
    let texture_kernel = Arc::new(Kernel::parse(
        r#"
@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3u) {
  let size = textureDimensions(output);
  if any(id.xy >= size) { return; }
  textureStore(output, id.xy, vec4f(f32(id.x) / f32(size.x - 1u), f32(id.y) / f32(size.y - 1u), 0.25, 1.));
}
"#,
    )?);
    let texture = runtime.create_texture(
        &owner,
        Scope::Attachment,
        "gradient",
        17,
        9,
        TextureFormat::Rgba8Unorm,
    )?;
    runtime.dispatch(
        &owner,
        Dispatch {
            asset: "gradient",
            kernel: texture_kernel.clone(),
            entry: "main",
            bindings: &BTreeMap::from([("output".into(), texture)]),
            params: &json!({}),
            groups: texture_kernel
                .entry("main")?
                .groups_for_extent([17, 9, 1])?,
        },
    )?;
    assert!(executor.submit(&gpu, &mut runtime)?);
    complete(&gpu, &mut executor, &mut runtime)?;
    let pixels = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test texture pixels"),
        size: 256 * 9,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        executor.texture(texture).unwrap().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &pixels,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(9),
            },
        },
        wgpu::Extent3d {
            width: 17,
            height: 9,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    pixels.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    gpu.wait()?;
    let mapped = pixels.get_mapped_range(..)?;
    for y in 0..9 {
        for x in 0..17 {
            let pixel = &mapped[y * 256 + x * 4..y * 256 + x * 4 + 4];
            assert!(pixel[0].abs_diff((x as f32 / 16. * 255.).round() as u8) <= 1);
            assert!(pixel[1].abs_diff((y as f32 / 8. * 255.).round() as u8) <= 1);
            assert!(pixel[2].abs_diff(64) <= 1);
            assert_eq!(pixel[3], 255);
        }
    }
    drop(mapped);
    pixels.unmap();
    let old_world = runtime.world();
    runtime.reset();
    assert_ne!(runtime.world(), old_world);
    assert!(!executor.submit(&gpu, &mut runtime)?);
    assert!(executor.texture_view(texture).is_none());
    assert_eq!(executor.statistics().allocations, 0);
    Ok(())
}
