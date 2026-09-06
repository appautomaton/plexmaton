//! Bounded semantic descriptors and collected outcomes for context compaction.

use plexmaton_core::{HeadName, SessionEntryId};
use serde::{Deserialize, Serialize};

use crate::{
    AssistantBlock, AssistantOutput, CompactionId, HeadRevision, RequestAttemptId,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestDispatchedOutcome,
    RequestEnvironment, StopReason,
};

/// Why one live compaction staging operation changed no agent or journal state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompactionRefusal {
    NoActiveStep,
    WrongStep { expected: crate::ModelStepId },
    Journal(crate::JournalError),
}

/// Largest summary text that one checkpoint may inject into model context.
pub const MAX_COMPACTION_SUMMARY_BYTES: usize = 64 * 1024;
const CHECKPOINT_VERSION: u16 = 1;

/// Context base selected by one branch's ancestry (CPL-5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "checkpoint", rename_all = "snake_case")]
pub enum ContextEpoch {
    Original,
    Checkpoint(SessionEntryId),
}

/// Frozen branch state from which one pure compaction plan was prepared (CPL-1).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSource {
    head: HeadName,
    head_revision: HeadRevision,
    boundary: SessionEntryId,
    epoch: ContextEpoch,
}

impl CompactionSource {
    #[must_use]
    pub const fn new(
        head: HeadName,
        head_revision: HeadRevision,
        boundary: SessionEntryId,
        epoch: ContextEpoch,
    ) -> Self {
        Self {
            head,
            head_revision,
            boundary,
            epoch,
        }
    }

    #[must_use]
    pub const fn head(&self) -> &HeadName {
        &self.head
    }

    #[must_use]
    pub const fn head_revision(&self) -> HeadRevision {
        self.head_revision
    }

    #[must_use]
    pub const fn boundary(&self) -> &SessionEntryId {
        &self.boundary
    }

    #[must_use]
    pub const fn epoch(&self) -> &ContextEpoch {
        &self.epoch
    }
}

/// Stable whole-atom range replaced by one checkpoint (CPL-1, CPL-3).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionCut {
    first_covered: SessionEntryId,
    last_covered: SessionEntryId,
    first_retained: Option<SessionEntryId>,
    pinned_user: Option<SessionEntryId>,
}

impl CompactionCut {
    #[must_use]
    pub const fn new(
        first_covered: SessionEntryId,
        last_covered: SessionEntryId,
        first_retained: Option<SessionEntryId>,
        pinned_user: Option<SessionEntryId>,
    ) -> Self {
        Self {
            first_covered,
            last_covered,
            first_retained,
            pinned_user,
        }
    }

    #[must_use]
    pub const fn first_covered(&self) -> &SessionEntryId {
        &self.first_covered
    }

    #[must_use]
    pub const fn last_covered(&self) -> &SessionEntryId {
        &self.last_covered
    }

    #[must_use]
    pub const fn first_retained(&self) -> Option<&SessionEntryId> {
        self.first_retained.as_ref()
    }

    #[must_use]
    pub const fn pinned_user(&self) -> Option<&SessionEntryId> {
        self.pinned_user.as_ref()
    }
}

/// Pure, bounded descriptor persisted when its source is successfully summarized.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompactionPlan {
    id: CompactionId,
    source: CompactionSource,
    cut: CompactionCut,
    environment: RequestEnvironment,
    max_summary_bytes: usize,
}

