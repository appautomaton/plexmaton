//! What crosses the loop's edge: what it is told, and what it asks for in return.
//!
//! Both directions are values. Nothing here can be performed by this crate, which is what keeps
//! the decision and the doing in different places — and what lets a whole turn be driven from a
//! script with no network, no clock and no terminal.

use plexmaton_core::{ApprovalDecision, ApprovalId, ConversationEventEnvelope, ToolCallId};

use crate::admission::{AdmissionOutcome, AdmissionRequest, AdmittedToolCall};
use crate::journal::JournalRecord;
use crate::model::{ModelCall, ModelError, ModelEvent, ModelStepId};
use crate::tools::ToolExecutionResult;
use crate::{RequestAttemptId, SkillActivation, TreeEditResult, TreeNavigationResult, UnixMillis};

/// Something the loop is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    /// The user submitted a message for this agent's next turn.
    Submitted {
        /// Exact text the user submitted.
        text: String,
    },
    /// The user submitted a message with separately loaded explicit skill context.
    SkillSubmitted {
        /// Exact text the user submitted.
        text: String,
        /// Exact skill content resolved for this turn.
        skill: SkillActivation,
    },
    /// The user amended the turn in flight, for its next step.
    Steered {
        /// Exact text the user submitted.
        text: String,
    },
    /// The user amended the turn with separately loaded explicit skill context.
    SkillSteered {
        /// Exact text the user submitted.
        text: String,
        /// Exact skill content resolved for the next step.
        skill: SkillActivation,
    },
    /// The model produced something.
    Streamed {
        /// Step that requested this exact event.
        step_id: ModelStepId,
        /// Ordered semantic provider output.
        event: ModelEvent,
    },
    /// The step failed before it could finish.
    Failed {
        /// Step whose owned provider operation failed.
        step_id: ModelStepId,
        /// Typed failure returned by the provider boundary.
        error: ModelError,
    },
    /// The trusted catalog answered one explicit admission effect.
    ToolAdmissionResolved(AdmissionOutcome),
    /// A dispatched tool call ended.
    ToolFinished {
        /// The call being answered.
        call_id: ToolCallId,
        /// Model-facing outcome plus the executor's bounded presentation fact.
        result: ToolExecutionResult,
    },
    /// The user answered one pending approval request.
    ApprovalDecided {
        /// Exact pending request being answered.
        approval_id: ApprovalId,
        /// Once-only decision APV-4 accepts.
        decision: ApprovalDecision,
    },
    /// The retained permission owner answered a remembered decision.
    PermissionPrepared(crate::PermissionPreparationOutcome),
    /// The owner published a changed policy; covered waiting calls may now pass ordinary policy.
    PermissionsChanged,
    /// The user asked the current turn to stop.
    Interrupted,
    /// The runtime began orderly shutdown.
    ShuttingDown,
}

/// Something the loop needs performed, and cannot perform itself.
#[derive(Debug, Eq, PartialEq)]
pub enum Effect {
    /// Ask the model, and feed what it says back in as [`Input::Streamed`].
    CallModel(ModelCall),
    /// Ask the trusted catalog to validate and canonicalize one raw model call.
    AdmitTool(AdmissionRequest),
    /// Run one admitted call, and feed the result back in as [`Input::ToolFinished`].
    RunTool {
        /// Immutable catalog-validated operation.
        call: AdmittedToolCall,
        /// Authority that the runtime rechecks immediately before dispatch.
        authorization: crate::ToolAuthorization,
    },
    /// Apply one remembered scope before the call can pass its Conversation audit boundary.
    PreparePermission(crate::PermissionPreparationRequest),
}

/// Why user input could not be claimed by the boundary it named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndeliveredReason {
    /// Steering named a current turn, but no turn was open.
    NoActiveTurn,
    /// The current turn ended without opening another step.
    TurnEnded,
    /// The user stopped the turn before its pending input was claimed.
    Interrupted,
    /// The step failed before its pending input was claimed.
    StepFailed,
    /// The turn spent its step budget before it could open another one.
    StepBudgetReached,
    /// The bounded input queue had no room for another entry.
    QueueFull,
    /// Explicit skill content could not be loaded before the input reached the agent.
    SkillUnavailable,
    /// The session journal did not accept the transition, so ownership returned before execution.
    PersistenceFailed,
    /// The runtime shut down before the named boundary opened.
    Shutdown,
    /// The user took it back before it was sent (IQU-4).
    Withdrawn,
}

/// Why a typed approval decision changed no pending call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecisionRefusal {
    /// No current turn has that request pending; it may be stale or belong elsewhere.
    NotPending,
    /// A previous decision is already being prepared for this exact request.
    Preparing,
    /// The selected offer is absent, foreign or no longer effective under current policy.
    PolicyChanged,
    /// The permission owner refused to prepare the requested scope.
    Permission(crate::PermissionChangeError),
}

/// A decision the loop did not apply, retaining its exact identity and action (APV-4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedApprovalDecision {
    /// Identity the caller attempted to resolve.
    pub approval_id: ApprovalId,
    /// Decision that was not applied.
    pub decision: ApprovalDecision,
    /// Typed refusal reason.
    pub reason: ApprovalDecisionRefusal,
    /// Fresh producer-issued offer after a refusal, if this call remains pending.
    pub current_offer: Option<plexmaton_core::RememberPermissionOffer>,
}

