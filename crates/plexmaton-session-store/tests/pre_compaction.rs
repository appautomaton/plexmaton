#[path = "../src/test_support.rs"]
mod directory;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use plexmaton_agent::{
    AssistantBlock, ContextAtomValue, ConversationEntry, ConversationJournal, HeadRevision,
    JournalEntryPayload, JournalProjection, JournalRecord, ModelRequest, RequestAccounting,
    RequestCost, ToolOutcome, UnixMillis, UsdCostTicks,
};
use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationId, HeadName, JournalRecordId, TokenCounts,
    TokenUsage, TranscriptItemId, TurnId,
};
use plexmaton_session_store::{JournalFile, JournalRecovery};

use directory::TestDir;

// Records captured through JournalFile's public API at 2bb0a70f56660d95d4feb8f11ec7ead3336db5a7.
// Integration adopts SKL-5's 2026-09-05 header; the pre-compaction records are unchanged.
const FIXTURE: &[u8] = include_bytes!("fixtures/schema-2026-09-05-conversation.jsonl");
const REPLAY_PAYLOAD: &str = r#"{"type":"fixture_replay","opaque":"sanitized"}"#;

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn materialize_fixture(directory: &TestDir) -> PathBuf {
    let path = directory.path().join("pre-compaction-session.jsonl");
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

fn expected_accounting() -> RequestAccounting {
    RequestAccounting {
        usage: TokenUsage::Complete(TokenCounts {
            input: 12,
            cached_input: Some(4),
            cache_write_input: Some(2),
            output: 5,
            reasoning_output: Some(1),
            total: 17,
        }),
        cost: RequestCost::Known {
            usd_ticks: UsdCostTicks::new(42),
        },
    }
}

fn assert_journal_shape(journal: &ConversationJournal, main: &HeadName, branch: &HeadName) {
    assert_eq!(
        journal.conversation_id(),
        &id("pre-compaction-session", ConversationId::new)
    );
    assert_eq!(
        journal.created_at_unix_ms(),
        UnixMillis::new(1_788_537_600_123)
    );
    assert_eq!(journal.records().len(), 17);
    assert_eq!(journal.next_sequence().get(), 18);
    assert_eq!(journal.head_revision(main), Ok(HeadRevision::new(9)));
    assert_eq!(journal.head_revision(branch), Ok(HeadRevision::new(2)));
    assert_eq!(
        journal
            .head_target(main)
            .map(|target| target.map(ConversationEntryId::as_str)),
        Ok(Some("entry-13"))
    );
    assert_eq!(
        journal
            .head_target(branch)
            .map(|target| target.map(ConversationEntryId::as_str)),
        Ok(Some("entry-16"))
    );
    assert_eq!(
        journal
            .path(main)
            .unwrap_or_else(|error| panic!("main path: {error:?}"))
            .len(),
        9
    );
    assert_eq!(
        journal
            .path(branch)
            .unwrap_or_else(|error| panic!("branch path: {error:?}"))
            .len(),
        9
    );
}

fn assert_user_atom(projection: &JournalProjection, index: usize, expected: &str) {
    assert!(matches!(
        projection.request().atoms[index].value(),
        ContextAtomValue::User { text } if text == expected
    ));
}

fn assert_text_atom(projection: &JournalProjection, index: usize, expected: &str) {
    assert!(matches!(
        projection.request().atoms[index].value(),
        ContextAtomValue::Assistant(output)
            if matches!(output.blocks(), [AssistantBlock::Text { text, .. }] if text == expected)
    ));
}

fn assert_shared_tool_batch(projection: &JournalProjection) {
    let atom = &projection.request().atoms[1];
    let ContextAtomValue::ToolBatch(batch) = atom.value() else {
        panic!("complete tool lifecycle did not project as one batch")
    };
    assert_eq!(atom.source_entries().len(), 4);
    assert_eq!(batch.results().len(), 1);
    assert_eq!(batch.results()[0].call_id().as_str(), "call-fixture");
    assert_eq!(
        batch.results()[0].outcome(),
        &ToolOutcome::Succeeded {
            output: "fixture contents".to_owned()
        }
    );
    assert!(matches!(
        batch.assistant().blocks(),
        [AssistantBlock::Reasoning { text, .. }, AssistantBlock::ToolCall { call, .. }]
            if text == "Checking the fixture."
                && call.call_id.as_str() == "call-fixture"
                && call.arguments == r#"{"path":"fixture.txt"}"#
    ));

    let replay = batch
        .assistant()
        .replay()
        .unwrap_or_else(|| panic!("fixture replay attachment is missing"));
    assert_eq!(replay.compatible_with().owner().as_str(), "fixture-route");
    assert_eq!(
        replay.compatible_with().codec().as_str(),
        "openai_responses"
    );
    assert_eq!(replay.compatible_with().codec_revision().get(), 1);
    assert_eq!(
        replay.compatible_with().model_family().as_str(),
        "fixture-model"
    );
    assert_eq!(replay.attachments().len(), 1);
    assert_eq!(replay.attachments()[0].block(), 0);
    assert_eq!(replay.attachments()[0].payload(), REPLAY_PAYLOAD);
}

fn assert_projected_history(main: &JournalProjection, branch: &JournalProjection) {
    assert_eq!(main.request().atoms.len(), 5);
    assert_eq!(branch.request().atoms.len(), 5);
    assert!(main.recovery().is_none());
    assert!(branch.recovery().is_none());
    assert_user_atom(main, 0, "Inspect the fixture.");
    assert_shared_tool_batch(main);
    assert_user_atom(main, 3, "Continue on main.");
    assert_user_atom(branch, 3, "Take the branch.");
    assert_text_atom(main, 4, "Main answer.");
    assert_text_atom(branch, 4, "Branch answer.");
}

fn assert_attempt_accounting(
    journal: &ConversationJournal,
    main: &JournalProjection,
    branch: &JournalProjection,
) {
    assert_eq!(main.request_attempts().len(), 1);
    assert_eq!(branch.request_attempts().len(), 1);
    let attempt = main.request_attempts()[0].authorization();
    assert_eq!(attempt.attempt_id().as_str(), "attempt-shared-1");
    assert_eq!(attempt.semantic_boundary().as_str(), "entry-2");
    assert_eq!(attempt.environment().fingerprint().as_bytes(), &[0x5a; 32]);
    assert!(main.request_attempts()[0].terminal().is_some());
    assert_eq!(journal.incurred_accounting(), Ok(expected_accounting()));
    assert_eq!(
        journal.turn_accounting(&id("turn-shared", TurnId::new)),
        Ok(expected_accounting())
    );
}

fn append_continuation(file: &mut JournalFile, main: &HeadName) {
    let sequence = file.journal().next_sequence();
    file.append(JournalRecord::AppendEntry {
        sequence,
        record_id: id("record-continuation", JournalRecordId::new),
        head: main.clone(),
        expected_head_revision: file
            .journal()
            .head_revision(main)
            .unwrap_or_else(|error| panic!("continued main revision: {error:?}")),
        entry: Box::new(ConversationEntry {
            id: id("entry-continuation", ConversationEntryId::new),
            parent_id: file
                .journal()
                .head_target(main)
                .unwrap_or_else(|error| panic!("continued main target: {error:?}"))
                .cloned(),
            payload: JournalEntryPayload::RuntimeWarning {
                agent_id: id("agent-fixture", AgentId::new),
                item_id: id("warning-continuation", TranscriptItemId::new),
                message: "Continued after fixture reopen.".to_owned(),
            },
        }),
    })
    .unwrap_or_else(|failure| panic!("append after fixture reopen: {failure:?}"));
}

fn assert_continued_journal(
    journal: &ConversationJournal,
    main: &HeadName,
    branch: &HeadName,
    main_request: &ModelRequest,
    branch_request: &ModelRequest,
) {
    assert_eq!(journal.records().len(), 18);
    assert_eq!(journal.next_sequence().get(), 19);
    assert_eq!(journal.head_revision(main), Ok(HeadRevision::new(10)));
    assert_eq!(journal.head_revision(branch), Ok(HeadRevision::new(2)));
    assert_eq!(
        journal
            .project(main)
            .unwrap_or_else(|error| panic!("project continued main: {error:?}"))
            .request(),
        main_request
    );
    assert_eq!(
        journal
            .project(branch)
            .unwrap_or_else(|error| panic!("project continued branch: {error:?}"))
            .request(),
        branch_request
    );
    assert_eq!(journal.incurred_accounting(), Ok(expected_accounting()));
}

/// JRN-3/JRN-5/TIM-3: pre-compaction records retain their semantics in the current schema epoch.
#[test]
fn schema_2026_09_05_fixture_reopens_projects_and_continues() {
    assert!(FIXTURE.starts_with(
        br#"{"format":"plexmaton.session","schema":"2026-09-05","session_id":"pre-compaction-session""#,
    ));
    assert_eq!(FIXTURE.last(), Some(&b'\n'));

    let directory = TestDir::new("pre-compaction-compatibility");
    let path = materialize_fixture(&directory);
    let mut file = JournalFile::open(&path).unwrap_or_else(|error| panic!("open fixture: {error}"));
    assert_eq!(file.recovery(), &JournalRecovery::Clean);

    let main = head("main");
    let branch = head("branch");
    assert_journal_shape(file.journal(), &main, &branch);
    let main_projection = file
        .journal()
        .project(&main)
        .unwrap_or_else(|error| panic!("project main: {error:?}"));
    let branch_projection = file
        .journal()
        .project(&branch)
        .unwrap_or_else(|error| panic!("project branch: {error:?}"));
    assert_projected_history(&main_projection, &branch_projection);
    assert_attempt_accounting(file.journal(), &main_projection, &branch_projection);

    let main_request = main_projection.into_request();
    let branch_request = branch_projection.into_request();
    append_continuation(&mut file, &main);
    drop(file);

    let reopened = JournalFile::open(&path)
        .unwrap_or_else(|error| panic!("reopen continued fixture: {error}"));
    assert_eq!(reopened.recovery(), &JournalRecovery::Clean);
    assert_continued_journal(
        reopened.journal(),
        &main,
        &branch,
        &main_request,
        &branch_request,
    );
}

/// JRN-3: adopting skill records does not introduce an implicit old-epoch migration reader.
#[test]
fn legacy_epoch_is_refused_without_modifying_fixture() {
    let directory = TestDir::new("legacy-epoch-refusal");
    let path = materialize_fixture(&directory);
    let legacy = std::str::from_utf8(FIXTURE)
        .expect("fixture UTF-8")
        .replacen("\"schema\":\"2026-09-05\"", "\"schema\":\"2026-09-04\"", 1);
    std::fs::write(&path, &legacy).expect("old header fixture");
    assert!(matches!(JournalFile::open(&path),
        Err(plexmaton_session_store::StoreError::UnsupportedSchema(epoch)) if epoch == "2026-09-04"));
    assert_eq!(
        std::fs::read(&path).expect("unchanged fixture"),
        legacy.as_bytes()
    );
}
