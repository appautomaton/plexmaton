//! Explicit allowlist for script stdin. Never serialize the journal, model config or budget atoms.

use plexmaton_agent::{
    JournalEntryPayload, RequestAttemptOwner, RequestAttemptTerminalState, RequestCost,
    USD_COST_TICKS_PER_DOLLAR,
};
use plexmaton_core::TokenUsage;
use plexmaton_provider::ResolvedModel;
use plexmaton_runtime::{ContextBudgetSnapshot, LiveRuntime};
use serde::Serialize;

#[cfg(test)]
mod tests;
use std::collections::BTreeSet;

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
    enabled: bool,
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
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct Dimensions {
    pub columns: u16,
    pub rows: u16,
}
#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
enum Context {
    Available {
        input_tokens: u64,
        output_reserve_tokens: u64,
        measured_prefix_tokens: Option<u64>,
        estimated_tokens: u64,
        opaque_replay_bytes: u64,
        estimator: &'static str,
    },
    Unavailable {
        reason: &'static str,
    },
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
    ) -> anyhow::Result<Self> {
        let context = match runtime.context_budget()? {
            ContextBudgetSnapshot::Available(ledger) => Context::Available {
                input_tokens: ledger.input_tokens,
                output_reserve_tokens: ledger.limits.output_reserve_tokens(),
                measured_prefix_tokens: ledger.anchor.as_ref().map(|anchor| anchor.input_tokens()),
                estimated_tokens: ledger.estimated_remainder.tokens,
                opaque_replay_bytes: ledger.estimated_remainder.opaque_replay_bytes,
                estimator: ledger.estimator.as_str(),
            },
            ContextBudgetSnapshot::Unavailable(reason) => Context::Unavailable {
                reason: match reason {
                    plexmaton_runtime::ContextBudgetUnavailable::ModelNotConfigured => {
                        "model_not_configured"
                    }
                    plexmaton_runtime::ContextBudgetUnavailable::PendingCommit => "pending_commit",
                    plexmaton_runtime::ContextBudgetUnavailable::PersistenceFailed => {
                        "persistence_failed"
                    }
                    plexmaton_runtime::ContextBudgetUnavailable::IncompleteToolBatch => {
                        "incomplete_tool_batch"
                    }
                },
            },
        };
        let mut result = Self::base(model, cwd, dimensions, context);
        if let Some((journal, head)) = runtime.acknowledged_session() {
            result.enrich(journal, head)?;
        }
        Ok(result)
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
                enabled: model.reasoning_effort() != plexmaton_provider::ReasoningEffort::None,
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
            },
        }
    }

    fn enrich(
        &mut self,
        journal: &plexmaton_agent::SessionJournal,
        head: &plexmaton_core::HeadName,
    ) -> anyhow::Result<()> {
        let result = self;
        result.session_id = Some(journal.session_id().to_string());
        result.plexmaton.head = Some(head.to_string());
        result.plexmaton.created_at_unix_ms = Some(journal.created_at_unix_ms().get());
        let accounting = journal.incurred_accounting()?;
        if let Some(counts) = accounting.usage.counts() {
            result.context_window.total_input_tokens = Some(counts.input);
            result.context_window.total_output_tokens = Some(counts.output);
        }
        if let RequestCost::Known { usd_ticks } = accounting.cost {
            result.cost.total_cost_usd =
                Some(usd_ticks.get() as f64 / USD_COST_TICKS_PER_DOLLAR as f64);
        }
        result.plexmaton.usage = accounting.usage;
        result.plexmaton.cost = accounting.cost;
        let path = journal
            .path(head)
            .map_err(|_| anyhow::anyhow!("invalid selected session path"))?;
        let selected: BTreeSet<_> = path.iter().map(|entry| &entry.id).collect();
        if let Some(attempt) = journal
            .request_attempts()
            .filter(|attempt| {
                selected.contains(attempt.authorization().semantic_boundary())
                    && attempt.authorization().owner().agent_step().is_some()
            })
            .last()
        {
            let terminal = attempt
                .terminal()
                .map(|terminal| terminal.terminal().clone());
            if let Some(RequestAttemptTerminalState::Dispatched { usage, .. }) = &terminal {
                result.context_window.current_usage = claude_usage(usage);
            }
            result.plexmaton.latest_request = Some(Request {
                id: attempt.authorization().attempt_id().to_string(),
                owner: attempt.authorization().owner().clone(),
                terminal,
            });
        }
        if let Some(turn_id) = path.iter().rev().find_map(|entry| match &entry.payload {
            JournalEntryPayload::TurnStarted { turn_id, .. } => Some(turn_id),
            _ => None,
        }) {
            let accounting = journal.turn_accounting(turn_id)?;
            let api_duration_ms = journal
                .request_attempts()
                .filter(|attempt| {
                    attempt
                        .authorization()
                        .owner()
                        .agent_step()
                        .is_some_and(|step| step.turn_id() == turn_id)
                })
                .try_fold(0_u64, |sum, attempt| {
                    match attempt.terminal().map(|terminal| terminal.terminal()) {
                        Some(RequestAttemptTerminalState::Dispatched { timing, .. }) => {
                            sum.checked_add(timing.terminal_after_ms().get())
                        }
                        Some(RequestAttemptTerminalState::NotDispatched { .. }) => Some(sum),
                        None => None,
                    }
                });
            result.plexmaton.turn = Some(Turn {
                id: turn_id.to_string(),
                usage: accounting.usage,
                cost: accounting.cost,
                api_duration_ms,
            });
        }
        Ok(())
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
