use std::{cell::Cell, path::Path, rc::Rc, task::Poll};

use super::*;
use crate::test_support::empty_session;
use crate::tests::FixtureWorkspace;

fn shell(script: &str, args: &[&Path]) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script, "clipboard-fixture"]);
    command.args(args);
    configure_copy_command(&mut command);
    command
}

fn block_at(ready: &Path) -> Command {
    shell(
        "printf '%s\\n' \"$$\" > \"$1\"; exec /bin/sleep 30",
        &[ready],
    )
}

async fn ready_pid(path: &Path) -> i32 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(source) = std::fs::read_to_string(path)
                && let Some(pid) = source.strip_suffix('\n').and_then(|s| s.parse().ok())
            {
                return pid;
            }
            // This filesystem boundary has no async readiness seam; the complete marker, never
            // the delay, proves the child is running. Idle fixtures themselves block in sleep.
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("helper readiness")
}

async fn running<W: Write>(owner: &mut TerminalClipboard<W>, path: &Path) -> i32 {
    tokio::select! {
        result = owner.next() => panic!("helper completed before readiness: {result:?}"),
        pid = ready_pid(path) => pid,
    }
}

fn gone(pid: i32) {
    let pid = rustix::process::Pid::from_raw(pid).expect("positive pid");
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        matches!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG),
            Err(rustix::io::Errno::CHILD)
        ),
        "delivery must have reaped, not just killed, its child"
    );
}

/// SEL-8/FR-1: replacements cannot accumulate children or let an older helper overwrite newer OSC.
#[tokio::test]
async fn clipboard_replacement_reaps_before_delivering_only_the_latest_source() {
    let root = FixtureWorkspace::new();
    let ready = root.path().join("ready");
    let delivered = root.path().join("delivered");
    let calls = Rc::new(Cell::new(0));
    let mut owner = TerminalClipboard::new(
        Vec::new(),
        ClipboardRoute::Tmux {
            dcs_passthrough: true,
        },
    );
    owner.helper = Box::new({
        let ready = ready.clone();
        let delivered = delivered.clone();
        let calls = calls.clone();
        move || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                block_at(&ready)
            } else {
                let pid = std::fs::read_to_string(&ready)
                    .expect("pid")
                    .trim()
                    .parse()
                    .expect("pid");
                gone(pid);
                shell("/bin/cat > \"$1\"", &[&delivered])
            }
        }
    });
    owner.submit("first".into()).expect("submit");
    let pid = running(&mut owner, &ready).await;
    for n in 0..64 {
        owner.submit(format!("replaced-{n}")).expect("replace");
    }
    let latest = "latest 中文 e\u{301} 👩‍💻\n\n";
    owner.submit(latest.into()).expect("latest");
    assert_eq!(calls.get(), 1);
    assert_eq!(
        owner.writer,
        osc52_sequence("first", owner.route).expect("first OSC")
    );
    assert_eq!(owner.next().await.expect("reap and start newest"), None);
    gone(pid);
    assert_eq!(
        owner.next().await.expect("latest helper accepted"),
        Some(CopyReceipt::Sent)
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(std::fs::read_to_string(delivered).expect("source"), latest);
    let mut expected = osc52_sequence("first", owner.route).expect("first");
    expected.extend(osc52_sequence(latest, owner.route).expect("latest"));
    assert_eq!(owner.writer, expected);
    assert!(matches!(owner.delivery, Delivery::Idle));
}

