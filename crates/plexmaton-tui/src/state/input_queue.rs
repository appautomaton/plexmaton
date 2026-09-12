//! Messages the user submitted that have not been sent to the model yet (IQU-1).
//!
//! The producer owns these queues. What is here is a copy for drawing, replaced in full every time
//! the composition root reads them again. None of it is a transcript entry: a waiting message is
//! not in the session journal, so it has no number to anchor to, cannot be selected or copied, and
//! the ordinary turn events report it once it is actually sent.

use ratatui::text::{Line, Span};

use super::ViewState;
use crate::theme::{Palette, Role};

/// How many waiting messages the band lists before it counts the rest instead of growing.
///
/// Three, the same as the Attention band, for the same reason: beyond that it would be taking rows
/// from the conversation to repeat text the user typed and can still scroll back to.
pub(crate) const LISTED: usize = 3;

/// The rule the band draws above its body, which is a row of its height but not of its content.
///
/// Named once so the height the band asks for and the rows the renderer builds a body into cannot
/// drift apart by one.
pub(crate) const QUEUE_RULE_ROWS: u16 = 1;

/// When one waiting message will be sent.
///
/// Named for when it is sent rather than for which queue is holding it: the same message moves
/// between the producer's collections, and when it goes out is what the user needs to know.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuedBoundary {
    /// With the current turn's next step, once the running tool calls have returned.
    Step,
    /// As a new turn, once this one ends.
    Turn,
    /// Not given to the producer yet, because an operation it owns is still running.
    Admission,
}

impl QueuedBoundary {
    const fn label(self) -> &'static str {
        match self {
            Self::Step => "Sends after the current tool call",
            Self::Turn => "Sends when this turn ends",
            Self::Admission => "Sends when the current operation ends",
        }
    }
}

/// One message the user submitted that has not been sent yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedInput {
    /// The text the user typed, unchanged.
    pub text: String,
    /// When it will be sent.
    pub boundary: QueuedBoundary,
}

impl ViewState {
    /// Replaces everything shown. The producer's order is the order drawn.
    pub(crate) fn set_queued_input(&mut self, queued: Vec<QueuedInput>) {
        if self.queued == queued {
            return;
        }
        self.queued = queued;
        self.touch();
    }

    /// The waiting messages, in the order they will be sent.
    pub(crate) fn queued_input(&self) -> &[QueuedInput] {
        &self.queued
    }

    /// Which conversation `Alt-↑` acts on: the primary, and only from its own composer.
    ///
    /// The band shows one runtime's queue. A cursor in a worker's window addresses that worker, so
    /// taking the primary's message back from there would remove something the user is not
    /// looking at (COM-4).
    pub(crate) fn withdraw_target(
        &self,
        surfaces: &crate::surface::SurfaceTree,
    ) -> Option<plexmaton_core::AgentId> {
        if self.queued.is_empty() || !self.composer().text().is_empty() {
            return None;
        }
        match self.focus.resolve(surfaces)? {
            crate::surface::SurfaceId::Composer => {
                self.agents.primary().map(|agent| agent.id.clone())
            }
            _ => None,
        }
    }

    /// Rows the band asks for at this width, its rule included, and none when nothing is waiting.
    ///
    /// One row per message, so the height follows [`LISTED`] and how many of the three sending
    /// times are in use — never how much the user typed.
    pub(crate) fn queued_rows(&self, width: u16) -> u16 {
        if self.queued.is_empty() {
            return 0;
        }
        let body = queued_lines(
            self,
            &Palette::default(),
            super::inner_width(width),
            u16::MAX,
        );
        u16::try_from(body.len())
            .unwrap_or(u16::MAX)
            .saturating_add(QUEUE_RULE_ROWS)
    }

    /// Rows below which the band would rather not appear at all.
    ///
    /// Its rule, one sending time, one message, the count of the rest and the way back. The band is
    /// chrome, so rows its content overflows are not somewhere the user can scroll to: a band given
    /// fewer rows than this would drop the very key it exists to advertise, and go on counting in
    /// its title messages it had stopped showing.
    pub(crate) fn queued_floor(&self, width: u16) -> u16 {
        self.queued_rows(width).min(QUEUE_RULE_ROWS + 4)
    }
}

