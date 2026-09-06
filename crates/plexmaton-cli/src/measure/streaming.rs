//! FR-4/FR-5: compare immediate drawing with the production coalescer on identical semantic input.

use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use plexmaton_core::{
    AgentId, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{SurfaceId, TranscriptEntryView};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::{Harness, SCALES, SIZE, stream_frames::StreamFrames};

const DELTAS: u64 = 160;
const REPETITIONS: usize = 3;
const PREFIX: &str = "### Notes\n\nA **bounded** paragraph with `inline code` and a [link](https://example.invalid).\n\n> Quoted context.\n\n- First item\n- Second item\n\n";
const CHUNKS: [&str; 8] = [
    "**bo",
    "ld** ",
    "`va",
    "lue` ",
    "中🙂 ",
    "[li",
    "nk](https://example.invalid) ",
    "\n\n",
];

#[derive(Clone, Copy)]
enum Drawing {
    Immediate,
    Coalesced,
}

impl Drawing {
    const fn label(self) -> &'static str {
        match self {
            Self::Immediate => "immediate reference",
            Self::Coalesced => "production coalescer",
        }
    }
}

#[derive(Clone, Copy)]
enum Arrivals {
    Burst,
    EveryMillisecond,
}

impl Arrivals {
    const fn elapsed(self, index: u64) -> Duration {
        match self {
            Self::Burst => Duration::from_millis(1),
            Self::EveryMillisecond => Duration::from_millis(index),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Burst => "one burst",
            Self::EveryMillisecond => "1 ms arrivals",
        }
    }
}

struct Observation {
    entries: usize,
    frames: u64,
    layouts: usize,
    elapsed: Duration,
    input: Duration,
}

fn envelope(sequence: &mut u64, event: SessionEvent) -> SessionEventEnvelope {
    *sequence += 1;
    SessionEventEnvelope {
        sequence: EventSequence::new(*sequence),
        event,
    }
}

fn sample(messages: usize, drawing: Drawing, arrivals: Arrivals) -> anyhow::Result<Observation> {
    let scenario = plexmaton_sim::Scenario::streaming(messages)?;
    // All scripted history is consumed before this driver becomes the sole stream producer.
    let mut sequence = u64::try_from(scenario.steps().len())?;
    let mut harness = Harness::new(scenario, SIZE)?;
    harness.warm(usize::MAX)?;
    let agent = AgentId::new("agent-0")?;
    let item = TranscriptItemId::new("rich-stream")?;
    let now = Instant::now();
    let mut frames = StreamFrames::new(now);
    let mut expected = PREFIX.repeat(32);
    for event in [
        SessionEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        },
        SessionEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item.clone(),
            item_revision: 1,
            text: expected.clone(),
        },
    ] {
        frames.receive(&mut harness.workspace, envelope(&mut sequence, event));
    }
    harness.draw_coalesced(&mut frames, now)?;
    for _ in 0..harness.workspace.surfaces().len() {
        if harness
            .workspace
            .state()
            .focused(harness.workspace.surfaces())
            == Some(SurfaceId::Composer)
        {
            break;
        }
        frames.handle(
            &mut harness.workspace,
            &Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        );
        harness.draw_coalesced(&mut frames, now)?;
    }
    ensure!(
        harness
            .workspace
            .state()
            .focused(harness.workspace.surfaces())
            == Some(SurfaceId::Composer),
        "input workload must own the composer cursor"
    );

    let painted = harness.workspace.frames();
    let layouts = harness.workspace.metrics().text_layouts();
    let mut input = Duration::ZERO;
    let started = Instant::now();
    for index in 1..=DELTAS {
        let text = CHUNKS[usize::try_from(index - 1)? % CHUNKS.len()];
        expected.push_str(text);
        let event = envelope(
            &mut sequence,
            SessionEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item.clone(),
                item_revision: index + 1,
                text: text.into(),
            },
        );
        let at = now + arrivals.elapsed(index);
        match drawing {
            Drawing::Immediate => {
                harness.workspace.emit(vec![event]);
                harness.draw()?;
            }
            Drawing::Coalesced => {
                frames.receive(&mut harness.workspace, event);
                harness.draw_coalesced(&mut frames, at)?;
            }
        }
        if index == DELTAS / 2 {
            let input_started = Instant::now();
            frames.handle(
                &mut harness.workspace,
                &Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            );
            let work = harness.draw_coalesced(&mut frames, at)?;
            input = input_started.elapsed();
            ensure!(
                work.is_some(),
                "the input sample must paint before another stream event"
            );
        }
    }
    frames.flush(&mut harness.workspace);
    harness.draw_coalesced(&mut frames, now + arrivals.elapsed(DELTAS))?;
    let elapsed = started.elapsed();
    ensure!(
        frames.deadline().is_none(),
        "a drained stream must leave no timer"
    );
    let stream = harness
        .workspace
        .state()
        .primary_agent()
        .context("primary")?
        .entries()
        .find(|entry| entry.id() == &item)
        .context("streamed item")?;
    ensure!(
        matches!(stream, TranscriptEntryView::Text(text) if text.source == expected),
        "stream coalescing changed exact source"
    );
    ensure!(
        stream.revision() == DELTAS + 1,
        "stream coalescing lost a revision"
    );
    ensure!(
        harness.workspace.state().composer().text() == "x",
        "stream traffic lost input"
    );
    Ok(Observation {
        entries: harness.entries_on_screen(),
        frames: harness.workspace.frames() - painted,
        layouts: harness.workspace.metrics().text_layouts() - layouts,
        elapsed,
        input,
    })
}

