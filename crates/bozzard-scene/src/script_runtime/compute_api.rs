//! Rhai adapters for the same typed command API used by compiled-in Rust systems.
use super::{Array, Dynamic, Engine, EvalAltResult, Host, ImmutableString, Map, fail};
use crate::{
    SceneCompute,
    compute::{BindingKind, Dispatch, Handle, Owner, ResourceKind, Scope, Ticket},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

type ScriptResult = std::result::Result<Dynamic, Box<EvalAltResult>>;
fn call(
    host: &Arc<Mutex<Host>>,
    action: impl FnOnce(&Host, &Owner, &mut SceneCompute) -> Result<Dynamic>,
) -> ScriptResult {
    let mut host = host.lock().unwrap_or_else(|e| e.into_inner());
    if !host.compute_ready {
        return Err(fail("compute is unavailable outside a running scene hook"));
    }
    if host.compute.is_none() {
        let kernels = std::mem::take(&mut host.compute_kernels);
        host.compute = Some(Arc::new(Mutex::new(SceneCompute::new(
            host.compute_capabilities.clone(),
            kernels,
        ))));
    }
    let owner = Owner::new(&host.owner, host.attachment);
    let state = host
        .compute
        .as_ref()
        .ok_or_else(|| fail("compute is unavailable outside a running scene hook"))?;
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    action(&host, &owner, &mut state).map_err(|error| {
        fail(format!(
            "compute on '{}' attachment {}: {error:#}",
            owner.object, owner.attachment
        ))
    })
}
fn uint(value: Dynamic, name: &str) -> Result<u32> {
    if let Some(n) = value.clone().try_cast::<rhai::INT>() {
        return u32::try_from(n).with_context(|| format!("{name} must be a nonnegative u32"));
    }
    let n = value
        .try_cast::<rhai::FLOAT>()
        .with_context(|| format!("{name} must be a number"))?;
    ensure!(
        n.is_finite() && n >= 0. && n.fract() == 0. && f64::from(n) <= f64::from(u32::MAX),
        "{name} must be a nonnegative u32"
    );
    Ok(n as u32)
}
fn dimensions(values: Array) -> Result<[u32; 3]> {
    ensure!(
        !values.is_empty() && values.len() <= 3,
        "compute dimensions need one, two or three positive integers"
    );
    let mut result = [1; 3];
    for (i, value) in values.into_iter().enumerate() {
        result[i] = uint(value, "dimension")?;
        ensure!(result[i] > 0, "compute dimensions must be positive");
    }
    Ok(result)
}
fn to_json(value: Dynamic, budget: &mut usize, depth: usize) -> Result<serde_json::Value> {
    ensure!(
        *budget > 0 && depth < 16,
        "compute value exceeds 65536 values or 16 nesting levels"
    );
    *budget -= 1;
    if value.is::<rhai::INT>() {
        return Ok(serde_json::Value::from(value.cast::<rhai::INT>()));
    }
    if value.is::<rhai::FLOAT>() {
        let n = value.cast::<rhai::FLOAT>();
        ensure!(n.is_finite(), "compute values must be finite");
        return Ok(serde_json::Value::from(n));
    }
    if value.is::<Array>() {
        return Ok(serde_json::Value::Array(
            value
                .cast::<Array>()
                .into_iter()
                .map(|v| to_json(v, budget, depth + 1))
                .collect::<Result<_>>()?,
        ));
    }
    if value.is::<Map>() {
        return Ok(serde_json::Value::Object(
            value
                .cast::<Map>()
                .into_iter()
                .map(|(key, value)| Ok((key.to_string(), to_json(value, budget, depth + 1)?)))
                .collect::<Result<_>>()?,
        ));
    }
    bail!("compute data requires numbers, arrays, or field maps")
}
fn from_json(value: serde_json::Value) -> Dynamic {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(n) = n.as_i64() {
                Dynamic::from_int(n)
            } else {
                Dynamic::from_float(n.as_f64().unwrap() as rhai::FLOAT)
            }
        }
        serde_json::Value::Array(values) => {
            Dynamic::from_array(values.into_iter().map(from_json).collect())
        }
        serde_json::Value::Object(values) => Dynamic::from_map(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), from_json(value)))
                .collect(),
        ),
        _ => Dynamic::UNIT,
    }
}

