//! What an assistant output still carries once its replay belongs to another model.
//!
//! PRV-4 makes the semantic record authority and replay an optimisation. So an output whose
//! sidecars a different model produced is not unencodable — it is an output that has to be spelled
//! from its blocks alone. This module is the one place that answers what survives, so four dialects
//! cannot drift into four answers.
//!
//! It is deliberately a pre-pass rather than a helper called inside each dialect's block loop. The
//! degraded arm then never has the semantic block in scope to reach for, which is what keeps a
//! dialect from re-deriving a shape only an exact replay can justify — Gemini's `thought` flag
//! without the signature that makes it legal, for instance.

use std::borrow::Cow;

use plexmaton_agent::{AssistantBlock, AssistantOutput, AssistantReplay, ToolCall};
use plexmaton_core::ToolCallId;

use crate::ResolvedModel;

const CALL_ID_DOMAIN: &[u8] = b"plexmaton.wire_call_id.sha256.v1";

/// The tightest call-id length any supported dialect imposes. Applying it everywhere costs the
/// looser dialects nothing and leaves one rule instead of four, so the call side and the result
/// side cannot disagree about an id and orphan a result.
const MAX_CALL_ID_LEN: usize = 40;

/// One block's semantic content, in the only two forms every dialect can spell without replay.
pub(crate) enum Carried<'a> {
    /// Visible assistant text. A finished thought demoted from reasoning arrives here too.
    Text(&'a str),
    /// The call itself, stripped of the dialect-private identity its sidecar held.
    Call(&'a ToolCall),
}

/// What `output` carries when `model` cannot use its replay, or `None` when the replay is its own.
///
/// PRV-3: a thought that finished is carried as text, because its content is real and the next
/// model can read it. A thought that was interrupted is dropped, because putting half a sentence in
/// the model's mouth as speech states something it never said. Having an attachment is what
/// separates the two, and the test is per block: an output can finish one thought and be cut off
/// during the next.
pub(crate) fn degraded<'a>(
    output: &'a AssistantOutput,
    model: &ResolvedModel,
) -> Option<Vec<Carried<'a>>> {
    let replay = output.replay()?;
    if !is_degraded(output, model) {
        return None;
    }
    Some(
        output
            .blocks()
            .iter()
            .enumerate()
            .filter_map(|(index, block)| match block {
                // Empty text is a block no dialect accepts and none of them mean anything by.
                AssistantBlock::Text { text, .. } => {
                    (!text.is_empty()).then_some(Carried::Text(text))
                }
                AssistantBlock::Reasoning { text, .. } => {
                    (!text.is_empty() && completed(replay, index)).then_some(Carried::Text(text))
                }
                AssistantBlock::ToolCall { call, .. } => Some(Carried::Call(call)),
                // A replay-only block was never anything but its sidecar.
                AssistantBlock::ReplayOnly { .. } => None,
                // A search the previous model ran. Its answer text carries what came of it, and
                // the next model did not ask for the query.
                AssistantBlock::ServerToolCall { .. } => None,
            })
            .collect(),
    )
}

/// Whether any reply in `request` carries replay `model` cannot use, and would therefore reach it
/// as text rather than as the thought the other model actually had.
///
/// Model selection asks this to say what a switch cost, so the answer is about the whole projection
/// rather than one reply.
#[must_use]
pub fn degrades_replay(model: &ResolvedModel, request: &plexmaton_agent::ModelRequest) -> bool {
    request.atoms.iter().any(|atom| {
        let output = match atom.value() {
            plexmaton_agent::ContextAtomValue::Assistant(output) => output,
            plexmaton_agent::ContextAtomValue::ToolBatch(batch) => batch.assistant(),
            _ => return false,
        };
        is_degraded(output, model)
    })
}

/// Whether this model has to spell `output` without its replay. Allocates nothing, so the result
/// side of a batch can ask the same question the call side already answered.
pub(crate) fn is_degraded(output: &AssistantOutput, model: &ResolvedModel) -> bool {
    output
        .replay()
        .is_some_and(|replay| replay.compatible_with() != &model.replay_compatibility())
}

/// The id one atom puts on the wire, for a call and for the result that answers it.
///
/// Both sides of a batch call this, with the same `degraded`, so they cannot disagree and leave a
/// result pointing at a call the request no longer contains.
pub(crate) fn atom_call_id(degraded: bool, id: &ToolCallId) -> Cow<'_, str> {
    if degraded {
        wire_call_id(id)
    } else {
        Cow::Borrowed(id.as_str())
    }
}

/// An id the destination will accept, for a call whose id another provider issued.
///
/// Deterministic, so independent evaluation at the two sites agrees. Ids are unique within an
/// output and the digest keeps them so. Rejected: carrying the original through and letting the
/// provider reject the request, which trades a refusal the user can act on for a failure one turn
/// later.
fn wire_call_id(id: &ToolCallId) -> Cow<'_, str> {
    let original = id.as_str();
    if original.len() <= MAX_CALL_ID_LEN
        && original
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Cow::Borrowed(original);
    }
    let digest = crate::environment::identity_key(CALL_ID_DOMAIN, original);
    let mut wire = String::with_capacity(5 + 32);
    wire.push_str("call_");
    wire.push_str(&digest[..32]);
    Cow::Owned(wire)
}

fn completed(replay: &AssistantReplay, block: usize) -> bool {
    replay
        .attachments()
        .iter()
        .any(|part| usize::from(part.block()) == block)
}

#[cfg(test)]
mod tests;
