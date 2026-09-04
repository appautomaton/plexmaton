mod support;

use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::{
    Agent, ApprovalPolicy, AssistantBlock, ContextAtomValue, Input, ModelEvent,
    ModelOutputPosition, ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId,
    ProviderReplay, ProviderReplayOwnerId, ReplayCompatibility, StopReason, TurnBudget, UnixMillis,
};
use plexmaton_core::{AgentId, HeadName};
use plexmaton_session_store::{JournalFile, JournalRecovery, StoreError};

use support::{TestDir, agent_created, id, session};

/// JRN-3/JRN-4: every journal-derived file is owner-only on Unix.
#[cfg(unix)]
#[test]
fn jrn_3_and_jrn_4_journal_fork_and_tail_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new("permissions");
    let source_path = directory.path().join("source.jsonl");
    let fork_path = directory.path().join("fork.jsonl");
    let source = JournalFile::create(&source_path, session("source"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create source: {error}"));
    let forked = source
        .fork(&fork_path, session("fork"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("fork source: {error}"));
    drop(forked);
    drop(source);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&source_path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    file.write_all(b"{")
        .unwrap_or_else(|error| panic!("write incomplete tail: {error}"));
    drop(file);
    let recovered =
        JournalFile::open(&source_path).unwrap_or_else(|error| panic!("recover source: {error}"));
    let tail_path = match recovered.recovery() {
        JournalRecovery::IsolatedFinalTail { path, .. } => path,
        other => panic!("expected isolated tail, got {other:?}"),
    };

    for path in [&source_path, &fork_path, tail_path] {
        let mode = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("read permissions: {error}"))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "unexpected mode for {}", path.display());
    }
}

/// JRN-3/JRN-4: an existing journal exposed to another account is not opened.
#[cfg(unix)]
#[test]
fn jrn_3_and_jrn_4_insecure_existing_journal_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new("insecure-permissions");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let mut permissions = std::fs::metadata(&path)
        .unwrap_or_else(|error| panic!("read permissions: {error}"))
        .permissions();
    permissions.set_mode(0o640);
    std::fs::set_permissions(&path, permissions)
        .unwrap_or_else(|error| panic!("widen permissions: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::InsecurePermissions(0o640))
    ));
}

/// JRN-3/JRN-4: exact opaque replay survives a real file without entering presentation.
#[test]
fn jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file() {
    let directory = TestDir::new("replay");
    let path = directory.path().join("session.jsonl");
    let session_id = session("session-a");
    let mut store = JournalFile::create(&path, session_id.clone(), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal(), 1))
        .unwrap_or_else(|failure| panic!("append agent: {}", failure.error()));
    let mut agent = Agent::from_journal(
        id("agent-a", AgentId::new),
        store.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .unwrap_or_else(|error| panic!("restore agent fixture: {error:?}"));
    let replay = ProviderReplay::new(
        ReplayCompatibility::new(
            ProviderReplayOwnerId::new("test-route")
                .unwrap_or_else(|error| panic!("fixture replay owner: {error:?}")),
            ProviderCodecId::new("openai_responses")
                .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
            ProviderCodecRevision::new(1)
                .unwrap_or_else(|error| panic!("fixture codec revision: {error:?}")),
            ProviderModelFamilyId::new("test-model")
                .unwrap_or_else(|error| panic!("fixture model family: {error:?}")),
        ),
        r#"{"encrypted_content":"secret-ciphertext"}"#.to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let mut reactions = vec![agent.handle_at(
        Input::Submitted {
            text: "retain private context".to_owned(),
        },
        UnixMillis::new(100),
    )];
    let step_id = agent
        .active_model_step()
        .unwrap_or_else(|| panic!("submission did not open a model step"));
    reactions.push(agent.handle_at(
        Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::Replay {
                position: ModelOutputPosition::new(0, 0),
                replay: replay.clone(),
            },
        },
        UnixMillis::new(200),
    ));
    reactions.push(agent.handle_at(
        Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        },
        UnixMillis::new(300),
    ));
    for record in reactions.into_iter().flat_map(|reaction| reaction.records) {
        store
            .append(record)
            .unwrap_or_else(|failure| panic!("append replay turn: {failure:?}"));
    }
    drop(store);

    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen replay: {error}"));
    let projected = reopened
        .journal()
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project replay: {error:?}"));
    assert_eq!(projected.request().atoms.len(), 2);
    let ContextAtomValue::Assistant(output) = projected.request().atoms[1].value() else {
        panic!("replay should remain attached to one assistant context atom")
    };
    assert!(matches!(
        output.blocks(),
        [AssistantBlock::Reasoning { text, .. }] if text.is_empty()
    ));
    let retained = output
        .replay()
        .unwrap_or_else(|| panic!("assistant output should retain replay"));
    assert_eq!(retained.compatible_with(), replay.compatible_with());
    assert_eq!(retained.attachments().len(), 1);
    assert_eq!(retained.attachments()[0].block(), 0);
    assert_eq!(retained.attachments()[0].payload(), replay.payload());
    assert!(!format!("{projected:?}").contains("secret-ciphertext"));
}
