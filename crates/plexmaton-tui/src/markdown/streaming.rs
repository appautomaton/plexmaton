//! Bounded append-only Markdown checkpoints.
//!
//! The parser still sees the complete source for every revision. This module owns the smaller
//! continuation seam: a validated source/event boundary and the immutable layout rows before it.
//! Rendering only the event suffix avoids repeating completed block and native formula work.

use std::ops::Range;

use pulldown_cmark::{Event, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::{
    Boundary, Completion, MAX_EVENTS, MAX_LINES, MAX_RENDERED_BYTES, MAX_SOURCE_BYTES, PlainReason,
};
use crate::{Role, math::MathPresentation, text_layout::Layout};

mod signature;

pub(crate) const MAX_FROZEN_PREFIX_BYTES: usize = 64 * 1024;
const MAX_CHECKPOINT_CANDIDATES: usize = 64;

/// A parser-checked top-level prefix that can be reused by a later append-only revision.
///
/// Source bytes establish append-only ownership, while the event digest catches parser changes
/// such as a late reference definition. The layout itself stays in the ordinary prepared result
/// and is sliced by these coordinates when a request is assembled for the owned worker.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct PrefixCheckpoint {
    source_prefix: String,
    source_bytes: usize,
    visible_text_bytes: usize,
    rows: usize,
    signature: signature::EventSignature,
}

impl PrefixCheckpoint {
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.source_prefix.capacity() + size_of::<Self>()
    }

    pub(crate) fn source_prefix(&self) -> &str {
        &self.source_prefix
    }

    pub(crate) const fn source_bytes(&self) -> usize {
        self.source_bytes
    }

    pub(crate) const fn visible_text_bytes(&self) -> usize {
        self.visible_text_bytes
    }

    pub(crate) const fn rows(&self) -> usize {
        self.rows
    }
}

/// A bounded cache-to-worker hint. It is request data, so the child can perform the suffix render
/// without depending on process-local cache state or a parser continuation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PrefixHint {
    checkpoint: PrefixCheckpoint,
    layout: Layout,
}

impl PrefixHint {
    pub(crate) fn new(checkpoint: PrefixCheckpoint, layout: Layout) -> Option<Self> {
        let hint = Self { checkpoint, layout };
        (hint.allocation_bytes() <= MAX_FROZEN_PREFIX_BYTES).then_some(hint)
    }

    pub(crate) fn allocation_bytes(&self) -> usize {
        self.checkpoint.allocation_bytes() + self.layout.allocation_bytes() + size_of::<Self>()
    }
}

#[derive(Debug)]
pub(crate) struct RenderedLayout {
    pub(crate) layout: Layout,
    pub(crate) checkpoint: Option<PrefixCheckpoint>,
    pub(crate) reused_prefix: bool,
    #[cfg(test)]
    pub(crate) formula_preparations: usize,
}

