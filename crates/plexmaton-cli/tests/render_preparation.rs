//! Real process boundaries; only the hostile/blocked child is replaced with a fixture.

use plexmaton_cli::preparation;

use std::{
    fs,
    path::{Path, PathBuf},
    task::Poll,
    time::Duration,
};

use plexmaton_core::{AgentId, TranscriptItemId, TranscriptRole};
use plexmaton_tui::{
    TranscriptEntryView, TranscriptItemView, TranscriptTextKind, preparation::Request,
};
use preparation::{Completion, Failure, Preparation};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("plexmaton-preparation-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&path).expect("reserve fixture directory");
        Self(path)
    }

    fn worker(&self, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = self.0.join("worker");
        let root = serde_json::to_string(&self.0).expect("quoted path");
        fs::write(
            &path,
            format!("#!/usr/bin/python3\n{PYTHON}\nROOT = pathlib.Path({root})\n{script}\n"),
        )
        .expect("write process fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("make fixture executable");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned fixture");
    }
}

const PYTHON: &str = r#"
import json, os, pathlib, signal, sys
signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGUSR1})
def mark(name):
    (ROOT / name).write_text(str(os.getpid()) + '\n')
def hold():
    signal.sigwait({signal.SIGUSR1})
def exact(size):
    result = b''
    while len(result) < size:
        part = os.read(0, size - len(result))
        if not part:
            raise RuntimeError('unexpected EOF')
        result += part
    return result
def request():
    return json.loads(exact(int.from_bytes(exact(4), 'big')))
def reply(ticket):
    return json.dumps({'ticket': ticket, 'result': {'Err': 'Capacity'}}).encode()
def write(data):
    while data:
        data = data[os.write(1, data):]
def send(data):
    write(len(data).to_bytes(4, 'big') + data)
"#;

fn request(source: &str, revision: u64, width: u16) -> Request {
    Request::new(
        AgentId::new("primary").expect("agent"),
        TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new("text").expect("entry"),
            role: TranscriptRole::Assistant,
            kind: TranscriptTextKind::Message,
            source: source.into(),
            revision,
            finalized: true,
        }),
        width,
        false,
    )
}

async fn marker(path: &Path) -> i32 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && let Some(pid) = text.strip_suffix('\n').and_then(|text| text.parse().ok())
            {
                return pid;
            }
            // The marker is the readiness fact. This filesystem boundary has no async event seam.
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("child readiness")
}

async fn running(owner: &mut Preparation, path: &Path) -> i32 {
    tokio::select! {
        result = owner.next() => panic!("child completed before readiness: {result:?}"),
        pid = marker(path) => pid,
    }
}

fn gone(pid: i32) {
    let pid = rustix::process::Pid::from_raw(pid).expect("pid");
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        matches!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG),
            Err(rustix::io::Errno::CHILD)
        ),
        "child must be reaped, not merely signalled"
    );
}

fn release(pid: i32) {
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid).expect("pid"),
        rustix::process::Signal::USR1,
    )
    .expect("release blocked child");
}

/// PRE-1/MD-5: the real same-build worker preserves rows, copy text and ordered style intent.
#[tokio::test]
async fn real_preparation_driver_round_trips_semantic_rows_at_three_widths() {
    let mut owner = Preparation::new(PathBuf::from(env!("CARGO_BIN_EXE_plexmaton")));
    for width in [120, 88, 60] {
        let mut requests = vec![request(
            "# Native **heading**\n\n*中文 e\u{301}* and `code`",
            4,
            width,
        )];
        requests.extend(other_entries(width));
        let expected = requests.iter().map(Request::prepare).collect::<Vec<_>>();
        let ticket = owner.submit(requests).expect("admit");
        let Completion::Ready(received, Ok(prepared)) = owner.next().await else {
            panic!("real prepared rows");
        };
        assert_eq!(received, ticket);
        assert_eq!(
            prepared[0].selection_text(),
            Ok("Native heading\n\n中文 e\u{301} and code")
        );
        assert_eq!(
            serde_json::to_value(&prepared).expect("received"),
            serde_json::to_value(expected).expect("expected")
        );
    }
    owner.shutdown().await.expect("reap idle persistent child");
    owner.shutdown().await.expect("idempotent shutdown");
}

