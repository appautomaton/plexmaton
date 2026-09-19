//! Codec-aware preparation of bounded compaction inputs.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, CompactionCut, CompactionId, CompactionPlan,
    CompactionPlanError, CompactionSource, ContextAtom, ConversationJournal,
    MAX_COMPACTION_SUMMARY_BYTES, ModelRequest, TokenEstimate, required_user_context,
};
use plexmaton_core::{ConversationEntryId, HeadName};
use thiserror::Error;

use crate::{
    BudgetedContext, ContextBudgetError, FunctionTool, ResolvedModel, budgeted_context,
    estimate_request,
};

// CPL-2: this instruction is appended after the complete frozen request. Putting it in the
// system instructions, replacing history, or changing tool configuration would break that prefix.
const SUMMARY_INSTRUCTION: &str = include_str!("compaction/prompt.md");

/// One complete frozen request extended with the compaction instruction (CPL-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionInput {
    request: ModelRequest,
}

impl CompactionInput {
    /// Wraps the complete summarizer request without a history-rewriting mode.
    #[must_use]
    pub const fn new(request: ModelRequest) -> Self {
        Self { request }
    }

    #[must_use]
    pub const fn request(&self) -> &ModelRequest {
        &self.request
    }
    #[must_use]
    pub fn into_request(self) -> ModelRequest {
        self.request
    }
}

/// One append-only summarizer request and its frozen checkpoint plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCompaction {
    plan: CompactionPlan,
    frozen_request: ModelRequest,
    input: CompactionInput,
}

impl PreparedCompaction {
    /// Frozen plan whose summarizer request this value owns.
    #[must_use]
    pub const fn plan(&self) -> &CompactionPlan {
        &self.plan
    }
    /// The only permitted input; overflow cannot select a rewritten history.
    #[must_use]
    pub const fn input(&self) -> &CompactionInput {
        &self.input
    }
}

/// Validated replacement occupancy and the source input it reduced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplacementFit {
    estimate: TokenEstimate,
    input_capacity: u64,
    reduced_from: u64,
}

impl ReplacementFit {
    /// Codec-derived replacement occupancy.
    #[must_use]
    pub const fn estimate(&self) -> TokenEstimate {
        self.estimate
    }
    /// Maximum input occupancy for the selected model.
    #[must_use]
    pub const fn input_capacity(&self) -> u64 {
        self.input_capacity
    }
    /// Codec-derived frozen request occupancy before replacement.
    #[must_use]
    pub const fn reduced_from(&self) -> u64 {
        self.reduced_from
    }
}

