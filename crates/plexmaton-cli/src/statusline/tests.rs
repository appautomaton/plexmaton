use super::*;

fn config(command: &str) -> StatusLineConfig {
    StatusLineConfig {
        command: command.into(),
        max_rows: 6,
        timeout_ms: 500,
        refresh_ms: None,
    }
}

fn plain(output: &StatusLineText) -> String {
    output
        .lines()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn status_command_reads_snapshot_eof_and_returns_only_styled_text() {
    // STL-2: a real pipe must close stdin, collect stdout, and reap the child.
    let output = process::execute(
        &config("cat >/dev/null; printf '\\033[38;2;1;2;3mhello\\033[0m\\nworld\\n'"),
        b"{}".to_vec(),
        std::path::Path::new("/"),
        &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
        CancellationToken::new(),
    )
    .await
    .expect("local script");
    assert_eq!(plain(&output), "hello\nworld");
    assert_eq!(
        output.lines()[0].spans[0].style.fg,
        Some(ratatui::style::Color::Rgb(1, 2, 3))
    );
}

#[tokio::test]
async fn status_command_failure_timeout_overflow_and_cancellation_are_bounded() {
    // STL-2: failures exercise real subprocesses, never a configured provider.
    for (command, expected) in [
        ("cat >/dev/null; exit 7", "command failed"),
        ("cat >/dev/null; sleep 30", "timed out"),
        ("cat >/dev/null; yes x", "output too large"),
        ("cat >/dev/null; yes x >&2", "output too large"),
        (
            "cat >/dev/null; printf '\\033]52;c;secret\\007'",
            "invalid styled output",
        ),
    ] {
        let mut config = config(command);
        config.timeout_ms = 100;
        let error = process::execute(
            &config,
            b"{}".to_vec(),
            std::path::Path::new("/"),
            &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
            CancellationToken::new(),
        )
        .await
        .expect_err("failure");
        assert!(error.to_string().contains(expected), "{error}");
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        process::execute(
            &config("exit 7"),
            vec![],
            std::path::Path::new("/"),
            &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
            cancel
        )
        .await,
        Err(Failure::Cancelled)
    ));
}

#[tokio::test]
async fn status_script_omits_null_fields_and_keeps_rainbow_path() {
    // STL-3: absence isn't zero, literal null, or an empty colored segment in the supplied script.
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/statusline-pastel.sh");
    let mut config = config(&format!("bash '{}'", script.display()));
    config.timeout_ms = 5000;
    let input = serde_json::json!({"model":{"display_name":"Luna"},"effort":{"level":"high"},
        "cwd":"/nonexistent/fixture/project", "context_window":{"context_window_size":272000},
        "plexmaton":{"terminal":{"columns":95},"context":{"availability":"available",
            "input_tokens":99999,"estimated_tokens":99999},
        "usage":{"coverage":"unavailable"}}, "cost":{"total_cost_usd":null}});
    let text = process::execute(
        &config,
        serde_json::to_vec(&input).expect("fixture"),
        std::path::Path::new("/"),
        &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
        CancellationToken::new(),
    )
    .await
    .expect("sample script");
    let output = plain(&text);
    for absent in [
        "null", "cache", "ctx", "", "", "", "", "~", "$", "↑", "↓",
    ] {
        assert!(!output.contains(absent), "{output}");
    }
    assert!(output.contains("Luna  high"));
    assert!(output.contains("") && output.contains(""));
    assert!(output.contains("nonexistent / fixture / project"));
    assert!(
        text.lines()
            .last()
            .expect("path row")
            .spans
            .iter()
            .filter_map(|span| span.style.fg)
            .collect::<std::collections::HashSet<_>>()
            .len()
            >= 3
    );
}

#[tokio::test]
async fn status_script_context_uses_reported_input_with_a_glyph_at_each_width() {
    // STL-3: no estimate disguised as measurement, including when the cache split is unknown.
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/statusline-pastel.sh");
    let mut config = config(&format!("bash '{}'", script.display()));
    config.timeout_ms = 5000;
    let directory = TestDirectory::new();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet", "--initial-branch=fixture-branch"])
            .arg(&directory.0)
            .status()
            .expect("isolated git fixture")
            .success()
    );
    for width in [120, 95, 60] {
        let input = serde_json::json!({"model":{"display_name":"Luna"},
            "cwd":directory.0,
            "cost":{"total_cost_usd":0.012},
            "context_window":{"context_window_size":272000,"used_percentage":99,
                "total_input_tokens":12345,"total_output_tokens":678,
                "current_usage":{"input_tokens":2345,"cache_read_input_tokens":10000,
                    "cache_creation_input_tokens":0}},
            "plexmaton":{"terminal":{"columns":width},
                "context":{"availability":"available","input_tokens":99999,"estimated_tokens":99999},
                "latest_request":{"terminal":{"usage":{"coverage":"complete",
                    "counts":{"input":12345,"cached_input":null}}}}}});
        let output = process::execute(
            &config,
            serde_json::to_vec(&input).expect("fixture"),
            std::path::Path::new("/"),
            &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
            CancellationToken::new(),
        )
        .await
        .expect("sample script");
        let output = plain(&output);
        for expected in [
            " Luna",
            " fixture-branch",
            " 12.3k/272.0k 4%",
            " 81%",
            " ↑12.3k ↓678",
            " $0.012",
            "",
        ] {
            assert!(output.contains(expected), "{output}");
        }
        for absent in ["ctx", "~", "100.0k", "99%", "null"] {
            assert!(!output.contains(absent), "{output}");
        }
    }
}

