//! One step's tool calls, and the rule that none of them may be left unanswered.
//!
//! The batch is where a turn keeps what it owes. A model that asked for three calls has to be
//! shown three results before it will say anything else, so a call the loop dispatched and never
//! answered does not break the turn it happened in — it breaks the *next* request, one turn later
//! than the mistake, which is why the rule lives in a type rather than in a reviewer's memory.

use plexmaton_core::{
    ApprovalDecision, ApprovalId, AttentionId, ToolCallId, ToolCallStatus, ToolDetail,
    ToolPresentation, TranscriptItemId, TurnId,
};
use serde::{Deserialize, Serialize};

use crate::admission::{AdmissionRefusal, AdmittedToolCall};

mod presentation;
pub(crate) use presentation::detail_fits_text_bound;
pub use presentation::{MAX_TOOL_PRESENTATION_TEXT_BYTES, ToolExecutionResult, bounded_tool_text};

/// A call the model asked for.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolCall {
    /// Identity the model used, and the result must answer.
    pub call_id: ToolCallId,
    /// Which tool. Routing, never a trust decision: what a call is allowed to do is its declared
    /// effect, and a name is a label the model chose.
    pub name: String,
    /// Arguments as the model wrote them, accumulated across deltas and parsed by whoever runs
    /// the tool. The loop does not read them, so it does not need a JSON parser to hold them.
    pub arguments: String,
}

/// How a call ended.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolOutcome {
    /// It ran and produced something, already bounded by whoever ran it.
    Succeeded {
        /// What the model is shown.
        output: String,
    },
    /// It ran and did not produce a usable result. The model is told, and the turn continues.
    Failed {
        /// What the model is shown instead of output.
        message: String,
    },
    /// Trusted admission refused the model request before it could run.
    AdmissionRefused {
        /// Typed catalog refusal.
        reason: AdmissionRefusal,
    },
    /// Policy forbade the call; approval cannot override this outcome.
    Forbidden,
    /// The user explicitly declined the admitted call.
    Denied,
    /// The call did not finish because its owning turn stopped.
    Cancelled {
        /// Transition that cancelled it.
        reason: ToolCancellationReason,
    },
}

/// Why a call was cancelled before producing an ordinary result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCancellationReason {
    /// The user interrupted the turn.
    Interrupted,
    /// The model step failed while calls were outstanding.
    StepFailed,
    /// The runtime began an orderly shutdown.
    Shutdown,
}

impl ToolOutcome {
    /// The lifecycle state a projection should show for this outcome.
    #[must_use]
    pub fn status(&self) -> ToolCallStatus {
        match self {
            Self::Succeeded { .. } => ToolCallStatus::Succeeded,
            Self::Denied => ToolCallStatus::Denied,
            Self::Cancelled { .. } => ToolCallStatus::Cancelled,
            Self::Failed { .. } | Self::AdmissionRefused { .. } | Self::Forbidden => {
                ToolCallStatus::Failed
            }
        }
    }
}

/// One admitted call parked for an explicit user decision (LOOP-5, APV-4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingApproval {
    approval_id: ApprovalId,
    attention_id: AttentionId,
    turn_id: TurnId,
    admitted: AdmittedToolCall,
}

impl PendingApproval {
    pub(crate) const fn new(
        approval_id: ApprovalId,
        attention_id: AttentionId,
        turn_id: TurnId,
        admitted: AdmittedToolCall,
    ) -> Self {
        Self {
            approval_id,
            attention_id,
            turn_id,
            admitted,
        }
    }

    /// Stable request identity a decision must echo.
    #[must_use]
    pub const fn approval_id(&self) -> &ApprovalId {
        &self.approval_id
    }

    /// Attention projection identity paired with this request.
    #[must_use]
    pub const fn attention_id(&self) -> &AttentionId {
        &self.attention_id
    }

