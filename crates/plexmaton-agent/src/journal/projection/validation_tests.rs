use plexmaton_core::{
    AgentId, ApprovalId, AttentionId, AttentionRequest, ConversationEvent, ConversationId, MailId,
    ToolCallId, ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
};

use super::super::{ConversationJournal, JournalEntryPayload, JournalProjectionError};
use super::tests::{agent, announce, append, assistant_calls, call, head, id, message};
use crate::test_support::{call_block, text_block};

fn declare_call(journal: &mut ConversationJournal, call: crate::ToolCall) {
    append(journal, 1, message(TranscriptRole::User, 1, "inspect"));
    append(
        journal,
        2,
        assistant_calls(1, 1, vec![call_block("tool-a", call.clone())]),
    );
    append(
        journal,
        3,
        JournalEntryPayload::ToolCallRequested {
            agent_id: agent(),
            call_id: call.call_id,
            presentation: ToolPresentation::default(),
        },
    );
}

/// JRN-5: recovery applies only to a final tail, never to corruption before later model facts.
#[test]
fn jrn_5_incomplete_tool_batch_before_later_content_is_rejected() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let (call, _) = call("call-a", "output");
    declare_call(&mut journal, call.clone());
    append(
        &mut journal,
        4,
        assistant_calls(1, 2, vec![text_block("assistant-tail", "impossible tail")]),
    );

    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::IncompleteToolBatchBeforeLaterFact(
            vec![call.call_id]
        ))
    );
}

/// JRN-5: invalid lifecycle data is rejected before either consumer sees it.
#[test]
fn jrn_5_invalid_tool_lifecycle_has_a_typed_projection_error() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let (call, _) = call("call-a", "output");
    declare_call(&mut journal, call.clone());
    append(
        &mut journal,
        4,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: call.call_id.clone(),
            item_revision: 2,
            status: ToolCallStatus::Running,
            presentation: ToolPresentation::default(),
            outcome: None,
        },
    );

    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::UnexpectedToolRevision {
            call_id: call.call_id,
            expected: 1,
            actual: 2,
        })
    );
}

/// JRN-5: one transcript identity cannot create two model/UI facts.
#[test]
fn jrn_5_duplicate_transcript_identity_is_rejected_before_projection() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let item_id = id("same-item", TranscriptItemId::new);
    append(
        &mut journal,
        1,
        JournalEntryPayload::TurnStarted {
            agent_id: agent(),
            item_id: item_id.clone(),
            turn_id: id("turn-1", plexmaton_core::TurnId::new),
            text: "visible once".to_owned(),
            accepted_at: crate::UnixMillis::EPOCH,
            opened_at: crate::UnixMillis::EPOCH,
        },
    );
    append(
        &mut journal,
        2,
        JournalEntryPayload::RuntimeWarning {
            agent_id: agent(),
            item_id: item_id.clone(),
            message: "must not replace it".to_owned(),
        },
    );

    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::DuplicateTranscriptItem(item_id))
    );
}

/// JRN-5: mail cannot produce an event for a recipient absent from the selected head.
#[test]
fn jrn_5_mail_requires_both_visible_endpoints() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let missing = id("agent-b", AgentId::new);
    append(
        &mut journal,
        1,
        JournalEntryPayload::MailDelivered {
            item_id: id("mail-item", TranscriptItemId::new),
            mail_id: id("mail-1", MailId::new),
            from: agent(),
            to: missing.clone(),
            summary: "hello".to_owned(),
        },
    );

    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::MissingAgent(missing))
    );
}

/// JRN-5: an Attention identity cannot move between visible agent owners.
#[test]
fn jrn_5_attention_resolution_keeps_its_request_owner() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let agent_b = id("agent-b", AgentId::new);
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_b.clone(),
            label: "Agent B".to_owned(),
            status: plexmaton_core::AgentStatus::Idle,
        },
    );
    let attention_id = id("attention-1", AttentionId::new);
    append(
        &mut journal,
        2,
        JournalEntryPayload::AttentionRequested {
            agent_id: agent(),
            attention_id: attention_id.clone(),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: id("approval-1", ApprovalId::new),
                call_id: id("call-1", ToolCallId::new),
                tool: "read_file".to_owned(),
                capabilities: Vec::new(),
                detail: "Read README.md".to_owned(),
            },
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::AttentionResolved {
            agent_id: agent_b.clone(),
            attention_id: attention_id.clone(),
        },
    );

    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::AttentionOwnerMismatch {
            attention_id,
            expected: agent(),
            actual: agent_b,
        })
    );
}

