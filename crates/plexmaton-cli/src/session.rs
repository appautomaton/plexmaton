use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use plexmaton_agent::UnixMillis;
use plexmaton_core::{AgentId, SessionId};
use plexmaton_provider::{ApiKey, ResolvedModel};
use plexmaton_runtime::{JournalTailRecovery, LiveRuntime, NativeToolCatalog, SessionRecovery};
use plexmaton_session_store::{AutomaticJournal, JournalFile, SessionDirectory};
use plexmaton_tui::{SessionRestoration, SessionTailRepair};

pub(super) const USAGE: &str =
    "Usage: plexmaton [--ephemeral | create <session-id> | resume <session-id>]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SessionSelection {
    Automatic,
    Ephemeral,
    Create(SessionId),
    Resume(SessionId),
}

pub(super) enum StartupAction {
    Run(SessionSelection),
    Help,
}

pub(super) struct PersistedSession {
    pub(super) id: SessionId,
    pub(super) path: PathBuf,
}

pub(super) struct OpenedSession {
    pub(super) runtime: LiveRuntime,
    pub(super) recovery: Option<SessionRecovery>,
    pub(super) persisted: Option<PersistedSession>,
}

pub(super) fn parse_startup_action(arguments: &[OsString]) -> anyhow::Result<StartupAction> {
    match arguments {
        [] => Ok(StartupAction::Run(SessionSelection::Automatic)),
        [flag] if flag == "--ephemeral" => Ok(StartupAction::Run(SessionSelection::Ephemeral)),
        [flag] if flag == "--help" || flag == "-h" || flag == "help" => Ok(StartupAction::Help),
        [command, session] if command == "create" || command == "resume" => {
            let session = session
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("session id must be UTF-8"))?;
            let session = SessionId::new(session).context("validate session id")?;
            if command == "create" {
                Ok(StartupAction::Run(SessionSelection::Create(session)))
            } else {
                Ok(StartupAction::Run(SessionSelection::Resume(session)))
            }
        }
        _ => anyhow::bail!("{USAGE}"),
    }
}

pub(super) async fn open_selected_session(
    root: &Path,
    selection: SessionSelection,
    agent_id: AgentId,
    model: ResolvedModel,
    key: ApiKey,
    tools: NativeToolCatalog,
) -> anyhow::Result<OpenedSession> {
    Ok(match selection {
        SessionSelection::Automatic => {
            let journal = AutomaticJournal::new(root, created_at_now()?);
            let persisted = PersistedSession {
                id: journal.metadata().session_id().clone(),
                path: journal.path().to_path_buf(),
            };
            let runtime = LiveRuntime::openai_with_automatic_journal(
                agent_id,
                "Plexmaton",
                model,
                key,
                tools,
                journal,
            )
            .await
            .context("configure automatic session")?;
            OpenedSession {
                runtime,
                recovery: None,
                persisted: Some(persisted),
            }
        }
        SessionSelection::Ephemeral => OpenedSession {
            runtime: LiveRuntime::openai(agent_id, "Plexmaton", model, key, tools)
                .context("configure live provider transport")?,
            recovery: None,
            persisted: None,
        },
        SessionSelection::Create(session_id) => {
            let journal = SessionDirectory::under(root)
                .context("open sessions directory")?
                .create(session_id.clone(), created_at_now()?)
                .context("create session")?;
            open_fresh_session(agent_id, model, key, tools, session_id, journal).await?
        }
        SessionSelection::Resume(session_id) => {
            let journal = SessionDirectory::under(root)
                .context("open sessions directory")?
                .resume(&session_id)
                .context("resume session")?;
            let path = journal.path().to_path_buf();
            let (runtime, recovery) =
                LiveRuntime::openai_with_resumed_journal(agent_id, model, key, tools, journal)
                    .await
                    .context("resume durable runtime")?;
            OpenedSession {
                runtime,
                recovery: Some(recovery),
                persisted: Some(PersistedSession {
                    id: session_id,
                    path,
                }),
            }
        }
    })
}

