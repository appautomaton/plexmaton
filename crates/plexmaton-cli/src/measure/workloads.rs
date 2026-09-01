//! What each declared workload does, and the claims its numbers are supposed to carry.
//!
//! Separated from the harness because they answer a different question. That file decides what a
//! measurement *is* — a frame, its work, and how a run is reported; this one decides which
//! situations are worth measuring, which is a reading of
//! `phase-00 §responsiveness workloads` rather than a mechanism.

use std::time::Instant;

use plexmaton_sim::Scenario;
use plexmaton_tui::SurfaceId;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};

use super::{COLD_SAMPLES, Harness, RESIZES, Run, SAMPLES, SIZE};

/// The first frame on a conversation nothing has measured.
///
/// The one frame that is deliberately proportional to history: knowing how tall a conversation is
/// means wrapping every item once, and that is what makes every later frame cheap (TR-1).
pub(super) fn cold_open(items: usize) -> anyhow::Result<Run> {
    let mut run = Run::new("cold open");
    let mut last = None;
    for _ in 0..COLD_SAMPLES {
        let mut harness = Harness::new(Scenario::streaming(items)?, SIZE)?;
        harness.advance_to(u64::MAX);
        let started = Instant::now();
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
        last = Some(harness);
    }
    Ok(match last {
        Some(harness) => run.finish(&harness),
        None => run,
    })
}

/// One conversation streaming, a frame per event.
pub(super) fn streaming(items: usize) -> anyhow::Result<Run> {
    let scenario = Scenario::streaming(items.saturating_add(SAMPLES))?;
    sample_events("streaming delta", scenario, SIZE)
}

/// Four conversations streaming with tool activity, one of them on screen.
pub(super) fn interleaved(items: usize) -> anyhow::Result<Run> {
    let scenario = Scenario::interleaved(4, items.saturating_add(SAMPLES))?;
    sample_events("four agents", scenario, SIZE)
}