/// The band's body, built to fit `rows`: a heading for each sending time it lists a message for,
/// then those messages, then a count of every message it did not list, then the way back.
///
/// `rows` is what the band was actually granted, which layout may cut below what it asked for. The
/// band is chrome, so it has no scrollback: content that does not fit is content the user cannot
/// reach. It therefore lists fewer messages rather than letting the rows below the fold fall off,
/// and the count keeps covering every message it stopped listing.
pub(crate) fn queued_lines(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    rows: u16,
) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let listed = listed_within(&state.queued, rows);
    let mut lines = Vec::new();
    let mut heading: Option<QueuedBoundary> = None;
    // Only above a message that is listed: a sending time with nothing under it names a queue the
    // band is not showing, and would take the row that would have shown one of its messages.
    for entry in state.queued.iter().take(listed) {
        if heading != Some(entry.boundary) {
            heading = Some(entry.boundary);
            lines.push(Line::from(Span::styled(
                entry.boundary.label().to_owned(),
                palette.style(Role::SectionHeading),
            )));
        }
        lines.push(entry_line(&entry.text, palette, width));
    }
    let held = state.queued.len().saturating_sub(listed);
    if held > 0 {
        lines.push(Line::from(Span::styled(
            format!("… {held} more waiting"),
            palette.style(Role::Muted),
        )));
    }
    // The way back, printed where the messages are, and the last row the band gives up. A queue the
    // user can read but cannot undo is worse than one they never see: showing it is what makes them
    // expect to be able to act on it.
    lines.push(Line::from(vec![
        Span::styled("Alt-↑".to_owned(), palette.style(Role::KeyHint)),
        Span::styled(
            if state.composer().text().is_empty() {
                " takes back the last one"
            } else {
                " needs an empty draft"
            }
            .to_owned(),
            palette.style(Role::Muted),
        ),
    ]));
    lines
}

/// The most messages the band can list in `rows` and still say what it left out.
///
/// Tried longest first over at most [`LISTED`] candidates, because a heading is only paid for by
/// the run of messages under it: dropping one message can free two rows or none.
fn listed_within(queued: &[QueuedInput], rows: u16) -> usize {
    // Its row comes off the top: the way back is the one line the band never trades for a message.
    let budget = usize::from(rows).saturating_sub(1);
    (0..=LISTED.min(queued.len()))
        .rev()
        .find(|&listed| body_rows(queued, listed) <= budget)
        .unwrap_or(0)
}

/// Rows a body listing the first `listed` messages needs, the way back excluded.
///
/// Walked the way the body is built, so the two cannot disagree about what a heading costs.
fn body_rows(queued: &[QueuedInput], listed: usize) -> usize {
    let mut heading: Option<QueuedBoundary> = None;
    let mut rows = 0;
    for entry in queued.iter().take(listed) {
        if heading != Some(entry.boundary) {
            heading = Some(entry.boundary);
            rows += 1;
        }
        rows += 1;
    }
    rows + usize::from(listed < queued.len())
}