/// JRN-5: a visible notice does not turn an incomplete final batch into corruption.
#[test]
fn jrn_5_visible_notice_after_incomplete_batch_remains_a_recoverable_tail() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let (call, _) = call("call-a", "output");
    declare_call(&mut journal, call.clone());
    append(
        &mut journal,
        4,
        message(TranscriptRole::System, 2, "turn interrupted"),
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project recoverable tail: {error:?}"));
    assert_eq!(
        projection
            .recovery()
            .map(|recovery| recovery.omitted_batch_calls()),
        Some([call.call_id].as_slice())
    );
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        ConversationEvent::RuntimeWarning { message, .. } if message == "turn interrupted"
    )));
}

/// JRN-5: later snapshots retain earlier presentation fields instead of erasing them.
#[test]
fn jrn_5_tool_presentation_accumulates_across_lifecycle_snapshots() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    announce(&mut journal);
    let (call, outcome) = call("call-a", "output");
    declare_call(&mut journal, call.clone());
    let invocation = ToolDetail::Text {
        source: "Read README.md".to_owned(),
        omitted_bytes: 0,
    };
    append(
        &mut journal,
        4,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: call.call_id.clone(),
            item_revision: 1,
            status: ToolCallStatus::Running,
            presentation: ToolPresentation {
                invocation: Some(invocation.clone()),
                outcome: None,
            },
            outcome: None,
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: call.call_id,
            item_revision: 2,
            status: ToolCallStatus::Succeeded,
            presentation: ToolPresentation::default(),
            outcome: Some(outcome),
        },
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project tool: {error:?}"));
    let terminal = projection
        .events()
        .iter()
        .find_map(|event| match &event.event {
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Succeeded,
                presentation,
                ..
            } => Some(presentation),
            _ => None,
        });
    assert_eq!(
        terminal.and_then(|detail| detail.invocation.as_ref()),
        Some(&invocation)
    );
}

/// PER-9/JRN-5: decision history must name a requested live call and cannot repeat at one boundary.
#[test]
fn per_9_provenance_refuses_foreign_late_and_duplicate_call_facts() {
    use crate::{
        PermissionDecisionAudit, PermissionDefinition, PermissionEvidence, PermissionProjectAudit,
        PolicyDecision, ToolDefinitionRevision,
    };
    let payload = |agent_id, call_id| JournalEntryPayload::ToolPermissionDecided {
        agent_id,
        call_id,
        audit: Box::new(PermissionDecisionAudit {
            definition: PermissionDefinition::new(
                id("native-read", plexmaton_core::ToolDefinitionId::new),
                ToolDefinitionRevision::new(1).expect("revision"),
            ),
            command_context: None,
            revision: None,
            project: PermissionProjectAudit::Disabled,
            evidence: PermissionEvidence::Fallback {
                decision: PolicyDecision::Allow,
            },
            user: None,
        }),
    };
    let mut journal = ConversationJournal::new(id("audit", ConversationId::new));
    announce(&mut journal);
    let (call, _) = call("call-a", "output");
    declare_call(&mut journal, call.clone());
    let mut missing = journal.clone();
    let foreign_call = id("absent", ToolCallId::new);
    append(&mut missing, 4, payload(agent(), foreign_call.clone()));
    assert_eq!(
        missing.project(&head("main")),
        Err(JournalProjectionError::MissingToolCall(foreign_call))
    );
    let mut wrong = journal.clone();
    append(
        &mut wrong,
        4,
        payload(id("foreign", AgentId::new), call.call_id.clone()),
    );
    assert_eq!(
        wrong.project(&head("main")),
        Err(JournalProjectionError::WrongToolAgent(call.call_id.clone()))
    );
    let mut duplicate = journal.clone();
    append(&mut duplicate, 4, payload(agent(), call.call_id.clone()));
    let first = duplicate.project(&head("main")).expect("valid audit");
    let before = journal.project(&head("main")).expect("before audit");
    assert_eq!(first.events(), before.events());
    assert_eq!(first.request(), before.request());
    append(&mut duplicate, 5, payload(agent(), call.call_id.clone()));
    assert_eq!(
        duplicate.project(&head("main")),
        Err(JournalProjectionError::InvalidPermissionDecision(
            call.call_id.clone()
        ))
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: call.call_id.clone(),
            item_revision: 1,
            status: ToolCallStatus::Running,
            presentation: ToolPresentation::default(),
            outcome: None,
        },
    );
    append(&mut journal, 5, payload(agent(), call.call_id.clone()));
    assert_eq!(
        journal.project(&head("main")),
        Err(JournalProjectionError::InvalidPermissionDecision(
            call.call_id
        ))
    );
}
