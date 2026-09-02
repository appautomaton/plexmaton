//! One step's tool calls, and the rule that none of them may be left unanswered.
//!
//! The batch is where a turn keeps what it owes. A model that asked for three calls has to be
//! shown three results before it will say anything else, so a call the loop dispatched and never
//! answered does not break the turn it happened in — it breaks the *next* request, one turn later
//! than the mistake, which is why the rule lives in a type rather than in a reviewer's memory.

use plexmaton_core::{ToolCallId, ToolCallStatus};

/// A call the model asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// It never finished, because the turn was interrupted first.
    Aborted,
}

impl ToolOutcome {
    /// The lifecycle state a projection should show for this outcome.
    #[must_use]
    pub fn status(&self) -> ToolCallStatus {
        match self {
            Self::Succeeded { .. } => ToolCallStatus::Succeeded,
            Self::Failed { .. } => ToolCallStatus::Failed,
            Self::Aborted => ToolCallStatus::Cancelled,
        }
    }
}

/// The calls one step dispatched, and their outcomes in the order the model asked for them.
///
/// Completion order is not model order. A batch keeps a slot per call so a result arriving second
/// is still shown second-to-last if that is where its call was, because reordering a batch teaches
/// the model that its own ordering means nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Batch {
    calls: Vec<ToolCall>,
    outcomes: Vec<Option<ToolOutcome>>,
}

impl Batch {
    /// Opens a batch over the calls one step produced, in the order it produced them.
    pub(crate) fn new(calls: Vec<ToolCall>) -> Self {
        let outcomes = vec![None; calls.len()];
        Self { calls, outcomes }
    }

    /// Records one outcome. `false` when no dispatched call has that identity, which is a defect
    /// in whoever ran it rather than something to answer the model with.
    pub(crate) fn settle(&mut self, call_id: &ToolCallId, outcome: ToolOutcome) -> bool {
        let Some(index) = self.calls.iter().position(|call| &call.call_id == call_id) else {
            return false;
        };
        let Some(slot) = self.outcomes.get_mut(index) else {
            return false;
        };
        *slot = Some(outcome);
        true
    }

    /// Whether every dispatched call has been answered.
    pub(crate) fn is_settled(&self) -> bool {
        self.outcomes.iter().all(Option::is_some)
    }

    /// Answers everything still outstanding as aborted, and says which those were.
    ///
    /// The turn is over either way; this is what keeps the conversation it leaves behind usable.
    pub(crate) fn abandon(&mut self) -> Vec<ToolCallId> {
        let mut abandoned = Vec::new();
        for (index, slot) in self.outcomes.iter_mut().enumerate() {
            if slot.is_some() {
                continue;
            }
            *slot = Some(ToolOutcome::Aborted);
            if let Some(call) = self.calls.get(index) {
                abandoned.push(call.call_id.clone());
            }
        }
        abandoned
    }

    /// Consumes the batch into call-and-outcome pairs in model order.
    ///
    /// Only a settled batch has pairs to give. An unsettled one yields nothing rather than a
    /// partial conversation, and the caller reaches this only through [`Self::abandon`].
    pub(crate) fn into_results(self) -> Vec<(ToolCall, ToolOutcome)> {
        self.calls
            .into_iter()
            .zip(self.outcomes)
            .filter_map(|(call, outcome)| outcome.map(|outcome| (call, outcome)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{ToolCallId, ToolCallStatus};

    use super::{Batch, ToolCall, ToolOutcome};

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

    fn ids(results: &[(ToolCall, ToolOutcome)]) -> Vec<String> {
        results
            .iter()
            .map(|(call, _)| call.call_id.to_string())
            .collect()
    }

    /// Finishing order is whatever the machine did; the model is shown its own order.
    #[test]
    fn results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished() {
        let mut batch = Batch::new(vec![call("one"), call("two"), call("three")]);

        assert!(batch.settle(&call("three").call_id, done("third")));
        assert!(batch.settle(&call("one").call_id, done("first")));
        assert!(!batch.is_settled(), "one call is still outstanding");
        assert!(batch.settle(&call("two").call_id, done("second")));
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
        let mut batch = Batch::new(vec![call("one"), call("two"), call("three")]);
        batch.settle(&call("two").call_id, done("kept"));

        let abandoned = batch.abandon();

        assert_eq!(
            abandoned
                .iter()
                .map(ToolCallId::to_string)
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
        let mut batch = Batch::new(vec![call("one")]);

        assert!(!batch.settle(&call("elsewhere").call_id, done("stray")));
        assert!(!batch.is_settled());
    }

    #[test]
    fn an_outcome_carries_the_lifecycle_state_a_projection_shows() {
        assert_eq!(done("x").status(), ToolCallStatus::Succeeded);
        assert_eq!(ToolOutcome::Aborted.status(), ToolCallStatus::Cancelled);
    }
}
