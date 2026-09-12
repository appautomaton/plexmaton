//! Independent historical projections; failures never erase facts owned by another section.
use super::*;
use plexmaton_agent::{JournalEntryPayload, RequestAccountingError, USD_COST_TICKS_PER_DOLLAR};
use std::collections::BTreeSet;

#[derive(Default, Serialize)]
pub(super) struct Issues {
    #[serde(skip_serializing_if = "Option::is_none")]
    session_accounting: Option<AccountingIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_path: Option<PathIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_accounting: Option<AccountingIssue>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AccountingIssue {
    UsageOverflow,
    CostOverflow,
}

impl From<RequestAccountingError> for AccountingIssue {
    fn from(error: RequestAccountingError) -> Self {
        match error {
            RequestAccountingError::UsageOverflow { .. } => Self::UsageOverflow,
            RequestAccountingError::CostOverflow { .. } => Self::CostOverflow,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum PathIssue {
    InvalidSelectedPath,
}

impl Snapshot<'_> {
    pub(super) fn enrich(
        &mut self,
        journal: &plexmaton_agent::ConversationJournal,
        head: &plexmaton_core::HeadName,
    ) {
        let result = self;
        result.session_id = Some(journal.conversation_id().to_string());
        result.plexmaton.head = Some(head.to_string());
        result.plexmaton.created_at_unix_ms = Some(journal.created_at_unix_ms().get());
        result.session_accounting(journal);
        result.selected_path(journal, head);
    }

    fn session_accounting(&mut self, journal: &plexmaton_agent::ConversationJournal) {
        let result = self;
        let accounting = match journal.incurred_accounting() {
            Ok(accounting) => accounting,
            Err(error) => {
                result.plexmaton.issues.session_accounting = Some(error.into());
                return;
            }
        };
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
    }

    fn selected_path(
        &mut self,
        journal: &plexmaton_agent::ConversationJournal,
        head: &plexmaton_core::HeadName,
    ) {
        let result = self;
        let path = match journal.path(head) {
            Ok(path) => path,
            Err(_) => {
                result.plexmaton.issues.selected_path = Some(PathIssue::InvalidSelectedPath);
                return;
            }
        };
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
            JournalEntryPayload::TurnStarted { turn_id, .. }
            | JournalEntryPayload::TurnRetried { turn_id, .. }
            | JournalEntryPayload::CollaborationTurnStarted { turn_id, .. } => Some(turn_id),
            _ => None,
        }) {
            let (usage, cost) = match journal.turn_accounting(turn_id) {
                Ok(accounting) => (accounting.usage, accounting.cost),
                Err(error) => {
                    result.plexmaton.issues.turn_accounting = Some(error.into());
                    (TokenUsage::Unavailable, RequestCost::Unavailable)
                }
            };
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
                usage,
                cost,
                api_duration_ms,
            });
        }
    }
}