/// One message on one row: a marker, then as much of its exact text as the row holds.
///
/// Enough to recognise it, not a preview of it. The message is about to appear in full in the
/// conversation, where it can be read, selected and copied; showing it twice would cost rows the
/// conversation needs.
fn entry_line(text: &str, palette: &Palette, width: usize) -> Line<'static> {
    const MARKER: &str = "↳ ";
    // The user's line breaks are theirs, but a row is one row: they become spaces rather than
    // silently joining two words into one.
    let inline = text.replace('\n', " ");
    let available = width.saturating_sub(MARKER.chars().count());
    Line::from(vec![
        Span::styled(MARKER.to_owned(), palette.style(Role::Muted)),
        Span::styled(
            crate::content::command_summary(inline.trim(), available),
            palette.style(Role::Ambient),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::{LISTED, QueuedBoundary, QueuedInput};
    use crate::{ViewState, theme::Palette};

    fn waiting(count: usize, boundary: QueuedBoundary) -> Vec<QueuedInput> {
        (0..count)
            .map(|index| QueuedInput {
                text: format!("message {index}"),
                boundary,
            })
            .collect()
    }

    /// IQU-2: the band reports a queue it does not list, so its height cannot follow its length.
    ///
    /// Remove the `LISTED` guard and this fails: forty waiting messages ask for forty rows, and
    /// layout would hand them over before the conversation's floor stopped it.
    #[test]
    fn height_is_bounded_by_what_is_listed_rather_than_by_what_is_waiting() {
        let mut few = ViewState::default();
        few.set_queued_input(waiting(LISTED, QueuedBoundary::Turn));
        let mut many = ViewState::default();
        many.set_queued_input(waiting(40, QueuedBoundary::Turn));

        assert_eq!(few.queued_rows(80), many.queued_rows(80).saturating_sub(1));
        assert_eq!(
            few.queued_rows(80),
            6,
            "rule, heading, three entries and the way back"
        );
        let lines: Vec<_> = super::queued_lines(&many, &Palette::default(), 78, u16::MAX)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            lines[lines.len() - 2],
            "… 37 more waiting",
            "what is not listed is still counted"
        );
    }

    /// IQU-2: one row per entry, whatever the user typed into it.
    ///
    /// A message with newlines must not silently join two words, and must not take a second row
    /// from the conversation to show a preview of something about to be said in full.
    #[test]
    fn a_multiline_message_occupies_one_row_without_joining_its_lines() {
        let mut state = ViewState::default();
        state.set_queued_input(vec![QueuedInput {
            text: "first\nsecond".to_owned(),
            boundary: QueuedBoundary::Turn,
        }]);

        let lines = super::queued_lines(&state, &Palette::default(), 40, u16::MAX);
        assert_eq!(lines.len(), 3, "one heading, one entry and the way back");
        assert_eq!(lines[1].to_string(), "↳ first second");
        assert_eq!(state.queued_rows(42), 4);
    }

    /// IQU-1: each boundary is named once, above the entries it will claim.
    #[test]
    fn every_boundary_names_itself_once_above_its_own_entries() {
        let mut state = ViewState::default();
        let mut queued = waiting(2, QueuedBoundary::Step);
        queued.extend(waiting(1, QueuedBoundary::Turn));
        state.set_queued_input(queued);

        let lines: Vec<_> = super::queued_lines(&state, &Palette::default(), 60, u16::MAX)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            lines,
            [
                "Sends after the current tool call",
                "↳ message 0",
                "↳ message 1",
                "Sends when this turn ends",
                "↳ message 0",
                "Alt-↑ takes back the last one",
            ]
        );
    }

    /// IQU-2: a sending time is named only above a message the band is actually showing.
    ///
    /// Emit the heading before the listing guard and this fails: a queue whose messages run out
    /// mid-band prints a sending time with nothing under it, and the count beneath that heading
    /// then covers messages belonging to the heading above.
    #[test]
    fn a_sending_time_the_band_stopped_listing_is_counted_rather_than_named() {
        let mut state = ViewState::default();
        let mut queued = waiting(LISTED + 1, QueuedBoundary::Turn);
        queued.extend(waiting(1, QueuedBoundary::Admission));
        state.set_queued_input(queued);

        let lines: Vec<_> = super::queued_lines(&state, &Palette::default(), 60, u16::MAX)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            lines,
            [
                "Sends when this turn ends",
                "↳ message 0",
                "↳ message 1",
                "↳ message 2",
                "… 2 more waiting",
                "Alt-↑ takes back the last one",
            ],
            "no heading without a message, and the count covers both queues"
        );
    }

    /// IQU-2: cut below what it asked for, the band lists less rather than losing its bottom rows.
    ///
    /// The band is chrome, so nothing it paints past its rectangle can be scrolled to. Build the
    /// body without consulting the rows it was granted and this fails at every height: the way back
    /// is the first line off the bottom, and the title goes on counting messages nobody can see.
    #[test]
    fn a_band_cut_short_drops_messages_before_it_drops_the_way_back() {
        let mut state = ViewState::default();
        state.set_queued_input(waiting(4, QueuedBoundary::Turn));
        let asked = state.queued_rows(80);
        assert_eq!(
            asked, 7,
            "the rule, a heading, three messages, the count and the way back"
        );

        for granted in state.queued_floor(80)..=asked {
            let body = granted.saturating_sub(super::QUEUE_RULE_ROWS);
            let lines: Vec<_> = super::queued_lines(&state, &Palette::default(), 78, body)
                .iter()
                .map(ToString::to_string)
                .collect();
            assert!(
                lines.len() <= usize::from(body),
                "granted {granted}: {} lines do not fit {body} rows",
                lines.len()
            );
            assert_eq!(
                lines.last().map(String::as_str),
                Some("Alt-↑ takes back the last one"),
                "granted {granted}: the way back is the row the band never gives up"
            );
            let listed = lines.iter().filter(|line| line.starts_with('↳')).count();
            let counted: usize = lines
                .iter()
                .find_map(|line| line.strip_prefix("… ")?.split_once(' ')?.0.parse().ok())
                .unwrap_or(0);
            assert_eq!(
                listed + counted,
                4,
                "granted {granted}: every waiting message is listed or counted"
            );
        }
    }

    /// IQU-3/IQU-4: the key is inert with nothing waiting and never crosses worker focus.
    ///
    /// Drop the emptiness guard and this fails: `Alt-↑` would cross the composition boundary on
    /// every press, asking the runtime to take back something nobody queued.
    #[test]
    fn the_way_back_is_inert_until_something_waits_and_then_names_the_primary() {
        let mut state = crate::test_support::canonical_state();
        let (surfaces, _) = crate::test_support::draw_frame(&state, &Palette::default(), 120, 40);
        state.focus_surface(&surfaces, crate::surface::SurfaceId::Composer);
        assert_eq!(
            state.withdraw_target(&surfaces),
            None,
            "nothing waiting is nothing to take back"
        );

        state.set_queued_input(waiting(1, QueuedBoundary::Turn));
        let (surfaces, _) = crate::test_support::draw_frame(&state, &Palette::default(), 120, 40);
        state.focus_surface(&surfaces, crate::surface::SurfaceId::Composer);
        assert_eq!(
            state.withdraw_target(&surfaces),
            state.text_target(&surfaces),
            "the message goes back to the input it was typed in"
        );
        assert!(state.withdraw_target(&surfaces).is_some());

        state.move_selection(crate::intent::Direction::Forward);
        let (surfaces, _) = crate::test_support::draw_frame(&state, &Palette::default(), 120, 40);
        state.inspect(&surfaces, crate::intent::InspectorIntent::Open);
        let (surfaces, _) = crate::test_support::draw_frame(&state, &Palette::default(), 120, 40);
        assert_eq!(state.focused(&surfaces), Some(crate::SurfaceId::Inspector));
        assert_eq!(
            state.withdraw_target(&surfaces),
            None,
            "the worker owns this cursor"
        );
    }
}
