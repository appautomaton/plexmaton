#[path = "../src/test_support.rs"]
mod directory;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use plexmaton_agent::{
    Agent, ApprovalPolicy, AssistantBlock, AssistantOutput, AssistantReplay,
    CompactionAttemptFinished, CompactionCut, CompactionFailure, CompactionId, CompactionInputMode,
    CompactionOutcome, CompactionPlan, ContextAtomValue, ContextEpoch, ConversationJournal,
    ConversationMetadata, DispatchedRequestTiming, ElapsedMillis, Input, JournalRecord,
    JournalSequence, ModelEvent, ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId,
    ProviderReplay, ProviderReplayOwnerId, Reaction, ReplayCompatibility, RequestAccounting,
    RequestAttemptOwner, RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost,
    RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint, StopReason,
    TurnBudget, UnixMillis,
};
use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationEvent, ConversationId, HeadName, JournalRecordId,
    TokenUsage, TranscriptItemId,
};
use plexmaton_session_store::{JournalFile, JournalRecovery};

use directory::TestDir;

// Records captured through public APIs on base 2bb0a70f56660d95d4feb8f11ec7ead3336db5a7.
// Integration adopts SKL-5's 2026-09-05 header; the checkpoint records are unchanged.
const FIXTURE: &[u8] = include_bytes!("fixtures/checkpoint-schema-2026-09-05.jsonl");

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn materialize_fixture(directory: &TestDir) -> PathBuf {
    let path = directory.path().join("checkpoint-session.jsonl");
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut destination = options
        .open(&path)
        .unwrap_or_else(|error| panic!("create fixture copy: {error}"));
    destination
        .write_all(FIXTURE)
        .unwrap_or_else(|error| panic!("write fixture copy: {error}"));
    path
}

fn persist(file: &mut JournalFile, reaction: Reaction) {
    for record in reaction.records {
        file.append(record)
            .unwrap_or_else(|failure| panic!("persist reaction: {failure:?}"));
    }
}

fn environment() -> RequestEnvironment {
    RequestEnvironment::new(
        ReplayCompatibility::new(
            ProviderReplayOwnerId::new("failed-fixture-route").expect("replay owner"),
            ProviderCodecId::new("openai_responses").expect("codec"),
            ProviderCodecRevision::new(1).expect("codec revision"),
            ProviderModelFamilyId::new("failed-fixture-model").expect("model family"),
        ),
        RequestEnvironmentFingerprint::new([0x21; 32]),
    )
}

fn assert_checkpoint_projection(journal: &ConversationJournal) {
    let main = id("main", HeadName::new);
    let source = journal
        .compaction_source(&main)
        .unwrap_or_else(|error| panic!("checkpoint source: {error:?}"));
    assert_eq!(source.head_revision().get(), 8);
    assert_eq!(
        source.boundary().as_str(),
        "checkpoint-fixture-session-entry-12"
    );
    assert_eq!(
        source.epoch(),
        &ContextEpoch::Checkpoint(id(
            "checkpoint-fixture-session-entry-11",
            ConversationEntryId::new
        ))
    );

    let projection = journal
        .project(&main)
        .unwrap_or_else(|error| panic!("checkpoint projection: {error:?}"));
    assert_eq!(projection.base_atom_count(), 2);
    assert_eq!(projection.context_epoch(), source.epoch());
    assert!(matches!(
        projection.request().atoms.as_slice(),
        [summary, current, answer]
            if summary.value() == &ContextAtomValue::CompactionSummary {
                text: "Earlier discussion condensed.".to_owned()
            }
            && current.value() == &ContextAtomValue::User {
                text: "Current question.".to_owned()
            }
            && matches!(answer.value(), ContextAtomValue::Assistant(output)
                if matches!(output.blocks(), [AssistantBlock::Text { text, .. }]
                    if text == "Current answer."))
    ));

    let visible_text: Vec<_> = projection
        .events()
        .iter()
        .filter_map(|event| match &event.event {
            ConversationEvent::TranscriptDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        visible_text,
        [
            "First question.",
            "First answer.",
            "Second question.",
            "Second answer.",
            "Current question.",
            "Current answer.",
        ]
    );
}

fn assert_compaction_audit(journal: &ConversationJournal) {
    let attempt_id = plexmaton_agent::RequestAttemptId::new("request-attempt-j9")
        .unwrap_or_else(|error| panic!("attempt identity: {error}"));
    let attempt = journal
        .request_attempt(&attempt_id)
        .unwrap_or_else(|| panic!("request attempt missing"));
    assert!(matches!(
        attempt.authorization().owner(),
        RequestAttemptOwner::Compaction { compaction_id }
            if compaction_id.as_str() == "checkpoint-fixture-compaction"
    ));
    assert!(attempt.terminal().is_some());
    let finished = journal
        .compaction_attempt(&attempt_id)
        .unwrap_or_else(|| panic!("collected compaction output missing"));
    let CompactionOutcome::Complete { output } = finished.outcome() else {
        panic!("fixture compaction did not complete")
    };
    assert!(matches!(
        output.blocks(),
        [AssistantBlock::Reasoning { text: reasoning, .. }, AssistantBlock::Text { text, .. }]
            if reasoning == "Retained audit reasoning."
                && text == "Earlier discussion condensed."
    ));
    assert_eq!(
        output
            .replay()
            .unwrap_or_else(|| panic!("audit replay missing"))
            .attachments()[0]
            .payload(),
        r#"{"opaque":"checkpoint-fixture"}"#
    );
    assert_eq!(
        journal.compaction_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Unavailable,
            cost: RequestCost::Unavailable,
        })
    );
}