impl CompactionPlan {
    pub fn new(
        id: CompactionId,
        source: CompactionSource,
        cut: CompactionCut,
        environment: RequestEnvironment,
        max_summary_bytes: usize,
    ) -> Result<Self, CompactionPlanError> {
        if max_summary_bytes == 0 {
            return Err(CompactionPlanError::EmptySummaryBudget);
        }
        if max_summary_bytes > MAX_COMPACTION_SUMMARY_BYTES {
            return Err(CompactionPlanError::SummaryBudgetTooLarge);
        }
        Ok(Self {
            id,
            source,
            cut,
            environment,
            max_summary_bytes,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &CompactionId {
        &self.id
    }

    #[must_use]
    pub const fn source(&self) -> &CompactionSource {
        &self.source
    }

    #[must_use]
    pub const fn cut(&self) -> &CompactionCut {
        &self.cut
    }

    #[must_use]
    pub const fn environment(&self) -> &RequestEnvironment {
        &self.environment
    }

    #[must_use]
    pub const fn max_summary_bytes(&self) -> usize {
        self.max_summary_bytes
    }
}

impl<'de> Deserialize<'de> for CompactionPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            id: CompactionId,
            source: CompactionSource,
            cut: CompactionCut,
            environment: RequestEnvironment,
            max_summary_bytes: usize,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.source,
            wire.cut,
            wire.environment,
            wire.max_summary_bytes,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Why a semantic compaction descriptor or collected result is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionPlanError {
    EmptySummaryBudget,
    SummaryBudgetTooLarge,
    EmptySummary,
    SummaryTooLarge,
    ToolCallsInCompleteOutput,
    CompleteOutputWithoutSuccessfulTerminal,
    OutputWithoutDispatch,
    UnsupportedCheckpointVersion,
}

impl std::fmt::Display for CompactionPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptySummaryBudget => "compaction summary budget must not be zero",
            Self::SummaryBudgetTooLarge => "compaction summary budget exceeds 64 KiB",
            Self::EmptySummary => "completed compaction output contains no summary text",
            Self::SummaryTooLarge => "completed compaction summary exceeds 64 KiB",
            Self::ToolCallsInCompleteOutput => "completed compaction output contains a tool call",
            Self::CompleteOutputWithoutSuccessfulTerminal => {
                "completed compaction output requires a successful request terminal"
            }
            Self::OutputWithoutDispatch => {
                "compaction output cannot exist before the request dispatch boundary"
            }
            Self::UnsupportedCheckpointVersion => "unsupported compaction checkpoint version",
        })
    }
}

impl std::error::Error for CompactionPlanError {}

/// Durable marker that a summarizer attempt received the exact projected request (CPL-2).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompactionInputMode {
    Verbatim,
}

/// Typed reason collected summarizer output cannot publish a checkpoint (CPL-6, CPL-8).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionFailure {
    Cancelled,
    TimedOut,
    ContextTooLong,
    OutputLimit,
    EmptyOutput,
    OutputTooLarge,
    ToolCallOutput,
    Refused,
    TransportFailed,
    ProviderFailed,
    Malformed,
    Unavailable,
    NoProgress,
}

impl std::fmt::Display for CompactionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "compaction was cancelled",
            Self::TimedOut => "compaction timed out",
            Self::ContextTooLong => "compaction input exceeded the provider context",
            Self::OutputLimit => "compaction summary reached its output limit",
            Self::EmptyOutput => "compaction produced no summary text",
            Self::OutputTooLarge => "compaction summary exceeded its byte limit",
            Self::ToolCallOutput => "compaction requested a tool",
            Self::Refused => "compaction was refused",
            Self::TransportFailed => "compaction transport failed",
            Self::ProviderFailed => "compaction provider failed",
            Self::Malformed => "compaction output was malformed",
            Self::Unavailable => "compaction is unavailable",
            Self::NoProgress => "compaction would not reduce the selected context",
        })
    }
}

/// Full semantic output retained with one collected summarizer result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompactionOutcome {
    Complete {
        output: AssistantOutput,
    },
    Failed {
        kind: CompactionFailure,
        output: Option<AssistantOutput>,
    },
}

impl CompactionOutcome {
    #[must_use]
    pub const fn output(&self) -> Option<&AssistantOutput> {
        match self {
            Self::Complete { output } => Some(output),
            Self::Failed { output, .. } => output.as_ref(),
        }
    }

    #[must_use]
    pub const fn failure(&self) -> Option<CompactionFailure> {
        match self {
            Self::Complete { .. } => None,
            Self::Failed { kind, .. } => Some(*kind),
        }
    }
}

/// One non-advancing terminal audit plus its collected bounded model output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompactionAttemptFinished {
    terminal: RequestAttemptTerminal,
    input_mode: CompactionInputMode,
    outcome: CompactionOutcome,
}