fn created_at_now() -> anyhow::Result<UnixMillis> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("session wall clock is before the Unix epoch")?;
    let millis =
        u64::try_from(elapsed.as_millis()).context("session wall clock is out of range")?;
    Ok(UnixMillis::new(millis))
}

async fn open_fresh_session(
    agent_id: AgentId,
    model: ResolvedModel,
    key: ApiKey,
    tools: NativeToolCatalog,
    session_id: SessionId,
    journal: JournalFile,
) -> anyhow::Result<OpenedSession> {
    let path = journal.path().to_path_buf();
    let runtime = match LiveRuntime::openai_with_fresh_journal(
        agent_id,
        "Plexmaton",
        model,
        key,
        tools,
        journal,
    )
    .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            if let Err(cleanup) = fs::remove_file(&path) {
                anyhow::bail!(
                    "create durable runtime: {error}; remove unusable session at {}: {cleanup}",
                    path.display()
                );
            }
            return Err(error).context("create durable runtime");
        }
    };
    Ok(OpenedSession {
        runtime,
        recovery: None,
        persisted: Some(PersistedSession {
            id: session_id,
            path,
        }),
    })
}

pub(super) fn restoration_feedback(
    recovery: Option<SessionRecovery>,
) -> Option<SessionRestoration> {
    let tail = recovery?.tail.map(|tail| match tail {
        JournalTailRecovery::AddedFinalNewline => SessionTailRepair::AddedFinalNewline,
        JournalTailRecovery::IsolatedFinalTail { bytes } => {
            SessionTailRepair::IsolatedFinalTail { bytes }
        }
    });
    Some(SessionRestoration { tail })
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, io::Write as _, time::Duration};

    use plexmaton_agent::{Agent, ApprovalPolicy, Input, SessionMetadata, TurnBudget, UnixMillis};
    use plexmaton_core::{AgentId, AgentStatus, HeadName, SessionId, TranscriptRole};
    use plexmaton_provider::{ApiKey, ModelRegistry, ResolvedModel, resolve_api_key};
    use plexmaton_runtime::{JournalTailRecovery, NativeToolCatalog, RuntimeUpdate};
    use plexmaton_session_store::{JournalFile, SessionDirectory};
    use plexmaton_tui::{ViewState, Workspace};
    use uuid::{Uuid, Version};

    use super::{
        OpenedSession, SessionSelection, StartupAction, open_selected_session,
        parse_startup_action, restoration_feedback,
    };
    use crate::tests::{FixtureWorkspace, fixture_http_server};

    #[test]
    fn only_a_resumed_session_gets_a_restoration_confirmation() {
        // JRN-5: startup confirmation is UI data, not a journal entry or a model message.
        assert!(restoration_feedback(None).is_none());
        assert_eq!(
            restoration_feedback(Some(plexmaton_runtime::SessionRecovery::default())),
            Some(plexmaton_tui::SessionRestoration { tail: None })
        );
    }

    fn agent_id() -> AgentId {
        AgentId::new("agent-primary").unwrap_or_else(|error| panic!("agent id: {error}"))
    }

    fn session_id(value: &str) -> SessionId {
        SessionId::new(value).unwrap_or_else(|error| panic!("session id: {error}"))
    }

    fn transport(
        root: &std::path::Path,
        base_url: &str,
    ) -> (ResolvedModel, ApiKey, NativeToolCatalog) {
        let config = ModelRegistry::parse(&format!(
            r#"active_model = {{ provider = "test", model = "fixture" }}

[providers.test]
base_url = "{base_url}"
api_key_env = "TEST_KEY"
[providers.test.models.fixture]
api = "openai_chat_completions"
id = "fixture"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#
        ))
        .unwrap_or_else(|error| panic!("test config: {error}"));
        let model = config.active_model();
        let key = resolve_api_key(model, Some(OsString::from("fixture-only")))
            .unwrap_or_else(|error| panic!("test key: {error}"));
        let tools = NativeToolCatalog::open(
            root,
            model.api_key_env(),
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("test tools: {error}"));
        (model.clone(), key, tools)
    }

    async fn project_until_idle(runtime: &mut plexmaton_runtime::LiveRuntime) -> ViewState {
        let mut workspace = Workspace::default();
        while let Some(event) = runtime.try_next_event() {
            workspace.emit(vec![event]);
        }
        while runtime.has_active_work() {
            match tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
                .await
                .unwrap_or_else(|_| panic!("runtime fixture timed out"))
                .unwrap_or_else(|error| panic!("runtime fixture failed: {error}"))
            {
                RuntimeUpdate::Event(event) => workspace.emit(vec![event]),
                RuntimeUpdate::Report(report) => panic!("unexpected report: {report:?}"),
                RuntimeUpdate::Finished => break,
            }
        }
        while let Some(event) = runtime.try_next_event() {
            workspace.emit(vec![event]);
        }
        workspace.state().clone()
    }

    #[test]
    fn jrn_4_cli_grammar_makes_only_the_explicit_flag_ephemeral() {
        assert!(matches!(
            parse_startup_action(&[]).unwrap_or_else(|error| panic!("empty args: {error}")),
            StartupAction::Run(SessionSelection::Automatic)
        ));
        assert!(matches!(
            parse_startup_action(&[OsString::from("--ephemeral")])
                .unwrap_or_else(|error| panic!("ephemeral args: {error}")),
            StartupAction::Run(SessionSelection::Ephemeral)
        ));
        for (command, expected_create) in [("create", true), ("resume", false)] {
            let parsed =
                parse_startup_action(&[OsString::from(command), OsString::from("work-01")])
                    .unwrap_or_else(|error| panic!("parse {command}: {error}"));
            assert!(
                matches!(
                    parsed,
                    StartupAction::Run(SessionSelection::Create(ref id))
                        if expected_create && id.as_str() == "work-01"
                ) || matches!(
                    parsed,
                    StartupAction::Run(SessionSelection::Resume(ref id))
                        if !expected_create && id.as_str() == "work-01"
                )
            );
        }
        assert!(parse_startup_action(&[OsString::from("resume")]).is_err());
    }

    /// JRN-4: automatic startup allocates identity but no file; the opt-out has no file owner.
    #[tokio::test]
    async fn a_default_session_is_durable_and_ephemeral_is_an_explicit_opt_out() {
        let root = FixtureWorkspace::new();
        let (profile, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let mut automatic = open_selected_session(
            root.path(),
            SessionSelection::Automatic,
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("open automatic session: {error}"));
        let persisted = automatic
            .persisted
            .as_ref()
            .unwrap_or_else(|| panic!("automatic session was not persisted"));
        let raw_uuid = persisted
            .id
            .as_str()
            .strip_prefix("session-")
            .unwrap_or_else(|| panic!("automatic session lacks its type prefix"));
        let uuid = Uuid::parse_str(raw_uuid)
            .unwrap_or_else(|error| panic!("automatic session UUID: {error}"));
        assert_eq!(uuid.get_version(), Some(Version::SortRand));
        assert!(!persisted.path.exists());
        let persisted_path = persisted.path.clone();
        automatic
            .runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown automatic session: {error}"));
        assert!(!persisted_path.exists());
        assert!(!root.path().join("sessions").exists());

        let ephemeral_root = FixtureWorkspace::new();
        let (profile, key, tools) = transport(ephemeral_root.path(), "http://127.0.0.1:9/v1");
        let mut ephemeral = open_selected_session(
            ephemeral_root.path(),
            SessionSelection::Ephemeral,
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("open ephemeral session: {error}"));
        assert!(ephemeral.persisted.is_none());
        ephemeral
            .runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown ephemeral session: {error}"));
        assert!(
            !ephemeral_root.path().join("sessions").exists(),
            "ephemeral startup must not create a session store"
        );
    }

    /// JRN-4/JRN-7: the automatic writer crosses its file boundary before model dispatch.
    #[tokio::test]
    async fn automatic_first_message_is_saved_and_resumes_without_another_request() {
        let root = FixtureWorkspace::new();
        let (url, server) = fixture_http_server([include_str!(
            "../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"
        )]);
        let (model, key, tools) = transport(root.path(), &url);
        let mut opened = open_selected_session(
            root.path(),
            SessionSelection::Automatic,
            agent_id(),
            model,
            key,
            tools,
        )
        .await
        .expect("automatic startup");
        let saved = opened.persisted.as_ref().expect("planned identity");
        let path = saved.path.clone();
        let id = saved.id.clone();
        assert!(!path.exists());
        opened
            .runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "Keep this first message".into(),
                },
            )
            .await
            .expect("first submission");
        assert!(path.exists());
        let _live = project_until_idle(&mut opened.runtime).await;
        let before = opened
            .runtime
            .acknowledged_session()
            .expect("acknowledged session")
            .0
            .clone();
        opened.runtime.shutdown().await.expect("shutdown");
        drop(opened);
        assert_eq!(
            server
                .join()
                .expect("server joins")
                .expect("fixture request")
                .len(),
            1
        );
        let file = JournalFile::open(&path).expect("reopen JSONL");
        assert_eq!(file.journal().session_id(), &id);
        assert_ne!(file.journal().created_at_unix_ms(), UnixMillis::EPOCH);
        drop(file);
        let (model, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let mut resumed = open_selected_session(
            root.path(),
            SessionSelection::Resume(id),
            agent_id(),
            model,
            key,
            tools,
        )
        .await
        .expect("resume");
        assert!(!resumed.runtime.has_active_work());
        let restored = project_until_idle(&mut resumed.runtime).await;
        assert!(
            restored
                .primary_agent()
                .expect("agent")
                .transcript()
                .any(|item| item.role == TranscriptRole::User
                    && item.source == "Keep this first message")
        );
        assert_eq!(
            resumed
                .runtime
                .acknowledged_session()
                .expect("acknowledged replay")
                .0,
            &before
        );
        resumed.runtime.shutdown().await.expect("shutdown resumed");
    }

    /// JRN-7: lazy creation failure retains exact user input and starts no request.
    #[tokio::test]
    async fn failed_automatic_creation_returns_first_input_without_dispatch() {
        let root = FixtureWorkspace::new();
        std::fs::write(root.path().join("sessions"), "blocking file").expect("failure fixture");
        let (model, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let mut opened = open_selected_session(
            root.path(),
            SessionSelection::Automatic,
            agent_id(),
            model,
            key,
            tools,
        )
        .await
        .expect("startup does no session IO");
        let report = opened
            .runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "Keep my exact draft\nplease".into(),
                },
            )
            .await
            .expect("typed failure report");
        assert!(report.persistence_failure.is_some());
        assert_eq!(report.undelivered.len(), 1);
        assert_eq!(report.undelivered[0].text, "Keep my exact draft\nplease");
        assert!(!opened.runtime.has_active_work());
        let events: Vec<_> = std::iter::from_fn(|| opened.runtime.try_next_event()).collect();
        assert!(!events.iter().any(|event| matches!(
            event.event,
            plexmaton_core::SessionEvent::TranscriptItemStarted {
                role: TranscriptRole::User,
                ..
            }
        )));
        assert!(matches!(
            opened.runtime.shutdown().await,
            Err(plexmaton_runtime::RuntimeError::JournalRequiresReopen)
        ));
        assert_eq!(
            std::fs::read_to_string(root.path().join("sessions")).expect("fixture intact"),
            "blocking file"
        );
    }

    /// JRN-4: a failed fresh runtime cannot leave a header-only session occupying its name.
    #[tokio::test]
    async fn failed_fresh_runtime_construction_removes_its_unusable_session() {
        let root = FixtureWorkspace::new();
        let named_id = session_id("invalid-transport");
        for selection in [
            SessionSelection::Create(named_id.clone()),
            SessionSelection::Automatic,
        ] {
            let (profile, key, _matching_tools) = transport(root.path(), "http://127.0.0.1:9/v1");
            let tools = NativeToolCatalog::open(
                root.path(),
                "OTHER_KEY",
                "/bin/false",
                "/bin/false",
                Vec::new(),
            )
            .unwrap_or_else(|error| panic!("open mismatched tools: {error}"));
            assert!(
                open_selected_session(root.path(), selection, agent_id(), profile, key, tools,)
                    .await
                    .is_err()
            );
        }

        let sessions = SessionDirectory::under(root.path())
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));
        assert!(
            !sessions
                .path_for(&named_id)
                .unwrap_or_else(|error| panic!("named session path: {error}"))
                .exists()
        );
        let retained = std::fs::read_dir(sessions.path())
            .unwrap_or_else(|error| panic!("list sessions: {error}"))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("read session entry: {error}"));
        assert!(retained.is_empty());
    }

    /// JRN-4/JRN-5: the production constructors preserve one real tool conversation on reopen.
    #[tokio::test]
    async fn a_real_created_session_resumes_with_equal_visible_and_model_projections() {
        let root = FixtureWorkspace::new();
        std::fs::write(root.path().join("README.md"), "Plexmaton fixture\n")
            .unwrap_or_else(|error| panic!("write fixture file: {error}"));
        let (base_url, server) = fixture_http_server([
            include_str!("../../plexmaton-provider/tests/fixtures/chat_tool_call.sse"),
            include_str!("../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"),
        ]);
        let (profile, key, tools) = transport(root.path(), &base_url);
        let mut created = open_selected_session(
            root.path(),
            SessionSelection::Create(session_id("conversation-01")),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("create runtime: {error}"))
        .runtime;
        created
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "Read the project name.".to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("submit saved turn: {error}"));
        let _live = project_until_idle(&mut created).await;
        created
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown created runtime: {error}"));
        drop(created);
        let requests = server
            .join()
            .unwrap_or_else(|_| panic!("fixture HTTP server panicked"))
            .unwrap_or_else(|error| panic!("fixture HTTP server: {error}"));
        assert_eq!(requests.len(), 2);

        let sessions = SessionDirectory::under(root.path())
            .unwrap_or_else(|error| panic!("sessions directory: {error}"));
        let expected_store = sessions
            .resume(&session_id("conversation-01"))
            .unwrap_or_else(|error| panic!("open expected projection: {error}"));
        let head = HeadName::new("main").unwrap_or_else(|error| panic!("main head: {error}"));
        let expected_projection = expected_store
            .journal()
            .project(&head)
            .unwrap_or_else(|error| panic!("project stored session: {error:?}"));
        let mut expected = ViewState::default();
        for event in expected_projection.events() {
            let _applied = expected.apply(event.clone());
        }
        drop(expected_store);

        let (resume_url, resume_server) = fixture_http_server([include_str!(
            "../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"
        )]);
        let (profile, key, tools) = transport(root.path(), &resume_url);
        let OpenedSession {
            runtime: mut resumed,
            recovery,
            ..
        } = open_selected_session(
            root.path(),
            SessionSelection::Resume(session_id("conversation-01")),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("resume runtime: {error}"));
        assert!(recovery.as_ref().expect("resumed").is_clean());
        assert!(restoration_feedback(recovery).is_some());
        let after = project_until_idle(&mut resumed).await;
        assert_eq!(after, expected);
        resumed
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "Use the saved context.".to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("continue resumed runtime: {error}"));
        let _continued = project_until_idle(&mut resumed).await;
        resumed
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown resumed runtime: {error}"));
        let resumed_requests = resume_server
            .join()
            .unwrap_or_else(|_| panic!("resume fixture server panicked"))
            .unwrap_or_else(|error| panic!("resume fixture server: {error}"));
        let request = std::str::from_utf8(&resumed_requests[0])
            .unwrap_or_else(|error| panic!("resume request UTF-8: {error}"));
        for retained in [
            "Read the project name.",
            "call_read_1",
            "Plexmaton.",
            "Use the saved context.",
        ] {
            assert!(request.contains(retained), "resume omitted {retained:?}");
        }
    }

    /// JRN-4: an incomplete final line is isolated through the production resume constructor.
    #[tokio::test]
    async fn a_torn_final_record_resumes_with_one_typed_visible_recovery() {
        let root = FixtureWorkspace::new();
        let (profile, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let created = open_selected_session(
            root.path(),
            SessionSelection::Create(session_id("torn-01")),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("create torn fixture: {error}"))
        .runtime;
        drop(created);
        let sessions = SessionDirectory::under(root.path())
            .unwrap_or_else(|error| panic!("sessions directory: {error}"));
        let path = sessions
            .path_for(&session_id("torn-01"))
            .unwrap_or_else(|error| panic!("session path: {error}"));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap_or_else(|error| panic!("open torn fixture: {error}"));
        file.write_all(br#"{"type":"#)
            .unwrap_or_else(|error| panic!("write torn fixture: {error}"));
        drop(file);

        let (profile, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let OpenedSession {
            runtime: mut resumed,
            recovery,
            ..
        } = open_selected_session(
            root.path(),
            SessionSelection::Resume(session_id("torn-01")),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("resume torn fixture: {error}"));
        assert_eq!(
            recovery.as_ref().expect("resumed").tail,
            Some(JournalTailRecovery::IsolatedFinalTail { bytes: 8 })
        );
        let mut workspace = Workspace::default();
        while let Some(event) = resumed.try_next_event() {
            workspace.emit(vec![event]);
        }
        workspace.report_session_recovery(
            restoration_feedback(recovery).unwrap_or_else(|| panic!("missing recovery notice")),
        );
        assert_eq!(workspace.state().notices().count(), 0);
        resumed
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown torn fixture: {error}"));
    }

    /// JRN-5: an open turn is settled durably and never restarts its model during resume.
    #[tokio::test]
    async fn an_unfinished_final_turn_resumes_once_as_interrupted_without_an_effect() {
        let root = FixtureWorkspace::new();
        let session = session_id("unfinished-01");
        let sessions = SessionDirectory::under(root.path())
            .unwrap_or_else(|error| panic!("sessions directory: {error}"));
        let mut file = sessions
            .create(session.clone(), UnixMillis::EPOCH)
            .unwrap_or_else(|error| panic!("create unfinished fixture: {error}"));
        let mut agent = Agent::for_session(
            agent_id(),
            SessionMetadata::new(session.clone(), UnixMillis::EPOCH),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        );
        for record in agent.announce("Plexmaton").records.into_iter().chain(
            agent
                .handle_at(
                    Input::Submitted {
                        text: "answer after recovery".to_owned(),
                    },
                    UnixMillis::EPOCH,
                )
                .records,
        ) {
            file.append(record)
                .unwrap_or_else(|failure| panic!("append unfinished fixture: {failure:?}"));
        }
        drop(file);

        let (profile, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let OpenedSession {
            runtime: mut resumed,
            recovery,
            ..
        } = open_selected_session(
            root.path(),
            SessionSelection::Resume(session.clone()),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("resume unfinished fixture: {error}"));
        assert!(recovery.as_ref().expect("resumed").interrupted_turn);
        assert!(
            restoration_feedback(recovery.clone()).is_some(),
            "restoration success is separate from the durable interruption warning"
        );
        assert!(!resumed.has_active_work());
        let state = project_until_idle(&mut resumed).await;
        let primary = state
            .primary_agent()
            .unwrap_or_else(|| panic!("resumed agent missing"));
        assert_eq!(primary.status, AgentStatus::Idle);
        assert!(primary.transcript().any(|item| {
            item.role == TranscriptRole::User && item.source == "answer after recovery"
        }));
        assert_eq!(
            primary
                .transcript()
                .filter(|item| {
                    item.role == TranscriptRole::System
                        && item.source == "The previous turn didn't finish. You can continue from here; no model requests or tools were rerun."
                })
                .count(),
            1
        );
        resumed
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown recovered runtime: {error}"));
        drop(resumed);

        let (profile, key, tools) = transport(root.path(), "http://127.0.0.1:9/v1");
        let OpenedSession {
            runtime: mut reopened,
            recovery: again,
            ..
        } = open_selected_session(
            root.path(),
            SessionSelection::Resume(session),
            agent_id(),
            profile,
            key,
            tools,
        )
        .await
        .unwrap_or_else(|error| panic!("reopen recovered fixture: {error}"));
        assert!(
            again.expect("resumed").is_clean(),
            "the interruption marker must be durable"
        );
        let reopened_state = project_until_idle(&mut reopened).await;
        assert_eq!(reopened_state, state);
        reopened
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown reopened runtime: {error}"));
    }
}
