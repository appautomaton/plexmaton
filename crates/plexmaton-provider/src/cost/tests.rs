use plexmaton_agent::{RequestCost, UsdCostTicks};
use plexmaton_core::{TokenCounts, TokenUsage};

use super::request_cost;
use crate::{ModelRegistry, ResolvedModel};

fn model(cost: Option<&str>) -> ResolvedModel {
    let cost = cost.map_or(String::new(), |cost| format!("cost = {cost}"));
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "local", model = "luna" }}
[providers.local]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"
[providers.local.models.luna]
id = "gpt-5.6-luna"
reasoning_effort = "high"
context_window_tokens = 10000
max_output_tokens = 2000
output_reserve_tokens = 1000
{cost}
"#
    ))
    .unwrap_or_else(|error| panic!("model fixture: {error}"))
    .active_model()
    .clone()
}

fn complete() -> TokenUsage {
    TokenUsage::Complete(TokenCounts {
        input: 100,
        cached_input: Some(20),
        cache_write_input: Some(10),
        output: 30,
        reasoning_output: Some(12),
        total: 130,
    })
}

/// TIM-3: one complete report is priced once at the resolved attempt rates and fixed precision.
#[test]
fn tim_3_complete_usage_calculates_one_stable_fixed_point_cost() {
    let model = model(Some(
        "{ input = 0.2, output = 1.2, cache_read = 0.02, cache_write = 0.25 }",
    ));
    assert_eq!(
        request_cost(&model, &complete()),
        RequestCost::Known {
            usd_ticks: UsdCostTicks::new(529_000)
        }
    );
}

/// TIM-3/TIM-5: an unknown bill remains unavailable instead of becoming free or approximate.
#[test]
fn tim_3_missing_price_partial_usage_and_invalid_subsets_have_no_cost() {
    assert_eq!(
        request_cost(&model(None), &complete()),
        RequestCost::Unavailable
    );
    let priced = model(Some(
        "{ input = 0.2, output = 1.2, cache_read = 0.02, cache_write = 0.25 }",
    ));
    let partial = match complete() {
        TokenUsage::Complete(counts) => TokenUsage::Partial(counts),
        _ => unreachable!("fixture is complete"),
    };
    assert_eq!(request_cost(&priced, &partial), RequestCost::Unavailable);

    let invalid = TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(8),
        cache_write_input: Some(4),
        output: 1,
        reasoning_output: Some(0),
        total: 11,
    });
    assert_eq!(request_cost(&priced, &invalid), RequestCost::Unavailable);
}