impl CompactionAttemptFinished {
    pub fn new(
        terminal: RequestAttemptTerminal,
        input_mode: CompactionInputMode,
        outcome: CompactionOutcome,
    ) -> Result<Self, CompactionPlanError> {
        validate_complete_output(&terminal, &outcome)?;
        Ok(Self {
            terminal,
            input_mode,
            outcome,
        })
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &RequestAttemptId {
        self.terminal.attempt_id()
    }

    #[must_use]
    pub const fn terminal(&self) -> &RequestAttemptTerminal {
        &self.terminal
    }

    #[must_use]
    pub const fn input_mode(&self) -> &CompactionInputMode {
        &self.input_mode
    }

    #[must_use]
    pub const fn outcome(&self) -> &CompactionOutcome {
        &self.outcome
    }

    pub(crate) fn complete_summary_text(&self) -> Option<String> {
        let CompactionOutcome::Complete { output } = &self.outcome else {
            return None;
        };
        let mut summary = String::new();
        for block in output.blocks() {
            if let AssistantBlock::Text { text, .. } = block {
                summary.push_str(text);
            }
        }
        Some(summary)
    }

    pub(crate) fn validate(&self) -> Result<(), CompactionPlanError> {
        self.terminal
            .validate()
            .map_err(|_| CompactionPlanError::CompleteOutputWithoutSuccessfulTerminal)?;
        validate_complete_output(&self.terminal, &self.outcome)
    }
}

impl<'de> Deserialize<'de> for CompactionAttemptFinished {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            terminal: RequestAttemptTerminal,
            input_mode: CompactionInputMode,
            outcome: CompactionOutcome,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.terminal, wire.input_mode, wire.outcome).map_err(serde::de::Error::custom)
    }
}

/// Versioned pointer from one semantic checkpoint to its successful full audit output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompactionCheckpoint {
    version: u16,
    plan: CompactionPlan,
    successful_attempt_id: RequestAttemptId,
}

impl CompactionCheckpoint {
    #[must_use]
    pub const fn new(plan: CompactionPlan, successful_attempt_id: RequestAttemptId) -> Self {
        Self {
            version: CHECKPOINT_VERSION,
            plan,
            successful_attempt_id,
        }
    }

    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn plan(&self) -> &CompactionPlan {
        &self.plan
    }

    #[must_use]
    pub const fn successful_attempt_id(&self) -> &RequestAttemptId {
        &self.successful_attempt_id
    }
}

impl<'de> Deserialize<'de> for CompactionCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u16,
            plan: CompactionPlan,
            successful_attempt_id: RequestAttemptId,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.version != CHECKPOINT_VERSION {
            return Err(serde::de::Error::custom(
                CompactionPlanError::UnsupportedCheckpointVersion,
            ));
        }
        Ok(Self::new(wire.plan, wire.successful_attempt_id))
    }
}