/// User input the loop did not deliver, retaining both its payload and the reason (LOOP-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndeliveredInput {
    /// Exact text the user submitted.
    pub text: String,
    /// Explicitly selected skill name whose binding must survive restoration, when present.
    pub skill: Option<String>,
    /// Why the boundary could not claim it.
    pub reason: UndeliveredReason,
}

/// Why output from an owned provider operation could not enter the turn record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelDeliveryRefusal {
    /// Provider usage belongs to a correlated request-attempt terminal (TIM-3).
    UsageRequiresAttemptTerminal,
    /// No model step is currently open.
    NoActiveStep,
    /// A different model step is open, so this output is stale or misrouted.
    WrongStep {
        /// Only identity the loop would currently accept.
        expected: ModelStepId,
    },
    /// Output names a stale or unrelated request attempt, even if its step identity is current.
    WrongAttempt {
        /// Attempt currently allowed to deliver output.
        expected: RequestAttemptId,
        /// Attempt carried by the refused output.
        received: RequestAttemptId,
    },
}

/// Correlated provider output the loop did not accept (LIVE-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndeliveredModelInput {
    /// Identity supplied by the runtime.
    pub step_id: ModelStepId,
    /// Typed reason it did not enter state.
    pub reason: ModelDeliveryRefusal,
}

/// Why one request-attempt audit transition changed no journal state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestAttemptRefusal {
    /// No model step currently owns a request authorization.
    NoActiveStep,
    /// A different model step is active, so the requested authorization is stale or misrouted.
    WrongStep {
        /// Only step that may be authorized now.
        expected: ModelStepId,
    },
    /// The canonical journal rejected the otherwise-correlated audit fact.
    Journal(crate::JournalError),
    /// The selected immutable attempt facts could not produce their cumulative UI projection.
    Projection(crate::JournalProjectionError),
}

impl UndeliveredInput {
    /// Returns ordinary user input without selected skill metadata.
    #[must_use]
    pub const fn new(text: String, reason: UndeliveredReason) -> Self {
        Self {
            text,
            skill: None,
            reason,
        }
    }

    /// Returns user input together with the exact explicitly selected skill name.
    #[must_use]
    pub const fn with_skill(
        text: String,
        skill: Option<String>,
        reason: UndeliveredReason,
    ) -> Self {
        Self {
            text,
            skill,
            reason,
        }
    }
}

/// One queued input removed by a transition, retaining its process-local arrival order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasedInput {
    order: u64,
    text: String,
    skill: Option<String>,
}

impl ReleasedInput {
    pub(crate) const fn new(order: u64, text: String, skill: Option<String>) -> Self {
        Self { order, text, skill }
    }

    /// Queue arrival order used when a failed transition returns mixed boundaries.
    #[must_use]
    pub const fn order(&self) -> u64 {
        self.order
    }

    /// Exact user text released by the transition.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Explicitly selected skill name carried with this released queue item, when present.
    #[must_use]
    pub fn skill(&self) -> Option<&str> {
        self.skill.as_deref()
    }
}

/// What one input produced.
#[derive(Debug, Eq, PartialEq)]
pub struct Reaction {
    observed_at: UnixMillis,
    /// Canonical mutations accepted by this transition, in append order (JRN-6).
    pub records: Vec<JournalRecord>,
    /// Queued user text removed by this transition, carrying original arrival order.
    /// The runtime returns it if the transition fails its durability boundary.
    pub released_inputs: Vec<ReleasedInput>,
    /// Events for the projection, numbered on this agent's one sequence.
    pub events: Vec<ConversationEventEnvelope>,
    /// A rare explicit branch selection replaces the UI projection after persistence acknowledgement.
    pub projection_reset: Option<Vec<ConversationEventEnvelope>>,
    /// Receipt for a pure conversation-tree navigation, published only after its records are acknowledged.
    pub tree_navigation: Option<TreeNavigationResult>,
    /// Metadata-only completion; the runtime publishes it after its journal record is acknowledged.
    pub tree_edit: Option<TreeEditResult>,
    /// Work for whoever owns the outside world.
    pub effects: Vec<Effect>,
    /// User input whose intended boundary cannot claim it. Ownership returns to the caller with
    /// the exact payload instead of leaving it in a queue that a later turn could misread.
    pub undelivered: Vec<UndeliveredInput>,
    /// Approval decisions that matched no pending request.
    pub unresolved_approvals: Vec<UnresolvedApprovalDecision>,
    /// Stale or post-cancellation provider output that changed no record or projection.
    pub undelivered_model: Vec<UndeliveredModelInput>,
}

impl Default for Reaction {
    fn default() -> Self {
        Self::at(UnixMillis::EPOCH)
    }
}

impl Reaction {
    pub(crate) const fn at(observed_at: UnixMillis) -> Self {
        Self {
            observed_at,
            records: Vec::new(),
            released_inputs: Vec::new(),
            events: Vec::new(),
            projection_reset: None,
            tree_navigation: None,
            tree_edit: None,
            effects: Vec::new(),
            undelivered: Vec::new(),
            unresolved_approvals: Vec::new(),
            undelivered_model: Vec::new(),
        }
    }

    pub(crate) const fn observed_at(&self) -> UnixMillis {
        self.observed_at
    }

    pub(crate) fn into_output(mut self) -> Self {
        self.observed_at = UnixMillis::EPOCH;
        self
    }
}
