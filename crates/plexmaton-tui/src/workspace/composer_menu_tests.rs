//! The composer menu's `/`: Commands, `/resume`'s saved conversations with one identity for
//! keyboard and pointer, and what leaves the workspace when a row is accepted (CMC-1, CMC-2,
//! SPK-1, CPL-9).
use super::*;
use crate::{
    Command, CommandRun, CommandTarget, CompactRefusal, CompactionNote, ConversationChoice,
    ConversationPickerStatus, ConversationRequest, Listing, Point, SurfaceId, SwitchRefusal,
    intent::TextIntent,
    test_support::{assert_frame, canonical_runtime, region_text},
};
use plexmaton_core::ConversationId;
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::Rect,
};

pub(super) fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

pub(super) fn mouse(kind: MouseEventKind, point: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: point.x,
        row: point.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn choices() -> Vec<ConversationChoice> {
    (0..12)
        .map(|i| ConversationChoice {
            id: ConversationId::new(format!("conversation-{i:02}")).expect("id"),
            title: format!("Discuss project {i:02}"),
        })
        .collect()
}

fn agent() -> AgentId {
    AgentId::new("agent-a").expect("agent")
}

/// A drawn workspace with the caret in the composer.
pub(super) fn setup(width: u16) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).expect("terminal");
    workspace.emit(canonical_runtime().ready(u64::MAX));
    workspace.settled_draw(&mut terminal).expect("draw");
    for _ in 0..=workspace.surfaces.len() {
        if workspace.state.focused(&workspace.surfaces) == Some(SurfaceId::Composer) {
            break;
        }
        workspace.handle(&key(KeyCode::Tab));
        workspace.settled_draw(&mut terminal).expect("draw");
    }
    assert_eq!(
        workspace.state.focused(&workspace.surfaces),
        Some(SurfaceId::Composer)
    );
    (workspace, terminal)
}

pub(super) fn typed(workspace: &mut Workspace, text: &str) -> Outcome {
    let mut last = Outcome::default();
    for character in text.chars() {
        last = workspace.handle(&key(KeyCode::Char(character)));
    }
    last
}

fn draw(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) {
    workspace.settled_draw(terminal).expect("draw");
}

fn menu_bounds(workspace: &Workspace) -> Option<Rect> {
    workspace
        .surfaces
        .get(SurfaceId::ComposerMenu)
        .map(|surface| surface.bounds)
}

/// The menu and the composer beneath it, as one cropped frame.
fn menu_and_composer(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let menu = menu_bounds(workspace).expect("the menu is registered");
    let composer = workspace
        .surfaces
        .get(SurfaceId::Composer)
        .expect("composer")
        .bounds;
    region_text(
        terminal.backend().buffer(),
        Rect::new(
            menu.x,
            menu.y,
            menu.width,
            composer.bottom().saturating_sub(menu.y),
        ),
    )
}

/// The primary conversation as painted, notes included.
fn conversation(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    region_text(
        terminal.backend().buffer(),
        workspace
            .surfaces
            .get(SurfaceId::Transcript)
            .expect("conversation")
            .bounds,
    )
}

fn row_point(workspace: &Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Point {
    let bounds = menu_bounds(workspace).expect("menu");
    let drawn = region_text(terminal.backend().buffer(), bounds);
    let (row, _) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .unwrap_or_else(|| panic!("{text:?} is a visible row:\n{drawn}"));
    Point {
        x: bounds.x + 4,
        y: bounds.y + u16::try_from(row).expect("row"),
    }
}

/// CMC-1/CMC-2: `/` lists the Commands, the query narrows them, `Enter` on `/compact` leaves a
/// run with its target and consumes the draft, and text that is not exactly a Command is text.
#[test]
fn the_slash_lists_the_commands_and_only_a_whole_command_runs() {
    let (mut workspace, mut terminal) = setup(95);
    typed(&mut workspace, "/");
    draw(&mut workspace, &mut terminal);
    assert_eq!(workspace.state.menu_listing(), Some(Listing::Commands));
    let drawn = menu_and_composer(&workspace, &terminal);
    for row in ["/new", "/resume", "/compact"] {
        assert!(drawn.contains(row), "{row} is listed:\n{drawn}");
    }
    assert!(drawn.starts_with("── Commands"));
    assert_frame("composer-menu-commands-medium", &drawn);

    typed(&mut workspace, "co");
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("/compact") && !drawn.contains("/new"));
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(
        outcome.command,
        Some(CommandRun {
            command: Command::Compact,
            target: CommandTarget { agent: agent() },
        })
    );
    assert!(outcome.submitted.is_none());
    assert_eq!(workspace.state.composer().text(), "");
    draw(&mut workspace, &mut terminal);
    assert!(menu_bounds(&workspace).is_none(), "the run closes the menu");

    for literal in ["/compact please", "/config"] {
        typed(&mut workspace, literal);
        draw(&mut workspace, &mut terminal);
        assert!(
            menu_bounds(&workspace).is_none(),
            "{literal:?} lists nothing"
        );
        let outcome = workspace.handle(&key(KeyCode::Enter));
        assert_eq!(
            outcome.submitted.map(|submission| submission.text),
            Some(literal.to_owned()),
            "{literal:?} is text"
        );
        assert!(outcome.command.is_none());
    }
}