/// Prepare Markdown while reusing a previously validated top-level prefix when possible.
///
/// The complete parser pass remains deliberate: pulldown-cmark resolves references from the whole
/// source. Only the mutable suffix reaches the renderer, so completed prefix layout and native
/// math allocations are not repeated. A mismatch falls back to the canonical full render.
pub(crate) fn render_layout_with_prefix(
    source: &str,
    width: usize,
    math: MathPresentation,
    completion: Completion,
    hint: Option<&PrefixHint>,
) -> Result<RenderedLayout, PlainReason> {
    validate_input(source, width)?;
    if width == 0 {
        return Ok(RenderedLayout {
            layout: Layout::default(),
            checkpoint: None,
            reused_prefix: false,
            #[cfg(test)]
            formula_preparations: 0,
        });
    }
    let syntax = super::syntax::Source::new(source)?;
    let event_ranges: Vec<_> = syntax
        .events_with_ranges()
        .take(MAX_EVENTS + 1)
        .collect::<Result<_, _>>()?;
    if event_ranges.len() > MAX_EVENTS {
        return Err(PlainReason::Complexity);
    }

    if completion == Completion::Streaming
        && let Some(hint) = hint
        && hint_matches(source, width, &event_ranges, hint)
    {
        let checkpoint = hint.checkpoint.clone();
        let suffix = source
            .get(checkpoint.source_bytes..)
            .ok_or(PlainReason::Complexity)?;
        if suffix.is_empty() {
            return Ok(RenderedLayout {
                layout: hint.layout.clone(),
                checkpoint: Some(checkpoint),
                reused_prefix: true,
                #[cfg(test)]
                formula_preparations: 0,
            });
        }
        // Use events from the complete parser pass. Re-parsing `suffix` would lose reference
        // definitions declared in the frozen prefix and could silently change link presentation.
        let tail_events = event_ranges
            .iter()
            .filter(|(_, range)| range.start >= checkpoint.source_bytes)
            .map(|(event, range)| (event.clone(), range.clone()))
            .collect();
        if let Ok((tail, tail_boundaries, formula_preparations)) =
            super::render_events_with_boundaries_stats(tail_events, width, math, completion)
        {
            #[cfg(not(test))]
            let _ = formula_preparations;
            let mut layout = hint.layout.clone();
            // A retained prefix has already been finished, so recreate both separator bytes before
            // appending the independently rendered tail. Finishing either segment early must not
            // erase the paragraph gap or shift its copy ranges.
            layout.blank();
            if layout
                .lines
                .last()
                .is_some_and(|line| line.spans.is_empty())
            {
                layout.text.push('\n');
            }
            layout.append(tail, "", Role::Body);
            layout.finish();
            if layout_within_limits(&layout, width) {
                let checkpoint = advanced_checkpoint(
                    source,
                    &event_ranges,
                    &tail_boundaries,
                    &checkpoint,
                    &layout,
                )
                .unwrap_or(checkpoint);
                return Ok(RenderedLayout {
                    layout,
                    checkpoint: Some(checkpoint),
                    reused_prefix: true,
                    #[cfg(test)]
                    formula_preparations,
                });
            }
        }
    }

    let (layout, boundaries, formula_preparations) =
        super::render_events_with_boundaries_stats(event_ranges.clone(), width, math, completion)?;
    #[cfg(not(test))]
    let _ = formula_preparations;
    let checkpoint = (completion == Completion::Streaming)
        .then(|| checkpoint_for(source, &event_ranges, &boundaries, &layout))
        .flatten();
    Ok(RenderedLayout {
        layout,
        checkpoint,
        reused_prefix: false,
        #[cfg(test)]
        formula_preparations,
    })
}

fn validate_input(source: &str, width: usize) -> Result<(), PlainReason> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(PlainReason::Size);
    }
    if width > 512 {
        return Err(PlainReason::Complexity);
    }
    Ok(())
}

fn layout_within_limits(layout: &Layout, width: usize) -> bool {
    layout.formulas_validate(width)
        && layout.rows.len() == layout.lines.len()
        && layout.lines.iter().all(|line| line.is_bounded(width))
        && layout.text_fragments_within_width(width)
        && layout.lines.len() <= MAX_LINES
        && layout.text.len() <= MAX_RENDERED_BYTES
        && layout.allocation_bytes() <= crate::preparation::MAX_PREPARED_BYTES
}