pub(super) fn report() -> anyhow::Result<()> {
    println!(
        "\nRich-Markdown stream: {DELTAS} deltas into one existing rich entry; one typed input halfway."
    );
    println!(
        "  terminal  {}x{}, warm history; median of {REPETITIONS} repetitions",
        SIZE.0, SIZE.1
    );
    println!(
        "  clock     deterministic arrival timeline; wall time measures processing, not scheduled wait or terminal I/O"
    );
    println!(
        "| history | arrival       | drawing              | frames | layouts | processing | input frame |"
    );
    println!(
        "| ------- | ------------- | -------------------- | ------ | ------- | ---------- | ----------- |"
    );
    for messages in SCALES {
        for arrivals in [Arrivals::Burst, Arrivals::EveryMillisecond] {
            for drawing in [Drawing::Immediate, Drawing::Coalesced] {
                let runs = (0..REPETITIONS)
                    .map(|_| sample(messages, drawing, arrivals))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                let first = runs.first().context("nonempty measurement")?;
                ensure!(
                    runs.iter()
                        .all(|run| run.frames == first.frames && run.layouts == first.layouts),
                    "clock-independent work changed between repetitions"
                );
                let mut elapsed: Vec<_> = runs.iter().map(|run| run.elapsed).collect();
                let mut input: Vec<_> = runs.iter().map(|run| run.input).collect();
                elapsed.sort_unstable();
                input.sort_unstable();
                println!(
                    "| {:>7} | {:<13} | {:<20} | {:>6} | {:>7} | {:>7} us | {:>8} us |",
                    first.entries,
                    arrivals.label(),
                    drawing.label(),
                    first.frames,
                    first.layouts,
                    elapsed[REPETITIONS / 2].as_micros(),
                    input[REPETITIONS / 2].as_micros()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-4/FR-5: the measured production path bounds redraw/map work without omitting input.
    #[test]
    fn rich_stream_measurement_asserts_frame_work_and_exact_source_at_both_scales() {
        for messages in [16, 160] {
            for arrivals in [Arrivals::Burst, Arrivals::EveryMillisecond] {
                let direct = sample(messages, Drawing::Immediate, arrivals).expect("reference");
                assert_eq!(
                    direct.frames,
                    DELTAS * 2 + 1,
                    "projection and preparation frames, plus input"
                );
                assert_eq!(
                    direct.layouts,
                    usize::try_from(DELTAS).expect("small count")
                );
                let coalesced = sample(messages, Drawing::Coalesced, arrivals).expect("production");
                match arrivals {
                    Arrivals::Burst => {
                        assert_eq!(coalesced.frames, 8);
                        assert_eq!(coalesced.layouts, 4);
                    }
                    Arrivals::EveryMillisecond => {
                        assert_eq!(coalesced.frames, 21);
                        assert_eq!(coalesced.layouts, 10);
                    }
                }
                assert_eq!(direct.entries, coalesced.entries);
            }
        }
    }
}
