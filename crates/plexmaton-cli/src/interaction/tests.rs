use super::*;
use crate::{clipboard::ClipboardRoute, test_support::empty_session, tests::FixtureWorkspace};
use futures_util::stream;
use plexmaton_cli::preparation::LivePreparation;
use plexmaton_core::{
    AgentId, ConversationEvent, ConversationEventEnvelope, EventSequence, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_runtime::{DispatchReport, OwnedStopReport};
use ratatui::{
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, time::Duration};

fn agent(value: &str) -> AgentId {
    AgentId::new(value).expect("agent")
}

async fn ready(path: &Path) -> i32 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && text.ends_with('\n')
                && let Some(pid) = text.lines().next().and_then(|line| line.parse().ok())
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("worker readiness")
}

fn gone(pid: i32) {
    let pid = rustix::process::Pid::from_raw(pid).expect("positive PID");
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        matches!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG),
            Err(rustix::io::Errno::CHILD)
        ),
        "worker was killed but not reaped"
    );
}

/// PRE-2/PRE-3/FR-3: actual process work is blocked while the production select loop accepts
/// typing, overlay changes, resize and quit, then the owner reaps every child before handoff.
#[tokio::test]
async fn blocked_preparation_never_holds_the_production_input_and_frame_loop() {
    for width in [120, 88, 60] {
        let root = FixtureWorkspace::new();
        let marker = root.path().join("preparing");
        let executable = root.path().join("preparation-worker");
        // PRE-2: readiness precedes an idle block in the same PID. No Python startup or
        // descendant process belongs in this input-loop witness.
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$$\" >> \"${0%/*}/preparing\"\nexec /bin/sleep 30\n",
        )
        .expect("write process fixture");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("executable fixture");
        let (mut runtime, mut picker, mut workspace, sequence) = empty_session(root.path());
        let agent = AgentId::new("primary").expect("agent");
        let item = TranscriptItemId::new("blocked-text").expect("entry");
        workspace.emit(vec![
            ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 1),
                event: ConversationEvent::TranscriptItemStarted {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    role: TranscriptRole::Assistant,
                },
            },
            ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 2),
                event: ConversationEvent::TranscriptDelta {
                    agent_id: agent,
                    item_id: item,
                    item_revision: 1,
                    text: "**blocked rich text** ".repeat(3000),
                },
            },
        ]);
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).expect("terminal");
        workspace.draw(&mut terminal).expect("pending frame");
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces())
                == Some(plexmaton_tui::SurfaceId::Composer)
            {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal).expect("focus composer");
        }
        let mut preparation = LivePreparation::new(executable);
        let mut clipboard = TerminalClipboard::new(Vec::new(), ClipboardRoute::Direct);
        let key = |code, modifiers| Ok(Event::Key(KeyEvent::new(code, modifiers)));
        let mut input = stream::once({
            let marker = marker.clone();
            async move {
                ready(&marker).await;
                Ok(Event::Paste("typed while preparing".into()))
            }
        })
        .chain(stream::iter([
            Ok(Event::Resize(width, 30)),
            key(KeyCode::Char('p'), KeyModifiers::CONTROL),
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
        ]))
        .boxed_local();
        let mut permissions =
            permission_controls::PermissionControls::new(runtime.coding_session());
        let result = drive_session(
            &mut terminal,
            &mut runtime,
            &mut clipboard,
            &mut workspace,
            &mut picker,
            &mut None,
            &mut permissions,
            &mut input,
            &mut preparation,
            None,
            |_, _| Ok(()),
        )
        .await;
        let prepared = workspace.metrics().text_layouts();
        let preparation_shutdown = preparation.shutdown().await;
        let clipboard_shutdown = clipboard.shutdown().await;
        let picker_shutdown = picker.shutdown().await;
        let permission_shutdown = permissions.shutdown().await;
        let runtime_shutdown = runtime.shutdown().await;
        result.expect("input continued while the worker was blocked");
        preparation_shutdown.expect("preparation reaped");
        clipboard_shutdown.expect("clipboard shutdown");
        picker_shutdown.expect("picker shutdown");
        permission_shutdown.expect("permission shutdown");
        crate::surface_shutdown_report(runtime_shutdown.expect("runtime shutdown"))
            .expect("clean runtime");
        assert_eq!(prepared, 0, "the blocked worker never supplied a result");
        assert_eq!(workspace.state().composer().text(), "typed while preparing");
        assert!(
            workspace.frames() >= 6,
            "input and overlays must actually repaint"
        );
        let text = terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("typed while preparing"));
        let pids = fs::read_to_string(marker).expect("real process reached readiness");
        assert!(!pids.is_empty());
        for pid in pids.lines() {
            gone(pid.parse().expect("PID"));
        }
    }
}

