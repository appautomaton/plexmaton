use plexmaton_core::{
    AgentId, AgentStatus, ConversationEntryId, ConversationId, HeadName, JournalRecordId,
    TokenUsage, ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId, TreeLabel,
    TreeRewindEligibility, TreeRowKind, TreeSnapshotError, TreeSnapshotLimit, TurnId,
};

use crate::journal::{ConversationEntry, ConversationJournal, JournalEntryPayload, JournalRecord};
use crate::test_support::{
    call_block, output, output_with_replay, reasoning_block, replay, step, text_block,
};
use crate::{
    Agent, CompactionAttemptFinished, CompactionCut, CompactionId, CompactionInputMode,
    CompactionOutcome, CompactionPlan, DispatchedRequestTiming, ElapsedMillis, Input, ModelEvent,
    ModelOutputPosition, RequestAttemptId, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestCost, RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
    StopReason, ToolCall, ToolOutcome, TurnFinished, TurnFinishedAt, TurnOutcome, UnixMillis,
};

use super::{
    MAX_TREE_AGGREGATE_PREVIEW_BYTES, MAX_TREE_ANCESTRY_ENTRY_COUNT, MAX_TREE_HEAD_COUNT,
    MAX_TREE_HEAD_NAME_BYTES, MAX_TREE_NODE_COUNT, MAX_TREE_PREVIEW_BYTES_PER_ROW,
    MAX_TREE_SCANNED_RECORD_COUNT,
};

