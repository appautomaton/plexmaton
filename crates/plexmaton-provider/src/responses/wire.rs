//! Small checked reads from untrusted Responses JSON events.

use serde_json::Value;

use crate::codec::DecodeError;

pub(super) fn provider_failed(event: &Value) -> DecodeError {
    let error = event
        .get("response")
        .and_then(|response| response.get("error"))
        .or_else(|| event.get("error"));
    DecodeError::ProviderFailed {
        code: error
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

pub(super) fn object_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value, DecodeError> {
    value
        .get(field)
        .filter(|value| value.is_object())
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}

pub(super) fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, DecodeError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}

pub(super) fn optional_string(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

pub(super) fn usize_field(value: &Value, field: &str) -> Result<usize, DecodeError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| DecodeError::UnsupportedEvent(format!("missing_{field}")))
}
