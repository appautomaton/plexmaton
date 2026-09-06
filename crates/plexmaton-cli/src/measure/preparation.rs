//! Real-process preparation cost, separately from frame/input latency and the in-process reference.

use std::time::Instant;

use anyhow::{Context as _, ensure};
use plexmaton_cli::preparation::{Completion, Preparation};
use plexmaton_core::{AgentId, TranscriptItemId, TranscriptRole};
use plexmaton_tui::{
    TranscriptEntryView, TranscriptItemView, TranscriptTextKind, preparation::Request,
};

const BATCHES: usize = 32;
const ITEMS: usize = 16;

fn requests() -> Vec<Request> {
    (0..ITEMS).map(|index| Request::new(
        AgentId::new("measurement").expect("fixed identity"),
        TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new(format!("entry-{index}")).expect("bounded fixture identity"),
            role: TranscriptRole::Assistant, kind: TranscriptTextKind::Message,
            source: "## Notes\n\nA **bounded** paragraph with `code` and 中文 e\u{301}.\n\n> Context.\n\n- First item\n- Second item".into(),
            revision: 1, finalized: true,
        }), 120, false,
    )).collect()
}

pub(super) fn report() -> anyhow::Result<()> {
    let executable = std::env::current_exe().context("resolve measurement child")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut owner = Preparation::new(executable);
        let mut times = Vec::new();
        let operation = async {
            for _ in 0..BATCHES {
                let requests = requests();
                let start = Instant::now();
                let ticket = owner.submit(requests)?;
                let Completion::Ready(received, Ok(results)) = owner.next().await else {
                    anyhow::bail!("preparation measurement did not complete");
                };
                times.push(start.elapsed());
                ensure!(received == ticket && results.len() == ITEMS, "preparation work changed");
                for result in results {
                    ensure!(result.row_count().is_ok_and(|rows| rows > 0), "prepared rows missing");
                    ensure!(result.selection_text().is_ok_and(|text| text.contains("中文 e\u{301}")), "prepared source changed");
                }
            }
            Ok::<_, anyhow::Error>(())
        }.await;
        let cleanup = owner.shutdown().await.context("reap measurement child");
        operation?;
        cleanup?;
        let first = times.remove(0);
        times.sort_unstable();
        println!("\nOwned preparation: {BATCHES} batches x {ITEMS} rich entries; real child + framed pipes + decoding.");
        println!("  first batch {:>7} us; warm p50 {:>7} us; input and terminal latency excluded", first.as_micros(), times[times.len() / 2].as_micros());
        Ok(())
    })
}