fn id<T>(
    value: impl Into<String>,
    build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>,
) -> T {
    build(value.into()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn agent() -> AgentId {
    id("agent-a", AgentId::new)
}

fn hidden_agent() -> AgentId {
    id("agent-hidden", AgentId::new)
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn append(
    journal: &mut ConversationJournal,
    entry_name: &str,
    payload: JournalEntryPayload,
) -> ConversationEntryId {
    let head_name = journal.selected_head().clone();
    let parent_id = journal
        .head_target(&head_name)
        .unwrap_or_else(|error| panic!("head target: {error:?}"))
        .cloned();
    let expected_head_revision = journal
        .head_revision(&head_name)
        .unwrap_or_else(|error| panic!("head revision: {error:?}"));
    let sequence = journal.next_sequence();
    let entry_id = id(entry_name, ConversationEntryId::new);
    journal
        .apply(JournalRecord::AppendEntry {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            head: head_name,
            expected_head_revision,
            entry: Box::new(ConversationEntry {
                id: entry_id.clone(),
                parent_id,
                payload,
            }),
        })
        .unwrap_or_else(|error| panic!("append fixture: {error:?}"));
    entry_id
}

fn warning(entry: &str, message: String) -> JournalEntryPayload {
    JournalEntryPayload::RuntimeWarning {
        agent_id: agent(),
        item_id: id(format!("item-{entry}"), TranscriptItemId::new),
        message,
    }
}

fn append_warning(
    journal: &mut ConversationJournal,
    entry: &str,
    message: String,
) -> ConversationEntryId {
    append(journal, entry, warning(entry, message))
}

fn append_hidden_warning(journal: &mut ConversationJournal, entry: &str) {
    append(
        journal,
        entry,
        JournalEntryPayload::RuntimeWarning {
            agent_id: hidden_agent(),
            item_id: id(format!("item-{entry}"), TranscriptItemId::new),
            message: "not shown in this agent tree".to_owned(),
        },
    );
}

fn create_head(journal: &mut ConversationJournal, name: &str, at: Option<ConversationEntryId>) {
    let sequence = journal.next_sequence();
    journal
        .apply(JournalRecord::CreateHead {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            head: head(name),
            at,
        })
        .unwrap_or_else(|error| panic!("create head: {error:?}"));
}

fn abandon_head(journal: &mut ConversationJournal, name: &str) {
    let sequence = journal.next_sequence();
    let head_name = head(name);
    let expected_head_revision = journal
        .head_revision(&head_name)
        .unwrap_or_else(|error| panic!("head revision: {error:?}"));
    journal
        .apply(JournalRecord::AbandonHead {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            head: head_name,
            expected_head_revision,
        })
        .unwrap_or_else(|error| panic!("abandon head: {error:?}"));
}

fn select_head(journal: &mut ConversationJournal, destination: &str) {
    let sequence = journal.next_sequence();
    let selected = journal.selected_head().clone();
    let destination = head(destination);
    let expected_destination_revision = journal
        .head_revision(&destination)
        .unwrap_or_else(|error| panic!("destination revision: {error:?}"));
    journal
        .apply(JournalRecord::SelectHead {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            expected_selected: selected,
            destination,
            expected_destination_revision,
        })
        .unwrap_or_else(|error| panic!("select head: {error:?}"));
}

fn announce(journal: &mut ConversationJournal) {
    append(
        journal,
        "agent-created",
        JournalEntryPayload::AgentCreated {
            agent_id: agent(),
            label: "Tree test".to_owned(),
            status: AgentStatus::Idle,
        },
    );
}

fn user_payload(text: &str) -> JournalEntryPayload {
    JournalEntryPayload::TurnStarted {
        agent_id: agent(),
        item_id: id("user-item", TranscriptItemId::new),
        turn_id: id("turn-1", TurnId::new),
        text: text.to_owned(),
        accepted_at: UnixMillis::EPOCH,
        opened_at: UnixMillis::EPOCH,
    }
}

fn append_tool_call_lifecycle(journal: &mut ConversationJournal, call_id: &ToolCallId, item: &str) {
    append(
        journal,
        &format!("requested-{item}"),
        JournalEntryPayload::ToolCallRequested {
            agent_id: agent(),
            call_id: call_id.clone(),
            presentation: ToolPresentation::default(),
        },
    );
    for (revision, status, outcome) in [
        (1, ToolCallStatus::Running, None),
        (
            2,
            ToolCallStatus::Succeeded,
            Some(ToolOutcome::Succeeded {
                output: format!("result-{item}"),
            }),
        ),
    ] {
        append(
            journal,
            &format!("changed-{item}-{revision}"),
            JournalEntryPayload::ToolCallChanged {
                agent_id: agent(),
                call_id: call_id.clone(),
                item_revision: revision,
                status,
                presentation: ToolPresentation::default(),
                outcome,
            },
        );
    }
}

fn finish_turn(journal: &mut ConversationJournal) {
    let head_name = journal.selected_head().clone();
    let semantic_boundary = journal
        .head_target(&head_name)
        .unwrap_or_else(|error| panic!("head target: {error:?}"))
        .cloned()
        .unwrap_or_else(|| panic!("completed turn boundary exists"));
    let expected_head_revision = journal
        .head_revision(&head_name)
        .unwrap_or_else(|error| panic!("head revision: {error:?}"));
    let sequence = journal.next_sequence();
    journal
        .apply(JournalRecord::TurnFinished {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            head: head_name,
            expected_head_revision,
            fact: TurnFinished {
                agent_id: agent(),
                turn_id: id("turn-1", TurnId::new),
                semantic_boundary,
                outcome: TurnOutcome::Completed,
                at: TurnFinishedAt::Observed {
                    completed_at: UnixMillis::EPOCH,
                },
            },
        })
        .unwrap_or_else(|error| panic!("finish turn: {error:?}"));
}

fn checkpoint_environment() -> RequestEnvironment {
    RequestEnvironment::new(
        crate::test_support::replay_compatibility(),
        RequestEnvironmentFingerprint::new([7; 32]),
    )
}

fn complete_agent_turn(agent: &mut Agent, question: &str, answer: &str) {
    let _opened = agent.handle(Input::Submitted {
        text: question.to_owned(),
    });
    let step_id = agent.active_model_step().expect("active model step");
    let _text = agent.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: answer.to_owned(),
        },
    });
    let _finished = agent.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
}

fn complete_checkpoint_attempt(attempt_id: RequestAttemptId) -> CompactionAttemptFinished {
    let terminal = RequestAttemptTerminal::new(
        attempt_id,
        RequestAttemptTerminalState::Dispatched {
            timing: DispatchedRequestTiming::new(
                UnixMillis::new(20),
                Some(ElapsedMillis::new(1)),
                Some(ElapsedMillis::new(2)),
                ElapsedMillis::new(3),
            )
            .expect("request timing"),
            outcome: RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn,
            },
            usage: TokenUsage::Unavailable,
            cost: RequestCost::Unavailable,
        },
    )
    .expect("successful request terminal");
    CompactionAttemptFinished::new(
        terminal,
        CompactionInputMode::Verbatim,
        CompactionOutcome::Complete {
            output: output(vec![text_block("checkpoint-summary", "summary")]),
        },
    )
    .expect("successful compaction attempt")
}

