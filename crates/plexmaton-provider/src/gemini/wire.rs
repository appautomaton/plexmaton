//! Typed Gemini content parts and their private replay metadata.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Part {
    pub text: Option<String>,
    pub thought: Option<bool>,
    pub thought_signature: Option<String>,
    pub function_call: Option<FunctionCall>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FunctionCall {
    pub id: Option<String>,
    pub name: String,
    pub args: Option<Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PartReplay {
    Text {
        thought: bool,
        text_present: bool,
        signature: String,
    },
    FunctionCall {
        upstream_id: Option<String>,
        signature: Option<String>,
    },
}