/// CMC-2: `Tab` completes the chosen Command into the draft without running it, a completed
/// `/new` runs on `Enter` with the menu gone, and `Escape` keeps the draft.
#[test]
fn tab_completes_a_command_and_escape_keeps_the_draft() {
    let (mut workspace, mut terminal) = setup(95);
    typed(&mut workspace, "/ne");
    draw(&mut workspace, &mut terminal);
    let outcome = workspace.handle(&key(KeyCode::Tab));
    assert!(outcome.command.is_none() && outcome.conversation.is_none());
    assert_eq!(workspace.state.composer().text(), "/new ");
    draw(&mut workspace, &mut terminal);
    assert!(
        menu_bounds(&workspace).is_none(),
        "a completed token lists nothing"
    );
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(outcome.conversation, Some(ConversationRequest::New));
    assert_eq!(workspace.state.composer().text(), "");

    typed(&mut workspace, "/");
    draw(&mut workspace, &mut terminal);
    assert!(menu_bounds(&workspace).is_some());
    workspace.handle(&key(KeyCode::Esc));
    draw(&mut workspace, &mut terminal);
    assert!(menu_bounds(&workspace).is_none());
    assert_eq!(workspace.state.composer().text(), "/");
    for motion in [KeyCode::Left, KeyCode::Right] {
        workspace.handle(&key(motion));
        draw(&mut workspace, &mut terminal);
        assert!(
            menu_bounds(&workspace).is_none(),
            "dismissal survives caret motion"
        );
    }
    typed(&mut workspace, "c");
    draw(&mut workspace, &mut terminal);
    assert!(
        menu_bounds(&workspace).is_some(),
        "a changed token lists again"
    );
    assert_eq!(workspace.state.composer().text(), "/c");
}

/// SPK-1: `/resume` asks the composition root for the listing once, shows its status until the
/// rows arrive, and a row chosen by keyboard or by a matching press and release leaves as the
/// same identity; a drag cancels.
#[test]
fn resume_lists_saved_conversations_by_identity_for_keyboard_and_pointer() {
    let (mut workspace, mut terminal) = setup(95);
    typed(&mut workspace, "/resume");
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(outcome.conversation, Some(ConversationRequest::List));
    assert_eq!(workspace.state.composer().text(), "/resume ");
    assert!(workspace.conversation_picker_open());
    draw(&mut workspace, &mut terminal);
    assert_eq!(workspace.state.menu_listing(), Some(Listing::Conversations));
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("Loading saved conversations…"), "{drawn}");
    assert_frame("composer-menu-resume-loading-medium", &drawn);
    assert!(
        !workspace.has_unsent_input(),
        "the request to switch is not a draft the switch would lose"
    );

    workspace.set_conversation_choices(choices(), false);
    typed(&mut workspace, "0");
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("> Discuss project 00") && drawn.contains("conversation-00"));
    assert!(!drawn.contains("Discuss project 10"), "{drawn}");
    assert_frame("composer-menu-resume-medium", &drawn);

    workspace.handle(&key(KeyCode::Down));
    workspace.handle(&key(KeyCode::Down));
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(
        outcome.conversation,
        Some(ConversationRequest::Saved(
            ConversationId::new("conversation-02").expect("id")
        ))
    );
    assert_eq!(
        workspace.state.composer().text(),
        "/resume 0",
        "the draft stays until the conversation is open"
    );

    draw(&mut workspace, &mut terminal);
    let at = row_point(&workspace, &terminal, "Discuss project 04");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
    let outcome = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
    assert_eq!(
        outcome.conversation,
        Some(ConversationRequest::Saved(
            ConversationId::new("conversation-04").expect("id")
        ))
    );
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
    workspace.handle(&mouse(
        MouseEventKind::Drag(MouseButton::Left),
        Point {
            x: at.x,
            y: at.y + 1,
        },
    ));
    let outcome = workspace.handle(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        Point {
            x: at.x,
            y: at.y + 1,
        },
    ));
    assert!(outcome.conversation.is_none(), "a drag activates nothing");

    workspace.close_conversation_picker();
    draw(&mut workspace, &mut terminal);
    assert_eq!(workspace.state.composer().text(), "");
    assert!(menu_bounds(&workspace).is_none());
    assert!(!workspace.conversation_picker_open());
}

