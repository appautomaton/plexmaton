use std::{ffi::OsStr, path::Path};

use super::{
    ConfigError, ModelApi, ModelRegistry, ReasoningEffort, TokenEstimator, resolve_api_key,
    resolve_home,
};
use crate::{DecodeLimits, encode_request};
use plexmaton_agent::ModelRequest;
use plexmaton_core::ConversationId;

const LOCAL_CONFIG: &str = r#"
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

[providers.local.models.sol]
id = "gpt-5.6-sol"
display_name = "Sol"
api = "openai_chat_completions"
reasoning_effort = "high"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 32768

[providers.local.models.sol.cost]
input = 0.2
output = 1.2
cache_read = 0.02
cache_write = 0.25
"#;

#[test]
fn prv_6_one_provider_resolves_two_exact_models_without_repeating_authority() {
    let registry = ModelRegistry::parse(LOCAL_CONFIG).expect("valid local registry");
    let luna = registry.active_model();
    let sol = registry.model("local", "sol").expect("resolved sol");

    assert_eq!(registry.active_selection().provider(), "local");
    assert_eq!(registry.active_selection().model(), "luna");
    assert!(registry.model("local", "Luna").is_none());
    assert_eq!(luna.provider_name(), "local");
    assert_eq!(luna.model_name(), "luna");
    assert_eq!(luna.api(), ModelApi::OpenaiResponses);
    assert_eq!(luna.wire_id(), "gpt-5.6-luna");
    assert_eq!(luna.display_name(), "Luna");
    assert_eq!(luna.reasoning_effort(), ReasoningEffort::Xhigh);
    assert_eq!(luna.context_window_tokens(), 272_000);
    assert_eq!(luna.max_output_tokens(), 128_000);
    assert_eq!(luna.output_reserve_tokens(), 16_384);
    assert_eq!(luna.compaction_keep_recent_tokens(), 20_000);
    assert_eq!(luna.token_estimator(), TokenEstimator::Utf8HeuristicV1);
    assert!(luna.cost().is_none(), "missing pricing is not free pricing");

    assert_eq!(sol.base_url(), luna.base_url());
    assert_eq!(sol.api_key_env(), luna.api_key_env());
    assert_eq!(sol.api(), ModelApi::OpenaiChatCompletions);
    let cost = sol.cost().expect("explicit sol pricing");
    assert_eq!(cost.input(), 0.2);
    assert_eq!(cost.output(), 1.2);
    assert_eq!(cost.cache_read(), 0.02);
    assert_eq!(cost.cache_write(), 0.25);
}

#[test]
fn prv_6_selection_and_every_model_fail_closed_before_network_work() {
    let missing_provider = LOCAL_CONFIG.replace("provider = \"local\"", "provider = \"missing\"");
    assert!(matches!(
        ModelRegistry::parse(&missing_provider),
        Err(ConfigError::UnknownActiveProvider(provider)) if provider == "missing"
    ));

    let missing_model = LOCAL_CONFIG.replace("model = \"luna\"", "model = \"missing\"");
    assert!(matches!(
        ModelRegistry::parse(&missing_model),
        Err(ConfigError::UnknownActiveModel { provider, model })
            if provider == "local" && model == "missing"
    ));

    for invalid in [
        LOCAL_CONFIG.replace(
            "context_window_tokens = 272000",
            "context_window_tokens = 0",
        ),
        LOCAL_CONFIG.replace("max_output_tokens = 128000", "max_output_tokens = 272000"),
        LOCAL_CONFIG.replace(
            "output_reserve_tokens = 16384",
            "output_reserve_tokens = 128001",
        ),
        LOCAL_CONFIG.replace(
            "output_reserve_tokens = 16384",
            "output_reserve_tokens = 16384\ncompaction_keep_recent_tokens = 0",
        ),
    ] {
        assert!(matches!(
            ModelRegistry::parse(&invalid),
            Err(ConfigError::InvalidTokenLimits { .. })
                | Err(ConfigError::InvalidRequestOption {
                    field: "compaction_keep_recent_tokens",
                    ..
                })
        ));
    }
    let invalid_cost = LOCAL_CONFIG.replace("input = 0.2", "input = -0.2");
    assert!(matches!(
        ModelRegistry::parse(&invalid_cost),
        Err(ConfigError::InvalidCost { field: "input", .. })
    ));

    let missing_api = LOCAL_CONFIG.replacen("api = \"openai_responses\"\n", "", 1);
    assert!(matches!(
        ModelRegistry::parse(&missing_api),
        Err(ConfigError::MissingModelApi { model, .. }) if model == "luna"
    ));

    let unknown_effort = LOCAL_CONFIG.replace(
        "reasoning_effort = \"xhigh\"",
        "reasoning_effort = \"super\"",
    );
    assert!(matches!(
        ModelRegistry::parse(&unknown_effort),
        Err(ConfigError::Toml)
    ));
}

