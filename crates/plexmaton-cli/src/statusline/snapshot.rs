//! Explicit allowlist for script stdin. Never serialize the journal, model config or budget atoms.

use plexmaton_agent::{RequestAttemptOwner, RequestAttemptTerminalState, RequestCost};
use plexmaton_core::TokenUsage;
use plexmaton_provider::ResolvedModel;
use plexmaton_runtime::LiveRuntime;
use serde::Serialize;

#[cfg(test)]
mod cache_tests;
mod context;
mod history;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod tests;
use context::Context;
use history::Issues;

#[derive(Serialize)]
pub(super) struct Snapshot<'a> {
    schema_version: u8,
    cwd: &'a str,
    workspace: Workspace<'a>,
    model: Model<'a>,
    effort: Effort<'a>,
    thinking: Thinking,
    session_id: Option<String>,
    context_window: ContextWindow,
    cost: Cost,
    plexmaton: Facts,
}

#[derive(Serialize)]
struct Workspace<'a> {
    current_dir: &'a str,
}
#[derive(Serialize)]
struct Model<'a> {
    id: &'a str,
    display_name: &'a str,
    provider: &'a str,
}
#[derive(Serialize)]
struct Effort<'a> {
    level: &'a str,
}
#[derive(Serialize)]
struct Thinking {
    enabled: Option<bool>,
}
#[derive(Serialize)]
struct Cost {
    total_cost_usd: Option<f64>,
}
#[derive(Serialize)]
struct ContextWindow {
    context_window_size: u32,
    used_percentage: Option<f64>,
    total_input_tokens: Option<u64>,
    total_output_tokens: Option<u64>,
    current_usage: Option<ClaudeUsage>,
}
#[derive(Serialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens: u64,
}

#[derive(Serialize)]
struct Facts {
    terminal: Dimensions,
    head: Option<String>,
    created_at_unix_ms: Option<u64>,
    usage: TokenUsage,
    cost: RequestCost,
    context: Context,
    latest_request: Option<Request>,
    turn: Option<Turn>,
    issues: Issues,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct Dimensions {
    pub columns: u16,
    pub rows: u16,
}
#[derive(Serialize)]
struct Request {
    id: String,
    owner: RequestAttemptOwner,
    terminal: Option<RequestAttemptTerminalState>,
}
#[derive(Serialize)]
struct Turn {
    id: String,
    usage: TokenUsage,
    cost: RequestCost,
    api_duration_ms: Option<u64>,
}

impl<'a> Snapshot<'a> {
    pub fn capture(
        runtime: &LiveRuntime,
        model: &'a ResolvedModel,
        cwd: &'a str,
        dimensions: Dimensions,
    ) -> Self {
        let mut result = Self::base(
            model,
            cwd,
            dimensions,
            Context::capture(runtime.context_budget()),
        );
        if let Some((journal, head)) = runtime.acknowledged_conversation() {
            result.enrich(journal, head);
        }
        result
    }

    fn base(
        model: &'a ResolvedModel,
        cwd: &'a str,
        dimensions: Dimensions,
        context: Context,
    ) -> Self {
        Self {
            schema_version: 1,
            cwd,
            workspace: Workspace { current_dir: cwd },
            model: Model {
                id: model.wire_id(),
                display_name: model.display_name(),
                provider: model.provider_name(),
            },
            effort: Effort {
                level: model.reasoning_effort().as_str(),
            },
            thinking: Thinking {
                enabled: match model.reasoning_effort() {
                    plexmaton_core::ReasoningEffort::Default => (model.api()
                        == plexmaton_provider::ModelApi::AnthropicMessages)
                        .then_some(true),
                    plexmaton_core::ReasoningEffort::None => Some(false),
                    _ => Some(true),
                },
            },
            session_id: None,
            context_window: ContextWindow {
                context_window_size: model.context_window_tokens(),
                used_percentage: match &context {
                    Context::Available { input_tokens, .. } => Some(
                        *input_tokens as f64 * 100.0 / f64::from(model.context_window_tokens()),
                    ),
                    _ => None,
                },
                total_input_tokens: None,
                total_output_tokens: None,
                current_usage: None,
            },
            cost: Cost {
                total_cost_usd: None,
            },
            plexmaton: Facts {
                terminal: dimensions,
                head: None,
                created_at_unix_ms: None,
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
                context,
                latest_request: None,
                turn: None,
                issues: Issues::default(),
            },
        }
    }
}

fn claude_usage(usage: &TokenUsage) -> Option<ClaudeUsage> {
    let counts = usage.counts()?;
    let uncached = counts
        .cached_input
        .zip(counts.cache_write_input)
        .and_then(|(read, write)| counts.input.checked_sub(read)?.checked_sub(write));
    Some(ClaudeUsage {
        input_tokens: uncached,
        cache_read_input_tokens: counts.cached_input,
        cache_creation_input_tokens: counts.cache_write_input,
        output_tokens: counts.output,
    })
}
