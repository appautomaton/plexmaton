mod support;

use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::{
    Agent, ApprovalPolicy, AssistantBlock, AssistantOutput, AssistantReplay, ContextAtomValue,
    Input, JournalEntryPayload, MAX_ASSISTANT_TEXT_BYTES, MAX_ASSISTANT_TOOL_ARGUMENT_BYTES,
    MAX_PROVIDER_REPLAY_BYTES, MAX_REQUESTED_TOOL_ARGUMENT_BYTES, MAX_TOOL_IDENTITY_BYTES,
    ModelEvent, ModelOutputPosition, ModelStepId, ProviderCodecId, ProviderCodecRevision,
    ProviderModelFamilyId, ProviderReplay, ProviderReplayOwnerId, ReplayCompatibility, StopReason,
    ToolCall, TurnBudget, UnixMillis,
};
use plexmaton_core::{AgentId, HeadName, ToolCallId, TranscriptItemId};
use plexmaton_session_store::{JournalFile, JournalRecovery, MAX_JOURNAL_LINE_BYTES, StoreError};

use support::{TestDir, agent_created, append, id, session};

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
        [AssistantBlock::ReplayOnly { .. }]
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

/// JRN-3/JRN-4 and PRV-2: every maximal provider output fits one bounded journal write.
#[test]
fn maximal_valid_assistant_output_fits_the_journal_line_envelope() {
    assert_eq!(
        MAX_ASSISTANT_TOOL_ARGUMENT_BYTES,
        MAX_REQUESTED_TOOL_ARGUMENT_BYTES * 8
    );
    let compatibility = ReplayCompatibility::new(
        ProviderReplayOwnerId::new("test-route")
            .unwrap_or_else(|error| panic!("fixture replay owner: {error:?}")),
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
        ProviderCodecRevision::new(1)
            .unwrap_or_else(|error| panic!("fixture codec revision: {error:?}")),
        ProviderModelFamilyId::new("test-model")
            .unwrap_or_else(|error| panic!("fixture model family: {error:?}")),
    );
    let replay = ProviderReplay::new(compatibility, "\0".repeat(MAX_PROVIDER_REPLAY_BYTES))
        .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let mut blocks = vec![
        AssistantBlock::Text {
            item_id: id("max-text", TranscriptItemId::new),
            text: "\0".repeat(MAX_ASSISTANT_TEXT_BYTES),
        },
        AssistantBlock::Reasoning {
            item_id: id("max-replay", TranscriptItemId::new),
            text: String::new(),
        },
    ];
    blocks.extend((0..8).map(|index| AssistantBlock::ToolCall {
        item_id: id(&format!("tool-item-{index}"), TranscriptItemId::new),
        call: ToolCall {
            call_id: id(
                &format!("{index}{}", "\0".repeat(MAX_TOOL_IDENTITY_BYTES - 1)),
                ToolCallId::new,
            ),
            name: "\0".repeat(MAX_TOOL_IDENTITY_BYTES),
            arguments: "\0".repeat(MAX_REQUESTED_TOOL_ARGUMENT_BYTES),
        },
    }));
    let output = AssistantOutput::new(
        blocks,
        AssistantReplay::from_positioned([(1, replay)])
            .unwrap_or_else(|error| panic!("fixture replay attachment: {error}")),
    )
    .unwrap_or_else(|error| panic!("maximal assistant output: {error}"));
    let step_id: ModelStepId = serde_json::from_value(serde_json::json!({
        "turn_id": "turn-a",
        "index": 1
    }))
    .unwrap_or_else(|error| panic!("fixture model step: {error}"));
    let journal = plexmaton_agent::ConversationJournal::new(session("maximal-output"));
    let record = append(
        &journal,
        1,
        JournalEntryPayload::AssistantOutput {
            agent_id: id("agent-a", AgentId::new),
            step_id,
            output,
        },
    );
    let encoded = serde_json::to_vec(&record)
        .unwrap_or_else(|error| panic!("encode maximal journal record: {error}"));

    assert!(encoded.len() > 2 * 1024 * 1024);
    assert!(encoded.len() < MAX_JOURNAL_LINE_BYTES);
}