    /// Turn that owns this request.
    #[must_use]
    pub const fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    /// Exact admitted call the decision controls.
    #[must_use]
    pub const fn admitted(&self) -> &AdmittedToolCall {
        &self.admitted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CallState {
    AwaitingAdmission,
    AwaitingApproval(PendingApproval),
    Running(AdmittedToolCall),
    Finished(ToolOutcome),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CallSlot {
    requested: ToolCall,
    state: CallState,
    entry_id: TranscriptItemId,
    entry_revision: u64,
    presentation: ToolPresentation,
}

pub(crate) enum ApprovalResolution {
    Run {
        attention_id: AttentionId,
        admitted: AdmittedToolCall,
    },
    Denied {
        attention_id: AttentionId,
        call_id: ToolCallId,
    },
}

pub(crate) struct AbandonedCall {
    pub(crate) call_id: ToolCallId,
    pub(crate) attention_id: Option<AttentionId>,
}

/// The calls one step dispatched, and their outcomes in the order the model asked for them.
///
/// Completion order is not model order. A batch keeps a slot per call so a result arriving second
/// is still shown second-to-last if that is where its call was, because reordering a batch teaches
/// the model that its own ordering means nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Batch {
    slots: Vec<CallSlot>,
}

impl Batch {
    /// Opens a batch over the calls one step produced, in the order it produced them.
    pub(crate) fn new(calls: Vec<(ToolCall, TranscriptItemId)>) -> Self {
        Self {
            slots: calls
                .into_iter()
                .map(|(requested, entry_id)| CallSlot {
                    requested,
                    state: CallState::AwaitingAdmission,
                    entry_id,
                    entry_revision: 0,
                    presentation: ToolPresentation::default(),
                })
                .collect(),
        }
    }

    /// Entry identity and revision paired with a call's current lifecycle state.
    pub(crate) fn entry(
        &self,
        call_id: &ToolCallId,
    ) -> Option<(&TranscriptItemId, u64, &ToolPresentation)> {
        self.slots
            .iter()
            .find(|slot| &slot.requested.call_id == call_id)
            .map(|slot| (&slot.entry_id, slot.entry_revision, &slot.presentation))
    }

    pub(crate) fn requested(&self, call_id: &ToolCallId) -> Option<&ToolCall> {
        self.slots
            .iter()
            .find(|slot| &slot.requested.call_id == call_id)
            .map(|slot| &slot.requested)
    }

    pub(crate) fn run(&mut self, admitted: AdmittedToolCall) -> bool {
        let call_id = &admitted.requested().call_id;
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| &slot.requested.call_id == call_id)
        else {
            return false;
        };
        if slot.requested != *admitted.requested()
            || !matches!(slot.state, CallState::AwaitingAdmission)
        {
            return false;
        }
        slot.presentation.invocation = admitted.invocation().cloned();
        slot.state = CallState::Running(admitted);
        slot.entry_revision = slot.entry_revision.saturating_add(1);
        true
    }

    pub(crate) fn await_approval(&mut self, pending: PendingApproval) -> bool {
        let admitted = pending.admitted();
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.requested.call_id == admitted.requested().call_id)
        else {
            return false;
        };
        if slot.requested != *admitted.requested()
            || !matches!(slot.state, CallState::AwaitingAdmission)
        {
            return false;
        }
        slot.presentation.invocation = admitted.invocation().cloned();
        slot.state = CallState::AwaitingApproval(pending);
        slot.entry_revision = slot.entry_revision.saturating_add(1);
        true
    }