#[tokio::test]
async fn status_owner_replacement_and_shutdown_join_before_returning() {
    // STL-2: cancellation while next() is polled keeps its JoinHandle in the owner.
    let mut owner = StatusLine {
        config: config("sleep 30"),
        model: model(),
        credential_envs: vec!["TEST_KEY".into()],
        cwd: "/".into(),
        active: None,
        due: None,
        generation: 0,
        last_input: Some(vec![]),
        force_refresh: false,
        cleanup_failed: false,
    };
    let token = CancellationToken::new();
    let child_token = token.clone();
    owner.active = Some(Active {
        generation: 0,
        cancel: token,
        task: tokio::spawn(async move {
            process::execute(
                &config("cat >/dev/null; sleep 30"),
                b"{}".to_vec(),
                std::path::Path::new("/"),
                &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
                child_token,
            )
            .await
        }),
    });
    owner.mark_dirty();
    assert!(owner.last_input.is_none());
    owner.shutdown().await.expect("join cancellation");
    assert!(owner.active.is_none());
}

/// STL-3: bounded projection diagnostics do not surface error payloads or warn on pending work.
#[tokio::test]
async fn status_script_context_diagnostics_are_content_free_and_pending_is_quiet() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/statusline-pastel.sh");
    let mut config = config(&format!("bash '{}'", script.display()));
    config.timeout_ms = 5000;
    for (reason, expected) in [
        ("encoding_failed", Some("encoding failed")),
        ("projection_failed", Some("projection failed")),
        ("arithmetic_overflow", Some("arithmetic overflow")),
        ("invalid_budget", Some("invalid budget")),
        ("pending_commit", None),
        ("incomplete_tool_batch", None),
        ("unrecognized-secret-marker", None),
    ] {
        let input = serde_json::json!({
            "model":{"display_name":"Luna"},
            "context_window":{"context_window_size":272000},
            "plexmaton":{"terminal":{"columns":60},
                "context":{"availability":"unavailable","reason":reason},
                "latest_request":{"terminal":{"usage":{"counts":{"input":100}}}}}
        });
        let text = process::execute(
            &config,
            serde_json::to_vec(&input).expect("JSON"),
            std::path::Path::new("/"),
            &[],
            CancellationToken::new(),
        )
        .await
        .expect("script");
        let output = plain(&text);
        assert!(output.contains("Luna"));
        assert_eq!(output.contains(""), expected.is_none());
        if let Some(expected) = expected {
            assert!(
                output.contains(&format!("Context unavailable · {expected}")),
                "{output}"
            );
        } else {
            assert!(!output.contains("Context unavailable"), "{output}");
        }
        assert!(!output.contains("unrecognized-secret-marker"));
    }
}

pub(super) fn model() -> ResolvedModel {
    plexmaton_provider::ModelRegistry::parse(CONFIG)
        .expect("fixture config")
        .active_model()
        .clone()
}

pub(super) const CONFIG: &str = r#"
active_model = { provider = "local", model = "luna" }
[providers.local]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "PLEXMATON_TEST_UNUSED_KEY"
api = "openai_responses"
[providers.local.models.luna]
id = "gpt-test"
display_name = "Luna"
reasoning_effort = "high"
context_window_tokens = 272000
max_output_tokens = 8192
output_reserve_tokens = 8192
"#;