/// Typed refusal while preparing or validating compaction input.
#[derive(Debug, Error)]
pub enum CompactionPreparationError {
    #[error("compaction source could not be projected: {0:?}")]
    Source(plexmaton_agent::JournalError),
    #[error(transparent)]
    Budget(#[from] ContextBudgetError),
    #[error(transparent)]
    Plan(#[from] CompactionPlanError),
    #[error("the request environment alone exceeds available input capacity")]
    UnfittableEnvironment,
    #[error("the latest required user context exceeds available input capacity")]
    OversizedRequiredUser,
    #[error("the selected source cannot be reduced while retaining required context")]
    NoUsefulReduction,
    #[error("the conversation is no larger than the tail a checkpoint would keep")]
    WithinRetention,
    #[error("the complete history plus compaction instruction exceeds available input capacity")]
    NoFittingInput,
    #[error("the collected compaction summary exceeds its frozen byte allowance")]
    OutputTooLarge,
    #[error("the checkpoint replacement exceeds available input capacity")]
    UnfittableReplacement,
}

/// Freezes one journal source and appends the compaction instruction without changing its prefix.
pub fn plan_compaction(
    journal: &ConversationJournal,
    head: &HeadName,
    model: &ResolvedModel,
    tools: &[FunctionTool],
    collaboration: &plexmaton_agent::collaboration::ResolvedContext,
    id: CompactionId,
    retention: Retention,
) -> Result<PreparedCompaction, CompactionPreparationError> {
    let source = journal
        .compaction_source(head)
        .map_err(CompactionPreparationError::Source)?;
    let budgeted = budgeted_context(journal, head, model, tools, collaboration)?;
    prepare(source, budgeted, model, tools, id, retention)
}

/// Whether the configured retention window may answer that there is nothing to compact (CPL-3).
///
/// The window is a configured count compared against `utf8_heuristic_v1` estimates, so "this
/// conversation is not long enough" is a statement about two numbers this runtime chose, never a
/// measurement of the provider's tokenizer. A user who disagrees overrides it by name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Retention {
    /// A conversation that fits inside its retention window has no history in front to summarize.
    #[default]
    Honoured,
    /// Summarize the oldest history even then, because the user asked for it by name.
    Overridden,
}

/// Validates a concrete replacement against the frozen request using codec-derived occupancy.
pub fn validate_replacement(
    model: &ResolvedModel,
    tools: &[FunctionTool],
    frozen: &ModelRequest,
    replacement: &ModelRequest,
) -> Result<ReplacementFit, CompactionPreparationError> {
    let frozen = estimate_request(model, frozen, tools)?;
    let estimate = estimate_request(model, replacement, tools)?;
    let input_capacity = input_capacity(model);
    // Whether the replacement is smaller than what it replaces is not checked, for automatic work
    // as much as for a request: size is an estimate and quality is unread, so a rule made of
    // either discards a finished summary on evidence this runtime does not have. Automatic work
    // needs no exception either, because it cannot reach the case — pressure arrives at 80% of a
    // real model's capacity, where the covered prefix dwarfs any summary. A rule would have to
    // wait for a model whose whole usable window is about one retention tail.
    // Rejected: refusing a replacement that does not reduce the frozen request, which spent the
    // call, wrote the summary, discarded it, and reported an estimator's verdict as a failure.
    if estimate.tokens > input_capacity {
        return Err(CompactionPreparationError::UnfittableReplacement);
    }
    Ok(ReplacementFit {
        estimate,
        input_capacity,
        reduced_from: frozen.tokens,
    })
}

/// Previews the exact replacement a successful collected output would publish.
pub fn validate_compaction_output(
    prepared: &PreparedCompaction,
    model: &ResolvedModel,
    tools: &[FunctionTool],
    output: &AssistantOutput,
) -> Result<ModelRequest, CompactionPreparationError> {
    let summary = output
        .blocks()
        .iter()
        .try_fold(String::new(), |mut summary, block| {
            if let AssistantBlock::Text { text, .. } = block {
                let total = summary
                    .len()
                    .checked_add(text.len())
                    .ok_or(CompactionPreparationError::OutputTooLarge)?;
                if total > prepared.plan.max_summary_bytes() {
                    return Err(CompactionPreparationError::OutputTooLarge);
                }
                summary.push_str(text);
            }
            Ok(summary)
        })?;
    if summary.is_empty() {
        return Err(CompactionPreparationError::NoUsefulReduction);
    }
    let cut = prepared.plan.cut();
    let first = index_of(&prepared.frozen_request, cut.first_covered())
        .ok_or(CompactionPreparationError::NoUsefulReduction)?;
    let last = index_of(&prepared.frozen_request, cut.last_covered())
        .ok_or(CompactionPreparationError::NoUsefulReduction)?;
    let suffix_start = cut
        .first_retained()
        .and_then(|id| index_of(&prepared.frozen_request, id))
        .unwrap_or(prepared.frozen_request.atoms.len());
    if first != 0 || last.checked_add(1) != Some(suffix_start) {
        return Err(CompactionPreparationError::NoUsefulReduction);
    }
    let mut atoms = vec![ContextAtom::compaction_summary(
        cut.first_covered().clone(),
        summary,
    )];
    if let Some(pinned) = cut.pinned_user()
        && let Some(index) = index_of(&prepared.frozen_request, pinned)
    {
        let range = required_user_context(&prepared.frozen_request.atoms)
            .filter(|range| range.start == index)
            .ok_or(CompactionPreparationError::NoUsefulReduction)?;
        atoms.extend_from_slice(
            &prepared.frozen_request.atoms[range.start..range.end.min(suffix_start)],
        );
    }
    atoms.extend(
        prepared.frozen_request.atoms[suffix_start..]
            .iter()
            .cloned(),
    );
    let replacement = ModelRequest {
        session_id: prepared.frozen_request.session_id.clone(),
        atoms,
    };
    validate_replacement(model, tools, &prepared.frozen_request, &replacement)?;
    Ok(replacement)
}

fn prepare(
    source: CompactionSource,
    budgeted: BudgetedContext,
    model: &ResolvedModel,
    tools: &[FunctionTool],
    id: CompactionId,
    retention: Retention,
) -> Result<PreparedCompaction, CompactionPreparationError> {
    let capacity = input_capacity(model);
    let environment = budgeted.ledger.environment_estimate.tokens;
    if environment > capacity {
        return Err(CompactionPreparationError::UnfittableEnvironment);
    }
    let available = capacity - environment;
    let atoms = &budgeted.request.atoms;
    if atoms.len() < 2 {
        return Err(CompactionPreparationError::NoUsefulReduction);
    }
    let required_user = required_user_context(atoms);
    if let Some(range) = &required_user
        && required_user_is_oversized(
            model,
            tools,
            &budgeted.request,
            &atoms[range.clone()],
            capacity,
        )?
    {
        return Err(CompactionPreparationError::OversizedRequiredUser);
    }
    let suffix_limit = u64::from(model.compaction_keep_recent_tokens()).min(available / 4);
    // CPL-3's gate, and the only thing that decides whether the summarizer is asked at all: a
    // conversation no larger than the tail a checkpoint would keep has nothing in front of that
    // tail to summarize. Both numbers are this runtime's own — a configured count and
    // `utf8_heuristic_v1` estimates — so the gate is a default the user overrides by name, never
    // a measurement of the provider's tokenizer. Everything past it is unchanged by the override.
    if retention == Retention::Honoured && conversation_tokens(&budgeted)? <= suffix_limit {
        return Err(CompactionPreparationError::WithinRetention);
    }
    let suffix_start = select_suffix_start(&budgeted, suffix_limit, required_user.as_ref());
    if suffix_start == 0 {
        return Err(CompactionPreparationError::NoUsefulReduction);
    }
    // A cut whose covered prefix is entirely required user context replaces nothing: the pinned
    // atoms are re-attached after the summary, so publication would refuse it structurally. That
    // refusal belongs here, before a summarizer call, and it is about what the cut *is* rather
    // than how large its result turned out to be.
    if pinned_len(required_user.as_ref(), suffix_start) >= suffix_start {
        return Err(CompactionPreparationError::NoUsefulReduction);
    }
    let cut = CompactionCut::new(
        atom_id(&atoms[0]),
        atom_id(&atoms[suffix_start - 1]),
        atoms.get(suffix_start).map(atom_id),
        required_user
            .filter(|range| range.start < suffix_start)
            .map(|range| atom_id(&atoms[range.start])),
    );
    let max_summary_bytes = summary_cap(model, available)?;
    let plan = CompactionPlan::new(
        id,
        source,
        cut,
        budgeted.ledger.environment.clone(),
        max_summary_bytes,
    )?;
    let instruction_source = plan.source().boundary().clone();
    let instruction_cost =
        instruction_cost(model, tools, &budgeted.request, instruction_source.clone())?;
    let input_tokens = budgeted
        .ledger
        .input_tokens
        .checked_add(instruction_cost)
        .ok_or(CompactionPreparationError::Budget(
            ContextBudgetError::Arithmetic(plexmaton_agent::BudgetError::Overflow),
        ))?;
    if input_tokens > capacity {
        return Err(CompactionPreparationError::NoFittingInput);
    }
    let input = CompactionInput::new(append_instruction(&budgeted.request, instruction_source));
    Ok(PreparedCompaction {
        plan,
        frozen_request: budgeted.request,
        input,
    })
}

/// How much of a covered prefix is required user context the replacement re-attaches verbatim.
fn pinned_len(required_user: Option<&std::ops::Range<usize>>, suffix_start: usize) -> usize {
    required_user
        .filter(|range| range.start < suffix_start)
        .map_or(0, |range| {
            range.end.min(suffix_start).saturating_sub(range.start)
        })
}

/// What the conversation itself occupies, without the environment a checkpoint never replaces.
///
/// The per-atom estimates are the only per-message quantity that exists: a provider's reported
/// input covers one whole request and cannot be partitioned across atoms (BUD-2), so a retention
/// decision is always made against `utf8_heuristic_v1` whatever the provider has said.
fn conversation_tokens(budgeted: &BudgetedContext) -> Result<u64, CompactionPreparationError> {
    budgeted
        .ledger
        .atoms
        .iter()
        .try_fold(0_u64, |total, atom| total.checked_add(atom.estimate.tokens))
        .ok_or(CompactionPreparationError::Budget(
            ContextBudgetError::Arithmetic(plexmaton_agent::BudgetError::Overflow),
        ))
}

fn input_capacity(model: &ResolvedModel) -> u64 {
    u64::from(model.context_window_tokens() - model.output_reserve_tokens())
}

fn summary_cap(model: &ResolvedModel, available: u64) -> Result<usize, CompactionPreparationError> {
    let tokens = (available / 4).min(u64::from(model.output_reserve_tokens()));
    let bytes = tokens.checked_mul(4).ok_or({
        CompactionPreparationError::Budget(ContextBudgetError::Arithmetic(
            plexmaton_agent::BudgetError::Overflow,
        ))
    })?;
    Ok(bytes.min(MAX_COMPACTION_SUMMARY_BYTES as u64) as usize)
}

fn required_user_is_oversized(
    model: &ResolvedModel,
    tools: &[FunctionTool],
    request: &ModelRequest,
    user: &[ContextAtom],
    capacity: u64,
) -> Result<bool, CompactionPreparationError> {
    let request = ModelRequest {
        session_id: request.session_id.clone(),
        atoms: user.to_vec(),
    };
    Ok(estimate_request(model, &request, tools)?.tokens > capacity)
}

fn select_suffix_start(
    budgeted: &BudgetedContext,
    suffix_limit: u64,
    required_user: Option<&std::ops::Range<usize>>,
) -> usize {
    let mut retained = 0_u64;
    let mut start = budgeted.request.atoms.len();
    // Keep at least the oldest whole atom covered. Otherwise a small request produces a
    // checkpoint descriptor with no useful replacement even though every suffix atom fits.
    for index in (1..budgeted.request.atoms.len()).rev() {
        let estimate = budgeted.ledger.atoms[index].estimate.tokens;
        if required_user.is_some_and(|range| range.contains(&index))
            || retained
                .checked_add(estimate)
                .is_some_and(|total| total <= suffix_limit)
        {
            retained = retained.saturating_add(estimate);
            start = index;
        } else {
            break;
        }
    }
    start
}

fn append_instruction(request: &ModelRequest, source: ConversationEntryId) -> ModelRequest {
    let mut request = request.clone();
    request.atoms.push(ContextAtom::compaction_summary(
        source,
        SUMMARY_INSTRUCTION.to_owned(),
    ));
    request
}

fn instruction_cost(
    model: &ResolvedModel,
    tools: &[FunctionTool],
    request: &ModelRequest,
    source: ConversationEntryId,
) -> Result<u64, CompactionPreparationError> {
    let empty = ModelRequest {
        session_id: request.session_id.clone(),
        atoms: Vec::new(),
    };
    let environment = estimate_request(model, &empty, tools)?.tokens;
    let instruction = ModelRequest {
        session_id: request.session_id.clone(),
        atoms: vec![ContextAtom::compaction_summary(
            source,
            SUMMARY_INSTRUCTION.to_owned(),
        )],
    };
    estimate_request(model, &instruction, tools)?
        .tokens
        .checked_sub(environment)
        .ok_or(CompactionPreparationError::NoFittingInput)
}

fn atom_id(atom: &ContextAtom) -> ConversationEntryId {
    atom.source_entries()
        .first()
        .cloned()
        .unwrap_or_else(|| unreachable!("context atoms have a source"))
}

fn index_of(request: &ModelRequest, id: &ConversationEntryId) -> Option<usize> {
    request
        .atoms
        .iter()
        .position(|atom| atom.source_entries().first() == Some(id))
}

#[cfg(test)]
mod tests;