    pub(crate) fn finish_before_run(
        &mut self,
        call_id: &ToolCallId,
        outcome: ToolOutcome,
        invocation: Option<ToolDetail>,
    ) -> bool {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| &slot.requested.call_id == call_id)
        else {
            return false;
        };
        if !matches!(slot.state, CallState::AwaitingAdmission) {
            return false;
        }
        slot.presentation.invocation = invocation;
        slot.presentation.outcome = presentation::unexecuted_outcome(&outcome);
        slot.state = CallState::Finished(outcome);
        slot.entry_revision = slot.entry_revision.saturating_add(1);
        true
    }

    pub(crate) fn resolve_approval(
        &mut self,
        approval_id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> Option<ApprovalResolution> {
        let slot = self.slots.iter_mut().find(|slot| {
            matches!(
                &slot.state,
                CallState::AwaitingApproval(pending)
                    if pending.approval_id() == approval_id
            )
        })?;
        let CallState::AwaitingApproval(pending) =
            std::mem::replace(&mut slot.state, CallState::Finished(ToolOutcome::Denied))
        else {
            return None;
        };
        let attention_id = pending.attention_id().clone();
        match decision {
            ApprovalDecision::AllowOnce => {
                let admitted = pending.admitted;
                slot.presentation.invocation = admitted.invocation().cloned();
                slot.state = CallState::Running(admitted.clone());
                slot.entry_revision = slot.entry_revision.saturating_add(1);
                Some(ApprovalResolution::Run {
                    attention_id,
                    admitted,
                })
            }
            ApprovalDecision::Deny => {
                slot.presentation.outcome = presentation::unexecuted_outcome(&ToolOutcome::Denied);
                slot.entry_revision = slot.entry_revision.saturating_add(1);
                Some(ApprovalResolution::Denied {
                    attention_id,
                    call_id: slot.requested.call_id.clone(),
                })
            }
        }
    }

    pub(crate) fn pending_approvals(&self) -> impl Iterator<Item = &PendingApproval> {
        self.slots.iter().filter_map(|slot| match &slot.state {
            CallState::AwaitingApproval(pending) => Some(pending),
            CallState::AwaitingAdmission | CallState::Running(_) | CallState::Finished(_) => None,
        })
    }

    /// Records one outcome. `false` when no dispatched call has that identity, which is a defect
    /// in whoever ran it rather than something to answer the model with.
    pub(crate) fn settle(&mut self, call_id: &ToolCallId, result: ToolExecutionResult) -> bool {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| &slot.requested.call_id == call_id)
        else {
            return false;
        };
        if !matches!(slot.state, CallState::Running(_)) {
            return false;
        }
        let (outcome, presentation) = result.into_parts();
        slot.presentation.outcome = presentation;
        slot.state = CallState::Finished(outcome);
        slot.entry_revision = slot.entry_revision.saturating_add(1);
        true
    }

    /// Whether every dispatched call has been answered.
    pub(crate) fn is_settled(&self) -> bool {
        self.slots
            .iter()
            .all(|slot| matches!(slot.state, CallState::Finished(_)))
    }

    /// Answers everything still outstanding as aborted, and says which those were.
    ///
    /// The turn is over either way; this is what keeps the conversation it leaves behind usable.
    pub(crate) fn abandon(&mut self, reason: ToolCancellationReason) -> Vec<AbandonedCall> {
        let mut abandoned = Vec::new();
        for slot in &mut self.slots {
            if matches!(slot.state, CallState::Finished(_)) {
                continue;
            }
            let attention_id = match &slot.state {
                CallState::AwaitingApproval(pending) => Some(pending.attention_id().clone()),
                CallState::AwaitingAdmission | CallState::Running(_) | CallState::Finished(_) => {
                    None
                }
            };
            let outcome = ToolOutcome::Cancelled { reason };
            slot.presentation.outcome = presentation::unexecuted_outcome(&outcome);
            slot.state = CallState::Finished(outcome);
            slot.entry_revision = slot.entry_revision.saturating_add(1);
            abandoned.push(AbandonedCall {
                call_id: slot.requested.call_id.clone(),
                attention_id,
            });
        }
        abandoned
    }

    /// Consumes the batch into call-and-outcome pairs in model order.
    ///
    /// Only a settled batch has pairs to give. An unsettled one yields nothing rather than a
    /// partial conversation, and the caller reaches this only through [`Self::abandon`].
    pub(crate) fn into_results(self) -> Vec<(ToolCall, ToolOutcome)> {
        self.slots
            .into_iter()
            .filter_map(|slot| match slot.state {
                CallState::Finished(outcome) => Some((slot.requested, outcome)),
                CallState::AwaitingAdmission
                | CallState::AwaitingApproval(_)
                | CallState::Running(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{ToolCallId, ToolCallStatus, ToolDetail};

    use super::{
        Batch, MAX_TOOL_PRESENTATION_TEXT_BYTES, ToolCall, ToolCancellationReason,
        ToolExecutionResult, ToolOutcome, bounded_tool_text,
    };

    /// ENT-4: bounded text preserves valid UTF-8 at both ends and counts both earlier and local
    /// omissions without asking the renderer to infer truncation.
    #[test]
    fn bounded_presentation_text_carries_exact_omission_metadata() {
        let source = format!("head-{}-tail", "λ".repeat(MAX_TOOL_PRESENTATION_TEXT_BYTES));
        let ToolDetail::Text {
            source: retained,
            omitted_bytes,
        } = bounded_tool_text(&source, 17)
        else {
            panic!("plain text presenter changed detail kind");
        };

        assert!(retained.len() <= MAX_TOOL_PRESENTATION_TEXT_BYTES);
        assert!(retained.starts_with("head-"));
        assert!(retained.ends_with("-tail"));
        let marker_start = retained
            .find("\n...[")
            .unwrap_or_else(|| panic!("omission marker"));
        let marker_end = retained[marker_start..]
            .find("]...\n")
            .map(|relative| marker_start + relative + "]...\n".len())
            .unwrap_or_else(|| panic!("omission marker end"));
        let retained_payload_bytes = retained.len() - (marker_end - marker_start);
        assert_eq!(
            omitted_bytes,
            17 + (source.len() - retained_payload_bytes) as u64
        );
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            call_id: ToolCallId::new(id).unwrap_or_else(|error| panic!("fixture: {error}")),
            name: "read".to_owned(),
            arguments: "{}".to_owned(),
        }
    }

    fn done(output: &str) -> ToolOutcome {
        ToolOutcome::Succeeded {
            output: output.to_owned(),
        }
    }

    fn result(output: &str) -> ToolExecutionResult {
        ToolExecutionResult::new(done(output), None)
    }

    fn ids(results: &[(ToolCall, ToolOutcome)]) -> Vec<String> {
        results
            .iter()
            .map(|(call, _)| call.call_id.to_string())
            .collect()
    }

    /// Finishing order is whatever the machine did; the model is shown its own order.
    #[test]
    fn results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished() {
        let mut batch = running_batch(vec![call("one"), call("two"), call("three")]);

        assert!(batch.settle(&call("three").call_id, result("third")));
        assert!(batch.settle(&call("one").call_id, result("first")));
        assert!(!batch.is_settled(), "one call is still outstanding");
        assert!(batch.settle(&call("two").call_id, result("second")));
        assert!(batch.is_settled());

        let results = batch.into_results();
        assert_eq!(ids(&results), ["one", "two", "three"]);
        assert_eq!(
            results.first().map(|(_, outcome)| outcome),
            Some(&done("first"))
        );
    }

    /// The debt rule: what an interrupt leaves behind is still a conversation.
    #[test]
    fn abandoning_answers_everything_outstanding_and_leaves_settled_calls_alone() {
        let mut batch = running_batch(vec![call("one"), call("two"), call("three")]);
        batch.settle(&call("two").call_id, result("kept"));

        let abandoned = batch.abandon(ToolCancellationReason::Interrupted);

        assert_eq!(
            abandoned
                .iter()
                .map(|call| call.call_id.to_string())
                .collect::<Vec<_>>(),
            ["one", "three"],
            "only the calls that had not answered"
        );
        assert!(batch.is_settled());
        let results = batch.into_results();
        assert_eq!(ids(&results), ["one", "two", "three"]);
        assert_eq!(
            results.get(1).map(|(_, outcome)| outcome),
            Some(&done("kept")),
            "a call that finished before the interrupt keeps what it produced"
        );
    }

    /// An outcome for a call this step never dispatched is a defect in the caller, and answering
    /// the model with it would put an identity in the conversation the model never used.
    #[test]
    fn an_outcome_for_a_call_that_was_never_dispatched_is_refused() {
        let mut batch = running_batch(vec![call("one")]);

        assert!(!batch.settle(&call("elsewhere").call_id, result("stray")));
        assert!(!batch.is_settled());
    }

    #[test]
    fn an_outcome_carries_the_lifecycle_state_a_projection_shows() {
        assert_eq!(done("x").status(), ToolCallStatus::Succeeded);
        assert_eq!(
            ToolOutcome::Cancelled {
                reason: ToolCancellationReason::Interrupted
            }
            .status(),
            ToolCallStatus::Cancelled
        );
    }

    fn running_batch(calls: Vec<ToolCall>) -> Batch {
        use plexmaton_core::{ToolCapability, ToolDefinitionId, TranscriptItemId};

        use crate::{AdmittedToolCall, ToolDefinitionRevision};

        let entries = calls
            .iter()
            .enumerate()
            .map(|(index, call)| {
                (
                    call.clone(),
                    TranscriptItemId::new(format!("tool-entry-{index}"))
                        .unwrap_or_else(|error| panic!("fixture: {error}")),
                )
            })
            .collect();
        let mut batch = Batch::new(entries);
        for call in calls {
            let admitted = AdmittedToolCall::new(
                call,
                ToolDefinitionId::new("fixture").unwrap_or_else(|error| panic!("fixture: {error}")),
                ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
                [ToolCapability::FileRead],
                "{}".to_owned(),
                "fixture".to_owned(),
                None,
            )
            .unwrap_or_else(|error| panic!("fixture: {error:?}"));
            assert!(batch.run(admitted));
        }
        batch
    }
}