fn hint_matches(
    source: &str,
    width: usize,
    events: &[(Event<'_>, Range<usize>)],
    hint: &PrefixHint,
) -> bool {
    let checkpoint = &hint.checkpoint;
    hint.allocation_bytes() <= MAX_FROZEN_PREFIX_BYTES
        && checkpoint.source_bytes == checkpoint.source_prefix.len()
        && checkpoint.source_bytes <= MAX_FROZEN_PREFIX_BYTES
        && source.starts_with(&checkpoint.source_prefix)
        && checkpoint.rows > 0
        && checkpoint.visible_text_bytes == hint.layout.text.len()
        && checkpoint.rows == hint.layout.rows.len()
        && layout_within_limits(&hint.layout, width)
        && signature::digest(events, checkpoint.source_bytes) == checkpoint.signature
        && safe_checkpoint(source, events, checkpoint.source_bytes)
}

fn checkpoint_for(
    source: &str,
    events: &[(Event<'_>, Range<usize>)],
    boundaries: &[Boundary],
    full: &Layout,
) -> Option<PrefixCheckpoint> {
    candidate_cuts(source, events)
        .into_iter()
        .rev()
        .find_map(|cut| {
            let last = events.iter().rfind(|(_, range)| range.end <= cut)?;
            if !matches!(last.0, Event::End(TagEnd::Paragraph | TagEnd::Heading(_))) {
                return None;
            }
            let boundary = boundaries
                .iter()
                .rev()
                .find(|boundary| boundary.event_end == last.1.end)?;
            if boundary.rows == 0
                || boundary.rows > full.rows.len()
                || boundary.visible_text_bytes > full.text.len()
                || full.formulas.iter().any(|formula| {
                    formula.row < boundary.rows
                        && matches!(
                            formula.content,
                            crate::text_layout::math::FormulaContent::Pending
                        )
                })
            {
                return None;
            }
            let prefix = source.get(..cut)?;
            let checkpoint = PrefixCheckpoint {
                source_prefix: prefix.to_owned(),
                source_bytes: cut,
                visible_text_bytes: boundary.visible_text_bytes,
                rows: boundary.rows,
                signature: signature::digest(events, cut),
            };
            (checkpoint.allocation_bytes() <= MAX_FROZEN_PREFIX_BYTES).then_some(checkpoint)
        })
}

fn advanced_checkpoint(
    source: &str,
    events: &[(Event<'_>, Range<usize>)],
    boundaries: &[Boundary],
    previous: &PrefixCheckpoint,
    full: &Layout,
) -> Option<PrefixCheckpoint> {
    candidate_cuts(source, events)
        .into_iter()
        .filter(|cut| *cut > previous.source_bytes)
        .rev()
        .find_map(|cut| {
            let last = events.iter().rfind(|(_, range)| range.end <= cut)?;
            if !matches!(last.0, Event::End(TagEnd::Paragraph | TagEnd::Heading(_))) {
                return None;
            }
            let boundary = boundaries
                .iter()
                .rev()
                .find(|boundary| boundary.event_end == last.1.end)?;
            let rows = previous.rows.checked_add(1)?.checked_add(boundary.rows)?;
            let visible_text_bytes = previous
                .visible_text_bytes
                .checked_add(2)?
                .checked_add(boundary.visible_text_bytes)?;
            if rows == 0
                || rows > full.rows.len()
                || visible_text_bytes > full.text.len()
                || full.formulas.iter().any(|formula| {
                    formula.row >= previous.rows.saturating_add(1)
                        && formula.row < rows
                        && matches!(
                            formula.content,
                            crate::text_layout::math::FormulaContent::Pending
                        )
                })
            {
                return None;
            }
            let prefix = source.get(..cut)?;
            let checkpoint = PrefixCheckpoint {
                source_prefix: prefix.to_owned(),
                source_bytes: cut,
                visible_text_bytes,
                rows,
                signature: signature::digest(events, cut),
            };
            (checkpoint.allocation_bytes() <= MAX_FROZEN_PREFIX_BYTES).then_some(checkpoint)
        })
}

/// Return only a bounded set of the newest candidate cuts. Each candidate still scans the full
/// event stream for structural safety, but the number of those scans cannot grow with line count.
fn candidate_cuts(source: &str, events: &[(Event<'_>, Range<usize>)]) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut search = 0;
    while let Some(relative) = source[search..].find("\n\n") {
        let cut = search + relative + 2;
        if cut <= MAX_FROZEN_PREFIX_BYTES {
            cuts.push(cut);
        }
        search = cut;
    }
    let mut newest = cuts
        .into_iter()
        .rev()
        .take(MAX_CHECKPOINT_CANDIDATES)
        .filter(|cut| safe_checkpoint(source, events, *cut))
        .collect::<Vec<_>>();
    newest.reverse();
    newest
}

fn safe_checkpoint(source: &str, events: &[(Event<'_>, Range<usize>)], cut: usize) -> bool {
    let Some(prefix) = source.get(..cut) else {
        return false;
    };
    if !prefix.ends_with("\n\n")
        || source
            .as_bytes()
            .get(cut)
            .is_some_and(|byte| *byte == b'\n')
    {
        return false;
    }
    if has_setext(prefix) {
        return false;
    }
    let mut depth = 0usize;
    let mut last_end = false;
    for (event, range) in events {
        // A block/container whose event range crosses the cut is still being resolved by the
        // suffix. Freezing before its closing event would make the sliced parse unsound.
        if range.start < cut && range.end > cut {
            return false;
        }
        if range.end > cut {
            continue;
        }
        match event {
            Event::Start(tag) => {
                depth = depth.saturating_add(1);
                if matches!(
                    tag,
                    Tag::BlockQuote(_)
                        | Tag::CodeBlock(_)
                        | Tag::List(_)
                        | Tag::Item
                        | Tag::Table(_)
                        | Tag::TableHead
                        | Tag::TableRow
                        | Tag::TableCell
                        | Tag::HtmlBlock
                ) {
                    return false;
                }
                last_end = false;
            }
            Event::End(tag) => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
                last_end = matches!(tag, TagEnd::Paragraph | TagEnd::Heading(_));
            }
            Event::InlineMath(source) | Event::DisplayMath(source) => {
                if source.is_empty() {
                    return false;
                }
                last_end = false;
            }
            _ => last_end = false,
        }
    }
    depth == 0 && last_end
}

fn has_setext(source: &str) -> bool {
    let lines: Vec<_> = source.lines().collect();
    lines.windows(2).any(|pair| {
        !pair[0].trim().is_empty()
            && !pair[1].trim().is_empty()
            && pair[1].trim().chars().all(|ch| ch == '=' || ch == '-')
    })
}
