//! The measurement harness: what a frame costs, in work and in wall time.
//!
//! Run it with `cargo run --release -p plexmaton-cli --bin plexmaton-measure`. The workloads are
//! the ones `phase-00 §responsiveness workloads` declares, driven through the same `Workspace` the
//! executable drives, so a measured frame is the frame the user gets.
//!
//! Work counts are deterministic and are asserted by the tests below and in `plexmaton-tui`.
//! Timings are not: they belong to whichever machine ran the command, which is why the report names
//! it rather than pretending a number is portable (FR-3).

#![allow(
    clippy::print_stdout,
    reason = "this binary owns no screen: it draws into a TestBackend and writes one report to \
              stdout, which is the only way its evidence reaches a reader"
)]

use std::time::{Duration, Instant};

use anyhow::Context;
use plexmaton_sim::{Runtime, Scenario};
use plexmaton_tui::{FrameWork, SurfaceId, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::Rect,
};

/// The terminal the workloads run at, unless one of them is about changing it.
const SIZE: (u16, u16) = (120, 40);

/// Conversation lengths every workload is run at.
///
/// Two, because the shape of the curve is the finding. One size cannot distinguish a frame whose
/// cost is bounded by the viewport from one that is merely fast today.
const SCALES: [usize; 2] = [500, 5_000];

/// Samples per workload. A cold frame gets fewer, because each one rebuilds the whole cache.
const SAMPLES: usize = 200;
const COLD_SAMPLES: usize = 10;

/// The sizes the resize workload cycles through: ultrawide, wide, and narrow.
const RESIZES: [(u16, u16); 3] = [(160, 40), (100, 30), (60, 24)];

fn main() -> anyhow::Result<()> {
    let mut runs = Vec::new();
    for items in SCALES {
        runs.push(cold_open(items)?);
        runs.push(streaming(items)?);
        runs.push(interleaved(items)?);
        runs.push(wheel(items)?);
        runs.push(switch_reader(items)?);
        runs.push(resize(items)?);
        runs.push(inspector(items)?);
    }
    report(&runs);
    Ok(())
}

/// A projection, its cache, a terminal, and the timeline feeding them.
struct Harness {
    workspace: Workspace,
    terminal: Terminal<TestBackend>,
    runtime: Runtime,
    tick: u64,
}

impl Harness {
    fn new(scenario: Scenario, size: (u16, u16)) -> anyhow::Result<Self> {
        let (width, height) = size;
        Ok(Self {
            workspace: Workspace::default(),
            terminal: Terminal::new(TestBackend::new(width, height))
                .context("build the measurement terminal")?,
            runtime: Runtime::new(scenario),
            tick: 0,
        })
    }

    /// Releases every event scheduled at or before `tick`.
    fn advance_to(&mut self, tick: u64) {
        self.tick = tick;
        let ready = self.runtime.ready(tick);
        self.workspace.emit(ready);
    }

    /// Releases the next tick's worth of events, which is one event in these workloads.
    fn advance(&mut self) {
        self.advance_to(self.tick.saturating_add(1));
    }

    fn draw(&mut self) -> anyhow::Result<Option<FrameWork>> {
        self.workspace
            .draw(&mut self.terminal)
            .context("draw a measured frame")
    }

    fn resize(&mut self, size: (u16, u16)) {
        let (width, height) = size;
        self.terminal.backend_mut().resize(width, height);
        self.workspace.handle(&Event::Resize(width, height));
    }

    /// A pointer just inside a surface, so an event aims where the user's would.
    fn inside(&self, surface_id: SurfaceId) -> (u16, u16) {
        let bounds = self
            .workspace
            .surfaces()
            .get(surface_id)
            .map_or(Rect::default(), |surface| surface.bounds);
        (
            bounds.x.saturating_add(1),
            bounds.y.saturating_add(bounds.height / 2),
        )
    }

    /// Items in the conversation on screen, reported rather than assumed.
    fn items_on_screen(&self) -> usize {
        self.workspace
            .state()
            .selected_agent()
            .map_or(0, |agent| agent.transcript().count())
    }

