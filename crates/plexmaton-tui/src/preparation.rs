//! Palette-neutral text preparation at the owned worker boundary. No runtime or terminal I/O.

use plexmaton_core::{AgentId, TranscriptItemId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{TranscriptEntryView, content, state::EntryAppearance, text_layout::Layout};

/// Maximum retained allocation in one prepared entry, independent of its wire encoding.
pub const MAX_PREPARED_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_KEY_BYTES: usize = 4096;
/// Maximum number of source snapshots in one owned preparation batch.
pub const MAX_BATCH_ITEMS: usize = 16;
/// Maximum accounted prepared allocation admitted as one completion.
pub const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;

/// A complete batch did not fit. The owner may retry a smaller batch, never truncate entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BatchRefusal {
    /// The entry count or aggregate prepared allocation exceeded the batch bound.
    Capacity,
}

/// Pure worker operation, also used by explicit offline fixtures. Input must already be bounded;
/// interactive drawing and input handling must only declare requests, never call this function.
pub fn prepare_batch(requests: &[Request]) -> Result<Vec<PreparedText>, BatchRefusal> {
    if requests.is_empty() || requests.len() > MAX_BATCH_ITEMS {
        return Err(BatchRefusal::Capacity);
    }
    let mut results = Vec::with_capacity(requests.len());
    let mut retained = 0;
    for request in requests {
        let prepared = request.prepare();
        retained += prepared.allocation_bytes();
        if retained > MAX_BATCH_BYTES {
            return Err(BatchRefusal::Capacity);
        }
        results.push(prepared);
    }
    Ok(results)
}

/// A workspace-local request identity. Holding it keeps the originating generation alive, so a
/// replacement workspace cannot reuse its identity even when semantic item IDs are identical.
#[derive(Clone, Debug)]
pub struct Token {
    pub(crate) generation: Arc<()>,
    pub(crate) sequence: u64,
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.generation, &other.generation) && self.sequence == other.sequence
    }
}
impl Eq for Token {}

/// A bounded source snapshot to hand to the owned preparation process. Never execute it in draw.
#[derive(Debug)]
pub struct Work {
    /// Return this unchanged on completion; it is not part of the child protocol.
    pub token: Token,
    /// At most sixteen reached entries, without queuing cloned history.
    pub requests: Vec<Request>,
}

/// A projection identity; a process ticket additionally distinguishes workspace replacements.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub(crate) agent: AgentId,
    pub(crate) item: TranscriptItemId,
    pub(crate) revision: u64,
    pub(crate) width: u16,
    pub(crate) open: bool,
    pub(crate) math: crate::math::MathPresentation,
}

impl Key {
    pub(crate) fn fits(agent: &AgentId, entry: &TranscriptEntryView) -> bool {
        size_of::<Self>() + agent.as_str().len() + entry.id().as_str().len() <= MAX_KEY_BYTES
    }
    pub(crate) fn new(
        agent: &AgentId,
        entry: &TranscriptEntryView,
        width: u16,
        open: bool,
    ) -> Self {
        Self {
            agent: agent.clone(),
            item: entry.id().clone(),
            revision: entry.revision(),
            width,
            open,
            math: crate::math::MathPresentation::default(),
        }
    }

    pub(crate) fn with_math(mut self, math: crate::math::MathPresentation) -> Self {
        self.math = math;
        self
    }

    pub(crate) fn matches(
        &self,
        agent: &AgentId,
        entry: &TranscriptEntryView,
        width: u16,
        open: bool,
    ) -> bool {
        &self.agent == agent
            && &self.item == entry.id()
            && self.revision == entry.revision()
            && self.width == width
            && self.open == open
    }

    pub(crate) fn same_geometry(&self, other: &Self) -> bool {
        self.agent == other.agent
            && self.item == other.item
            && self.width == other.width
            && self.open == other.open
            && self.math == other.math
    }

    pub(crate) fn allocation_bytes(&self) -> usize {
        size_of::<Self>() + self.agent.as_str().len() + self.item.as_str().len()
    }
}

/// One immutable source snapshot, shared by ordinary text, Markdown, tools, artifacts and mail.
/// The transport must bound encoded input before invoking synchronous preparation.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    key: Key,
    entry: TranscriptEntryView,
    prefix: Option<crate::markdown::PrefixHint>,
}

impl Request {
    /// Takes ownership of one snapshot; never consults session state during preparation.
    pub fn new(agent: AgentId, entry: TranscriptEntryView, width: u16, open: bool) -> Self {
        Self {
            key: Key {
                agent,
                item: entry.id().clone(),
                revision: entry.revision(),
                width,
                open,
                math: crate::math::MathPresentation::default(),
            },
            entry,
            prefix: None,
        }
    }

    /// Select the output owner's measured math capability before preparing this snapshot.
    #[must_use]
    pub fn with_math(mut self, math: crate::math::MathPresentation) -> Self {
        self.key.math = math;
        self
    }

    /// Supply a bounded, parser-checked prefix from the presentation cache. The child still
    /// receives the complete source and validates the checkpoint before rendering its tail.
    pub(crate) fn with_prefix(mut self, prefix: crate::markdown::PrefixHint) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Drop an optional streaming hint before a bounded wire retry; the complete source remains.
    pub fn discard_prefix(&mut self) {
        self.prefix = None;
    }

    /// Identity checked before worker output is admitted to a retained cache.
    pub const fn key(&self) -> &Key {
        &self.key
    }

