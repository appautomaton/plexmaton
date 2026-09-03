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

use std::time::Duration;

use anyhow::Context;
use plexmaton_sim::{Scenario, ScriptedRuntime};
use plexmaton_tui::{FrameWork, SurfaceId, Workspace};
use ratatui::{Terminal, backend::TestBackend, crossterm::event::Event, layout::Rect};

/// The terminal the workloads run at, unless one of them is about changing it.
pub(crate) const SIZE: (u16, u16) = (120, 40);

/// Assistant-message counts every workload is run at; tool entries are added by the fixture.
///
/// Two, because the shape of the curve is the finding. One size cannot distinguish a frame whose
/// cost is bounded by the viewport from one that is merely fast today.
const SCALES: [usize; 2] = [500, 5_000];

/// Samples per workload. A cold frame gets fewer, because each one rebuilds the whole cache.
pub(crate) const SAMPLES: usize = 200;
pub(crate) const COLD_SAMPLES: usize = 10;

/// The sizes the resize workload cycles through: ultrawide, wide, and narrow.
pub(crate) const RESIZES: [(u16, u16); 3] = [(160, 40), (100, 30), (60, 24)];

mod workloads;

use workloads::{
    cold_open, hidden_conversation, inspector, interleaved, resize, select, streaming,
    two_conversations, wheel,
};

fn main() -> anyhow::Result<()> {
    let mut runs = Vec::new();
    for messages in SCALES {
        runs.push(cold_open(messages)?);
        runs.push(streaming(messages)?);
        runs.push(interleaved(messages)?);
        runs.push(wheel(messages)?);
        runs.push(resize(messages)?);
        runs.push(inspector(messages)?);
        runs.push(hidden_conversation(messages)?);
        runs.push(two_conversations(messages)?);
        runs.push(select(messages)?);
    }
    report(&runs);
    Ok(())
}

/// A projection, its cache, a terminal, and the timeline feeding them.
pub(crate) struct Harness {
    pub(crate) workspace: Workspace,
    terminal: Terminal<TestBackend>,
    runtime: ScriptedRuntime,
    tick: u64,
}

impl Harness {
    pub(crate) fn new(scenario: Scenario, size: (u16, u16)) -> anyhow::Result<Self> {
        let (width, height) = size;
        Ok(Self {
            workspace: Workspace::default(),
            terminal: Terminal::new(TestBackend::new(width, height))
                .context("build the measurement terminal")?,
            runtime: ScriptedRuntime::new(scenario),
            tick: 0,
        })
    }

    /// Releases every event scheduled at or before `tick`.
    pub(crate) fn advance_to(&mut self, tick: u64) {
        self.tick = tick;
        let ready = self.runtime.ready(tick);
        self.workspace.emit(ready);
    }

    /// Releases the next tick's worth of events, which is one event in these workloads.
    pub(crate) fn advance(&mut self) {
        self.advance_to(self.tick.saturating_add(1));
    }

    pub(crate) fn draw(&mut self) -> anyhow::Result<Option<FrameWork>> {
        self.workspace
            .draw(&mut self.terminal)
            .context("draw a measured frame")
    }

    pub(crate) fn resize(&mut self, size: (u16, u16)) {
        let (width, height) = size;
        self.terminal.backend_mut().resize(width, height);
        self.workspace.handle(&Event::Resize(width, height));
    }

    /// A pointer just inside a surface, so an event aims where the user's would.
    pub(crate) fn inside(&self, surface_id: SurfaceId) -> (u16, u16) {
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

    /// Entries in the primary's conversation, which is the one always on screen.
    pub(crate) fn entries_on_screen(&self) -> usize {
        self.workspace
            .state()
            .primary_agent()
            .map_or(0, |agent| agent.entries().count())
    }

    /// Applies every scheduled event and paints the frame that shows them.
    pub(crate) fn warm(&mut self, keep_back: usize) -> anyhow::Result<()> {
        self.advance_to(u64::try_from(keep_back).unwrap_or(u64::MAX));
        self.draw()?;
        Ok(())
    }
}

/// One workload's result.
pub(crate) struct Run {
    workload: &'static str,
    entries: usize,
    wrapped: usize,
    built: usize,
    retained: usize,
    latencies: Vec<Duration>,
}

impl Run {
    pub(crate) fn new(workload: &'static str) -> Self {
        Self {
            workload,
            entries: 0,
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
    pub(crate) fn record(&mut self, elapsed: Duration, work: Option<FrameWork>) {
        let Some(work) = work else {
            // Nothing painted, so there is no frame to attribute this time to.
            return;
        };
        self.wrapped = self.wrapped.max(work.entries_wrapped);
        self.built = self.built.max(work.lines_built);
        self.latencies.push(elapsed);
    }

    pub(crate) fn finish(mut self, harness: &Harness) -> Self {
        self.entries = harness.entries_on_screen();
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
        "| {:<24} | {:>7} | {:>7} | {:>6} | {:>6} | {:>8} | {:>8} | {:>8} | {:>8} |",
        "workload", "entries", "samples", "wraps", "lines", "retained", "p50", "p95", "max"
    );
    println!(
        "| {:-<24} | {:->7} | {:->7} | {:->6} | {:->6} | {:->8} | {:->8} | {:->8} | {:->8} |",
        "", "", "", "", "", "", "", "", ""
    );
    for run in runs {
        println!(
            "| {:<24} | {:>7} | {:>7} | {:>6} | {:>6} | {:>8} | {:>8} | {:>8} | {:>8} |",
            run.workload,
            run.entries,
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
