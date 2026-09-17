use bozzard_compute::{
    BindingKind, Capabilities, Command, Dispatch, JobState, Kernel, Owner, Runtime, Scope,
    Submission, TextureFormat,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

// A protocol executor profile, not an implementation of WGSL on a CPU.
fn capabilities() -> Capabilities {
    Capabilities {
        backend: Some("test command recorder".into()),
        device_generation: 1,
        max_buffer_bytes: 64 * 1024 * 1024,
        max_uniform_bytes: 16 * 1024,
        max_texture_dimension: 4096,
        max_workgroup_size: [256, 256, 64],
        max_workgroup_invocations: 256,
        max_workgroup_bytes: 16384,
        max_workgroups: 65535,
        max_storage_buffers: 4,
        max_storage_textures: 4,
        max_sampled_textures: 16,
        max_samplers: 16,
    }
}
const SOURCE: &str = r#"
struct Params { scale: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> data: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x < arrayLength(&data) { data[id.x] *= params.scale; }
}
"#;
fn fixture() -> (Runtime, Owner, Arc<Kernel>, bozzard_compute::Handle) {
    let kernel = Arc::new(Kernel::parse(SOURCE).unwrap());
    let BindingKind::Storage { layout, .. } =
        &kernel.entry("main").unwrap().binding("data").unwrap().kind
    else {
        panic!()
    };
    let mut runtime = Runtime::new(capabilities());
    let owner = Owner::new("player", 0);
    let handle = runtime
        .create_buffer(
            &owner,
            Scope::Attachment,
            "values",
            Arc::new(layout.clone()),
            4,
        )
        .unwrap();
    (runtime, owner, kernel, handle)
}
fn dispatch(
    runtime: &mut Runtime,
    owner: &Owner,
    kernel: &Arc<Kernel>,
    handle: bozzard_compute::Handle,
    scale: f32,
) -> bozzard_compute::Ticket {
    runtime
        .dispatch(
            owner,
            Dispatch {
                asset: "multiply",
                kernel: kernel.clone(),
                entry: "main",
                bindings: &BTreeMap::from([("data".into(), handle)]),
                params: &json!({"scale": scale}),
                groups: [1, 1, 1],
            },
        )
        .unwrap()
}

#[test]
fn catch_up_ticks_preserve_upload_order_and_snapshots_and_viewports_do_not_replay() {
    let (mut runtime, owner, kernel, handle) = fixture();
    runtime.begin_tick();
    runtime.write(&owner, handle, &json!([1, 2, 3, 4])).unwrap();
    let first = dispatch(&mut runtime, &owner, &kernel, handle, 2.);
    runtime.begin_tick();
    runtime
        .write_range(&owner, handle, 2, &json!([10, 20]))
        .unwrap();
    let second = dispatch(&mut runtime, &owner, &kernel, handle, 3.);
    let readback = runtime.readback_range(&owner, handle, 1, 2).unwrap();
    let mut submitted = None;
    let queued = runtime.statistics().queued_commands;
    assert!(
        !runtime
            .submit_with(|batch, _| {
                assert_eq!(batch.requests.len(), queued);
                Submission::Deferred
            })
            .unwrap()
    );
    assert_eq!(runtime.job(&owner, first).unwrap().state, JobState::Queued);
    runtime.submit_with(|batch, sink| {
        assert!(batch.requests.windows(2).all(|pair| pair[0].sequence < pair[1].sequence));
        let times: Vec<_> = batch.requests.iter().filter_map(|r| if let Command::Dispatch { params, .. } = &r.command {
            Some((r.tick, f32::from_le_bytes(params[..4].try_into().unwrap())))
        } else { None }).collect();
        assert_eq!(times, vec![(1, 2.), (2, 3.)]);
        assert!(matches!(&batch.requests[3].command, Command::Write { offset: 8, bytes, .. } if bytes.len() == 8));
        assert!(matches!(&batch.requests[5].command, Command::Readback { offset: 4, bytes: 8, .. }));
        submitted = Some((batch.serial, sink));
        Submission::Submitted
    }).unwrap();
    assert!(
        !runtime
            .submit_with(|_, _| panic!("a second viewport must see no work"))
            .unwrap()
    );
    let (serial, sink) = submitted.unwrap();
    sink.submitted_work_done(serial);
    sink.readback_done(
        readback,
        Ok([12f32.to_le_bytes(), 30f32.to_le_bytes()].concat().into()),
    );
    // GPU callbacks during a paused repaint cannot change gameplay-visible completion state.
    assert_eq!(
        runtime.job(&owner, first).unwrap().state,
        JobState::Submitted
    );
    assert!(runtime.take_result(&owner, readback, 10).is_err());
    runtime.begin_tick();
    assert_eq!(
        runtime.job(&owner, first).unwrap().state,
        JobState::Complete
    );
    assert_eq!(
        runtime.job(&owner, second).unwrap().state,
        JobState::Complete
    );
    assert_eq!(
        runtime.take_result(&owner, readback, 10).unwrap(),
        json!([12., 30.])
    );
    assert_eq!(runtime.statistics().pending_readbacks, 0);
    assert_eq!(runtime.statistics().submitted_dispatches, 2);
}

#[test]
fn ownership_and_world_device_generations_reject_stale_resources_and_late_completions() {
    let (mut runtime, owner, kernel, private) = fixture();
    let other = Owner::new("player", 1);
    assert!(runtime.resource(&other, private).is_err());
    let shared = runtime
        .create_texture(
            &owner,
            Scope::Scene,
            "surface",
            8,
            8,
            TextureFormat::Rgba8Unorm,
        )
        .unwrap();
    assert_eq!(
        runtime.find(&other, Scope::Scene, "surface").unwrap(),
        shared
    );
    assert!(runtime.resource(&other, shared).is_ok());
    let job = dispatch(&mut runtime, &owner, &kernel, private, 1.);
    assert!(runtime.job(&other, job).is_err());
    let mut callback = None;
    runtime
        .submit_with(|batch, sink| {
            callback = Some((batch.serial, sink));
            Submission::Submitted
        })
        .unwrap();
    let old_world = runtime.world();
    runtime.reset();
    assert_ne!(runtime.world(), old_world);
    assert!(runtime.resource(&owner, private).is_err());
    assert!(runtime.job(&owner, job).is_err());
    let (serial, sink) = callback.unwrap();
    sink.submitted_work_done(serial);
    runtime.begin_tick();
    assert_eq!(runtime.statistics().jobs, 0);
    assert_eq!(runtime.statistics().resources, 0);
    let h = runtime
        .create_sampler(&owner, Scope::Attachment, "filter", true)
        .unwrap();
    let mut caps = capabilities();
    caps.device_generation = 2;
    runtime.set_capabilities(caps);
    assert!(runtime.resource(&owner, h).is_err());
}

#[test]
fn cancellation_retirement_and_owner_destruction_respect_in_flight_lifetimes() {
    let (mut runtime, owner, kernel, handle) = fixture();
    let queued = dispatch(&mut runtime, &owner, &kernel, handle, 1.);
    runtime.cancel(&owner, queued).unwrap();
    let submitted = dispatch(&mut runtime, &owner, &kernel, handle, 2.);
    let rb = runtime.readback(&owner, handle).unwrap();
    let shared = runtime
        .create_sampler(&owner, Scope::Scene, "shared", true)
        .unwrap();
    let mut completion = None;
    runtime
        .submit_with(|batch, sink| {
            assert_eq!(
                batch
                    .requests
                    .iter()
                    .filter(|r| matches!(r.command, Command::Dispatch { .. }))
                    .count(),
                1
            );
            completion = Some((batch.serial, sink));
            Submission::Submitted
        })
        .unwrap();
    runtime.cancel(&owner, submitted).unwrap();
    runtime.release_owner(&owner).unwrap();
    assert_eq!(runtime.statistics().pending_readbacks, 1);
    assert!(runtime.resource(&owner, handle).is_err());
    assert!(runtime.resource(&owner, shared).is_ok());
    assert_eq!(
        runtime.statistics().resource_bytes,
        16,
        "retired memory is still reserved while the GPU uses it"
    );
    let (serial, sink) = completion.unwrap();
    sink.submitted_work_done(serial);
    sink.readback_done(rb, Ok(vec![0; 16].into()));
    runtime.begin_tick();
    assert_eq!(runtime.statistics().pending_readbacks, 0);
    assert_eq!(
        runtime.statistics().resource_bytes,
        16,
        "release command has not been submitted"
    );
    runtime
        .submit_with(|batch, sink| {
            sink.submitted_work_done(batch.serial);
            Submission::Submitted
        })
        .unwrap();
    runtime.begin_tick();
    assert_eq!(runtime.statistics().resource_bytes, 0);
    assert_eq!(
        runtime.statistics().resources,
        1,
        "explicit scene-scoped resources survive the creator"
    );
}

#[test]
fn errors_are_atomic_and_quotas_apply_to_results_as_well_as_work() {
    let (mut runtime, owner, kernel, handle) = fixture();
    let before = runtime.statistics().queued_commands;
    assert!(runtime.write(&owner, handle, &json!([1, 2])).is_err());
    assert!(runtime.write_range(&owner, handle, 4, &json!([1])).is_err());
    assert!(runtime.readback_range(&owner, handle, u32::MAX, 2).is_err());
    assert!(
        runtime
            .create_texture(
                &owner,
                Scope::Attachment,
                "too_large",
                4097,
                1,
                TextureFormat::Rgba8Unorm
            )
            .is_err()
    );
    assert_eq!(runtime.statistics().queued_commands, before);
    let mut changed = SOURCE.replace("array<f32>", "array<vec2f>");
    // The same buffer name with a new layout must not silently reinterpret old allocations.
    let changed = Arc::new(Kernel::parse(std::mem::take(&mut changed)).unwrap());
    assert!(
        runtime
            .dispatch(
                &owner,
                Dispatch {
                    asset: "multiply",
                    kernel: changed,
                    entry: "main",
                    bindings: &BTreeMap::from([("data".into(), handle)]),
                    params: &json!({"scale": 1}),
                    groups: [1, 1, 1]
                }
            )
            .is_err()
    );
    let tickets: Vec<_> = (0..8)
        .map(|_| runtime.readback(&owner, handle).unwrap())
        .collect();
    assert!(
        runtime
            .readback(&owner, handle)
            .unwrap_err()
            .to_string()
            .contains("backpressure")
    );
    runtime
        .submit_with(|batch, sink| {
            for ticket in &tickets {
                sink.readback_done(*ticket, Ok(vec![0; 16].into()));
            }
            sink.submitted_work_done(batch.serial);
            Submission::Submitted
        })
        .unwrap();
    runtime.begin_tick();
    assert!(
        runtime.readback(&owner, handle).is_err(),
        "untaken results consume the bounded slots"
    );
    runtime.forget(&owner, tickets[0]).unwrap();
    assert!(runtime.readback(&owner, handle).is_ok());
    dispatch(&mut runtime, &owner, &kernel, handle, 1.);
}

#[test]
fn unavailable_headless_policy_and_rejected_batches_fail_explicitly() {
    let owner = Owner::new("visuals", 0);
    let mut headless = Runtime::default();
    assert!(!headless.capabilities().available());
    assert!(
        headless
            .create_sampler(&owner, Scope::Attachment, "filter", true)
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
    assert_eq!(headless.statistics().resources, 0);
    let (mut runtime, owner, kernel, handle) = fixture();
    let job = dispatch(&mut runtime, &owner, &kernel, handle, 1.);
    runtime
        .submit_with(|_, _| {
            Submission::Rejected("shader pipeline compilation failed at main".into())
        })
        .unwrap();
    assert!(
        matches!(&runtime.job(&owner, job).unwrap().state, JobState::Failed(error) if error.contains("compilation"))
    );
    assert!(runtime.resource(&owner, handle).is_err());
    // Failed allocations can still be released and their reservations reclaimed.
    runtime.release(&owner, handle).unwrap();
    runtime
        .submit_with(|_, _| Submission::Rejected("device lost".into()))
        .unwrap();
    assert_eq!(runtime.statistics().resource_bytes, 0);
}

#[test]
fn texture_feedback_alias_and_enabled_device_limits_are_validated_before_submission() {
    let kernel = Arc::new(
        Kernel::parse(
            r#"
@group(0) @binding(0) var input: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) id: vec3u) {
  textureStore(output, id.xy, textureLoad(input, vec2i(id.xy), 0));
}
"#,
        )
        .unwrap(),
    );
    let mut runtime = Runtime::new(capabilities());
    let owner = Owner::new("water", 0);
    let a = runtime
        .create_texture(
            &owner,
            Scope::Attachment,
            "a",
            8,
            8,
            TextureFormat::Rgba8Unorm,
        )
        .unwrap();
    let b = runtime
        .create_texture(
            &owner,
            Scope::Attachment,
            "b",
            8,
            8,
            TextureFormat::Rgba16Float,
        )
        .unwrap();
    for output in [a, b] {
        assert!(
            runtime
                .dispatch(
                    &owner,
                    Dispatch {
                        asset: "water",
                        kernel: kernel.clone(),
                        entry: "main",
                        bindings: &BTreeMap::from([("input".into(), a), ("output".into(), output)]),
                        params: &json!({}),
                        groups: [1, 1, 1]
                    }
                )
                .is_err()
        );
    }
    let mut caps = capabilities();
    caps.max_storage_textures = 0;
    assert!(caps.validate_entry(kernel.entry("main").unwrap()).is_err());
    let kernel =
        Kernel::parse(SOURCE.replace("workgroup_size(64)", "workgroup_size(256, 2)")).unwrap();
    assert!(
        capabilities()
            .validate_entry(kernel.entry("main").unwrap())
            .is_err()
    );
}