/// CPL-4/CPL-5/CPL-6: a checkpoint JSONL reloads with one audit output, original visible history,
/// selected replacement context and a historical fork using its own original epoch.
#[test]
fn checkpoint_fixture_reopens_and_historical_fork_keeps_original_epoch() {
    assert!(FIXTURE.starts_with(
        br#"{"format":"plexmaton.session","schema":"2026-09-05","session_id":"checkpoint-fixture-session""#,
    ));
    let directory = TestDir::new("checkpoint-fixture");
    let path = materialize_fixture(&directory);
    let mut file =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("open checkpoint fixture: {error}"));
    assert_eq!(file.recovery(), &JournalRecovery::Clean);
    assert_eq!(file.journal().records().len(), 13);
    assert_eq!(file.journal().next_sequence(), JournalSequence::new(14));
    assert_checkpoint_projection(file.journal());
    assert_compaction_audit(file.journal());

    file.append(JournalRecord::CreateHead {
        sequence: JournalSequence::new(14),
        record_id: id("checkpoint-fixture-historical-head", JournalRecordId::new),
        head: id("historical", HeadName::new),
        at: Some(id(
            "checkpoint-fixture-session-entry-6",
            ConversationEntryId::new,
        )),
    })
    .unwrap_or_else(|failure| panic!("append historical head: {failure:?}"));
    let expected = file.journal().clone();
    drop(file);

    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen checkpoint: {error}"));
    assert_eq!(reopened.journal(), &expected);
    assert_checkpoint_projection(reopened.journal());
    assert_compaction_audit(reopened.journal());
    let historical = id("historical", HeadName::new);
    let source = reopened
        .journal()
        .compaction_source(&historical)
        .unwrap_or_else(|error| panic!("historical source: {error:?}"));
    assert_eq!(source.epoch(), &ContextEpoch::Original);
    let projection = reopened
        .journal()
        .project(&historical)
        .unwrap_or_else(|error| panic!("historical projection: {error:?}"));
    assert_eq!(projection.context_epoch(), &ContextEpoch::Original);
    assert_eq!(projection.base_atom_count(), 0);
    assert_eq!(projection.request().atoms.len(), 4);
}

