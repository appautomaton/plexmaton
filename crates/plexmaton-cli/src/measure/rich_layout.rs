//! Cold rich histories and paint invalidation are separate workloads from one warm stream.
use super::{Harness, SCALES, SIZE};
use anyhow::ensure;
use plexmaton_core::{
    AgentId, ConversationEvent, ConversationEventEnvelope, EventSequence, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_sim::Scenario;
use plexmaton_tui::{FrameWork, Palette};
use std::time::{Duration, Instant};

const SOURCE: &str = "## Notes\n\nA **bounded** paragraph with `inline code` and a [link](https://example.invalid).\n\n> Context.\n\n- First item\n- Second item\n";
const REPETITIONS: usize = 3;

struct Observation {
    elapsed: Duration,
    work: FrameWork,
    layouts: usize,
}

fn draw(harness: &mut Harness) -> anyhow::Result<Observation> {
    let before = harness.workspace.metrics().text_layouts();
    let started = Instant::now();
    let work = harness
        .draw()?
        .ok_or_else(|| anyhow::anyhow!("rich layout sample did not paint"))?;
    Ok(Observation {
        elapsed: started.elapsed(),
        work,
        layouts: harness.workspace.metrics().text_layouts() - before,
    })
}

fn sample(messages: usize) -> anyhow::Result<[Observation; 3]> {
    let scenario = Scenario::streaming(0)?;
    let mut sequence = u64::try_from(scenario.steps().len())?;
    let mut harness = Harness::new(scenario, SIZE)?;
    harness.advance_to(u64::MAX);
    let agent = AgentId::new("agent-0")?;
    for index in 0..messages {
        let item = TranscriptItemId::new(format!("rich-{index}"))?;
        for event in [
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item,
                item_revision: 1,
                text: SOURCE.into(),
            },
        ] {
            sequence += 1;
            harness.workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            }]);
        }
    }
    ensure!(
        harness.entries_on_screen() == messages,
        "rich history count changed"
    );
    let cold = draw(&mut harness)?;
    harness.resize((88, 40));
    let resized = draw(&mut harness)?;
    harness.workspace.set_palette(Palette::pastel());
    let painted = draw(&mut harness)?;
    ensure!(
        cold.work.entries_wrapped == messages && resized.work.entries_wrapped == messages,
        "cold rich history did not measure every entry once"
    );
    ensure!(
        painted.work.entries_wrapped == 0,
        "color invalidated rich history heights"
    );
    Ok([cold, resized, painted])
}

pub(super) fn report() -> anyhow::Result<()> {
    println!(
        "\nRich-Markdown history: every entry contains a heading, inline styles/link, quote and list."
    );
    println!(
        "  120x40 cold -> 88x40 resize -> palette repaint; median of {REPETITIONS}; no terminal I/O"
    );
    println!("| entries | stage           | wraps | layouts | processing |");
    println!("| ------- | --------------- | ----- | ------- | ---------- |");
    for messages in SCALES {
        let samples = (0..REPETITIONS)
            .map(|_| sample(messages))
            .collect::<anyhow::Result<Vec<_>>>()?;
        for (stage, name) in ["cold rich open", "rich resize", "rich palette"]
            .into_iter()
            .enumerate()
        {
            let mut times: Vec<_> = samples.iter().map(|sample| sample[stage].elapsed).collect();
            times.sort_unstable();
            let observed = &samples[0][stage];
            for sample in &samples {
                ensure!(
                    sample[stage].work == observed.work
                        && sample[stage].layouts == observed.layouts,
                    "rich history work is nondeterministic"
                );
            }
            println!(
                "| {messages:>7} | {name:<15} | {:>5} | {:>7} | {:>7} us |",
                observed.work.entries_wrapped,
                observed.layouts,
                times[REPETITIONS / 2].as_micros()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// FR-4/TR-1/MD-4: a rich-history palette repaint reaches only visible styled layouts.
    #[test]
    fn rich_history_measurement_separates_cold_resize_and_paint_work() {
        for messages in [16, 160] {
            let [cold, resized, paint] = super::sample(messages).expect("rich history");
            assert_eq!(cold.layouts, messages);
            assert_eq!(resized.layouts, messages);
            assert_eq!(paint.work.entries_wrapped, 0);
            assert!(paint.layouts > 0 && paint.layouts < messages);
        }
    }
}