fn validate_complete_output(
    terminal: &RequestAttemptTerminal,
    outcome: &CompactionOutcome,
) -> Result<(), CompactionPlanError> {
    if outcome.output().is_some()
        && matches!(
            terminal.terminal(),
            RequestAttemptTerminalState::NotDispatched { .. }
        )
    {
        return Err(CompactionPlanError::OutputWithoutDispatch);
    }
    let CompactionOutcome::Complete { output } = outcome else {
        return Ok(());
    };
    if !matches!(
        terminal.terminal(),
        RequestAttemptTerminalState::Dispatched {
            outcome: RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn,
            },
            ..
        }
    ) {
        return Err(CompactionPlanError::CompleteOutputWithoutSuccessfulTerminal);
    }
    if output.tool_calls().next().is_some() {
        return Err(CompactionPlanError::ToolCallsInCompleteOutput);
    }
    let mut summary_bytes = 0_usize;
    let mut has_summary_text = false;
    for block in output.blocks() {
        match block {
            AssistantBlock::Text { text, .. } => {
                summary_bytes = summary_bytes
                    .checked_add(text.len())
                    .ok_or(CompactionPlanError::SummaryTooLarge)?;
                has_summary_text |= !text.trim().is_empty();
            }
            AssistantBlock::Reasoning { .. } | AssistantBlock::ReplayOnly { .. } => {}
            AssistantBlock::ToolCall { .. } => unreachable!("tool calls were rejected above"),
        }
    }
    if !has_summary_text {
        return Err(CompactionPlanError::EmptySummary);
    }
    if summary_bytes > MAX_COMPACTION_SUMMARY_BYTES {
        return Err(CompactionPlanError::SummaryTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{HeadName, SessionEntryId, TokenUsage, TranscriptItemId};

    use super::*;
    use crate::test_support::{
        call_block, output, output_with_replay, replay, replay_compatibility, text_block,
    };
    use crate::{
        DispatchedRequestTiming, ElapsedMillis, JournalRecord, JournalSequence, RequestCost,
        RequestEnvironmentFingerprint, RequestNotDispatchedOutcome, ToolCall,
    };

    fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
        build(value.to_owned()).expect("fixture id")
    }

    fn terminal(stop_reason: StopReason) -> RequestAttemptTerminal {
        RequestAttemptTerminal::new(
            RequestAttemptId::new("attempt").expect("attempt id"),
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    crate::UnixMillis::EPOCH,
                    None,
                    None,
                    ElapsedMillis::new(1),
                )
                .expect("timing"),
                outcome: RequestDispatchedOutcome::Completed { stop_reason },
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
            },
        )
        .expect("terminal")
    }

    fn plan(max_summary_bytes: usize) -> Result<CompactionPlan, CompactionPlanError> {
        CompactionPlan::new(
            CompactionId::new("compact").expect("compaction id"),
            CompactionSource::new(
                id("main", HeadName::new),
                HeadRevision::new(2),
                id("boundary", SessionEntryId::new),
                ContextEpoch::Original,
            ),
            CompactionCut::new(
                id("first", SessionEntryId::new),
                id("last", SessionEntryId::new),
                None,
                Some(id("user", SessionEntryId::new)),
            ),
            RequestEnvironment::new(
                replay_compatibility(),
                RequestEnvironmentFingerprint::new([1; 32]),
            ),
            max_summary_bytes,
        )
    }

    /// CPL-1/CPL-3: the persisted descriptor rejects unbounded summary policy on construction and
    /// decoding while retaining its exact frozen source fields.
    #[test]
    fn plan_wire_is_bounded_and_round_trips_its_frozen_source() {
        assert_eq!(plan(0), Err(CompactionPlanError::EmptySummaryBudget));
        assert_eq!(
            plan(MAX_COMPACTION_SUMMARY_BYTES + 1),
            Err(CompactionPlanError::SummaryBudgetTooLarge)
        );
        let plan = plan(MAX_COMPACTION_SUMMARY_BYTES).expect("maximal plan");
        let encoded = serde_json::to_vec(&plan).expect("encode plan");
        let decoded: CompactionPlan = serde_json::from_slice(&encoded).expect("decode plan");
        assert_eq!(decoded, plan);
        let mut invalid: serde_json::Value = serde_json::from_slice(&encoded).expect("plan value");
        invalid["max_summary_bytes"] = serde_json::json!(MAX_COMPACTION_SUMMARY_BYTES + 1);
        assert!(serde_json::from_value::<CompactionPlan>(invalid).is_err());

        let checkpoint = CompactionCheckpoint::new(
            plan,
            RequestAttemptId::new("successful-attempt").expect("attempt id"),
        );
        let mut invalid: serde_json::Value =
            serde_json::to_value(checkpoint).expect("checkpoint value");
        invalid["version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<CompactionCheckpoint>(invalid).is_err());
    }

    /// CPL-2/CPL-6: exact input provenance and successful collected output are validated before
    /// they can become a journal fact; failed audits may retain valid partial output.
    #[test]
    fn collected_attempt_validation_distinguishes_success_from_partial_failure() {
        let complete = CompactionAttemptFinished::new(
            terminal(StopReason::EndOfTurn),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Complete {
                output: output(vec![text_block("summary", "bounded summary")]),
            },
        )
        .expect("complete attempt");
        let encoded = serde_json::to_vec(&complete).expect("encode attempt");
        assert_eq!(
            serde_json::from_slice::<CompactionAttemptFinished>(&encoded).expect("decode attempt"),
            complete
        );
        let mut whitespace_wire = serde_json::to_value(&complete).expect("attempt value");
        whitespace_wire["outcome"]["output"]["blocks"][0]["text"] = serde_json::json!(" \n\t");
        assert!(serde_json::from_value::<CompactionAttemptFinished>(whitespace_wire).is_err());

        let called = output(vec![call_block(
            "tool-item",
            ToolCall {
                call_id: id("call", plexmaton_core::ToolCallId::new),
                name: "read_file".to_owned(),
                arguments: "{}".to_owned(),
            },
        )]);
        assert_eq!(
            CompactionAttemptFinished::new(
                terminal(StopReason::EndOfTurn),
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete {
                    output: called.clone()
                }
            ),
            Err(CompactionPlanError::ToolCallsInCompleteOutput)
        );
        assert_eq!(
            CompactionAttemptFinished::new(
                terminal(StopReason::OutputLimit),
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete {
                    output: output(vec![text_block("truncated", "partial")])
                }
            ),
            Err(CompactionPlanError::CompleteOutputWithoutSuccessfulTerminal)
        );
        let failed = CompactionAttemptFinished::new(
            terminal(StopReason::EndOfTurn),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Failed {
                kind: CompactionFailure::ToolCallOutput,
                output: Some(called),
            },
        )
        .expect("failed audit with partial output");
        assert!(failed.outcome().output().is_some());
        assert_eq!(
            failed.outcome().failure(),
            Some(CompactionFailure::ToolCallOutput)
        );

        let not_dispatched = RequestAttemptTerminal::new(
            RequestAttemptId::new("not-dispatched").expect("attempt id"),
            RequestAttemptTerminalState::NotDispatched {
                outcome: RequestNotDispatchedOutcome::PreparationFailed,
            },
        )
        .expect("not-dispatched terminal");
        assert_eq!(
            CompactionAttemptFinished::new(
                not_dispatched,
                CompactionInputMode::Verbatim,
                CompactionOutcome::Failed {
                    kind: CompactionFailure::Unavailable,
                    output: Some(output(vec![text_block("impossible-output", "impossible")]))
                }
            ),
            Err(CompactionPlanError::OutputWithoutDispatch)
        );
        let mut invalid_wire = serde_json::to_value(&failed).expect("failed attempt value");
        invalid_wire["terminal"]["terminal"] = serde_json::json!({
            "state": "not_dispatched",
            "outcome": "preparation_failed"
        });
        assert!(serde_json::from_value::<CompactionAttemptFinished>(invalid_wire).is_err());

        let empty_text = AssistantOutput::new(
            vec![AssistantBlock::Reasoning {
                item_id: id("reasoning", TranscriptItemId::new),
                text: "private".to_owned(),
            }],
            None,
        )
        .expect("reasoning output");
        assert_eq!(
            CompactionAttemptFinished::new(
                terminal(StopReason::EndOfTurn),
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete { output: empty_text }
            ),
            Err(CompactionPlanError::EmptySummary)
        );
        assert_eq!(
            CompactionAttemptFinished::new(
                terminal(StopReason::EndOfTurn),
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete {
                    output: output(vec![text_block("whitespace", " \n\t")])
                }
            ),
            Err(CompactionPlanError::EmptySummary)
        );
    }

    /// CPL-3/CPL-6: publishable text obeys the checkpoint cap while the same valid provider output
    /// can remain intact as failure audit evidence.
    #[test]
    fn summary_cap_does_not_discard_failed_audit_output() {
        let large = output(vec![text_block(
            "large-summary",
            &"x".repeat(MAX_COMPACTION_SUMMARY_BYTES + 1),
        )]);
        assert_eq!(
            CompactionAttemptFinished::new(
                terminal(StopReason::EndOfTurn),
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete {
                    output: large.clone()
                }
            ),
            Err(CompactionPlanError::SummaryTooLarge)
        );
        let failed = CompactionAttemptFinished::new(
            terminal(StopReason::EndOfTurn),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Failed {
                kind: CompactionFailure::OutputTooLarge,
                output: Some(large.clone()),
            },
        )
        .expect("large failed output remains valid audit evidence");
        assert_eq!(failed.outcome().output(), Some(&large));
    }

    /// JRN-3/CPL-6: the additive terminal record round-trips full opaque output while Debug keeps
    /// provider replay bytes redacted.
    #[test]
    fn collected_attempt_record_round_trips_without_debugging_replay() {
        let secret = "opaque-compaction-secret";
        let fact = CompactionAttemptFinished::new(
            terminal(StopReason::EndOfTurn),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Complete {
                output: output_with_replay(
                    vec![text_block("summary", "summary")],
                    [(0, replay(secret))],
                ),
            },
        )
        .expect("collected attempt");
        let record = JournalRecord::CompactionAttemptFinished {
            sequence: JournalSequence::new(1),
            record_id: id("record", plexmaton_core::JournalRecordId::new),
            fact,
        };
        let encoded = serde_json::to_vec(&record).expect("encode record");
        assert!(String::from_utf8_lossy(&encoded).contains(secret));
        let decoded: JournalRecord = serde_json::from_slice(&encoded).expect("decode record");
        assert_eq!(decoded, record);
        assert!(!format!("{decoded:?}").contains(secret));
    }
}