    /// Applies every scheduled event and paints the frame that shows them.
    fn warm(&mut self, keep_back: usize) -> anyhow::Result<()> {
        self.advance_to(u64::try_from(keep_back).unwrap_or(u64::MAX));
        self.draw()?;
        Ok(())
    }
}

/// One workload's result.
struct Run {
    workload: &'static str,
    items: usize,
    wrapped: usize,
    built: usize,
    retained: usize,
    latencies: Vec<Duration>,
}

impl Run {
    fn new(workload: &'static str) -> Self {
        Self {
            workload,
            items: 0,
            wrapped: 0,
            built: 0,
            retained: 0,
            latencies: Vec::new(),
        }
    }

    /// Records one sample, keeping the *worst* frame's work rather than an average.
    ///
    /// A budget is about the frame the user waits on, and averaging work over samples would hide
    /// exactly the frame worth knowing about.
    fn record(&mut self, elapsed: Duration, work: Option<FrameWork>) {
        let Some(work) = work else {
            // Nothing painted, so there is no frame to attribute this time to.
            return;
        };
        self.wrapped = self.wrapped.max(work.items_wrapped);
        self.built = self.built.max(work.lines_built);
        self.latencies.push(elapsed);
    }

    fn finish(mut self, harness: &Harness) -> Self {
        self.items = harness.items_on_screen();
        self.retained = harness.workspace.metrics().retained();
        self.latencies.sort_unstable();
        self
    }

    fn percentile(&self, fraction: f64) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an index into at most a few hundred samples is exact in f64"
        )]
        let index = ((self.latencies.len() - 1) as f64 * fraction).round() as usize;
        self.latencies.get(index).copied().unwrap_or_default()
    }
}

/// The first frame on a conversation nothing has measured.
///
/// The one frame that is deliberately proportional to history: knowing how tall a conversation is
/// means wrapping every item once, and that is what makes every later frame cheap (TR-1).
fn cold_open(items: usize) -> anyhow::Result<Run> {
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
fn streaming(items: usize) -> anyhow::Result<Run> {
    let scenario = Scenario::streaming(items.saturating_add(SAMPLES))?;
    sample_events("streaming delta", scenario, SIZE)
}

/// Four conversations streaming with tool activity, one of them on screen.
fn interleaved(items: usize) -> anyhow::Result<Run> {
    let scenario = Scenario::interleaved(4, items.saturating_add(SAMPLES))?;
    sample_events("four agents", scenario, SIZE)
}

/// Applies one event and paints, `SAMPLES` times, after warming on everything before them.
fn sample_events(
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
fn wheel(items: usize) -> anyhow::Result<Run> {
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
fn switch_reader(items: usize) -> anyhow::Result<Run> {
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
fn resize(items: usize) -> anyhow::Result<Run> {
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
fn inspector(items: usize) -> anyhow::Result<Run> {
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

fn report(runs: &[Run]) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let (width, height) = SIZE;
    println!("Plexmaton frame measurements");
    println!("  profile   {profile}");
    println!("  terminal  {width}x{height}, except the resize workload");
    println!("  timings   this machine only; work columns are the same everywhere");
    println!();
    println!(
        "| {:<16} | {:>6} | {:>7} | {:>6} | {:>6} | {:>8} | {:>8} | {:>8} | {:>8} |",
        "workload", "items", "samples", "wraps", "lines", "retained", "p50", "p95", "max"
    );
    println!(
        "| {:-<16} | {:->6} | {:->7} | {:->6} | {:->6} | {:->8} | {:->8} | {:->8} | {:->8} |",
        "", "", "", "", "", "", "", "", ""
    );
    for run in runs {
        println!(
            "| {:<16} | {:>6} | {:>7} | {:>6} | {:>6} | {:>8} | {:>8} | {:>8} | {:>8} |",
            run.workload,
            run.items,
            run.latencies.len(),
            run.wrapped,
            run.built,
            run.retained,
            micros(run.percentile(0.50)),
            micros(run.percentile(0.95)),
            micros(run.percentile(1.00)),
        );
    }
    println!();
    println!("wraps and lines are the worst frame in the run, not an average.");
}

fn micros(duration: Duration) -> String {
    format!("{} us", duration.as_micros())
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
