use super::*;
use crate::{clipboard::ClipboardRoute, test_support::empty_session, tests::FixtureWorkspace};
use futures_util::stream;
use plexmaton_cli::preparation::LivePreparation;
use plexmaton_core::{
    AgentId, ConversationEvent, ConversationEventEnvelope, EventSequence, TranscriptItemId,
    TranscriptRole,
};
use ratatui::{
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, time::Duration};

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
        fs::write(&executable, format!("#!/usr/bin/python3\nimport os, signal\nwith open({}, 'a') as marker:\n    marker.write(str(os.getpid()) + '\\n')\nwhile True:\n    signal.pause()\n", serde_json::to_string(&marker).expect("quoted path"))).expect("write process fixture");
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
