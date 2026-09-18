//! The readable opening of a document, for a row that previews it.
//!
//! Separate from the renderer because it answers a different question with the same parser: not
//! "how does this look laid out" but "what does it say", which is all a single row can carry.

use pulldown_cmark::{Event, TagEnd};

use super::syntax;

/// How much of a document a one-row preview may read before it certainly has enough.
const MAX_PREVIEW_BYTES: usize = 2 * 1024;

/// The readable opening of a document, for a row that previews it.
///
/// What a reader needs from an entry's heading is what the entry says, not how it was marked up.
/// Collected from the parser that already owns this source, so a heading marker, emphasis, a list
/// bullet or a fence never reaches the row as characters. A display formula contributes nothing:
/// it has no one-row form, and its delimiters read as noise. Inline math keeps its source, which
/// is the shortest true thing a single row can say about it.
///
/// Bounded to a prefix, because a preview is one row and a letter can be a page. Rejected:
/// trimming markers with a hand-written scan, a second grammar that would drift from the one the
/// body is drawn with; and previewing the rendered body, whose text depends on the width the row
/// happens to be drawn at.
pub(crate) fn preview(source: &str) -> Option<String> {
    let mut end = source.len().min(MAX_PREVIEW_BYTES);
    while end > 0 && !source.is_char_boundary(end) {
        end -= 1;
    }
    let parsed = syntax::Source::new(source.get(..end)?).ok()?;
    let mut text = String::new();
    for event in parsed.events_with_ranges() {
        match event.ok()?.0 {
            Event::Text(part) | Event::Code(part) | Event::InlineMath(part) => {
                text.push_str(&part);
            }
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) => text.push(' '),
            _ => {}
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