/// TRE-2/TRE-6: all-head rows deduplicate shared ancestry, retain journal chronology, and expose
/// inactive and root heads without deriving order from stable IDs.
#[test]
fn tre_2_all_heads_deduplicate_shared_ancestry_and_keep_chronology() {
    let mut journal = ConversationJournal::new(id("tree-session", ConversationId::new));
    let first = append_warning(&mut journal, "z-earlier", "first".to_owned());
    create_head(&mut journal, "experiment", Some(first.clone()));
    append_warning(&mut journal, "a-later", "later".to_owned());
    create_head(&mut journal, "empty-root", None);
    select_head(&mut journal, "experiment");

    let snapshot = journal.tree_snapshot(&agent()).expect("complete tree");
    assert_eq!(snapshot.rows.len(), 2);
    assert_eq!(snapshot.rows[0].entry_id, first);
    assert_eq!(snapshot.rows[0].chronological_ordinal, 0);
    assert_eq!(snapshot.rows[0].kind, TreeRowKind::Notice);
    assert_eq!(snapshot.rows[0].label, None);
    assert!(snapshot.rows[0].active_ancestry);
    assert_eq!(snapshot.rows[0].head_markers, [head("experiment")]);
    assert_eq!(snapshot.rows[1].entry_id.as_str(), "a-later");
    assert_eq!(snapshot.rows[1].chronological_ordinal, 1);
    assert_eq!(
        snapshot.rows[1]
            .parent_id
            .as_ref()
            .expect("shared parent")
            .as_str(),
        "z-earlier"
    );
    assert!(!snapshot.rows[1].active_ancestry);
    assert_eq!(snapshot.rows[1].head_markers, [head("main")]);
    assert_eq!(snapshot.heads.len(), 3);
    assert!(
        snapshot
            .heads
            .iter()
            .any(|branch| branch.name == head("empty-root") && branch.target.is_none())
    );
    assert_eq!(snapshot.origin.selected_head, head("experiment"));
    assert_eq!(
        snapshot.origin.revision.get(),
        journal.next_sequence().get()
    );
}

/// TRE-2/TRE-8: snapshots expose the current label for its immutable semantic node only.
#[test]
fn tre_8_snapshot_reads_the_authoritative_node_label() {
    let mut journal = ConversationJournal::new(id("label-tree", ConversationId::new));
    let entry_id = append_warning(&mut journal, "labeled-entry", "visible row".to_owned());
    let label = TreeLabel::new("decision point".to_owned()).expect("valid label");
    let sequence = journal.next_sequence();
    journal
        .apply(JournalRecord::SetEntryLabel {
            sequence,
            record_id: id(format!("record-{}", sequence.get()), JournalRecordId::new),
            entry_id: entry_id.clone(),
            label: Some(label.clone()),
        })
        .unwrap_or_else(|error| panic!("set tree label: {error:?}"));

    let snapshot = journal.tree_snapshot(&agent()).expect("labeled snapshot");
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].entry_id, entry_id);
    assert_eq!(snapshot.rows[0].label, Some(label));
}

