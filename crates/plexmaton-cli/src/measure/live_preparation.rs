//! Real executable-to-workspace cost: first pending paint and all reached preparation settled.

use anyhow::{Context as _, ensure};
use plexmaton_cli::preparation::LivePreparation;
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::Workspace;
use ratatui::{Terminal, backend::TestBackend};
use std::time::{Duration, Instant};

struct Observation {
    first: Duration,
    settled: Duration,
    frames: u64,
    layouts: usize,
}

fn workspace(messages: usize) -> anyhow::Result<Workspace> {
    let mut workspace = Workspace::default();
    let agent = AgentId::new("primary")?;
    let mut sequence = 0;
    let mut emit = |event| {
        sequence += 1;
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(sequence),
            event,
        }]);
    };
    emit(ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Running,
    });
    for index in 0..messages {
        let item = TranscriptItemId::new(format!("rich-{index}"))?;
        emit(ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        });
        emit(ConversationEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            text: super::rich_layout::SOURCE.into(),
        });
    }
    Ok(workspace)
}

async fn sample(messages: usize, width: u16) -> anyhow::Result<Observation> {
    let mut workspace = workspace(messages)?;
    let mut terminal = Terminal::new(TestBackend::new(width, 40))?;
    let mut owner =
        LivePreparation::new(std::env::current_exe().context("locate measurement child")?);
    let started = Instant::now();
    let operation = async {
        workspace.draw(&mut terminal)?;
        let first = started.elapsed();
        ensure!(
            workspace.metrics().text_layouts() == 0,
            "pending paint performed preparation"
        );
        for _ in 0..128 {
            owner.sync(&mut workspace);
            if owner.is_pending() {
                let result = owner.next().await;
                owner.apply(result, &mut workspace);
            } else if !workspace.needs_draw() {
                let observation = Observation {
                    first,
                    settled: started.elapsed(),
                    frames: workspace.frames(),
                    layouts: workspace.metrics().text_layouts(),
                };
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .chunks(usize::from(width))
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                ensure!(
                    text.contains("Second item")
                        && !text.contains("Preparing text")
                        && !text.contains("Text unavailable"),
                    "live preparation did not paint complete data"
                );
                ensure!(
                    observation.layouts > 0 && observation.layouts <= 40,
                    "preparation reached hidden history"
                );
                return Ok::<_, anyhow::Error>(observation);
            }
            workspace.draw(&mut terminal)?;
        }
        anyhow::bail!("live preparation measurement did not settle")
    }
    .await;
    let cleanup = owner
        .shutdown()
        .await
        .context("reap live measurement worker");
    let observation = operation?;
    cleanup?;
    Ok(observation)
}

pub(super) fn report() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        println!("\nLive preparation: real executable, framed pipes, workspace adoption and TestBackend paints; cold, median of 3.");
        println!("  first pending paint / reached data settled; no terminal transport or outer input wait.");
        println!("| history | width | first paint | settled | frames | prepared |");
        println!("| ------- | ----- | ----------- | ------- | ------ | -------- |");
        for messages in super::SCALES {
            for width in [120, 88, 60] {
                let mut observations = Vec::new();
                for _ in 0..3 { observations.push(sample(messages, width).await?); }
                let frames = observations[0].frames;
                let layouts = observations[0].layouts;
                ensure!(observations.iter().all(|o| o.frames == frames && o.layouts == layouts), "live work changed across identical samples");
                let mut first: Vec<_> = observations.iter().map(|o| o.first).collect();
                let mut settled: Vec<_> = observations.iter().map(|o| o.settled).collect();
                first.sort_unstable(); settled.sort_unstable();
                println!("| {messages:>7} | {width:>5} | {:>8} us | {:>4} us | {frames:>6} | {layouts:>8} |", first[1].as_micros(), settled[1].as_micros());
            }
        }
        Ok(())
    })
}
