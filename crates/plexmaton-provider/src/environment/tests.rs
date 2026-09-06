use serde_json::json;

use super::request_environment;
use crate::{FunctionTool, ModelRegistry, ResolvedModel};

const BASE: &str = r#"
active_model = { provider = "local", model = "luna" }

[providers.local]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
api = "openai_responses"

[providers.local.models.luna]
id = "gpt-5.6-luna"
display_name = "Luna"
reasoning_effort = "xhigh"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 16384
cost = { input = 0.2, output = 1.2, cache_read = 0.02, cache_write = 0.25 }
"#;

fn model(source: &str) -> ResolvedModel {
    ModelRegistry::parse(source)
        .unwrap_or_else(|error| panic!("parse model fixture: {error}"))
        .active_model()
        .clone()
}

fn tool(name: &str, description: &str, parameters: serde_json::Value) -> FunctionTool {
    FunctionTool::new(name, description, parameters)
        .unwrap_or_else(|error| panic!("tool fixture: {error}"))
}

fn fingerprint(model: &ResolvedModel, tools: &[FunctionTool]) -> String {
    request_environment(model, tools, Some(model.max_output_tokens()))
        .fingerprint()
        .to_string()
}

/// TIM-3/TIM-4: equal request environments have one stable fixed-width identity.
#[test]
fn tim_3_equal_environment_inputs_have_one_canonical_fingerprint() {
    let model = model(BASE);
    let schema_a = json!({
        "type": "object",
        "properties": { "path": { "type": "string" }, "line": { "type": "integer" } }
    });
    let schema_b = json!({
        "properties": { "line": { "type": "integer" }, "path": { "type": "string" } },
        "type": "object"
    });
    let first = fingerprint(&model, &[tool("read_file", "Read a file", schema_a)]);
    let second = fingerprint(&model, &[tool("read_file", "Read a file", schema_b)]);

    assert_eq!(first, second, "object insertion order is not semantic");
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

/// TIM-3: every variable input that changes request bytes changes the environment identity.
#[test]
fn tim_3_request_affecting_route_model_and_tool_inputs_break_the_fingerprint() {
    let base = model(BASE);
    let base_tools = vec![tool(
        "read_file",
        "Read a file",
        json!({"type":"object","properties":{"path":{"type":"string"}}}),
    )];
    let expected = fingerprint(&base, &base_tools);
    let variants = [
        BASE.replace("provider = \"local\"", "provider = \"remote\"")
            .replace("[providers.local]", "[providers.remote]")
            .replace(
                "[providers.local.models.luna]",
                "[providers.remote.models.luna]",
            ),
        BASE.replace("127.0.0.1:8317", "127.0.0.1:9418"),
        BASE.replace("PLEXMATON_LOCAL_API_KEY", "PLEXMATON_OTHER_API_KEY"),
        BASE.replace("openai_responses", "openai_chat_completions"),
        BASE.replace("gpt-5.6-luna", "gpt-5.6-sol"),
        BASE.replace(
            "reasoning_effort = \"xhigh\"",
            "reasoning_effort = \"high\"",
        ),
        BASE.replace("max_output_tokens = 128000", "max_output_tokens = 127999"),
    ];
    for variant in variants {
        assert_ne!(fingerprint(&model(&variant), &base_tools), expected);
    }

    for tools in [
        vec![tool(
            "search_files",
            "Read a file",
            base_tools[0].parameters().clone(),
        )],
        vec![tool(
            "read_file",
            "Read one file",
            base_tools[0].parameters().clone(),
        )],
        vec![tool(
            "read_file",
            "Read a file",
            json!({"type":"object","properties":{"path":{"type":"number"}}}),
        )],
        vec![
            base_tools[0].clone(),
            tool("search_files", "Search", json!({"type":"object"})),
        ],
    ] {
        assert_ne!(fingerprint(&base, &tools), expected);
    }
    assert_ne!(
        request_environment(&base, &base_tools, None).fingerprint(),
        request_environment(&base, &base_tools, Some(base.max_output_tokens())).fingerprint()
    );
}

/// TIM-4: budgeting, pricing, and presentation config do not perturb encoded-request identity.
#[test]
fn tim_4_non_request_model_metadata_preserves_the_fingerprint() {
    let base = model(BASE);
    let expected = fingerprint(&base, &[]);
    for variant in [
        BASE.replace("display_name = \"Luna\"", "display_name = \"Cheap Luna\""),
        BASE.replace(
            "context_window_tokens = 272000",
            "context_window_tokens = 300000",
        ),
        BASE.replace(
            "output_reserve_tokens = 16384",
            "output_reserve_tokens = 8192",
        ),
        BASE.replace(
            "output_reserve_tokens = 16384",
            "output_reserve_tokens = 16384\ncompaction_keep_recent_tokens = 24000",
        ),
        BASE.replace(
            "cost = { input = 0.2, output = 1.2, cache_read = 0.02, cache_write = 0.25 }",
            "cost = { input = 9.0, output = 9.0, cache_read = 9.0, cache_write = 9.0 }",
        ),
    ] {
        assert_eq!(fingerprint(&model(&variant), &[]), expected);
    }
}
