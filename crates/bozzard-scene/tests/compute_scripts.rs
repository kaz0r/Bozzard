use bozzard_ecs::World;
use bozzard_scene::{
    GameplayInput, Scene, SceneInstance, Transform,
    compute::{Capabilities, Command, JobState, Kernel, Owner, Scope, Submission},
};
use std::{collections::BTreeMap, sync::Arc};

const KERNEL: &str = r#"
struct Params { amount: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> values: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    if id.x < arrayLength(&values) { values[id.x] += params.amount; }
}
"#;
const SCRIPT: &str = r#"
fn on_start(me) {
    if !compute_available() { return; }
    compute_create_buffer("values", "kernel", "main", "values", 4);
    compute_write(compute_buffer("values"), [1.0, 2.0, 3.0, 4.0]);
    compute_create_texture("surface", 8, 8, "rgba8unorm");
    compute_bind_material(me, "base_color", compute_texture("surface"));
}
fn on_update(me, dt) {
    if !compute_available() { return; }
    if compute_poll("answer") == "complete" {
        let result = compute_take_result("answer");
        set_position(me, [result[0], result[1], result[2]]);
    } else if compute_poll("answer") == "missing" {
        compute_dispatch_extent("kernel", "main", #{ values: compute_buffer("values") }, #{ amount: 2.0 }, [4]);
        compute_readback("answer", compute_buffer("values"));
    }
}
"#;
fn scene(source: &str) -> (SceneInstance, World) {
    let scene = Scene::from_json(r#"{"version":1,"name":"compute","views":{},
      "assets":{"script":{"kind":"script","path":"compute.rs"},"kernel":{"kind":"compute_shader","path":"test.compute.wgsl"}},
      "objects":[{"id":"object","name":"Object","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
      "script_manager":{"scripts":[{"enabled":true,"script":"script"}]}}]}"#).unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_compute_kernels(BTreeMap::from([(
            "kernel".into(),
            Arc::new(Kernel::parse(KERNEL).unwrap()),
        )]))
        .unwrap();
    instance
        .register_script("script".into(), source.into())
        .unwrap();
    (instance, world)
}
fn capabilities() -> Capabilities {
    Capabilities {
        backend: Some("protocol test".into()),
        device_generation: 1,
        max_buffer_bytes: 1024 * 1024,
        max_uniform_bytes: 16384,
        max_texture_dimension: 1024,
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
fn tick(instance: &mut SceneInstance, world: &mut World) {
    instance
        .step_scripts(world, 1. / 60., GameplayInput::default())
        .unwrap();
}

#[test]
fn scripts_without_compute_leave_the_runtime_uninitialized() {
    let (mut instance, mut world) =
        scene("fn on_update(me, dt) { set_position(me, [1.0, 2.0, 3.0]); }");
    instance.register_compute_kernels(BTreeMap::new()).unwrap();
    instance.set_compute_capabilities(capabilities());
    for _ in 0..3 {
        tick(&mut instance, &mut world);
    }
    assert!(instance.compute_if_initialized().is_none());
}

#[test]
fn rhai_uses_named_resources_and_delivers_readbacks_only_at_a_simulation_boundary() {
    let (mut instance, mut world) = scene(SCRIPT);
    assert!(instance.compute_if_initialized().is_none());
    instance.set_compute_capabilities(capabilities());
    assert!(instance.compute_if_initialized().is_none());
    tick(&mut instance, &mut world);
    let owner = Owner::new("object", 0);
    let buffer = instance
        .compute()
        .runtime
        .find(&owner, Scope::Attachment, "values")
        .unwrap();
    assert!(instance.compute().material_texture("object").is_some());
    let mut callback = None;
    instance
        .compute()
        .runtime
        .submit_with(|batch, sink| {
            let mut readback = None;
            for request in batch.requests {
                match &request.command {
                    Command::Dispatch { groups, params, .. } => {
                        assert_eq!(*groups, [1, 1, 1]);
                        assert_eq!(params.as_ref(), &2f32.to_le_bytes());
                    }
                    Command::Readback { ticket, .. } => {
                        readback = Some(*ticket);
                    }
                    _ => {}
                }
            }
            callback = Some((batch.serial, readback.unwrap(), sink));
            Submission::Submitted
        })
        .unwrap();
    let (serial, ticket, sink) = callback.unwrap();
    sink.submitted_work_done(serial);
    // This recorder tests delivery, not WGSL execution (the real GPU oracle test lives in render).
    sink.readback_done(
        ticket,
        Ok([7f32, 8., 9., 10.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
            .into()),
    );
    assert_eq!(
        instance
            .compute()
            .runtime
            .job(&owner, ticket)
            .unwrap()
            .state,
        JobState::Submitted
    );
    tick(&mut instance, &mut world);
    assert_eq!(
        world
            .get::<Transform>(instance.entity("object").unwrap())
            .unwrap()
            .translation,
        [7., 8., 9.]
    );
    assert_eq!(instance.compute().runtime.statistics().pending_readbacks, 0);
    tick(&mut instance, &mut world);
    assert_eq!(
        instance
            .compute()
            .runtime
            .find(&owner, Scope::Attachment, "values")
            .unwrap(),
        buffer,
        "on_start allocation persists across updates"
    );
    instance.set_script_enabled("object", 0, false).unwrap();
    tick(&mut instance, &mut world);
    assert_eq!(
        instance.compute().runtime.statistics().pending_readbacks,
        0,
        "disable cancels queued results"
    );
    assert!(
        instance.compute().runtime.resource(&owner, buffer).is_ok(),
        "disable retains reusable resources until destruction"
    );
    instance.set_script_enabled("object", 0, true).unwrap();
    tick(&mut instance, &mut world);
    assert_eq!(instance.compute().runtime.statistics().pending_readbacks, 1);
}

#[test]
fn save_load_and_restart_recreate_visuals_and_late_callbacks_cannot_reach_the_new_world() {
    let (mut instance, mut world) = scene(SCRIPT);
    instance.set_compute_capabilities(capabilities());
    tick(&mut instance, &mut world);
    let owner = Owner::new("object", 0);
    let old = instance
        .compute()
        .runtime
        .find(&owner, Scope::Attachment, "values")
        .unwrap();
    let mut late = None;
    instance
        .compute()
        .runtime
        .submit_with(|batch, sink| {
            late = Some((batch.serial, sink));
            Submission::Submitted
        })
        .unwrap();
    let saved = instance.save_game_json(&world).unwrap();
    assert!(!saved.contains("ComputeResource") && !saved.contains("named_jobs"));
    instance.restart_runtime_scene(&mut world).unwrap();
    tick(&mut instance, &mut world);
    assert!(instance.compute().runtime.resource(&owner, old).is_err());
    let current_world = instance.compute().runtime.world();
    let (serial, sink) = late.unwrap();
    sink.submitted_work_done(serial);
    tick(&mut instance, &mut world);
    assert_eq!(instance.compute().runtime.world(), current_world);
    assert_eq!(instance.compute().runtime.statistics().resources, 2);
    instance.load_game_json(&mut world, &saved).unwrap();
    tick(&mut instance, &mut world);
    assert_ne!(instance.compute().runtime.world(), current_world);
    assert_eq!(instance.compute().runtime.statistics().resources, 2);
}

#[test]
fn headless_visual_guard_skips_work_and_required_compute_fails_with_attachment_context() {
    let (mut instance, mut world) = scene(SCRIPT);
    tick(&mut instance, &mut world);
    assert_eq!(instance.compute().runtime.statistics().resources, 0);
    assert_eq!(instance.compute().runtime.statistics().queued_commands, 0);
    let (mut instance, mut world) =
        scene("fn on_start(me) { compute_create_sampler(\"filter\", true); }");
    let error = format!(
        "{:#}",
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap_err()
    );
    assert!(
        error.contains("unavailable") && error.contains("'object' attachment 0"),
        "{error}"
    );
}

#[test]
fn two_attachments_have_private_names_and_scene_resources_require_explicit_scope() {
    let (instance, _) = scene(
        r#"
fn on_start(me) {
    compute_create_buffer("private", "kernel", "main", "values", 1);
}
"#,
    );
    let mut document = instance.document().clone();
    let attachment = document.objects[0].script_manager.as_ref().unwrap().scripts[0].clone();
    document.objects[0]
        .script_manager
        .as_mut()
        .unwrap()
        .scripts
        .push(attachment);
    let mut world = World::default();
    let mut instance = document.spawn(&mut world).unwrap();
    instance
        .register_compute_kernels(BTreeMap::from([(
            "kernel".into(),
            Arc::new(Kernel::parse(KERNEL).unwrap()),
        )]))
        .unwrap();
    instance.register_script("script".into(), "fn on_start(me) { compute_create_buffer(\"private\", \"kernel\", \"main\", \"values\", 1); }".into()).unwrap();
    instance.set_compute_capabilities(capabilities());
    tick(&mut instance, &mut world);
    let first = Owner::new("object", 0);
    let second = Owner::new("object", 1);
    let state = instance.compute();
    let a = state
        .runtime
        .find(&first, Scope::Attachment, "private")
        .unwrap();
    let b = state
        .runtime
        .find(&second, Scope::Attachment, "private")
        .unwrap();
    assert_ne!(a, b);
    assert!(state.runtime.resource(&second, a).is_err());
    assert!(
        state
            .runtime
            .find(&second, Scope::Scene, "private")
            .is_err()
    );
}
