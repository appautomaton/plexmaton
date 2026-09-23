//! Where a compaction the runtime takes on its own shows itself, for review (stage 30).
//! cargo run -p plexmaton-tui --example automatic_compaction_preview -- <output-directory>
//!
//! An automatic checkpoint lands mid-turn, between the message the user sent and its answer, where
//! a requested one lands after the last entry. Two frames of that turn: while the summarizer runs,
//! and afterwards, with the checkpoint's row where it landed. The user chose the durable row over
//! the transient note `/compact` used to leave, which a reopened conversation lost.

use std::path::Path;

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{ConfigurationSummary, MarkdownTheme, Palette, Workspace};
use ratatui::{Terminal, backend::TestBackend};

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Stream {
    agent: AgentId,
    sequence: u64,
}

impl Stream {
    fn send(&mut self, workspace: &mut Workspace, events: Vec<ConversationEvent>) {
        let envelopes = events
            .into_iter()
            .map(|event| {
                self.sequence += 1;
                ConversationEventEnvelope {
                    sequence: EventSequence::new(self.sequence),
                    event,
                }
            })
            .collect();
        workspace.emit(envelopes);
    }

    fn text(
        &self,
        name: &str,
        role: TranscriptRole,
        source: &str,
    ) -> Result<Vec<ConversationEvent>> {
        let item = TranscriptItemId::new(name)?;
        Ok(vec![
            ConversationEvent::TranscriptItemStarted {
                agent_id: self.agent.clone(),
                item_id: item.clone(),
                role,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: self.agent.clone(),
                item_id: item.clone(),
                item_revision: 1,
                text: source.to_owned(),
            },
            ConversationEvent::TranscriptItemFinalized {
                agent_id: self.agent.clone(),
                item_id: item,
                item_revision: 2,
            },
        ])
    }

    fn status(&self, status: AgentStatus) -> Vec<ConversationEvent> {
        vec![ConversationEvent::AgentStatusChanged {
            agent_id: self.agent.clone(),
            status,
        }]
    }
}

/// The conversation up to the message whose turn triggers the checkpoint.
fn before_the_checkpoint() -> Result<(Workspace, Stream)> {
    let agent = AgentId::new("primary")?;
    let mut stream = Stream {
        agent: agent.clone(),
        sequence: 0,
    };
    let mut workspace =
        Workspace::with_palette(Palette::pastel().with_markdown_theme(MarkdownTheme::Pastel));
    stream.send(
        &mut workspace,
        vec![ConversationEvent::AgentCreated {
            agent_id: agent,
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        }],
    );
    let earlier = [
        stream.text(
            "q1",
            TranscriptRole::User,
            "Split the session store into modules, keeping its public API unchanged.",
        )?,
        stream.text(
            "a1",
            TranscriptRole::Assistant,
            "Done: `store/`, `store/index.rs` and `store/journal.rs`, with the old paths re-exported. \
             Nothing outside the crate had to change.",
        )?,
        stream.text(
            "q2",
            TranscriptRole::User,
            "Now update the call sites in the runtime and run the tests.",
        )?,
    ];
    for events in earlier {
        stream.send(&mut workspace, events);
    }
    stream.send(&mut workspace, stream.status(AgentStatus::Running));
    workspace.set_model(ConfigurationSummary {
        provider: "local".into(),
        model: "muse-spark-1.3".into(),
        display_name: "Muse Spark 1.3".into(),
        configured_name: "muse".into(),
        reasoning_effort: ReasoningEffort::High,
    });
    workspace.set_working_directory("~/plexmaton".into());
    Ok((workspace, stream))
}

fn answer(stream: &Stream) -> Result<Vec<ConversationEvent>> {
    stream.text(
        "a2",
        TranscriptRole::Assistant,
        "Updated the 14 call sites in the runtime; all 212 tests pass.",
    )
}

fn write(directory: &Path, name: &str, workspace: &mut Workspace, width: u16) -> Result<()> {
    let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
    prepared_frame::draw(workspace, &mut terminal)?;
    std::fs::write(
        directory.join(format!("{name}-{width}.svg")),
        frame_svg::svg(terminal.backend().buffer()),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let primary = AgentId::new("primary")?;
    for width in [120, 88, 60] {
        // While the summarizer runs: the activity line says so, whoever asked for it.
        let (mut workspace, mut stream) = before_the_checkpoint()?;
        let started = ConversationEvent::CompactionStarted {
            agent_id: primary.clone(),
        };
        stream.send(&mut workspace, vec![started.clone()]);
        write(directory, "running", &mut workspace, width)?;

        // Afterwards: the checkpoint's row where it landed, then the answer it made room for.
        let (mut workspace, mut stream) = before_the_checkpoint()?;
        let mut events = vec![
            started,
            ConversationEvent::CompactionEnded {
                agent_id: primary.clone(),
            },
            ConversationEvent::ContextCompacted {
                agent_id: primary.clone(),
                item_id: TranscriptItemId::new("primary-item-j9")?,
            },
        ];
        events.extend(answer(&stream)?);
        events.extend(stream.status(AgentStatus::Idle));
        stream.send(&mut workspace, events);
        write(directory, "row-in-place", &mut workspace, width)?;
    }
    Ok(())
}