/// TRE-2/TRE-7: assistant blocks and a complete parallel tool batch form coherent semantic rows;
/// every rewind flag matches the canonical resolver, including an earlier assistant step. TRE-8:
/// incomplete source copy refuses; complete source preserves call order and omits opaque replay.
#[test]
fn tre_2_groups_assistant_blocks_and_parallel_tool_batch() {
    let mut journal = ConversationJournal::new(id("tool-tree", ConversationId::new));
    announce(&mut journal);
    let user = append(&mut journal, "user-entry", user_payload("inspect"));
    let first = ToolCall {
        call_id: id("call-a", ToolCallId::new),
        name: "read_file".to_owned(),
        arguments: "{\"path\":\"a.txt\"}".to_owned(),
    };
    let second = ToolCall {
        call_id: id("call-b", ToolCallId::new),
        name: "list_files".to_owned(),
        arguments: "{}".to_owned(),
    };
    let tool_step = append(
        &mut journal,
        "assistant-tools",
        JournalEntryPayload::AssistantOutput {
            agent_id: agent(),
            step_id: step("turn-1", 1),
            output: output_with_replay(
                vec![
                    crate::AssistantBlock::ReplayOnly {
                        item_id: id("hidden-replay-block", TranscriptItemId::new),
                    },
                    text_block("assistant-tool-text", "checking"),
                    call_block("tool-item-a", first.clone()),
                    call_block("tool-item-b", second.clone()),
                ],
                [(0, replay("SECRET_REPLAY_NOT_A_TREE_ROW"))],
            ),
        },
    );
    append_tool_call_lifecycle(&mut journal, &first.call_id, "a");
    let incomplete = journal
        .tree_snapshot(&agent())
        .expect("in-progress tool tree");
    assert_eq!(incomplete.rows.len(), 2);
    assert_eq!(incomplete.rows[1].rewind, TreeRewindEligibility::Ineligible);
    assert_eq!(
        incomplete.rows[1].preview.text,
        "checking read_file list_files"
    );
    assert_eq!(
        journal.tree_source(&plexmaton_core::TreeSourceRequest {
            origin: incomplete.origin,
            entry_id: tool_step.clone(),
        }),
        Err(plexmaton_core::TreeSourceError::IncompleteBatch)
    );
    append_tool_call_lifecycle(&mut journal, &second.call_id, "b");
    let completed_batch = journal.tree_snapshot(&agent()).expect("completed batch");
    assert_eq!(
        journal.tree_source(&plexmaton_core::TreeSourceRequest {
            origin: completed_batch.origin,
            entry_id: tool_step.clone(),
        }),
        Ok("checking\n\nread_file\n\n{\"path\":\"a.txt\"}\n\nresult-a\n\nlist_files\n\n{}\n\nresult-b".to_owned())
    );
    let final_step = append(
        &mut journal,
        "assistant-final",
        JournalEntryPayload::AssistantOutput {
            agent_id: agent(),
            step_id: step("turn-1", 2),
            output: output(vec![
                text_block("final-a", "answer one"),
                reasoning_block("final-reasoning", "reasoning"),
                text_block("final-b", "answer two"),
            ]),
        },
    );
    finish_turn(&mut journal);

    let snapshot = journal.tree_snapshot(&agent()).expect("tool tree");
    assert_eq!(snapshot.rows.len(), 3);
    assert_eq!(snapshot.rows[0].entry_id, user);
    assert_eq!(snapshot.rows[0].kind, TreeRowKind::User);
    assert_eq!(snapshot.rows[1].entry_id, tool_step);
    assert_eq!(snapshot.rows[1].kind, TreeRowKind::ToolBatch);
    assert!(snapshot.rows[1].preview.text.contains("checking"));
    assert!(snapshot.rows[1].preview.text.contains("read_file"));
    assert!(snapshot.rows[1].preview.text.contains("list_files"));
    assert!(!snapshot.rows[1].preview.text.contains("a.txt"));
    assert_eq!(snapshot.rows[2].entry_id, final_step);
    assert_eq!(snapshot.rows[2].parent_id.as_ref(), Some(&tool_step));
    assert_eq!(snapshot.rows[2].kind, TreeRowKind::Assistant);
    assert_eq!(
        snapshot.rows[2].preview.text,
        "answer one reasoning answer two"
    );
    assert_eq!(snapshot.rows[2].head_markers, [head("main")]);
    for row in &snapshot.rows {
        let expected = journal
            .resolve_rewind_target(&agent(), &row.entry_id)
            .is_ok();
        assert_eq!(row.rewind == TreeRewindEligibility::Eligible, expected);
    }
    assert!(matches!(
        journal.resolve_rewind_target(&agent(), &tool_step),
        Err(crate::TreeNavigationRefusal::InteriorAssistantTarget(_))
    ));
    // TRE-8: disjoint branches may reuse a provider's call ID. Copy must bind the result to its
    // canonical assistant ancestor, not whichever matching ID was appended most recently.
    create_head(
        &mut journal,
        "other",
        Some(id("agent-created", ConversationEntryId::new)),
    );
    select_head(&mut journal, "other");
    append(
        &mut journal,
        "other-user",
        JournalEntryPayload::TurnStarted {
            agent_id: agent(),
            item_id: id("other-user-item", TranscriptItemId::new),
            turn_id: id("turn-2", TurnId::new),
            text: "other branch".to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        },
    );
    append(
        &mut journal,
        "other-output",
        JournalEntryPayload::AssistantOutput {
            agent_id: agent(),
            step_id: step("turn-2", 1),
            output: output(vec![call_block("other-tool-item", first.clone())]),
        },
    );
    append_tool_call_lifecycle(&mut journal, &first.call_id, "other-result");
    assert_eq!(
        journal.tree_source(&plexmaton_core::TreeSourceRequest {
            origin: journal
                .tree_snapshot(&agent())
                .expect("branched tree")
                .origin,
            entry_id: tool_step,
        }),
        Ok("checking\n\nread_file\n\n{\"path\":\"a.txt\"}\n\nresult-a\n\nlist_files\n\n{}\n\nresult-b".to_owned())
    );
}