fn other_entries(width: u16) -> Vec<Request> {
    use plexmaton_core::{
        ArtifactId, MailId, ToolCallId, ToolCallStatus, ToolDetail, ToolPresentation,
    };
    use plexmaton_tui::{ArtifactView, MailView, ToolCallView};

    let agent = AgentId::new("other-agent").expect("agent");
    let tool = TranscriptEntryView::Tool(ToolCallView {
        saved_project_permission: None,
        entry_id: TranscriptItemId::new("tool-entry").expect("entry"),
        id: ToolCallId::new("call").expect("tool"),
        label: "apply_patch".into(),
        status: ToolCallStatus::Succeeded,
        revision: 3,
        presentation: ToolPresentation {
            invocation: Some(ToolDetail::Text {
                source: "update 中文".into(),
                omitted_bytes: 0,
            }),
            outcome: Some(ToolDetail::Diff {
                patch: "*** Begin Patch\n*** Update File: code.rs\n@@\n-old\n+new\n*** End Patch"
                    .into(),
            }),
        },
    });
    vec![
        Request::new(agent.clone(), tool.clone(), width, false),
        Request::new(agent.clone(), tool, width, true),
        Request::new(
            agent.clone(),
            TranscriptEntryView::Artifact(ArtifactView {
                entry_id: TranscriptItemId::new("artifact-entry").expect("entry"),
                id: ArtifactId::new("artifact").expect("artifact"),
                label: "report".into(),
                pointer: "./report.md".into(),
                revision: 1,
            }),
            width,
            false,
        ),
        Request::new(
            agent.clone(),
            TranscriptEntryView::Mail(MailView {
                entry_id: TranscriptItemId::new("mail-entry").expect("entry"),
                id: MailId::new("mail").expect("mail"),
                from: agent.clone(),
                to: AgentId::new("primary").expect("recipient"),
                summary: "**literal** mail 中文".into(),
                revision: 1,
            }),
            width,
            false,
        ),
        Request::new(
            agent,
            TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new("literal-entry").expect("entry"),
                role: TranscriptRole::User,
                kind: TranscriptTextKind::Message,
                source: "**literal** user 中文".into(),
                revision: 1,
                finalized: true,
            }),
            width,
            false,
        ),
    ]
}

/// PRE-1/MTH-1/MTH-4: all 61 formulas traverse the actual child and validated native reply codec.
#[tokio::test]
async fn real_preparation_worker_preserves_the_complete_native_math_reply() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../plexmaton-math/fixtures/attention-derivatives.json"
    ))
    .expect("source-linked reply");
    let source = fixture["text"].as_str().expect("source");
    let ranges = fixture["math"].as_array().expect("original UTF-8 ranges");
    let mut owner = Preparation::new(PathBuf::from(env!("CARGO_BIN_EXE_plexmaton")));
    let mut canonical = None;
    for width in [114, 82, 54] {
        let request =
            request(source, 1, width).with_math(plexmaton_tui::math::MathPresentation::Native);
        let key = request.key().clone();
        let ticket = owner.submit(vec![request]).expect("request");
        let result = owner.next().await;
        let Completion::Ready(received, Ok(prepared)) = result else {
            panic!("complete native reply: {result:?}");
        };
        assert_eq!(received, ticket);
        assert_eq!(prepared.len(), 1);
        assert!(prepared[0].validates(&key));
        let text = prepared[0].selection_text().expect("semantic copy");
        let value = serde_json::to_value(&prepared[0]).expect("native reply");
        let formulas = value["result"]["Ok"]["formulas"]
            .as_array()
            .expect("formulas");
        assert_eq!(formulas.len(), 61);
        for (formula, original) in formulas.iter().zip(ranges) {
            assert!(
                formula["content"]["Native"].is_object(),
                "native, not source fallback"
            );
            let offset = |value: &serde_json::Value, name: &str| {
                usize::try_from(value[name].as_u64().expect("offset")).expect("usize offset")
            };
            assert_eq!(
                &text[offset(&formula["text"], "start")..offset(&formula["text"], "end")],
                &source[offset(original, "start")..offset(original, "end")]
            );
        }
        if let Some(canonical) = &canonical {
            assert_eq!(text, canonical);
        }
        canonical = Some(text.to_owned());
    }
    owner
        .shutdown()
        .await
        .expect("reap native preparation child");
}

