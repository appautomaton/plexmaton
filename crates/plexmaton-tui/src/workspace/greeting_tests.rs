//! Launch's greeting: once, over an empty conversation, on the motion clock, then gone.

use std::time::{Duration, Instant};

use super::*;
use plexmaton_core::{
    AgentStatus, ConversationEvent, EventSequence, TranscriptItemId, TranscriptRole,
};
use ratatui::backend::TestBackend;

const PRIMARY: &str = "primary";

/// A new, empty, idle conversation, as launch opens onto.
fn fresh() -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: AgentId::new(PRIMARY).expect("agent"),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
    }]);
    let terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
    (workspace, terminal)
}

fn conversation(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> String {
    workspace.settled_draw(terminal).expect("draw");
    crate::test_support::region_text(
        terminal.backend().buffer(),
        workspace
            .surfaces
            .get(crate::SurfaceId::Transcript)
            .expect("conversation")
            .bounds,
    )
}

fn braille(text: &str) -> bool {
    text.chars()
        .any(|glyph| ('\u{2801}'..='\u{28FF}').contains(&glyph))
}

/// Runs the motion clock from `start` to `until`, one wake at a time, as the event loop does.
fn run(workspace: &mut Workspace, start: Instant, until: Duration) {
    let mut now = start;
    while now <= start + until {
        let Some(deadline) = workspace.motion_deadline(now) else {
            return;
        };
        now = deadline.max(now);
        workspace.advance_motion(now);
    }
}

/// Launch greets an empty conversation with the mark and its name, on the motion clock; after
/// about two and a half seconds it has left and wakes the clock no more.
#[test]
fn launch_greets_once_and_leaves() {
    let (mut workspace, mut terminal) = fresh();
    let start = Instant::now();
    workspace.greet(start);
    run(&mut workspace, start, Duration::from_millis(1200));
    let drawn = conversation(&mut workspace, &mut terminal);
    assert!(braille(&drawn), "the mark is drawn: {drawn}");
    assert!(drawn.contains("Plexmaton"), "{drawn}");

    run(&mut workspace, start, Duration::from_millis(3000));
    let drawn = conversation(&mut workspace, &mut terminal);
    assert!(!braille(&drawn), "the mark has left: {drawn}");
    assert_eq!(
        workspace.motion_deadline(start + Duration::from_secs(4)),
        None,
        "nothing is left moving"
    );
}

/// A conversation that is never greeted, such as one reopened, never draws the mark.
#[test]
fn a_conversation_launch_did_not_greet_shows_no_mark() {
    let (mut workspace, mut terminal) = fresh();
    assert!(!braille(&conversation(&mut workspace, &mut terminal)));
    assert_eq!(workspace.motion_deadline(Instant::now()), None);
}

/// The first message ends the greeting at once, and for good: a later empty conversation is not
/// greeted.
#[test]
fn the_first_message_ends_the_greeting_for_good() {
    let (mut workspace, mut terminal) = fresh();
    let start = Instant::now();
    workspace.greet(start);
    run(&mut workspace, start, Duration::from_millis(500));
    assert!(braille(&conversation(&mut workspace, &mut terminal)));

    let agent_id = AgentId::new(PRIMARY).expect("agent");
    let item_id = TranscriptItemId::new("first").expect("item");
    workspace.emit(vec![
        ConversationEventEnvelope {
            sequence: EventSequence::new(2),
            event: ConversationEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::User,
            },
        },
        ConversationEventEnvelope {
            sequence: EventSequence::new(3),
            event: ConversationEvent::TranscriptDelta {
                agent_id,
                item_id,
                item_revision: 1,
                text: "hello".into(),
            },
        },
    ]);
    let drawn = conversation(&mut workspace, &mut terminal);
    assert!(!braille(&drawn), "{drawn}");
    assert!(drawn.contains("hello"));
    assert_eq!(
        workspace.greeting, None,
        "nothing is left to resume on an empty conversation"
    );
    assert_eq!(workspace.state.greeting_phase(), None);
}