/// TRE-2/TRE-6: an empty root is a complete tree state, and a head-count overflow is explicit.
#[test]
fn tre_2_empty_root_and_exact_head_limit_are_explicit() {
    let mut journal = ConversationJournal::new(id("empty-tree", ConversationId::new));
    let empty = journal
        .tree_snapshot(&agent())
        .expect("empty root snapshot");
    assert!(empty.rows.is_empty());
    assert_eq!(empty.heads.len(), 1);
    assert!(empty.heads[0].target.is_none());

    for index in 1..MAX_TREE_HEAD_COUNT {
        create_head(&mut journal, &format!("branch-{index}"), None);
    }
    assert_eq!(
        journal
            .tree_snapshot(&agent())
            .expect("head limit")
            .heads
            .len(),
        MAX_TREE_HEAD_COUNT
    );
    create_head(&mut journal, "branch-over", None);
    assert_eq!(
        journal.tree_snapshot(&agent()),
        Err(TreeSnapshotError::LimitExceeded {
            limit: TreeSnapshotLimit::Heads,
            maximum: MAX_TREE_HEAD_COUNT,
            observed: MAX_TREE_HEAD_COUNT + 1,
        })
    );
}

/// TRE-2: legacy branch names obey an exact byte bound before the snapshot clones any of them.
#[test]
fn tre_2_legacy_head_names_are_byte_bounded_before_snapshot_retention() {
    for (name, accepted) in [
        ("é".repeat(MAX_TREE_HEAD_NAME_BYTES / 2), true),
        (
            format!("{}x", "é".repeat(MAX_TREE_HEAD_NAME_BYTES / 2)),
            false,
        ),
    ] {
        let mut journal = ConversationJournal::new(id("head-name-tree", ConversationId::new));
        create_head(&mut journal, &name, None);
        let before = journal.clone();
        let result = journal.tree_snapshot(&agent());
        if accepted {
            let snapshot = result.expect("exact name byte limit");
            assert!(snapshot.heads.iter().any(|head| head.name.as_str() == name));
        } else {
            assert_eq!(
                result,
                Err(TreeSnapshotError::LimitExceeded {
                    limit: TreeSnapshotLimit::HeadNameBytes,
                    maximum: MAX_TREE_HEAD_NAME_BYTES,
                    observed: MAX_TREE_HEAD_NAME_BYTES + 1,
                })
            );
        }
        assert_eq!(journal, before);
    }
}