#[test]
fn status_configuration_stays_outside_the_model_registry() {
    // STL-2: presentation is user configuration; neither secrets nor parser source enters errors.
    let source = format!("[status_line]\ncommand = 'printf hello'\nmax_rows = 4\n{CONFIG}");
    // active_model must remain at the TOML root, before entering a table.
    let source = source.replace(
        "\nactive_model = { provider = \"local\", model = \"luna\" }",
        "",
    );
    let source = format!("active_model = {{ provider = \"local\", model = \"luna\" }}\n{source}");
    let crate::user_config::UserConfig {
        models,
        status_line: status,
        ..
    } = crate::user_config::parse(&source).expect("user configuration");
    assert_eq!(models.active_model(), &model());
    assert_eq!(status.expect("script").max_rows, 4);
    assert!(crate::user_config::parse(&source.replace("max_rows = 4", "max_rows = 0")).is_err());
    assert!(
        crate::user_config::parse(&source.replace("max_rows = 4", "surprise = 'secret-marker'"))
            .expect_err("unknown key")
            .to_string()
            .find("secret-marker")
            .is_none()
    );
}

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("plexmaton-status-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&path).expect("isolated test directory");
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn status_shutdown_joins_descendants_after_a_dropped_poll() {
    use std::{future::Future as _, task::Poll};
    // STL-2: the ready marker proves the actual shell and descendant exist before cancellation.
    let directory = TestDirectory::new();
    let ready = directory.0.join("ready");
    let command = format!(
        "cat >/dev/null; sleep 30 & child=$!; printf '%s %s' \"$$\" \"$child\" > '{}'; wait; printf stale",
        ready.display()
    );
    let mut owner = StatusLine::new(config(&command), model(), directory.0.clone());
    let mut child_config = config(&command);
    child_config.timeout_ms = 5000;
    let cancel = CancellationToken::new();
    let child_cancel = cancel.clone();
    let cwd = directory.0.clone();
    owner.active = Some(Active {
        generation: 0,
        cancel,
        task: tokio::spawn(async move {
            process::execute(
                &child_config,
                b"{}".to_vec(),
                &cwd,
                &["PLEXMATON_TEST_UNUSED_KEY".to_owned()],
                child_cancel,
            )
            .await
        }),
    });
    let pids = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&ready) {
                let values: Vec<i32> = text
                    .split_whitespace()
                    .filter_map(|word| word.parse().ok())
                    .collect();
                if values.len() == 2 {
                    break values;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("script never reached readiness");
    let mut poll = Box::pin(owner.next());
    std::future::poll_fn(|cx| {
        assert!(poll.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(poll);
    tokio::time::timeout(Duration::from_secs(2), owner.shutdown())
        .await
        .expect("shutdown settled")
        .expect("clean shutdown");
    for pid in pids {
        assert_eq!(
            rustix::process::test_kill_process(
                rustix::process::Pid::from_raw(pid).expect("positive pid")
            ),
            Err(rustix::io::Errno::SRCH)
        );
    }
    owner.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn status_refresh_coalesces_without_cancelling_inflight_work() {
    use std::{future::Future as _, task::Poll};
    // STL-2: resize/semantic invalidation keeps one bounded run alive, then captures latest input.
    let mut owner = StatusLine::new(config("exit 0"), model(), "/".into());
    let cancel = CancellationToken::new();
    let (finish, finished) = tokio::sync::oneshot::channel();
    owner.last_input = Some(b"old width".to_vec());
    owner.due = None;
    owner.active = Some(Active {
        generation: 0,
        cancel: cancel.clone(),
        task: tokio::spawn(async move {
            finished.await.expect("test releases execution");
            StatusLineText::parse(b"stale width").map_err(|_| Failure::InvalidOutput)
        }),
    });
    for _ in 0..20 {
        owner.mark_dirty();
    }
    assert!(!cancel.is_cancelled());
    assert!(owner.last_input.is_none());
    let mut update = Box::pin(owner.next());
    std::future::poll_fn(|cx| {
        assert!(update.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(update);
    assert!(owner.active.is_some());
    finish
        .send(())
        .expect("owned task still awaiting completion");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), owner.next())
            .await
            .expect("refresh settles"),
        Update::Capture
    ));
    assert!(owner.active.is_none());
    assert!(owner.due.is_none());
    owner.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn status_stale_cleanup_failure_blocks_replacement_and_remains_visible() {
    // STL-2: only stale presentation is discardable, never failed ownership cleanup.
    let mut owner = StatusLine::new(config("exit 0"), model(), "/".into());
    owner.active = Some(Active {
        generation: 0,
        cancel: CancellationToken::new(),
        task: tokio::spawn(async { Err(Failure::Cleanup) }),
    });
    owner.mark_dirty();
    assert!(matches!(
        owner.next().await,
        Update::Output(Err(Failure::Cleanup))
    ));
    owner.mark_dirty();
    assert!(owner.due.is_none());
    assert!(owner.shutdown().await.is_err());
}

#[test]
fn status_cleanup_error_does_not_hide_session_shutdown_failures() {
    // STL-2: optional presentation cannot replace the durable-session diagnostic at handoff.
    let error = crate::session_result(
        Err(anyhow::anyhow!("terminal failure")),
        Err(anyhow::anyhow!("retained user input; persistence failure")),
        Err(anyhow::anyhow!("status-line cleanup failure")),
        Err(anyhow::anyhow!("clipboard cleanup failure")),
        Err(anyhow::anyhow!("preparation cleanup failure")),
        Err(anyhow::anyhow!("picker cleanup failure")),
        Err(anyhow::anyhow!("permission cleanup failure")),
    )
    .expect_err("combined shutdown failures")
    .to_string();
    assert!(error.starts_with("retained user input"));
    assert!(error.contains("terminal failure"));
    assert!(error.contains("status-line cleanup failure"));
    assert!(error.contains("clipboard cleanup failure"));
    assert!(error.contains("preparation cleanup failure"));
    assert!(error.contains("picker cleanup failure"));
    assert!(error.contains("permission cleanup failure"));
    assert!(crate::session_result(Ok(()), Ok(()), Ok(()), Ok(()), Ok(()), Ok(()), Ok(())).is_ok());
}