/// SPK-1/SPK-2: the listing's failures and an empty match are rows the user can read, an open
/// in flight offers no row, and `Escape` withdraws the listing's destination.
#[test]
fn resume_status_rows_cover_failure_no_match_and_opening() {
    let (mut workspace, mut terminal) = setup(60);
    typed(&mut workspace, "/resume ");
    workspace.open_conversation_picker();
    workspace.set_conversation_picker_status(ConversationPickerStatus::ListFailed);
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(
        drawn.contains("Could not read saved conversations."),
        "{drawn}"
    );
    assert!(
        workspace
            .handle(&key(KeyCode::Enter))
            .conversation
            .is_none()
    );

    workspace.set_conversation_choices(choices(), true);
    typed(&mut workspace, "zzz");
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("No saved conversation matches"), "{drawn}");
    for _ in 0..3 {
        workspace.handle(&key(KeyCode::Backspace));
    }
    workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("Opening conversation…"), "{drawn}");
    assert!(
        workspace
            .handle(&key(KeyCode::Enter))
            .conversation
            .is_none(),
        "no second open while one is in flight"
    );

    workspace.handle(&key(KeyCode::Esc));
    assert!(!workspace.conversation_picker_open());
    assert_eq!(workspace.state.composer().text(), "/resume ");
}

/// CMC-2/SKP-3: a paste opens the listing like typing does, and a character no Command starts
/// with closes it so the draft stays text.
#[test]
fn paste_and_unicode_inside_the_token_follow_the_same_rule() {
    let (mut workspace, mut terminal) = setup(95);
    workspace.apply(
        TuiIntent::Text(TextIntent::Paste("/re".to_owned())),
        Instant::now(),
    );
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(drawn.contains("/resume"), "{drawn}");
    typed(&mut workspace, "中");
    draw(&mut workspace, &mut terminal);
    assert!(menu_bounds(&workspace).is_none());
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(
        outcome.submitted.map(|submission| submission.text),
        Some("/re中".to_owned())
    );
}

/// CPL-9: a requested compaction shows on the activity line while it runs and ends with one
/// note after the last entry; a refusal is one sentence in the same place.
#[test]
fn a_requested_compaction_shows_on_the_activity_line_and_ends_with_a_note() {
    let (mut workspace, mut terminal) = setup(95);
    workspace.report_compaction(&agent(), CompactionNote::Started);
    draw(&mut workspace, &mut terminal);
    let drawn = conversation(&workspace, &terminal);
    assert!(
        drawn
            .lines()
            .last()
            .is_some_and(|row| row.contains("· Compacting…")),
        "{drawn}"
    );

    workspace.report_compaction(&agent(), CompactionNote::Published);
    draw(&mut workspace, &mut terminal);
    let drawn = conversation(&workspace, &terminal);
    assert!(drawn.contains("✓ Context compacted."), "{drawn}");
    assert!(!drawn.contains("Compacting…"));

    workspace.report_compaction(
        &agent(),
        CompactionNote::Refused(CompactRefusal::TurnActive),
    );
    draw(&mut workspace, &mut terminal);
    let drawn = conversation(&workspace, &terminal);
    assert!(
        drawn.contains("Could not compact: the turn is still running."),
        "{drawn}"
    );
    assert!(!drawn.contains("Context compacted"), "one note at a time");
}

/// SPK-2/SPK-3: a refused `/new` is one sentence after the last entry with nothing left to wait
/// for; a refused row is the listing's status row and the rows can be chosen again; a draft that
/// stops asking for the rows withdraws the listing.
#[test]
fn a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again() {
    let (mut workspace, mut terminal) = setup(95);
    typed(&mut workspace, "/new");
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).conversation,
        Some(ConversationRequest::New)
    );
    workspace.begin_conversation_switch();
    assert!(workspace.conversation_picker_open());
    workspace.report_switch_refusal(SwitchRefusal::Busy);
    draw(&mut workspace, &mut terminal);
    let drawn = conversation(&workspace, &terminal);
    assert!(
        drawn.contains("Stop the current run before switching conversations."),
        "{drawn}"
    );
    assert!(
        !workspace.conversation_picker_open(),
        "nothing is left to wait for"
    );

    typed(&mut workspace, "/resume ");
    workspace.open_conversation_picker();
    workspace.set_conversation_choices(choices(), false);
    draw(&mut workspace, &mut terminal);
    let first = ConversationId::new("conversation-00").expect("id");
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).conversation,
        Some(ConversationRequest::Saved(first.clone()))
    );
    workspace.begin_conversation_switch();
    draw(&mut workspace, &mut terminal);
    assert!(
        menu_and_composer(&workspace, &terminal).contains("Opening conversation…"),
        "the rows wait behind the open"
    );
    workspace.report_switch_refusal(SwitchRefusal::OpenFailed);
    draw(&mut workspace, &mut terminal);
    let drawn = menu_and_composer(&workspace, &terminal);
    assert!(
        drawn.contains("Cannot open: check configuration or conversation file.")
            && !drawn.contains("Opening conversation…"),
        "the listing answers in its status row:\n{drawn}"
    );
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).conversation,
        Some(ConversationRequest::Saved(first)),
        "the rows can be chosen again"
    );
    workspace.report_switch_refusal(SwitchRefusal::DraftPresent);
    for _ in 0.."/resume ".len() {
        workspace.handle(&key(KeyCode::Backspace));
    }
    assert!(
        !workspace.conversation_picker_open(),
        "a draft that stops asking withdraws the listing"
    );
}