/// TRE-2: ancestry and chronology scan bounds reject one-over snapshots without returning a prefix.
#[test]
fn tre_2_ancestry_and_scanned_record_limits_are_explicit() {
    let mut ancestry = ConversationJournal::new(id("exact-ancestry", ConversationId::new));
    for index in 0..MAX_TREE_ANCESTRY_ENTRY_COUNT {
        append_hidden_warning(&mut ancestry, &format!("hidden-{index}"));
    }
    let exact = ancestry
        .tree_snapshot(&agent())
        .expect("exact ancestry bound");
    assert!(exact.rows.is_empty());
    append_hidden_warning(&mut ancestry, "hidden-over");
    assert_eq!(
        ancestry.tree_snapshot(&agent()),
        Err(TreeSnapshotError::LimitExceeded {
            limit: TreeSnapshotLimit::AncestryEntries,
            maximum: MAX_TREE_ANCESTRY_ENTRY_COUNT,
            observed: MAX_TREE_ANCESTRY_ENTRY_COUNT + 1,
        })
    );

    let mut records = ConversationJournal::new(id("exact-records", ConversationId::new));
    for index in 0..MAX_TREE_SCANNED_RECORD_COUNT / 2 {
        let name = format!("temporary-{index}");
        create_head(&mut records, &name, None);
        abandon_head(&mut records, &name);
    }
    assert_eq!(
        records
            .tree_snapshot(&agent())
            .expect("exact record bound")
            .rows
            .len(),
        0
    );
    create_head(&mut records, "temporary-over", None);
    assert_eq!(
        records.tree_snapshot(&agent()),
        Err(TreeSnapshotError::LimitExceeded {
            limit: TreeSnapshotLimit::ScannedRecords,
            maximum: MAX_TREE_SCANNED_RECORD_COUNT,
            observed: MAX_TREE_SCANNED_RECORD_COUNT + 1,
        })
    );
}

/// TRE-2: row and preview limits distinguish exact bounds from one-over input and retain valid UTF-8.
#[test]
fn tre_2_node_preview_and_aggregate_bounds_are_typed_and_utf8_safe() {
    let mut exact = ConversationJournal::new(id("exact-preview", ConversationId::new));
    append_warning(
        &mut exact,
        "exact",
        "x".repeat(MAX_TREE_PREVIEW_BYTES_PER_ROW),
    );
    let exact_preview = &exact.tree_snapshot(&agent()).expect("exact preview").rows[0].preview;
    assert_eq!(exact_preview.text.len(), MAX_TREE_PREVIEW_BYTES_PER_ROW);
    assert!(!exact_preview.truncated);

    let mut clipped = ConversationJournal::new(id("clipped-preview", ConversationId::new));
    append_warning(
        &mut clipped,
        "clipped",
        format!("{}é", "x".repeat(MAX_TREE_PREVIEW_BYTES_PER_ROW - 1)),
    );
    let clipped_preview = &clipped
        .tree_snapshot(&agent())
        .expect("clipped preview")
        .rows[0]
        .preview;
    assert!(clipped_preview.truncated);
    assert!(clipped_preview.text.ends_with('…'));
    assert!(clipped_preview.text.len() <= MAX_TREE_PREVIEW_BYTES_PER_ROW);
    assert!(std::str::from_utf8(clipped_preview.text.as_bytes()).is_ok());

    let mut nodes = ConversationJournal::new(id("exact-nodes", ConversationId::new));
    for index in 0..MAX_TREE_NODE_COUNT {
        append_warning(&mut nodes, &format!("node-{index}"), "x".to_owned());
    }
    assert_eq!(
        nodes
            .tree_snapshot(&agent())
            .expect("exact nodes")
            .rows
            .len(),
        MAX_TREE_NODE_COUNT
    );
    append_warning(&mut nodes, "node-over", "x".to_owned());
    assert_eq!(
        nodes.tree_snapshot(&agent()),
        Err(TreeSnapshotError::LimitExceeded {
            limit: TreeSnapshotLimit::Nodes,
            maximum: MAX_TREE_NODE_COUNT,
            observed: MAX_TREE_NODE_COUNT + 1,
        })
    );

    let mut aggregate = ConversationJournal::new(id("exact-aggregate", ConversationId::new));
    for index in 0..(MAX_TREE_AGGREGATE_PREVIEW_BYTES / MAX_TREE_PREVIEW_BYTES_PER_ROW) {
        append_warning(
            &mut aggregate,
            &format!("aggregate-{index}"),
            "x".repeat(MAX_TREE_PREVIEW_BYTES_PER_ROW),
        );
    }
    assert_eq!(
        aggregate
            .tree_snapshot(&agent())
            .expect("exact aggregate")
            .rows
            .len(),
        MAX_TREE_AGGREGATE_PREVIEW_BYTES / MAX_TREE_PREVIEW_BYTES_PER_ROW
    );
    append_warning(&mut aggregate, "aggregate-over", "y".to_owned());
    assert_eq!(
        aggregate.tree_snapshot(&agent()),
        Err(TreeSnapshotError::LimitExceeded {
            limit: TreeSnapshotLimit::AggregatePreviewBytes,
            maximum: MAX_TREE_AGGREGATE_PREVIEW_BYTES,
            observed: MAX_TREE_AGGREGATE_PREVIEW_BYTES + 1,
        })
    );
}

