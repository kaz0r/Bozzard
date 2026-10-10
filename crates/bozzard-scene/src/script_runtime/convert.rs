//! Conversions between Rhai values and the engine values scripts read and write.
use super::*;

pub(super) fn fail(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}

pub(super) fn ensure_script(
    condition: bool,
    message: impl Fn() -> String,
) -> Result<(), Box<EvalAltResult>> {
    if condition {
        Ok(())
    } else {
        Err(fail(message()))
    }
}

pub(super) fn array_of(vector: [f32; 3]) -> Array {
    vector.iter().map(|v| Dynamic::from(*v)).collect()
}

pub(super) fn vector_of(value: Array) -> Result<[f32; 3], Box<EvalAltResult>> {
    ensure_script(value.len() == 3, || {
        format!("expected a vector of 3 numbers, got {}", value.len())
    })?;
    let mut out = [0.; 3];
    for (slot, value) in out.iter_mut().zip(value) {
        *slot = value
            .as_float()
            .map_err(|_| fail("vector components must be numbers"))?;
        ensure_script(slot.is_finite(), || "vector must be finite".into())?;
    }
    Ok(out)
}

pub(super) fn number_of(value: Dynamic, what: &str) -> Result<f32, Box<EvalAltResult>> {
    let number = value
        .as_float()
        .map_err(|_| fail(format!("{what} must be a number")))?;
    ensure_script(number.is_finite(), || format!("{what} must be finite"))?;
    Ok(number)
}

pub(super) fn text_of(value: Dynamic, what: &str) -> Result<String, Box<EvalAltResult>> {
    value
        .into_string()
        .map_err(|_| fail(format!("{what} must be text")))
}

/// Convert a script value into the kind a blackboard variable declares.
pub(super) fn scalar_of(value: Dynamic, kind: PinType) -> Result<Value, Box<EvalAltResult>> {
    ensure_script(kind != PinType::Exec, || {
        "Exec cannot be stored in a variable".into()
    })?;
    Ok(match kind {
        PinType::Number => Value::Number(number_of(value, "variable")?),
        PinType::Bool => Value::Bool(
            value
                .as_bool()
                .map_err(|_| fail("variable must be a boolean"))?,
        ),
        PinType::Text => Value::Text(text_of(value, "variable")?),
        PinType::Vector => Value::Vector(vector_of(
            value
                .into_array()
                .map_err(|_| fail("variable must be a vector"))?,
        )?),
        PinType::Object => {
            if value.is_unit() {
                Value::Object(ObjectRef::None)
            } else {
                Value::Object(ObjectRef::Id(text_of(value, "object reference")?))
            }
        }
        PinType::Exec => unreachable!(),
    })
}

pub(super) fn dynamic_of(value: &Value) -> Dynamic {
    match value {
        Value::Text(text) => Dynamic::from(text.clone()),
        Value::Number(number) => Dynamic::from(*number),
        Value::Bool(flag) => Dynamic::from(*flag),
        Value::Vector(vector) => Dynamic::from(array_of(*vector)),
        Value::Object(ObjectRef::Id(id)) => Dynamic::from(id.clone()),
        _ => Dynamic::UNIT,
    }
}
