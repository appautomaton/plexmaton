//! Small checked reads from untrusted provider JSON events.

use serde_json::Value;

use crate::codec::DecodeError;

/// PRV-5: empty additions contain no output to preserve; populated unknown fields are unsupported.
pub(crate) fn check_additive_fields(
    fields: &serde_json::Map<String, Value>,
    surface: &'static str,
) -> Result<(), DecodeError> {
    for (name, value) in fields {
        let empty = value.is_null()
            || value.as_array().is_some_and(Vec::is_empty)
            || value.as_object().is_some_and(serde_json::Map::is_empty);
        if !empty {
            return Err(DecodeError::UnsupportedEvent(format!("{surface}.{name}")));
        }
        tracing::debug!(surface, field = name, "ignored empty provider field");
    }
    Ok(())
}

pub(crate) fn provider_failed(event: &Value) -> DecodeError {
    let error = event
        .get("response")
        .and_then(|response| response.get("error"))
        .or_else(|| event.get("error"));
    DecodeError::ProviderFailed {
        code: error
            .and_then(|error| error.get("code").or_else(|| error.get("type")))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

pub(crate) fn object_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value, DecodeError> {
    value
        .get(field)
        .filter(|value| value.is_object())
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}

pub(crate) fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, DecodeError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}

pub(crate) fn optional_string(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn usize_field(value: &Value, field: &str) -> Result<usize, DecodeError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}