/// PRV-6/TIM-4: retention policy defaults locally, validates, and never changes a wire request.
#[test]
fn compaction_keep_recent_tokens_is_configured_without_changing_request_bytes() {
    let default = ModelRegistry::parse(LOCAL_CONFIG).expect("default model");
    let override_source = LOCAL_CONFIG.replace(
        "output_reserve_tokens = 16384",
        "output_reserve_tokens = 16384\ncompaction_keep_recent_tokens = 24_000",
    );
    let overridden = ModelRegistry::parse(&override_source).expect("override model");
    assert_eq!(
        default.active_model().compaction_keep_recent_tokens(),
        20_000
    );
    assert_eq!(
        overridden.active_model().compaction_keep_recent_tokens(),
        24_000
    );
    let request = ModelRequest {
        session_id: ConversationId::new("config-retention-test").expect("session"),
        atoms: Vec::new(),
    };
    assert_eq!(
        encode_request(default.active_model(), &request, &[], Some(128_000))
            .expect("default request"),
        encode_request(overridden.active_model(), &request, &[], Some(128_000))
            .expect("override request"),
    );
}

#[test]
fn prv_6_inline_authority_and_legacy_profiles_are_not_a_second_config_path() {
    let legacy = r#"
active_provider = "local_luna"
[providers.local_luna]
kind = "openai_compatible"
protocol = "responses"
base_url = "http://127.0.0.1:8317/v1"
model = "gpt-5.6-luna"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
reasoning_effort = "xhigh"
"#;

    assert!(matches!(
        ModelRegistry::parse(legacy),
        Err(ConfigError::Toml)
    ));

    let inline_key = LOCAL_CONFIG.replace(
        "api_key_env = \"PLEXMATON_LOCAL_API_KEY\"",
        "api_key_env = \"PLEXMATON_LOCAL_API_KEY\"\napi_key = \"inline-secret\"",
    );
    let error = ModelRegistry::parse(&inline_key).expect_err("inline key must be rejected");
    assert!(matches!(&error, ConfigError::Toml));
    let diagnostics = format!("{error}\n{error:?}");
    assert!(!diagnostics.contains("inline-secret"));
}

#[test]
fn prv_7_model_config_cannot_widen_runtime_output_memory() {
    let configured_bound = LOCAL_CONFIG.replace(
        "output_reserve_tokens = 16384",
        "output_reserve_tokens = 16384\nmax_retained_output_bytes = 999999999",
    );
    assert!(matches!(
        ModelRegistry::parse(&configured_bound),
        Err(ConfigError::Toml)
    ));
    assert_eq!(
        DecodeLimits::production().max_retained_output_bytes,
        plexmaton_agent::MAX_ASSISTANT_TEXT_BYTES
    );
}

#[test]
fn prv_6_resolves_only_an_override_or_the_user_root() {
    assert_eq!(
        resolve_home(
            Some(OsStr::new(".local/plexmaton")),
            Some(Path::new("/users/ac"))
        )
        .expect("explicit development root"),
        Path::new(".local/plexmaton")
    );
    assert_eq!(
        resolve_home(None, Some(Path::new("/users/ac"))).expect("user root"),
        Path::new("/users/ac/.plexmaton")
    );
    assert!(matches!(
        resolve_home(None, None),
        Err(ConfigError::HomeUnavailable)
    ));
}

#[test]
fn prv_6_key_resolution_is_explicit_and_redacted() {
    let registry = ModelRegistry::parse(LOCAL_CONFIG).expect("valid local registry");
    let model = registry.active_model();
    assert!(matches!(
        resolve_api_key(model, None),
        Err(ConfigError::MissingApiKeyEnvironment(environment))
            if environment == "PLEXMATON_LOCAL_API_KEY"
    ));
    assert!(matches!(
        resolve_api_key(model, Some("not header safe".into())),
        Err(ConfigError::InvalidApiKeyValue(_))
    ));
    let key = resolve_api_key(model, Some("fixture-secret".into()))
        .unwrap_or_else(|error| panic!("resolve fixture key: {error}"));
    assert_eq!(key.expose(), "fixture-secret");
    assert_eq!(format!("{key:?}"), "ApiKey([REDACTED])");
}

