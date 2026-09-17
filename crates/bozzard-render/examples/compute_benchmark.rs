//! Opt-in measurement; waits are intentional here, never in the interactive executor.
//! cargo run --release -p bozzard-render --example compute_benchmark
use anyhow::{Result, ensure};
use bozzard_compute::{BindingKind, Dispatch, Kernel, Owner, Runtime, Scope};
use bozzard_render::{Backend, Gpu, compute::Executor, instance};
use serde_json::json;
use std::{collections::BTreeMap, hint::black_box, sync::Arc, time::Instant};

const SOURCE: &str = r#"
@group(0) @binding(0) var<storage, read_write> values: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= arrayLength(&values) { return; }
    var v = values[id.x];
    for (var i = 0u; i < 32u; i++) { v = sin(v) * 0.9 + 0.1; }
    values[id.x] = v;
}
"#;
fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn finish(gpu: &Gpu, executor: &mut Executor, runtime: &mut Runtime) -> Result<Option<f64>> {
    ensure!(
        executor.submit(gpu, runtime)?,
        "benchmark unexpectedly backpressured"
    );
    gpu.wait()?;
    let profiles = executor.poll(gpu)?;
    runtime.begin_tick();
    Ok(profiles
        .into_iter()
        .flat_map(|f| f.passes)
        .filter_map(|p| p.milliseconds)
        .reduce(|a, b| a + b))
}
fn main() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let kernel = Arc::new(Kernel::parse(SOURCE)?);
    let BindingKind::Storage { layout, .. } = &kernel.entry("main")?.binding("values")?.kind else {
        unreachable!()
    };
    let mut rows = Vec::new();
    for count in [256u32, 262_144] {
        let mut executor = Executor::new(&gpu);
        executor.set_profiling(true);
        let mut runtime = Runtime::new(executor.capabilities().clone());
        let owner = Owner::new("benchmark", 0);
        let buffer = runtime.create_buffer(
            &owner,
            Scope::Attachment,
            "values",
            Arc::new(layout.clone()),
            count,
        )?;
        let mut cpu: Vec<f32> = (0..count).map(|i| i as f32 / count as f32).collect();
        let input = json!(cpu);
        runtime.write(&owner, buffer, &input)?;
        let bindings = BTreeMap::from([("values".into(), buffer)]);
        let params = json!({});
        let dispatch = || Dispatch {
            asset: "benchmark",
            kernel: kernel.clone(),
            entry: "main",
            bindings: &bindings,
            params: &params,
            groups: [count.div_ceil(64), 1, 1],
        };
        for _ in 0..3 {
            runtime.dispatch(&owner, dispatch())?;
            finish(&gpu, &mut executor, &mut runtime)?;
        }
        let warm = executor.statistics();
        let repeats = 20.;
        let mut resident_ms = 0.;
        let mut encode_ms = 0.;
        let mut submit_ms = 0.;
        let mut gpu_ms = Vec::new();
        for _ in 0..repeats as usize {
            let start = Instant::now();
            runtime.dispatch(&owner, dispatch())?;
            if let Some(ms) = finish(&gpu, &mut executor, &mut runtime)? {
                gpu_ms.push(ms);
            }
            resident_ms += elapsed(start);
            encode_ms += executor.statistics().encode_ms;
            submit_ms += executor.statistics().submit_ms;
        }
        let cpu_start = Instant::now();
        for _ in 0..repeats as usize {
            for value in black_box(&mut cpu) {
                for _ in 0..32 {
                    *value = value.sin() * 0.9 + 0.1;
                }
            }
            black_box(&cpu);
        }
        let cpu_ms = elapsed(cpu_start) / repeats;
        let mut upload_ms = 0.;
        let mut readback_ms = 0.;
        for iteration in 0..5 {
            let start = Instant::now();
            runtime.write(&owner, buffer, &input)?;
            finish(&gpu, &mut executor, &mut runtime)?;
            upload_ms += elapsed(start);
            let start = Instant::now();
            let result = runtime.readback(&owner, buffer)?;
            finish(&gpu, &mut executor, &mut runtime)?;
            let output = runtime.take_result(&owner, result, count as usize + 1)?;
            readback_ms += elapsed(start);
            ensure!(output == input, "transfer roundtrip changed buffer values");
            if iteration > 0 {
                ensure!(
                    executor.statistics().readback_allocations == 1,
                    "readback pool did not reuse its allocation"
                );
            }
        }
        let after = executor.statistics();
        ensure!(
            after.pipeline_compilations == warm.pipeline_compilations
                && after.bind_group_creations == warm.bind_group_creations
                && after.resource_creations == warm.resource_creations
                && after.upload_allocations == warm.upload_allocations,
            "warm workload allocated GPU objects"
        );
        rows.push(json!({"elements": count, "bytes": count * 4, "cpu_ms": cpu_ms, "gpu_pass_ms": if gpu_ms.is_empty() { None } else { Some(gpu_ms.iter().sum::<f64>() / gpu_ms.len() as f64) }, "resident_roundtrip_ms": resident_ms / repeats, "cpu_encode_ms": encode_ms / repeats, "queue_submit_ms": submit_ms / repeats, "typed_upload_roundtrip_ms": upload_ms / 5., "typed_readback_roundtrip_ms": readback_ms / 5., "warm_gpu_allocations": 0}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"adapter": gpu.adapter.get_info().name, "backend": gpu.adapter.get_info().backend.to_str(), "profile": if cfg!(debug_assertions) { "debug" } else { "release" }, "kernel": "32 dependent sin/scale/add iterations per element", "workloads": rows})
        )?
    );
    Ok(())
}