/// SEL-8: cancellation while writing a full pipe reaps the actual child and discards queued effects.
#[tokio::test]
async fn clipboard_shutdown_reaps_a_blocked_writer_and_never_starts_pending_copy() {
    let root = FixtureWorkspace::new();
    let ready = root.path().join("ready");
    let calls = Rc::new(Cell::new(0));
    let mut owner = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
    owner.helper = Box::new({
        let ready = ready.clone();
        let calls = calls.clone();
        move || {
            calls.set(calls.get() + 1);
            block_at(&ready)
        }
    });
    owner.submit("x".repeat(1_048_576)).expect("large source");
    let pid = running(&mut owner, &ready).await;
    owner
        .submit("pending must not run".into())
        .expect("pending");
    owner.shutdown().await.expect("reap");
    gone(pid);
    assert_eq!(calls.get(), 1);
    assert!(
        owner.writer.is_empty(),
        "native helpers never receive a terminal writer"
    );
    assert!(matches!(owner.delivery, Delivery::Idle));
    owner.shutdown().await.expect("idempotent cleanup");
    let mut idle = Box::pin(owner.next());
    std::future::poll_fn(|cx| {
        assert!(idle.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

/// SEL-8: cancellation before the first poll must not spawn even the first prepared helper.
#[tokio::test]
async fn clipboard_shutdown_before_polling_starts_no_process() {
    let root = FixtureWorkspace::new();
    let ready = root.path().join("must-not-exist");
    let mut owner = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
    owner.helper = Box::new({
        let ready = ready.clone();
        move || block_at(&ready)
    });
    owner.submit("source".into()).expect("submit");
    owner.shutdown().await.expect("cancel before spawn");
    assert!(!ready.exists());
}

/// SEL-8: admission precedes cancellation and terminal effects; capacity, not just length, is bounded.
#[tokio::test]
async fn oversized_copy_preserves_the_admitted_pending_source_without_terminal_effects() {
    let mut owner = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
    owner.helper = Box::new(|| shell("/bin/cat > /dev/null", &[]));
    owner.submit("accepted".into()).expect("accepted");
    owner.submit("latest".into()).expect("pending");
    let mut oversized = String::with_capacity(MAX_COPY_BYTES + 1);
    oversized.push('x');
    let error = owner.submit(oversized).expect_err("capacity limit");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let Delivery::Active(active) = &owner.delivery else {
        panic!("active owner");
    };
    assert_eq!(active.pending.as_deref(), Some("latest"));
    assert!(owner.writer.is_empty());
    owner.shutdown().await.expect("cleanup");
}

struct BrokenTerminal;

impl Write for BrokenTerminal {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture"))
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("failed write cannot flush");
    }
}

/// SEL-5: either tmux delivery leg may succeed; helper failure cannot invent native success.
#[tokio::test]
async fn clipboard_delivery_keeps_route_failures_and_cleanup_separate() {
    let mut tmux = TerminalClipboard::new(
        BrokenTerminal,
        ClipboardRoute::Tmux {
            dcs_passthrough: true,
        },
    );
    tmux.helper = Box::new(|| shell("/bin/cat > /dev/null", &[]));
    tmux.submit("source".into())
        .expect("helper can still accept");
    assert_eq!(
        tmux.next()
            .await
            .expect("helper accepted despite failed terminal"),
        Some(CopyReceipt::Sent)
    );

    let mut tmux = TerminalClipboard::new(
        Vec::new(),
        ClipboardRoute::Tmux {
            dcs_passthrough: true,
        },
    );
    tmux.helper = Box::new(|| shell("/bin/cat > /dev/null; exit 7", &[]));
    tmux.submit("source".into()).expect("submit");
    assert_eq!(
        tmux.next().await.expect("OSC has already been written"),
        Some(CopyReceipt::Sent)
    );
    assert!(!tmux.writer.is_empty());

    let mut native = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
    native.helper = Box::new(|| shell("/bin/cat > /dev/null; exit 7", &[]));
    native.submit("source".into()).expect("submit");
    assert!(native.next().await.is_err());
    assert!(native.writer.is_empty());

    let mut direct = TerminalClipboard::new(BrokenTerminal, ClipboardRoute::Direct);
    assert_eq!(
        direct
            .submit("source".into())
            .expect_err("terminal failed")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

/// SEL-4/SEL-8/FR-1: the actual select loop accepts copy, paste, overlay keys and a resize repaint
/// while its real helper is still blocked. No mock dispatcher or timing percentile proves this.
#[tokio::test]
async fn clipboard_wait_never_holds_the_production_input_and_frame_loop() {
    use futures_util::{StreamExt as _, stream};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
    };

    for width in [120, 88, 60] {
        let root = FixtureWorkspace::new();
        let ready = root.path().join("ready");
        let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).expect("terminal");
        workspace.draw(&mut terminal).expect("initial");
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces())
                == Some(plexmaton_tui::SurfaceId::Composer)
            {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal).expect("focus composer");
        }
        workspace.handle(&Event::Paste("copy source".into()));
        workspace.draw(&mut terminal).expect("source");
        let buffer = terminal.backend().buffer();
        let (column, row) = (0..30)
            .find_map(|y| {
                (0..width - 11)
                    .find(|&x| {
                        "copy source"
                            .chars()
                            .enumerate()
                            .all(|(n, c)| buffer[(x + n as u16, y)].symbol() == c.to_string())
                    })
                    .map(|x| (x, y))
            })
            .expect("visible draft");
        let pointer = |kind, x| {
            Ok(Event::Mouse(MouseEvent {
                kind,
                column: x,
                row,
                modifiers: KeyModifiers::NONE,
            }))
        };
        let key = |code, modifiers| Ok(Event::Key(KeyEvent::new(code, modifiers)));
        let mut events = stream::iter([
            pointer(MouseEventKind::Down(MouseButton::Left), column),
            pointer(MouseEventKind::Drag(MouseButton::Left), column + 11),
            pointer(MouseEventKind::Up(MouseButton::Left), column + 11),
        ])
        .chain(stream::once({
            let ready = ready.clone();
            async move {
                ready_pid(&ready).await;
                Ok(Event::Paste("typed while copying".into()))
            }
        }))
        .chain(stream::iter([
            // TestBackend retains this size. This event proves invalidation/repaint while the
            // helper waits; actual changing terminal geometry is covered by the PTY smoke.
            Ok(Event::Resize(width, 30)),
            key(KeyCode::Char('p'), KeyModifiers::CONTROL),
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
        ]))
        .boxed_local();
        let mut owner = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
        owner.helper = Box::new({
            let ready = ready.clone();
            move || block_at(&ready)
        });
        let mut preparation = plexmaton_cli::preparation::LivePreparation::new(
            std::env::current_exe().expect("test executable"),
        );
        let mut permissions =
            crate::permission_controls::PermissionControls::new(runtime.coding_session());
        let result = crate::drive_session(
            &mut terminal,
            &mut runtime,
            &mut owner,
            &mut workspace,
            &mut picker,
            &mut None,
            &mut permissions,
            &mut events,
            &mut preparation,
            |_, _| Ok(()),
        )
        .await;
        let active = matches!(owner.delivery, Delivery::Active(_));
        let pid = std::fs::read_to_string(&ready)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let copied = match &owner.delivery {
            Delivery::Active(active) => active.pending.is_none(),
            _ => false,
        };
        let copy_shutdown = owner.shutdown().await;
        let preparation_shutdown = preparation.shutdown().await;
        let picker_shutdown = picker.shutdown().await;
        let permission_shutdown = permissions.shutdown().await;
        let runtime_shutdown = runtime.shutdown().await;
        result.expect("input and quit progress without awaiting the helper");
        copy_shutdown.expect("clipboard shutdown");
        preparation_shutdown.expect("preparation shutdown");
        picker_shutdown.expect("picker shutdown");
        permission_shutdown.expect("permission shutdown");
        crate::surface_shutdown_report(runtime_shutdown.expect("runtime shutdown"))
            .expect("clean runtime");
        assert!(
            active && copied,
            "the helper cannot have completed before the input witness"
        );
        gone(pid.expect("real helper reached readiness"));
        assert_eq!(workspace.state().composer().text(), "typed while copying");
        assert!(
            workspace.frames() >= 8,
            "pointer, paste, resize and overlays must really paint"
        );
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .chunks(usize::from(width))
                .any(|row| {
                    row.iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                        .contains("typed while copying")
                })
        );
        assert!(owner.writer.is_empty());
    }
}

/// SEL-5/SEL-8: native acceptance gets a receipt; explicit cancellation cannot borrow a terminal send.
#[tokio::test]
async fn clipboard_receipts_require_native_acceptance_and_suppress_cancellation() {
    let mut native = TerminalClipboard::new(Vec::new(), ClipboardRoute::LocalMacOs);
    native.helper = Box::new(|| shell("/bin/cat > /dev/null", &[]));
    assert_eq!(native.submit("source".into()).expect("submit"), None);
    assert_eq!(
        native.next().await.expect("accepted"),
        Some(CopyReceipt::Copied)
    );
    let mut tmux = TerminalClipboard::new(
        Vec::new(),
        ClipboardRoute::Tmux {
            dcs_passthrough: true,
        },
    );
    tmux.helper = Box::new(|| shell("exec /bin/sleep 30", &[]));
    assert_eq!(tmux.submit("source".into()).expect("submit"), None);
    let Delivery::Active(active) = &tmux.delivery else {
        panic!("active copy")
    };
    active.cancel.cancel();
    assert_eq!(tmux.next().await.expect("cancelled"), None);
}