    /// Performs synchronous pure preparation. Only the process worker may call this in production.
    #[must_use]
    pub fn prepare(&self) -> PreparedText {
        let result = if !Key::fits(&self.key.agent, &self.entry) {
            Err(Refusal::Capacity)
        } else if self.key.item != *self.entry.id()
            || self.key.revision != self.entry.revision()
            || self.key.width == 0
            || self.key.width > 4096
        {
            Err(Refusal::InvalidRequest)
        } else {
            let (layout, checkpoint, reused_prefix) = content::transcript_layout_with_prefix(
                &self.entry,
                EntryAppearance {
                    open: self.key.open,
                    ..EntryAppearance::default()
                },
                self.key.width,
                self.key.math,
                self.prefix.as_ref(),
            );
            if layout.allocation_bytes() > MAX_PREPARED_BYTES
                || layout.lines.len() > crate::markdown::MAX_LINES + 1
            {
                Err(Refusal::Capacity)
            } else {
                let checkpoint = checkpoint.filter(|checkpoint| {
                    layout
                        .allocation_bytes()
                        .saturating_add(checkpoint.allocation_bytes())
                        <= MAX_PREPARED_BYTES
                });
                return PreparedText {
                    key: self.key.clone(),
                    result: Ok(layout),
                    checkpoint,
                    reused_prefix,
                };
            }
        };
        PreparedText {
            key: self.key.clone(),
            result,
            checkpoint: None,
            reused_prefix: false,
        }
    }
}

/// A visible degradation reason, distinct from a corrupt or disconnected process protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Refusal {
    /// The identity or dimensions are not a coherent preparation request.
    InvalidRequest,
    /// The complete prepared entry exceeds retention limits; it is never silently truncated.
    Capacity,
    /// The owned worker could not prepare the entry; the rest of the workspace stays usable.
    Unavailable,
}

/// Owned rows, copy ranges and semantic style intent. Terminal styles are resolved only at paint.
#[derive(Debug, Serialize, Deserialize)]
pub struct PreparedText {
    key: Key,
    pub(crate) result: Result<Layout, Refusal>,
    checkpoint: Option<crate::markdown::PrefixCheckpoint>,
    reused_prefix: bool,
}

impl PreparedText {
    pub(crate) fn unavailable(key: Key, reason: Refusal) -> Self {
        Self {
            key,
            result: Err(reason),
            checkpoint: None,
            reused_prefix: false,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Key,
        Result<Layout, Refusal>,
        Option<crate::markdown::PrefixCheckpoint>,
    ) {
        (self.key, self.result, self.checkpoint)
    }
    /// Checks identity and structural bounds without reparsing or reconstructing semantic text.
    #[must_use]
    pub fn validates(&self, key: &Key) -> bool {
        if &self.key != key {
            return false;
        }
        let Ok(layout) = &self.result else {
            return self.checkpoint.is_none() && !self.reused_prefix;
        };
        let checkpoint_valid = self.checkpoint.as_ref().is_none_or(|checkpoint| {
            checkpoint.source_bytes() == checkpoint.source_prefix().len()
                && checkpoint.source_bytes() <= crate::markdown::MAX_FROZEN_PREFIX_BYTES
                && checkpoint.source_prefix().ends_with("\n\n")
                && checkpoint.rows() > 0
                && checkpoint.rows() <= layout.rows.len()
                && checkpoint.visible_text_bytes() <= layout.text.len()
                && layout.prefix_valid(checkpoint.rows(), checkpoint.visible_text_bytes())
        });
        layout.allocation_bytes().saturating_add(
            self.checkpoint
                .as_ref()
                .map_or(0, crate::markdown::PrefixCheckpoint::allocation_bytes),
        ) <= MAX_PREPARED_BYTES
            && layout.lines.len() <= crate::markdown::MAX_LINES + 1
            && layout.lines.len() == layout.rows.len()
            && layout.formulas_validate(usize::from(self.key.width))
            && layout
                .lines
                .iter()
                .all(|line| line.is_bounded(usize::from(self.key.width)))
            && layout.text_fragments_within_width(usize::from(self.key.width))
            && checkpoint_valid
    }

    /// Owned capacity, used before retaining a complete batch rather than each entry alone.
    pub fn allocation_bytes(&self) -> usize {
        size_of::<Self>()
            + self.key.agent.as_str().len()
            + self.key.item.as_str().len()
            + self
                .checkpoint
                .as_ref()
                .map_or(0, crate::markdown::PrefixCheckpoint::allocation_bytes)
            + self.result.as_ref().map_or(0, Layout::allocation_bytes)
    }

    /// Whether this result rendered only the mutable suffix of a validated Markdown prefix.
    pub const fn reused_prefix(&self) -> bool {
        self.reused_prefix
    }

    #[cfg(test)]
    pub(crate) fn checkpoint(&self) -> Option<&crate::markdown::PrefixCheckpoint> {
        self.checkpoint.as_ref()
    }

    /// Refusal is carried explicitly to the presentation owner, never replaced with empty success.
    pub fn refusal(&self) -> Option<Refusal> {
        self.result.as_ref().err().copied()
    }

    /// Number of prepared rows. No palette or terminal state is needed for measurement.
    pub fn row_count(&self) -> Result<usize, Refusal> {
        self.result
            .as_ref()
            .map(|layout| layout.lines.len())
            .map_err(|reason| *reason)
    }

    /// Width-independent selection text; whole-message copy still belongs to semantic source.
    pub fn selection_text(&self) -> Result<&str, Refusal> {
        self.result
            .as_ref()
            .map(|layout| layout.text.as_str())
            .map_err(|reason| *reason)
    }
}