/// Applies one event and paints, `SAMPLES` times, after warming on everything before them.
pub(super) fn sample_events(
    workload: &'static str,
    scenario: Scenario,
    size: (u16, u16),
) -> anyhow::Result<Run> {
    let warm_until = scenario.steps().len().saturating_sub(SAMPLES);
    let mut harness = Harness::new(scenario, size)?;
    harness.warm(warm_until)?;

    let mut run = Run::new(workload);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        harness.advance();
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// The wheel over the conversation, which must read cached heights and recompute none.
pub(super) fn wheel(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::streaming(items)?, SIZE)?;
    harness.warm(usize::MAX)?;
    let (column, row) = harness.inside(SurfaceId::Transcript);

    let mut run = Run::new("wheel");
    for sample in 0..SAMPLES {
        // Up then down, so the workload does not simply run out of conversation to scroll.
        let kind = if sample % 40 < 20 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        };
        let event = Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        let started = Instant::now();
        harness.workspace.handle(&event);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// Selecting the other conversation and back, which is canonical journey step 4.
pub(super) fn switch_reader(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::interleaved(2, items)?, SIZE)?;
    harness.warm(usize::MAX)?;

    let mut run = Run::new("switch reader");
    for sample in 0..SAMPLES {
        let code = if sample.is_multiple_of(2) {
            KeyCode::Down
        } else {
            KeyCode::Up
        };
        let event = Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let started = Instant::now();
        harness.workspace.handle(&event);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// Wide, medium, and narrow in turn. Every height is width-dependent, so each one re-measures.
pub(super) fn resize(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::streaming(items)?, SIZE)?;
    harness.warm(usize::MAX)?;

    let mut run = Run::new("resize");
    for sample in 0..SAMPLES {
        let size = RESIZES.get(sample % RESIZES.len()).copied().unwrap_or(SIZE);
        let started = Instant::now();
        harness.resize(size);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// Opening and closing the inspector, which is the surface open and close budget.
///
/// A shelf splits the conversation region vertically, so the conversation keeps its width and every
/// cached height stays valid. Opening therefore costs a relayout and a repaint and no wrapping at
/// all, which is what the wraps column is here to show rather than assert in prose.
pub(super) fn inspector(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::interleaved(2, items)?, SIZE)?;
    harness.warm(usize::MAX)?;

    let mut run = Run::new("open inspector");
    for sample in 0..SAMPLES {
        let code = if sample.is_multiple_of(2) {
            KeyCode::Enter
        } else {
            KeyCode::Esc
        };
        let event = Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let started = Instant::now();
        harness.workspace.handle(&event);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// Opening an agent whose conversation is not on screen, which is the phase's declared
/// *large hidden transcript opened into an inspector*.
///
/// It differs from `open inspector` in the one way that costs: the inspected agent's conversation
/// has never been measured, so the first frame after opening wraps its whole history at the shelf's
/// width. That is the same deliberate cost as a cold open (TR-1), paid here by a surface appearing
/// rather than by a process starting — and it is the number the surface-open budget was quietly not
/// measuring while an inspector held a detail panel.
pub(super) fn hidden_conversation(items: usize) -> anyhow::Result<Run> {
    let mut run = Run::new("open hidden conversation");
    let mut last = None;
    for _ in 0..COLD_SAMPLES {
        let mut harness = Harness::new(Scenario::interleaved(2, items)?, SIZE)?;
        harness.warm(usize::MAX)?;
        // Open the second agent, pin it, and take the rail back to the first — with no frame in
        // between, so nothing measures the second conversation until the timed draw. A frame here
        // would warm the cache and leave the measurement with nothing to do.
        for (code, modifiers) in [
            (KeyCode::Down, KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
            (KeyCode::Char('p'), KeyModifiers::CONTROL),
            (KeyCode::Up, KeyModifiers::NONE),
        ] {
            harness
                .workspace
                .handle(&Event::Key(KeyEvent::new(code, modifiers)));
        }
        let started = Instant::now();
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
        last = Some(harness);
    }
    Ok(match last {
        Some(harness) => run.finish(&harness),
        None => run,
    })
}

/// Two conversations on screen, scrolled in turn, which is the phase's declared
/// *two visible independently scrolling transcripts*.
///
/// The claim under it is that neither reader costs the other anything: heights are keyed by agent
/// and width, a shelf splits vertically so both panels share one width, and a wheel moves one
/// stored position. So the wraps column must read zero here exactly as it does for one panel — the
/// second conversation is paid for once, when it is opened, and never again.
pub(super) fn two_conversations(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::interleaved(2, items)?, SIZE)?;
    harness.warm(usize::MAX)?;
    for code in [KeyCode::Down, KeyCode::Enter, KeyCode::Char('p')] {
        let modifiers = if code == KeyCode::Char('p') {
            KeyModifiers::CONTROL
        } else {
            KeyModifiers::NONE
        };
        harness
            .workspace
            .handle(&Event::Key(KeyEvent::new(code, modifiers)));
        harness.draw()?;
    }
    let conversation = harness.inside(SurfaceId::Transcript);
    let inspected = harness.inside(SurfaceId::Inspector);

    let mut run = Run::new("two conversations");
    for sample in 0..SAMPLES {
        // Alternate panels and alternate directions, so neither reader runs out of conversation.
        let (column, row) = if sample.is_multiple_of(2) {
            conversation
        } else {
            inspected
        };
        let kind = if sample % 40 < 20 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        };
        let event = Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        let started = Instant::now();
        harness.workspace.handle(&event);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

/// Extending a selection through a long conversation, one entry per press.
///
/// Selection changes a style and never a character, so a selected frame must re-wrap nothing: the
/// heights the cache holds were measured unselected and stay valid. That is the claim the wraps
/// column checks. What it cannot see is the other half — copying is bounded by the selection rather
/// than by the history, which `sources` gets by ranging its iterators instead of a finished vector.
pub(super) fn select(items: usize) -> anyhow::Result<Run> {
    let mut harness = Harness::new(Scenario::streaming(items)?, SIZE)?;
    harness.warm(usize::MAX)?;
    // Onto the conversation, which is the list with a history worth selecting through.
    harness
        .workspace
        .handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    harness.draw()?;

    let mut run = Run::new("extend selection");
    for sample in 0..SAMPLES {
        // Back through history, then forward again, so the workload does not run out of messages.
        let code = if sample % 40 < 20 {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        let event = Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT));
        let started = Instant::now();
        harness.workspace.handle(&event);
        let work = harness.draw()?;
        run.record(started.elapsed(), work);
    }
    Ok(run.finish(&harness))
}

#[cfg(test)]
mod tests {
    use super::{Harness, RESIZES, SIZE};
    use plexmaton_sim::Scenario;
    use plexmaton_tui::SurfaceId;
    use ratatui::crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
    };

    /// The harness measures the claims it reports, at a scale small enough for the test suite.
    ///
    /// Running the workloads here rather than only under the command is what keeps the harness
    /// honest: a driver whose numbers nothing checks will happily report that a broken renderer is
    /// fast.
    fn harness(items: usize) -> Harness {
        let scenario =
            Scenario::streaming(items).unwrap_or_else(|error| panic!("workload fixture: {error}"));
        let mut harness = Harness::new(scenario, SIZE)
            .unwrap_or_else(|error| panic!("measurement terminal: {error}"));
        harness
            .warm(usize::MAX)
            .unwrap_or_else(|error| panic!("warm frame: {error}"));
        harness
    }

    /// FR-2 through the harness: the wheel reads cached heights and recomputes none.
    #[test]
    fn the_wheel_workload_costs_no_measurement() {
        let mut harness = harness(300);
        let (column, row) = harness.inside(SurfaceId::Transcript);
        assert!(column > 0, "the transcript has to be on screen");

        for _ in 0..10 {
            harness.workspace.handle(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            let work = harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"))
                .unwrap_or_else(|| panic!("a scroll is a visible change"));
            assert_eq!(work.items_wrapped, 0, "scrolling must not re-wrap anything");
        }
    }

    /// SEL-1 through the harness: selecting changes a style, so it re-measures nothing.
    ///
    /// The cache keys heights on item revision and panel width, and a selection is neither. If a
    /// height ever depended on selection, every arrow press would invalidate the item under it and
    /// the cheapest gesture in the workspace would become one of the more expensive ones.
    #[test]
    fn extending_a_selection_costs_no_measurement() {
        let mut harness = harness(300);
        harness
            .workspace
            .handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        harness
            .draw()
            .unwrap_or_else(|error| panic!("measured frame: {error}"));

        for _ in 0..10 {
            harness
                .workspace
                .handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
            let work = harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"))
                .unwrap_or_else(|| panic!("a selection is a visible change"));
            assert_eq!(work.items_wrapped, 0, "selecting must not re-wrap anything");
        }
        assert!(
            harness.workspace.state().selection().is_some(),
            "the conversation is the focused surface, so the arrows reached it"
        );
    }

    /// The declared *two visible independently scrolling transcripts* workload, as a work count.
    ///
    /// A second conversation on screen is paid for once, when the surface that shows it opens, and
    /// costs nothing per frame afterwards — because both panels share one width and heights are
    /// keyed by agent and width. A renderer that measured the visible conversation each frame, or
    /// that keyed heights by panel, would land at one full pass per scroll here and the assertion
    /// would say so rather than the timings being slightly worse.
    #[test]
    fn scrolling_either_of_two_conversations_costs_no_measurement() {
        let scenario = Scenario::interleaved(2, 300)
            .unwrap_or_else(|error| panic!("workload fixture: {error}"));
        let mut harness = Harness::new(scenario, SIZE)
            .unwrap_or_else(|error| panic!("measurement terminal: {error}"));
        harness
            .warm(usize::MAX)
            .unwrap_or_else(|error| panic!("warm frame: {error}"));

        // Inspect the second agent, pin it, and take the conversation back to the first.
        for (code, modifiers) in [
            (KeyCode::Down, KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
            (KeyCode::Char('p'), KeyModifiers::CONTROL),
            (KeyCode::Up, KeyModifiers::NONE),
        ] {
            harness
                .workspace
                .handle(&Event::Key(KeyEvent::new(code, modifiers)));
            harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"));
        }

        let conversation = harness.inside(SurfaceId::Transcript);
        let inspected = harness.inside(SurfaceId::Inspector);
        assert_ne!(
            conversation, inspected,
            "two conversations have to be on screen, or this proves nothing"
        );

        for sample in 0..10 {
            let (column, row) = if sample % 2 == 0 {
                conversation
            } else {
                inspected
            };
            harness.workspace.handle(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            let work = harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"))
                .unwrap_or_else(|| panic!("a scroll is a visible change"));
            assert_eq!(
                work.items_wrapped, 0,
                "scrolling either conversation must re-wrap neither"
            );
        }
    }

    /// TR-1's expensive case, measured rather than assumed: a resize re-measures everything once.
    ///
    /// This is the workload that justifies the cache existing. If a resize cost less than one pass
    /// the heights would not be width-keyed, and the reader would land on the wrong message.
    #[test]
    fn the_resize_workload_re_measures_every_item_exactly_once() {
        let mut harness = harness(300);
        let items = harness.items_on_screen();
        assert!(items >= 300, "the fixture streamed its messages");

        for size in RESIZES {
            harness.resize(size);
            let work = harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"))
                .unwrap_or_else(|| panic!("a resize always forces a frame"));
            assert_eq!(
                work.items_wrapped, items,
                "a resize at {size:?} must re-measure each item once and no item twice"
            );
        }
    }

    /// A background conversation's traffic must not cost the foreground a re-measure.
    ///
    /// The bound is derived rather than chosen: a message arrives as four events, each of which
    /// bumps its item's revision and so costs one wrap, and only the conversation on screen is
    /// measured at all. With four agents taking turns, a renderer that measured whichever
    /// conversation an event named would land near four times this number.
    #[test]
    fn a_background_agent_streaming_does_not_re_measure_the_foreground() {
        const WINDOW: usize = 40;
        /// Events per message: started, two deltas, finalized.
        const PER_MESSAGE: usize = 4;

        let scenario = Scenario::interleaved(4, 60)
            .unwrap_or_else(|error| panic!("workload fixture: {error}"));
        let steps = scenario.steps().len();
        let mut harness = Harness::new(scenario, SIZE)
            .unwrap_or_else(|error| panic!("measurement terminal: {error}"));
        harness
            .warm(steps.saturating_sub(WINDOW))
            .unwrap_or_else(|error| panic!("warm frame: {error}"));

        // Select the last agent, so most of the remaining traffic belongs to somebody else.
        for _ in 0..3 {
            harness.workspace.handle(&Event::Key(KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE,
            )));
        }
        harness
            .draw()
            .unwrap_or_else(|error| panic!("measured frame: {error}"));
        let before = harness.items_on_screen();

        let mut foreground = 0;
        for _ in 0..WINDOW {
            harness.advance();
            if let Some(work) = harness
                .draw()
                .unwrap_or_else(|error| panic!("measured frame: {error}"))
            {
                foreground += work.items_wrapped;
            }
        }

        let own = harness.items_on_screen().saturating_sub(before);
        assert!(
            own > 0,
            "the selected conversation has to grow, or this proves nothing"
        );
        // One message of slack, because the window can open and close part-way through one.
        let allowed = own.saturating_add(1).saturating_mul(PER_MESSAGE);
        assert!(
            foreground <= allowed,
            "{WINDOW} events across four agents grew the selected conversation by {own} messages \
             and cost it {foreground} wraps, more than the {allowed} its own traffic can explain"
        );
    }
}
