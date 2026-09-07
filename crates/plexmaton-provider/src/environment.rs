//! Canonical identity of request inputs that do not live in context atoms.

use plexmaton_agent::{RequestEnvironment, RequestEnvironmentFingerprint};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{FunctionTool, ResolvedModel};

const FINGERPRINT_DOMAIN: &[u8] = b"plexmaton.request_environment.sha256.v1";

/// Builds the immutable request environment paired with one semantic atom prefix.
///
/// Fixed dialect flags are owned by the codec revision inside replay compatibility. The digest
/// covers every variable non-context input currently emitted by the codec. Credentials are not
/// available through [`ResolvedModel`] and therefore cannot enter the digest.
#[must_use]
pub fn request_environment(
    model: &ResolvedModel,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> RequestEnvironment {
    let compatibility = model.replay_compatibility();
    let mut digest = Sha256::new();
    field(&mut digest, b"domain", FINGERPRINT_DOMAIN);
    field(
        &mut digest,
        b"owner",
        compatibility.owner().as_str().as_bytes(),
    );
    field(
        &mut digest,
        b"codec",
        compatibility.codec().as_str().as_bytes(),
    );
    field(
        &mut digest,
        b"codec_revision",
        &compatibility.codec_revision().get().to_be_bytes(),
    );
    field(
        &mut digest,
        b"model_family",
        compatibility.model_family().as_str().as_bytes(),
    );
    field(
        &mut digest,
        b"reasoning_effort",
        model.reasoning_effort().as_str().as_bytes(),
    );
    match max_output_tokens {
        Some(limit) => field(&mut digest, b"max_output_tokens", &limit.to_be_bytes()),
        None => field(&mut digest, b"max_output_tokens_absent", &[]),
    }

    field(
        &mut digest,
        b"instructions_v1",
        model.instructions().as_bytes(),
    );
    if !model.workspace_instructions().is_empty() {
        field(
            &mut digest,
            b"workspace_instructions_v1",
            model.workspace_instructions().as_bytes(),
        );
    }
    field(
        &mut digest,
        b"prompt_cache",
        match model.prompt_cache() {
            crate::PromptCache::Automatic => b"automatic",
            crate::PromptCache::Disabled => b"disabled",
        },
    );
    field(
        &mut digest,
        b"tool_count",
        &usize_as_u64(tools.len()).to_be_bytes(),
    );
    for tool in tools {
        field(&mut digest, b"tool_name", tool.name().as_bytes());
        field(
            &mut digest,
            b"tool_description",
            tool.description().as_bytes(),
        );
        hash_json(&mut digest, tool.parameters());
    }

    let bytes: [u8; 32] = digest.finalize().into();
    RequestEnvironment::new(compatibility, RequestEnvironmentFingerprint::new(bytes))
}

fn field(digest: &mut Sha256, name: &[u8], value: &[u8]) {
    digest.update(usize_as_u64(name.len()).to_be_bytes());
    digest.update(name);
    digest.update(usize_as_u64(value.len()).to_be_bytes());
    digest.update(value);
}

fn usize_as_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or_else(|_| unreachable!("request input length fits u64"))
}

fn hash_json(digest: &mut Sha256, value: &Value) {
    match value {
        Value::Null => digest.update(b"null"),
        Value::Bool(value) => digest.update(if *value {
            b"true".as_slice()
        } else {
            b"false".as_slice()
        }),
        Value::Number(value) => field(digest, b"number", value.to_string().as_bytes()),
        Value::String(value) => field(digest, b"string", value.as_bytes()),
        Value::Array(values) => {
            field(
                digest,
                b"array_length",
                &usize_as_u64(values.len()).to_be_bytes(),
            );
            for value in values {
                hash_json(digest, value);
            }
        }
        Value::Object(values) => {
            field(
                digest,
                b"object_length",
                &usize_as_u64(values.len()).to_be_bytes(),
            );
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (key, value) in entries {
                field(digest, b"object_key", key.as_bytes());
                hash_json(digest, value);
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// Hash arbitrary portable session names into a bounded, non-identifying routing key.
pub(crate) fn session_cache_key(request: &plexmaton_agent::ModelRequest) -> String {
    identity_key(b"plexmaton.prompt_cache.v1:", request.session_id.as_str())
}

pub(crate) fn identity_key(domain: &[u8], identity: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(identity.as_bytes());
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut key = String::with_capacity(64);
    for byte in digest.finalize() {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 15)]));
    }
    key
}
