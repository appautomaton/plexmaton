use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationId, HeadName, JournalRecordId, TranscriptItemId,
    TurnId,
};

use super::{
    ConversationEntry, ConversationJournal, HeadRevision, JournalEntryPayload, JournalError,
    JournalRecord, JournalSequence,
};
use crate::UnixMillis;

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn record(value: &str) -> JournalRecordId {
    id(value, JournalRecordId::new)
}

fn warning(value: &str, parent_id: Option<ConversationEntryId>) -> ConversationEntry {
    ConversationEntry {
        id: id(value, ConversationEntryId::new),
        parent_id,
        payload: JournalEntryPayload::RuntimeWarning {
            agent_id: id("agent-a", AgentId::new),
            item_id: id(&format!("item-{value}"), TranscriptItemId::new),
            message: value.to_owned(),
        },
    }
}

fn append(
    sequence: u64,
    record_id: &str,
    head_name: &str,
    revision: u64,
    entry: ConversationEntry,
) -> JournalRecord {
    JournalRecord::AppendEntry {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        head: head(head_name),
        expected_head_revision: HeadRevision::new(revision),
        entry: Box::new(entry),
    }
}

fn rooted() -> (ConversationJournal, ConversationEntryId) {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    let root = warning("entry-1", None);
    let root_id = root.id.clone();
    journal
        .apply(append(1, "record-1", "main", 0, root))
        .unwrap_or_else(|error| panic!("append root: {error:?}"));
    (journal, root_id)
}

fn fork_and_select(
    journal: &ConversationJournal,
    record_id: &str,
    destination: &str,
    at: Option<ConversationEntryId>,
) -> JournalRecord {
    let source = journal.selected_head().clone();
    JournalRecord::ForkAndSelectHead {
        sequence: journal.next_sequence(),
        record_id: record(record_id),
        expected_source_revision: journal
            .head_revision(&source)
            .unwrap_or_else(|error| panic!("source revision: {error:?}")),
        source,
        destination: head(destination),
        at,
    }
}

/// TRE-3: rewind creates and selects a fresh head in one mutation; the source continuation stays.
#[test]
fn tre_3_fork_and_select_creates_and_selects_without_moving_source() {
    let (mut journal, root_id) = rooted();
    let source_revision = journal
        .head_revision(&head("main"))
        .unwrap_or_else(|error| panic!("main revision: {error:?}"));
    journal
        .apply(fork_and_select(
            &journal,
            "record-2",
            "rewound",
            Some(root_id.clone()),
        ))
        .unwrap_or_else(|error| panic!("fork and select: {error:?}"));

    assert_eq!(journal.selected_head(), &head("rewound"));
    assert_eq!(journal.head_target(&head("main")), Ok(Some(&root_id)));
    assert_eq!(journal.head_revision(&head("main")), Ok(source_revision));
    assert_eq!(journal.head_target(&head("rewound")), Ok(Some(&root_id)));
    assert_eq!(
        journal.head_revision(&head("rewound")),
        Ok(HeadRevision::new(0))
    );
}

/// TRE-3: selecting an existing head moves only the durable selection.
#[test]
fn tre_3_select_head_moves_only_durable_selection() {
    let (mut journal, root_id) = rooted();
    journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-2"),
            head: head("experiment"),
            at: Some(root_id.clone()),
        })
        .unwrap_or_else(|error| panic!("create experiment: {error:?}"));
    let before = journal.clone();
    journal
        .apply(JournalRecord::SelectHead {
            sequence: JournalSequence::new(3),
            record_id: record("record-3"),
            expected_selected: head("main"),
            destination: head("experiment"),
            expected_destination_revision: HeadRevision::new(0),
        })
        .unwrap_or_else(|error| panic!("select experiment: {error:?}"));

    assert_eq!(journal.selected_head(), &head("experiment"));
    assert_eq!(
        journal.head_target(&head("main")),
        before.head_target(&head("main"))
    );
    assert_eq!(
        journal.head_revision(&head("main")),
        before.head_revision(&head("main"))
    );
    assert_eq!(
        journal.head_target(&head("experiment")),
        before.head_target(&head("experiment"))
    );
    assert_eq!(
        journal.head_revision(&head("experiment")),
        before.head_revision(&head("experiment"))
    );
}

/// TRE-3: a stale source revision refuses before any head or selection change.
#[test]
fn tre_3_stale_source_revision_refuses_fork_and_select() {
    let (journal, root_id) = rooted();
    let unchanged = journal.clone();
    let mut attempt = journal;
    let result = attempt.apply(JournalRecord::ForkAndSelectHead {
        sequence: JournalSequence::new(2),
        record_id: record("stale"),
        source: head("main"),
        expected_source_revision: HeadRevision::new(0),
        destination: head("rewound"),
        at: Some(root_id),
    });

    assert!(matches!(result, Err(JournalError::StaleHead { .. })));
    assert_eq!(attempt, unchanged);
}

/// TRE-3: destination names cannot collide with active or retired heads.
#[test]
fn tre_3_name_collision_refuses_fork_and_select() {
    let (mut journal, root_id) = rooted();
    let unchanged = journal.clone();
    let result = journal.apply(fork_and_select(&journal, "collide", "main", Some(root_id)));

    assert_eq!(result, Err(JournalError::UnavailableHeadName(head("main"))));
    assert_eq!(journal, unchanged);
}