/// EFF-1/EFF-2/EFF-5: the production select loop routes /effort through to the real driver.
#[tokio::test]
async fn effort_command_changes_the_live_driver_without_submitting_a_message() {
    use plexmaton_core::ReasoningEffort;
    let root = FixtureWorkspace::new();
    let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
    let agent = runtime.agent_id().clone();
    let model = runtime.set_reasoning_effort(&agent, ReasoningEffort::Max);
    let model = model.expect("initial max");
    workspace.set_model(crate::configuration_summary(&model));
    workspace.set_effort_choices(model.allowed_reasoning_efforts().map(<[_]>::to_vec));
    let mut terminal = Terminal::new(TestBackend::new(88, 30)).expect("terminal");
    workspace.draw(&mut terminal).expect("first frame");
    for _ in 0..workspace.surfaces().len() {
        if workspace.state().focused(workspace.surfaces())
            == Some(plexmaton_tui::SurfaceId::Composer)
        {
            break;
        }
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        workspace.draw(&mut terminal).expect("focus");
    }
    let key = |code, modifiers| Ok(Event::Key(KeyEvent::new(code, modifiers)));
    let mut events = stream::iter([
        Ok(Event::Paste("/effort ".to_owned())),
        key(KeyCode::Left, KeyModifiers::NONE),
        key(KeyCode::Left, KeyModifiers::NONE),
        key(KeyCode::Enter, KeyModifiers::NONE),
        key(KeyCode::Char('d'), KeyModifiers::CONTROL),
        key(KeyCode::Char('d'), KeyModifiers::CONTROL),
    ]);
    let mut preparation = LivePreparation::new("/bin/false".into());
    let mut clipboard = TerminalClipboard::new(Vec::new(), ClipboardRoute::Direct);
    let mut permissions = permission_controls::PermissionControls::new(runtime.coding_session());
    let result = drive_session(
        &mut terminal,
        &mut runtime,
        &mut clipboard,
        &mut workspace,
        &mut picker,
        &mut None,
        &mut permissions,
        &mut events,
        &mut preparation,
        None,
        |_, _| Ok(()),
    )
    .await;
    assert_eq!(
        runtime
            .configured_model()
            .expect("model")
            .reasoning_effort(),
        ReasoningEffort::High
    );
    assert!(workspace.state().composer().text().is_empty());
    assert!(!root.path().join("sessions").exists());
    result.expect("loop");
    preparation.shutdown().await.expect("preparation");
    clipboard.shutdown().await.expect("clipboard");
    picker.shutdown().await.expect("picker");
    permissions.shutdown().await.expect("permissions");
    runtime.shutdown().await.expect("runtime");
}

/// SEL-5: the production admission boundary publishes only observed sends and withdraws old receipts.
#[test]
fn copy_admission_publishes_observed_delivery_without_a_timer_for_empty_requests() {
    let mut workspace = Workspace::default();
    let mut clipboard = TerminalClipboard::new(Vec::new(), ClipboardRoute::Direct);
    deliver_copy(None, &mut clipboard, &mut workspace).expect("no copy");
    assert_eq!(workspace.note_deadline(), None);
    let started = Instant::now();
    deliver_copy(
        Some(plexmaton_tui::CopyRequest {
            text: "source".into(),
            entries: 1,
        }),
        &mut clipboard,
        &mut workspace,
    )
    .expect("sent");
    let deadline = workspace.note_deadline().expect("receipt deadline");
    assert!(deadline >= started + Duration::from_secs(2));
    assert!(deadline <= Instant::now() + Duration::from_secs(2));
    let mut terminal = Terminal::new(TestBackend::new(88, 26)).expect("terminal");
    workspace.draw(&mut terminal).expect("receipt frame");
    let row: String = (0..88)
        .map(|x| terminal.backend().buffer()[(x, 25)].symbol())
        .collect();
    assert!(row.ends_with(" Copy sent "));
    workspace.clear_copy_receipt();
    assert_eq!(workspace.note_deadline(), None);
}

/// INV-7/SCH-2: root Ctrl-C uses the root runtime, while a child target uses only the owner.
#[tokio::test]
async fn ctrl_c_child_refusal_never_falls_back_to_root_and_root_remains_routable() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, mut workspace, _) =
        crate::test_support::empty_session_for(fixture.path(), agent("root"));
    while runtime.try_next_event().is_some() {}
    let (mut collaboration, _ingress) =
        crate::collaboration::open(fixture.path(), runtime.conversation_id())
            .expect("open root collaboration");
    let child = agent("delegated-1");
    collaboration.announce_resumed_child_for_test(
        plexmaton_core::ConversationId::new("resumed-child").expect("conversation"),
        child.clone(),
    );

    // The roster lookup succeeds, then the owner refuses because this resumed child has no
    // process-local runner. The route consumes that typed refusal instead of falling back to root.
    for _ in 0..2 {
        apply_interrupt(
            child.clone(),
            &mut runtime,
            &mut workspace,
            Some(&mut collaboration),
        )
        .await
        .expect("child Stop refusal is not a session error");
        assert!(
            runtime.try_next_event().is_none(),
            "root stayed uninterrupted"
        );
    }

    // The root branch still addresses the live runtime directly.
    apply_interrupt(
        runtime.agent_id().clone(),
        &mut runtime,
        &mut workspace,
        Some(&mut collaboration),
    )
    .await
    .expect("root interrupt route");

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// SCH-4/LOOP-6: Stop reports return exact child-owned text through the existing report path.
#[tokio::test]
async fn stop_settlement_restores_exact_child_input_without_a_new_event() {
    let mut workspace = Workspace::default();
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) =
        crate::test_support::empty_session_for(fixture.path(), agent("root"));
    apply_stop_settlement(
        Ok(OwnedStopReport {
            scheduled: Some(DispatchReport {
                undelivered: vec![plexmaton_agent::UndeliveredInput {
                    text: "exact child draft".to_owned(),
                    skill: None,
                    reason: plexmaton_agent::UndeliveredReason::Interrupted,
                }],
                ..DispatchReport::default()
            }),
            stopped: DispatchReport::default(),
        }),
        agent("delegated-1"),
        &runtime,
        &mut workspace,
    );
    assert_eq!(
        workspace.state().draft(&agent("delegated-1")).text(),
        "exact child draft"
    );
    assert!(workspace.state().notices().next().is_none());
    runtime.shutdown().await.expect("shutdown root");
}