pub(super) fn register(engine: &mut Engine, host: Arc<Mutex<Host>>) {
    engine.register_type_with_name::<Handle>("ComputeResource");
    engine.register_type_with_name::<Ticket>("ComputeTicket");
    macro_rules! api {
        ($name:expr, ($($arg:ident : $ty:ty),*), |$view:pat_param, $owner:pat_param, $state:pat_param| $body:expr) => {{
            let host = host.clone();
            engine.register_fn($name, move |$($arg: $ty),*| -> ScriptResult {
                call(&host, |$view, $owner, $state| $body)
            });
        }};
    }
    api!("compute_available", (), |_, _, state| Ok(
        Dynamic::from_bool(state.runtime.capabilities().available())
    ));
    api!("compute_capabilities", (), |_, _, state| {
        let caps = state.runtime.capabilities();
        Ok(Dynamic::from_map(Map::from_iter([
            ("available".into(), Dynamic::from_bool(caps.available())),
            (
                "backend".into(),
                Dynamic::from(caps.backend.as_deref().unwrap_or("unavailable").to_owned()),
            ),
            (
                "max_buffer_bytes".into(),
                Dynamic::from_int(caps.max_buffer_bytes as i64),
            ),
            (
                "max_texture_dimension".into(),
                Dynamic::from_int(caps.max_texture_dimension.into()),
            ),
            (
                "max_workgroups".into(),
                Dynamic::from_int(caps.max_workgroups.into()),
            ),
        ])))
    });
    for (scope, prefix) in [
        (Scope::Attachment, "compute_"),
        (Scope::Scene, "compute_scene_"),
    ] {
        api!(format!("{prefix}create_buffer"), (name: ImmutableString, asset: ImmutableString, entry: ImmutableString, binding: ImmutableString, elements: Dynamic), |_, owner, state| {
            let kernel = state.kernels.get(asset.as_str()).with_context(|| format!("unknown declared compute shader '{asset}'"))?;
            let BindingKind::Storage { layout, .. } = &kernel.entry(&entry)?.binding(&binding)?.kind else { bail!("buffer creation requires a storage buffer binding"); };
            Ok(Dynamic::from(state.runtime.create_buffer(owner, scope, &name, Arc::new(layout.clone()), uint(elements, "element count")?)?))
        });
        api!(format!("{prefix}create_texture"), (name: ImmutableString, width: Dynamic, height: Dynamic, format: ImmutableString), |_, owner, state| {
            Ok(Dynamic::from(state.runtime.create_texture(owner, scope, &name, uint(width, "width")?, uint(height, "height")?, format.parse()?)?))
        });
        api!(format!("{prefix}create_sampler"), (name: ImmutableString, linear: bool), |_, owner, state| {
            Ok(Dynamic::from(state.runtime.create_sampler(owner, scope, &name, linear)?))
        });
        for kind in ["buffer", "texture", "sampler"] {
            api!(format!("{prefix}{kind}"), (name: ImmutableString), |_, owner, state| {
                let handle = state.runtime.find(owner, scope, &name)?;
                let resource = state.runtime.resource(owner, handle)?;
                ensure!(matches!((kind, &resource.kind), ("buffer", ResourceKind::Buffer { .. }) | ("texture", ResourceKind::Texture { .. }) | ("sampler", ResourceKind::Sampler { .. })), "compute resource '{name}' is not a {kind}");
                Ok(Dynamic::from(handle))
            });
        }
    }
    api!("compute_release", (handle: Handle), |_, owner, state| {
        state.runtime.release(owner, handle)?; Ok(Dynamic::UNIT)
    });
    api!("compute_write", (handle: Handle, value: Dynamic), |_, owner, state| {
        state.runtime.write(owner, handle, &to_json(value, &mut 65536, 0)?)?; Ok(Dynamic::UNIT)
    });
    api!("compute_write_range", (handle: Handle, first: Dynamic, value: Array), |_, owner, state| {
        state.runtime.write_range(owner, handle, uint(first, "first element")?, &to_json(Dynamic::from_array(value), &mut 65536, 0)?)?; Ok(Dynamic::UNIT)
    });
    for extent in [false, true] {
        api!(if extent { "compute_dispatch_extent" } else { "compute_dispatch" }, (asset: ImmutableString, entry: ImmutableString, bindings: Map, params: Map, size: Array), |_, owner, state| {
            let kernel = state.kernels.get(asset.as_str()).with_context(|| format!("unknown declared compute shader '{asset}'"))?.clone();
            let size = dimensions(size)?;
            let groups = if extent { kernel.entry(&entry)?.groups_for_extent(size)? } else { size };
            let bindings = bindings.into_iter().map(|(name, value)| {
                Ok((name.to_string(), value.try_cast::<Handle>().with_context(|| format!("binding '{name}' requires a compute resource handle"))?))
            }).collect::<Result<BTreeMap<_, _>>>()?;
            let params = to_json(Dynamic::from_map(params), &mut 65536, 0)?;
            Ok(Dynamic::from(state.runtime.dispatch(owner, Dispatch { asset: &asset, kernel, entry: &entry, bindings: &bindings, params: &params, groups })?))
        });
    }
    api!("compute_readback", (name: ImmutableString, handle: Handle), |_, owner, state| {
        let ticket = state.runtime.readback(owner, handle)?;
        if let Err(error) = state.name_job(owner, &name, ticket) { state.runtime.cancel(owner, ticket)?; state.runtime.forget(owner, ticket)?; return Err(error); }
        Ok(Dynamic::from(ticket))
    });
    api!("compute_readback_range", (name: ImmutableString, handle: Handle, first: Dynamic, count: Dynamic), |_, owner, state| {
        let ticket = state.runtime.readback_range(owner, handle, uint(first, "first element")?, uint(count, "element count")?)?;
        if let Err(error) = state.name_job(owner, &name, ticket) { state.runtime.cancel(owner, ticket)?; state.runtime.forget(owner, ticket)?; return Err(error); }
        Ok(Dynamic::from(ticket))
    });
    api!("compute_poll", (name: ImmutableString), |_, owner, state| {
        let Ok(ticket) = state.named_job(owner, &name) else { return Ok(Dynamic::from("missing")); };
        Ok(Dynamic::from(state.runtime.job(owner, ticket)?.state.name()))
    });
    api!("compute_poll", (ticket: Ticket), |_, owner, state| {
        Ok(Dynamic::from(state.runtime.job(owner, ticket)?.state.name()))
    });
    api!("compute_error", (name: ImmutableString), |_, owner, state| {
        let ticket = state.named_job(owner, &name)?;
        Ok(Dynamic::from(match &state.runtime.job(owner, ticket)?.state { crate::compute::JobState::Failed(error) => error.clone(), _ => String::new() }))
    });
    api!("compute_take_result", (name: ImmutableString), |_, owner, state| {
        let ticket = state.named_job(owner, &name)?;
        let value = state.runtime.take_result(owner, ticket, 65536)?;
        state.named_jobs.remove(&(owner.clone(), name.to_string()));
        Ok(from_json(value))
    });
    api!("compute_cancel", (name: ImmutableString), |_, owner, state| {
        let ticket = state.named_job(owner, &name)?;
        state.runtime.cancel(owner, ticket)?; state.runtime.forget(owner, ticket)?;
        state.named_jobs.remove(&(owner.clone(), name.to_string()));
        Ok(Dynamic::UNIT)
    });
    api!("compute_cancel", (ticket: Ticket), |_, owner, state| {
        state.runtime.cancel(owner, ticket)?; Ok(Dynamic::UNIT)
    });
    api!("compute_bind_material", (object: ImmutableString, slot: ImmutableString, handle: Handle), |view, owner, state| {
        ensure!(slot == "base_color", "compute material output currently supports the base_color slot");
        let target = view.target_of(&object).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        state.bind_material(owner, &target, handle)?;
        Ok(Dynamic::UNIT)
    });
}