/// PRE-1: the private dispatch cannot read configuration or take over a terminal before framing.
#[test]
fn preparation_driver_eof_exits_without_configuration_or_terminal_output() {
    let root = Fixture::new();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plexmaton"))
        .arg(preparation::DRIVER_ARGUMENT)
        .env_clear()
        .current_dir(&root.0)
        .env("PLEXMATON_HOME", root.0.join("not-configured"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("private driver");
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{output:?}"
    );
    assert_eq!(fs::read_dir(&root.0).expect("directory").count(), 0);
}

/// PRE-2: replacement is latest-only; rejected admission cannot cancel the already accepted tail.
#[tokio::test]
async fn preparation_replacement_reaps_before_starting_only_the_latest_batch() {
    let root = Fixture::new();
    let script = r#"
if not (ROOT / 'first').exists():
    mark('first')
    hold()
else:
    data = request()
    (ROOT / 'received').write_text(json.dumps(data))
    mark('second')
    send(reply(data['ticket']))
    hold()
"#;
    let mut owner = Preparation::new(root.worker(script));
    let first = owner
        .submit(vec![request(&"blocked ".repeat(20_000), 0, 60)])
        .expect("first");
    let pid = running(&mut owner, &root.0.join("first")).await;
    for revision in 1..64 {
        owner
            .submit(vec![request("replaced", revision, 60)])
            .expect("replace");
    }
    let latest = owner
        .submit(vec![request("latest 中文\n\n", 64, 88)])
        .expect("latest");
    assert!(matches!(
        owner.submit(vec![request(&"x".repeat(256 * 1024), 65, 60)]),
        Err(Failure::Protocol(_))
    ));
    assert!(matches!(owner.next().await, Completion::Cancelled(ticket) if ticket == first));
    gone(pid);
    assert!(
        !root.0.join("second").exists(),
        "replacement must not start during cleanup"
    );
    let completion = owner.next().await;
    let second = marker(&root.0.join("second")).await;
    owner.shutdown().await.expect("reap replacement");
    assert!(
        matches!(completion, Completion::Ready(ticket, Err(preparation::Refusal::Capacity)) if ticket == latest),
        "{completion:?}"
    );
    gone(second);
    let received: serde_json::Value =
        serde_json::from_slice(&fs::read(root.0.join("received")).expect("request")).expect("json");
    assert_eq!(
        received["requests"][0]["entry"]["Text"]["source"],
        "latest 中文\n\n"
    );
    assert_eq!(received["requests"][0]["key"]["width"], 88);
}

/// PRE-2: selecting another event never restarts a partially written request or consumed reply.
#[tokio::test]
async fn preparation_retains_partial_pipe_io_across_select_interruptions() {
    let root = Fixture::new();
    let script = r#"
header = exact(1)
mark('request-header')
hold()
header += exact(3)
data = json.loads(exact(int.from_bytes(header, 'big')))
(ROOT / 'received').write_text(json.dumps(data))
response = reply(data['ticket'])
header = len(response).to_bytes(4, 'big')
write(header[:2])
mark('reply-header')
hold()
write(header[2:] + response[:7])
mark('reply-body')
hold()
write(response[7:])
data = request()
mark('reused')
send(reply(data['ticket']))
hold()
"#;
    let mut owner = Preparation::new(root.worker(script));
    let source = "中文e\u{301} ".repeat(18_000);
    let ticket = owner
        .submit(vec![request(&source, 3, 60)])
        .expect("bounded request");
    let mut pid = None;
    for name in ["request-header", "reply-header", "reply-body"] {
        let current = running(&mut owner, &root.0.join(name)).await;
        if let Some(pid) = pid {
            assert_eq!(current, pid);
        }
        pid = Some(current);
        for _ in 0..8 {
            let mut next = Box::pin(owner.next());
            std::future::poll_fn(|cx| {
                assert!(next.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
        release(current);
    }
    let completion = owner.next().await;
    let next_ticket = owner
        .submit(vec![request("same persistent child", 4, 88)])
        .expect("reuse");
    let reused = owner.next().await;
    let reused_pid = marker(&root.0.join("reused")).await;
    owner.shutdown().await.expect("reap idle child");
    gone(pid.expect("real child"));
    assert_eq!(Some(reused_pid), pid, "successful work retains one process");
    assert!(
        matches!(reused, Completion::Ready(received, Err(preparation::Refusal::Capacity)) if received == next_ticket),
        "{reused:?}"
    );
    assert!(
        matches!(completion, Completion::Ready(received, Err(preparation::Refusal::Capacity)) if received == ticket),
        "{completion:?}"
    );
    let received: serde_json::Value =
        serde_json::from_slice(&fs::read(root.0.join("received")).expect("request")).expect("json");
    assert_eq!(received["requests"][0]["entry"]["Text"]["source"], source);
}

/// PRE-2: cancel/quit reap blocked computation; cancellation before polling spawns no child.
#[tokio::test]
async fn preparation_shutdown_cancels_active_and_pending_without_an_idle_wake() {
    let root = Fixture::new();
    let executable = root.worker("mark('ready')\nhold()");
    let mut owner = Preparation::new(executable.clone());
    owner
        .submit(vec![request("not started", 0, 60)])
        .expect("submit");
    owner.shutdown().await.expect("cancel before poll");
    assert!(!root.0.join("ready").exists());
    let mut owner = Preparation::new(executable);
    owner
        .submit(vec![request("active", 0, 60)])
        .expect("submit");
    let pid = running(&mut owner, &root.0.join("ready")).await;
    owner
        .submit(vec![request("pending", 1, 60)])
        .expect("pending");
    owner.shutdown().await.expect("shutdown");
    gone(pid);
    assert!(matches!(
        owner.submit(vec![request("after stop", 2, 60)]),
        Err(Failure::Unavailable)
    ));
    owner.shutdown().await.expect("idempotent shutdown");
    let mut next = Box::pin(owner.next());
    std::future::poll_fn(|cx| {
        assert!(next.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

/// PRE-1/PRE-2: a stale ticket and a hostile length fail closed, and each failed child is reaped.
#[tokio::test]
async fn malformed_preparation_replies_cannot_attach_or_escape_the_output_bound() {
    for output in [
        "send(reply(data['ticket'] + 1))",
        "write((2 * 1024 * 1024 + 1).to_bytes(4, 'big'))",
    ] {
        let root = Fixture::new();
        let mut owner = Preparation::new(root.worker(&format!(
            "data = request()\nmark('ready')\n{output}\nhold()"
        )));
        let ticket = owner
            .submit(vec![request("source", 0, 60)])
            .expect("submit");
        let result = owner.next().await;
        let pid = marker(&root.0.join("ready")).await;
        owner.shutdown().await.expect("settled owner");
        gone(pid);
        assert!(
            matches!(result, Completion::Failed(Some(received), Failure::Protocol(_)) if received == ticket),
            "{result:?}"
        );
    }
}

/// PRE-2: a child stuck in computation is actually terminated when its operation deadline expires.
#[tokio::test]
async fn preparation_timeout_kills_and_reaps_computation() {
    let root = Fixture::new();
    let mut owner = Preparation::new(root.worker("mark('ready')\nhold()"));
    let ticket = owner
        .submit(vec![request("blocked", 0, 60)])
        .expect("submit");
    let pid = running(&mut owner, &root.0.join("ready")).await;
    let completion = owner.next().await;
    owner.shutdown().await.expect("settled owner");
    gone(pid);
    assert!(
        matches!(completion, Completion::Failed(Some(received), Failure::TimedOut) if received == ticket),
        "{completion:?}"
    );
}

fn projected(source: &str) -> plexmaton_tui::Workspace {
    use plexmaton_core::{
        AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    };
    let agent = AgentId::new("primary").expect("agent");
    let item = TranscriptItemId::new("source").expect("item");
    let events = [
        ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Running,
        },
        ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        },
        ConversationEvent::TranscriptDelta {
            agent_id: agent,
            item_id: item,
            item_revision: 1,
            text: source.into(),
        },
    ];
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
    );
    workspace
}

async fn settle(
    owner: &mut preparation::LivePreparation,
    workspace: &mut plexmaton_tui::Workspace,
    terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
) {
    for attempt in 0..32 {
        workspace.draw(terminal).expect("live frame");
        owner.sync(workspace);
        if owner.is_pending() {
            assert!(attempt < 31, "live preparation did not settle");
            let completion = owner.next().await;
            owner.apply(completion, workspace);
        } else if !workspace.needs_draw() {
            return;
        }
    }
    panic!("live preparation did not settle");
}

/// PRE-1/PRE-3/SEL-2: actual executable preparation reaches real workspace cells and pointer copy.
#[tokio::test]
async fn real_preparation_process_drives_painted_rows_and_exact_pointer_copy() {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    };
    let mut owner = preparation::LivePreparation::new(env!("CARGO_BIN_EXE_plexmaton").into());
    for width in [120, 88, 60] {
        let mut workspace = projected("# Heading\n\n**visible** 中文 e\u{301}");
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        settle(&mut owner, &mut workspace, &mut terminal).await;
        assert_eq!(workspace.metrics().text_layouts(), 1);
        let buffer = terminal.backend().buffer();
        let (x, y) = (0..24)
            .find_map(|y| {
                (0..width - 7)
                    .find(|x| {
                        "visible".chars().enumerate().all(|(offset, ch)| {
                            buffer[(*x + offset as u16, y)].symbol() == ch.to_string()
                        })
                    })
                    .map(|x| (x, y))
            })
            .expect("prepared text is painted");
        let event = |kind, x| {
            Event::Mouse(MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            })
        };
        workspace.handle(&event(MouseEventKind::Down(MouseButton::Left), x));
        workspace.handle(&event(MouseEventKind::Drag(MouseButton::Left), x + 7));
        assert_eq!(
            workspace
                .handle(&event(MouseEventKind::Up(MouseButton::Left), x + 7))
                .copied
                .expect("prepared pointer copy")
                .text,
            "visible"
        );
        assert_eq!(
            workspace.metrics().text_layouts(),
            1,
            "copy uses the worker's map"
        );
    }
    owner
        .shutdown()
        .await
        .expect("reap persistent preparation worker");
}

/// PRE-3: a real reply already received by the process adapter cannot attach after replacement,
/// even when the new workspace repeats every semantic identity, revision and width.
#[tokio::test]
async fn late_real_reply_cannot_attach_to_a_replaced_workspace() {
    let mut owner = preparation::LivePreparation::new(env!("CARGO_BIN_EXE_plexmaton").into());
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(88, 24)).expect("terminal");
    let mut workspace = projected("**old workspace**");
    workspace.draw(&mut terminal).expect("old pending");
    owner.sync(&mut workspace);
    let late = owner.next().await;
    workspace = projected("**new workspace**");
    workspace.draw(&mut terminal).expect("replacement pending");
    owner.apply(late, &mut workspace);
    assert_eq!(workspace.metrics().text_layouts(), 0);
    settle(&mut owner, &mut workspace, &mut terminal).await;
    owner.shutdown().await.expect("reap worker");
    let text = terminal
        .backend()
        .buffer()
        .content
        .chunks(88)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("new workspace"));
    assert!(!text.contains("old workspace"));
}

/// PRE-1/PRE-3: a wire/allocation-limited batch is retried in smaller bounded pieces. A large
/// neighbour cannot turn otherwise valid entries into permanent failures or silently truncate them.
#[tokio::test]
async fn live_preparation_splits_capacity_batches_without_losing_valid_entries() {
    use plexmaton_core::{ConversationEvent, ConversationEventEnvelope, EventSequence};
    let source = "**bounded** ".repeat(1000);
    let size = request(&source, 1, 118).prepare();
    assert!(size.refusal().is_none(), "one entry must fit");
    assert!(
        size.allocation_bytes() * 16 > plexmaton_tui::preparation::MAX_BATCH_BYTES,
        "the fixture must overflow a batch, not an individual entry"
    );
    let mut workspace = projected(&source);
    let agent = AgentId::new("primary").expect("agent");
    let mut sequence = 3;
    for index in 1..16 {
        let item = TranscriptItemId::new(format!("source-{index}")).expect("item");
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
                text: source.clone(),
            },
        ] {
            sequence += 1;
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            }]);
        }
    }
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).expect("terminal");
    let mut owner = preparation::LivePreparation::new(env!("CARGO_BIN_EXE_plexmaton").into());
    let mut refused = 0;
    let mut completed = Vec::new();
    for _ in 0..32 {
        workspace.draw(&mut terminal).expect("frame");
        owner.sync(&mut workspace);
        if !owner.is_pending() && !workspace.needs_draw() {
            break;
        }
        if owner.is_pending() {
            let completion = owner.next().await;
            match &completion {
                Completion::Ready(_, Err(preparation::Refusal::Capacity)) => refused += 1,
                Completion::Ready(_, Ok(results)) => {
                    completed.push(results.len());
                    for result in results {
                        assert_eq!(
                            result.refusal(),
                            None,
                            "the individual entry must remain valid"
                        );
                        assert_eq!(
                            result
                                .selection_text()
                                .expect("plain text")
                                .matches("bounded")
                                .count(),
                            1000
                        );
                    }
                }
                other => panic!("unexpected process result: {other:?}"),
            }
            owner.apply(completion, &mut workspace);
        }
    }
    owner.shutdown().await.expect("reap process");
    assert!(
        refused > 0 && refused <= 4,
        "split retries are bounded: {refused}"
    );
    assert!(completed.iter().all(|size| *size < 16) && !completed.is_empty());
    assert!(!workspace.needs_draw());
    let text = terminal
        .backend()
        .buffer()
        .content
        .chunks(120)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("bounded")
            && !text.contains("preparation limit")
            && !text.contains("Preparing text")
    );
}