/// CPL-6/CPL-8: a failed collected audit reopens as one deterministic diagnostic on every head
/// selecting its boundary, while partial reasoning/replay remains outside model context.
#[test]
fn failed_compaction_diagnostic_reopens_without_exposing_partial_output() {
    let directory = TestDir::new("failed-compaction-reopen");
    let path = directory.path().join("failed-compaction.jsonl");
    let session_id = id("failed-compaction-session", ConversationId::new);
    let metadata = ConversationMetadata::new(session_id.clone(), UnixMillis::new(500));
    let mut file = JournalFile::create(&path, session_id, metadata.created_at_unix_ms())
        .unwrap_or_else(|error| panic!("create failed-compaction journal: {error}"));
    let mut agent = Agent::for_conversation(
        id("failed-compaction-agent", AgentId::new),
        metadata,
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    persist(&mut file, agent.announce("Failed compaction agent"));
    persist(
        &mut file,
        agent.handle_at(
            Input::Submitted {
                text: "Keep this exact user input.".to_owned(),
            },
            UnixMillis::new(510),
        ),
    );
    let step_id = agent.active_model_step().expect("active step");
    let source = agent.compaction_source().expect("compaction source");
    let user_source = agent.record()[0].source_entries()[0].clone();
    let plan = CompactionPlan::new(
        CompactionId::new("failed-compaction").expect("compaction id"),
        source.clone(),
        CompactionCut::new(
            user_source.clone(),
            user_source.clone(),
            None,
            Some(user_source),
        ),
        environment(),
        1024,
    )
    .expect("failed compaction plan");
    let (attempt_id, authorized) = agent
        .authorize_compaction_attempt(&plan, UnixMillis::new(520))
        .expect("authorize failed compaction");
    persist(&mut file, authorized);

    let replay = ProviderReplay::new(
        environment().compatibility().clone(),
        "opaque-failed-summary".to_owned(),
    )
    .expect("partial replay");
    let partial = AssistantOutput::new(
        vec![
            AssistantBlock::Reasoning {
                item_id: id("failed-summary-reasoning", TranscriptItemId::new),
                text: "private partial reasoning".to_owned(),
            },
            AssistantBlock::Text {
                item_id: id("failed-summary-text", TranscriptItemId::new),
                text: "incomplete summary".to_owned(),
            },
        ],
        AssistantReplay::from_positioned([(0, replay)]).expect("partial replay sidecar"),
    )
    .expect("partial output");
    let finished = CompactionAttemptFinished::new(
        RequestAttemptTerminal::new(
            attempt_id.clone(),
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(521),
                    Some(ElapsedMillis::new(1)),
                    Some(ElapsedMillis::new(2)),
                    ElapsedMillis::new(3),
                )
                .expect("failed timing"),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::OutputLimit,
                },
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
            },
        )
        .expect("failed terminal"),
        CompactionInputMode::Verbatim,
        CompactionOutcome::Failed {
            kind: CompactionFailure::OutputLimit,
            output: Some(partial.clone()),
        },
    )
    .expect("failed collected attempt");
    let live_failure = agent
        .finish_compaction_attempt(finished)
        .expect("finish failed compaction");
    let [live_diagnostic] = live_failure.events.as_slice() else {
        panic!("failed compaction emits one diagnostic")
    };
    let live_diagnostic = live_diagnostic.clone();
    persist(&mut file, live_failure);
    persist(
        &mut file,
        agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::EndOfTurn),
            },
            UnixMillis::new(530),
        ),
    );
    file.append(JournalRecord::CreateHead {
        sequence: file.journal().next_sequence(),
        record_id: id("failed-compaction-branch-record", JournalRecordId::new),
        head: id("failed-sibling", HeadName::new),
        at: Some(source.boundary().clone()),
    })
    .unwrap_or_else(|failure| panic!("create failed sibling: {failure:?}"));
    drop(file);

    let reopened = JournalFile::open(&path)
        .unwrap_or_else(|error| panic!("reopen failed compaction: {error}"));
    for selected in [
        id("main", HeadName::new),
        id("failed-sibling", HeadName::new),
    ] {
        let projection = reopened
            .journal()
            .project(&selected)
            .unwrap_or_else(|error| panic!("project {selected}: {error:?}"));
        assert_eq!(projection.request().atoms.len(), 1);
        assert!(matches!(
            projection.request().atoms[0].value(),
            ContextAtomValue::User { text } if text == "Keep this exact user input."
        ));
        let diagnostics: Vec<_> = projection
            .events()
            .iter()
            .filter(|event| matches!(event.event, ConversationEvent::RuntimeError { .. }))
            .collect();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0], &live_diagnostic);
    }
    assert_eq!(
        reopened
            .journal()
            .compaction_attempt(&attempt_id)
            .and_then(|attempt| attempt.outcome().output()),
        Some(&partial)
    );
}