#[test]
fn prv_6_resolution_rejects_unsafe_routes_without_echoing_them() {
    for unsafe_url in [
        "https://user:inline-secret@example.test/v1",
        "https://example.test/v1?key=inline-secret",
        "https://example.test/v1#inline-secret",
        "ftp://example.test/inline-secret",
        "not-a-url-inline-secret",
    ] {
        let source = LOCAL_CONFIG.replace("http://127.0.0.1:8317/v1", unsafe_url);
        let error = ModelRegistry::parse(&source).expect_err("unsafe route must be rejected");
        assert!(matches!(&error, ConfigError::InvalidBaseUrl(provider) if provider == "local"));
        let diagnostics = format!("{error}\n{error:?}");
        assert!(!diagnostics.contains("inline-secret"));
    }
}

#[test]
fn prv_3_replay_route_owner_encoding_is_unambiguous() {
    let source = |provider: &str, base_url: &str| {
        format!(
            r#"
active_model = {{ provider = "{provider}", model = "luna" }}
[providers."{provider}"]
base_url = "{base_url}"
api_key_env = "KEY"
api = "openai_responses"
[providers."{provider}".models.luna]
id = "gpt-5.6-luna"
reasoning_effort = "low"
context_window_tokens = 100
max_output_tokens = 20
output_reserve_tokens = 10
"#
        )
    };
    let first =
        ModelRegistry::parse(&source("a|b", "https://x")).expect("first delimiter-bearing route");
    let second = ModelRegistry::parse(&source("a", "https://x/b%7Chttps%3A%2F%2Fx"))
        .expect("second delimiter-bearing route");

    assert_ne!(
        first.active_model().replay_compatibility().owner(),
        second.active_model().replay_compatibility().owner()
    );
}

/// PRV-6: native options are checked against their dialect before an HTTP owner can exist.
#[test]
fn prv_6_native_thinking_options_are_explicit_and_validated() {
    let source = |api: &str, effort: &str, budget: Option<u32>| {
        let budget = budget.map_or(String::new(), |value| {
            format!("thinking_budget_tokens = {value}")
        });
        format!(
            r#"
active_model = {{ provider = "native", model = "model" }}
[providers.native]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "{api}"
[providers.native.models.model]
id = "fixture-model"
reasoning_effort = "{effort}"
context_window_tokens = 8192
max_output_tokens = 4096
output_reserve_tokens = 1024
{budget}
"#
        )
    };
    for (api, effort, budget) in [
        ("anthropic_messages", "default", None),
        ("anthropic_messages", "high", None),
        ("google_generate_content", "low", None),
        ("google_generate_content", "medium", None),
        ("google_generate_content", "high", None),
        ("google_generate_content", "default", None),
    ] {
        ModelRegistry::parse(&source(api, effort, budget)).expect("supported explicit options");
    }
    for (api, effort, budget) in [
        ("anthropic_messages", "minimal", None),
        ("anthropic_messages", "default", Some(1024)),
        ("anthropic_messages", "high", Some(1024)),
        ("anthropic_messages", "none", Some(1024)),
        ("anthropic_messages", "default", Some(4096)),
        ("google_generate_content", "minimal", None),
        ("google_generate_content", "none", None),
        ("google_generate_content", "none", Some(0)),
        ("google_generate_content", "default", Some(128)),
        ("google_generate_content", "max", None),
        ("google_generate_content", "high", Some(128)),
        ("openai_responses", "minimal", None),
        ("openai_chat_completions", "minimal", None),
        ("openai_responses", "high", Some(1024)),
    ] {
        assert!(ModelRegistry::parse(&source(api, effort, budget)).is_err());
    }
    assert!(
        ModelRegistry::parse(
            &source("google_generate_content", "default", None)
                .replace("fixture-model", "../other-endpoint")
        )
        .is_err()
    );
}

/// PRV-6: the documented four-dialect configuration resolves without hidden defaults or aliases.
#[test]
fn prv_6_provider_example_selects_each_documented_dialect() {
    let registry = ModelRegistry::parse(include_str!("../../../../examples/providers.toml"))
        .expect("provider example config");
    for (provider, name, api) in [
        ("openai", "responses", ModelApi::OpenaiResponses),
        ("proxy", "chat", ModelApi::OpenaiChatCompletions),
        ("anthropic", "messages", ModelApi::AnthropicMessages),
        ("google", "gemini", ModelApi::GoogleGenerateContent),
        ("cliproxy_gemini", "gemini", ModelApi::GoogleGenerateContent),
    ] {
        assert_eq!(
            registry
                .model(provider, name)
                .expect("documented model")
                .api(),
            api
        );
    }
}