/// TRE-3: a missing target is distinct from a known parent mismatch.
#[test]
fn tre_3_missing_target_refuses_fork_and_select() {
    let (journal, _) = rooted();
    let unchanged = journal.clone();
    let missing = id("missing", ConversationEntryId::new);
    let mut attempt = journal;
    let result = attempt.apply(fork_and_select(
        &attempt,
        "missing-target",
        "rewound",
        Some(missing.clone()),
    ));

    assert_eq!(result, Err(JournalError::MissingEntry(missing)));
    assert_eq!(attempt, unchanged);
}

/// TRE-3: fork/select must name the current durable selection as source.
#[test]
fn tre_3_foreign_source_refuses_fork_and_select() {
    let (mut journal, root_id) = rooted();
    journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-2"),
            head: head("experiment"),
            at: Some(root_id.clone()),
        })
        .unwrap_or_else(|error| panic!("create experiment: {error:?}"));
    let unchanged = journal.clone();
    let result = journal.apply(JournalRecord::ForkAndSelectHead {
        sequence: JournalSequence::new(3),
        record_id: record("foreign"),
        source: head("experiment"),
        expected_source_revision: HeadRevision::new(0),
        destination: head("rewound"),
        at: Some(root_id),
    });

    assert_eq!(
        result,
        Err(JournalError::SelectedHeadMismatch {
            expected: head("experiment"),
            actual: head("main"),
        })
    );
    assert_eq!(journal, unchanged);
}

/// TRE-3: rewind cannot split an unfinished turn.
#[test]
fn tre_3_unstable_target_refuses_fork_and_select() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    let started = ConversationEntry {
        id: id("turn-entry", ConversationEntryId::new),
        parent_id: None,
        payload: JournalEntryPayload::TurnStarted {
            agent_id: id("agent-a", AgentId::new),
            item_id: id("item-turn", TranscriptItemId::new),
            turn_id: id("turn-1", TurnId::new),
            text: "hello".to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        },
    };
    journal
        .apply(append(1, "record-1", "main", 0, started.clone()))
        .unwrap_or_else(|error| panic!("start turn: {error:?}"));
    let unchanged = journal.clone();
    let result = journal.apply(fork_and_select(
        &journal,
        "partial",
        "rewound",
        Some(started.id),
    ));

    assert_eq!(
        result,
        Err(JournalError::UnstableTurnTarget(id("turn-1", TurnId::new)))
    );
    assert_eq!(journal, unchanged);
}

/// TRE-3: renaming the selected head preserves selection identity under the new name.
#[test]
fn tre_3_rename_of_selected_preserves_selection_identity() {
    let (mut journal, _) = rooted();
    journal
        .apply(JournalRecord::RenameHead {
            sequence: JournalSequence::new(2),
            record_id: record("rename"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(1),
            renamed: head("kept"),
        })
        .unwrap_or_else(|error| panic!("rename selected: {error:?}"));

    assert_eq!(journal.selected_head(), &head("kept"));
    assert_eq!(
        journal.head_revision(&head("kept")),
        Ok(HeadRevision::new(2))
    );
    assert_eq!(
        journal.head_target(&head("main")),
        Err(JournalError::MissingHead(head("main")))
    );
}

/// TRE-3: the selected head cannot be abandoned until another head is selected.
#[test]
fn tre_3_abandon_selected_refuses() {
    let (journal, _) = rooted();
    let unchanged = journal.clone();
    let mut attempt = journal;
    let result = attempt.apply(JournalRecord::AbandonHead {
        sequence: JournalSequence::new(2),
        record_id: record("abandon-selected"),
        head: head("main"),
        expected_head_revision: HeadRevision::new(1),
    });

    assert_eq!(
        result,
        Err(JournalError::CannotAbandonSelectedHead(head("main")))
    );
    assert_eq!(attempt, unchanged);
}

/// TRE-3: journals without selection records reconstruct `main` as the durable selection.
#[test]
fn tre_3_old_journals_without_selection_records_default_to_main() {
    let (mut journal, root_id) = rooted();
    journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-2"),
            head: head("experiment"),
            at: Some(root_id),
        })
        .unwrap_or_else(|error| panic!("create experiment: {error:?}"));

    assert_eq!(journal.selected_head(), &head("main"));
}

/// TRE-3: a stale destination revision refuses existing-head selection.
#[test]
fn tre_3_select_head_stale_destination_revision_refuses() {
    let (mut journal, root_id) = rooted();
    journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-2"),
            head: head("experiment"),
            at: Some(root_id),
        })
        .unwrap_or_else(|error| panic!("create experiment: {error:?}"));
    let unchanged = journal.clone();
    let result = journal.apply(JournalRecord::SelectHead {
        sequence: JournalSequence::new(3),
        record_id: record("stale-dest"),
        expected_selected: head("main"),
        destination: head("experiment"),
        expected_destination_revision: HeadRevision::new(1),
    });

    assert!(matches!(result, Err(JournalError::StaleHead { .. })));
    assert_eq!(journal, unchanged);
}

/// TRE-3: two writes are not a substitute for ForkAndSelectHead; CreateHead does not select.
#[test]
fn tre_3_create_head_does_not_change_selection() {
    let (mut journal, root_id) = rooted();
    journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-2"),
            head: head("branch"),
            at: Some(root_id),
        })
        .unwrap_or_else(|error| panic!("create branch: {error:?}"));

    assert_eq!(journal.selected_head(), &head("main"));
    assert_eq!(
        journal.head_revision(&head("branch")),
        Ok(HeadRevision::new(0))
    );
}