/// TRE-7: steering rows remain visible without becoming rewind boundaries.
#[test]
fn tre_7_steering_rows_are_visible_but_not_rewindable() {
    let mut journal = ConversationJournal::new(id("steering-tree", ConversationId::new));
    announce(&mut journal);
    let user = append(&mut journal, "user-entry", user_payload("question"));
    let steering = append(
        &mut journal,
        "steering-entry",
        JournalEntryPayload::SteeringAccepted {
            agent_id: agent(),
            item_id: id("steering-item", TranscriptItemId::new),
            turn_id: id("turn-1", TurnId::new),
            text: "clarification".to_owned(),
            accepted_at: UnixMillis::EPOCH,
        },
    );
    let snapshot = journal.tree_snapshot(&agent()).expect("steering snapshot");
    let steering_row = snapshot
        .rows
        .iter()
        .find(|row| row.entry_id == steering)
        .expect("steering row");
    assert_eq!(steering_row.kind, TreeRowKind::Steering);
    assert_eq!(steering_row.rewind, TreeRewindEligibility::Ineligible);
    assert_eq!(snapshot.rows[0].entry_id, user);
    assert_eq!(snapshot.rows[0].rewind, TreeRewindEligibility::Eligible);
}

/// TRE-7: a validated branch-local checkpoint is visible without becoming a rewind boundary.
#[test]
fn tre_7_checkpoint_row_is_visible_but_not_rewindable() {
    let agent_id = agent();
    let mut conversation_agent = Agent::new(agent_id.clone());
    let _announcement = conversation_agent.announce("Tree checkpoint test agent");
    complete_agent_turn(&mut conversation_agent, "question", "answer");

    let atoms = conversation_agent.record();
    assert_eq!(atoms.len(), 2);
    let user_entry = atoms[0].source_entries()[0].clone();
    let assistant_entry = atoms[1].source_entries()[0].clone();
    let source = conversation_agent
        .compaction_source()
        .expect("completed turn is a compaction source");
    let plan = CompactionPlan::new(
        CompactionId::new("tree-checkpoint-plan").expect("compaction id"),
        source,
        CompactionCut::new(
            user_entry.clone(),
            assistant_entry.clone(),
            None,
            Some(user_entry),
        ),
        checkpoint_environment(),
        1024,
    )
    .expect("valid checkpoint plan");
    let (attempt_id, _authorization) = conversation_agent
        .authorize_compaction_attempt(&plan, UnixMillis::new(10))
        .expect("authorize checkpoint attempt");
    let _finished = conversation_agent
        .finish_compaction_attempt(complete_checkpoint_attempt(attempt_id.clone()))
        .expect("finish checkpoint attempt");
    let _committed = conversation_agent
        .commit_compaction_checkpoint(plan, attempt_id)
        .expect("commit checkpoint");

    let snapshot = conversation_agent
        .journal()
        .tree_snapshot(&agent_id)
        .expect("snapshot with checkpoint");
    assert_eq!(snapshot.rows.len(), 3);
    let checkpoint = &snapshot.rows[2];
    assert_eq!(checkpoint.kind, TreeRowKind::Checkpoint);
    assert_eq!(checkpoint.preview.text, "");
    assert_eq!(checkpoint.parent_id.as_ref(), Some(&assistant_entry));
    assert_eq!(checkpoint.head_markers, [head("main")]);
    assert_eq!(checkpoint.rewind, TreeRewindEligibility::Ineligible);
}
