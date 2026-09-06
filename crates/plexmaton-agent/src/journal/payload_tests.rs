use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, AttentionId, AttentionRequest, ConversationEntryId, HeadName,
    JournalRecordId, MailId, ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
    TurnId,
};

use super::{ConversationEntry, HeadRevision, JournalEntryPayload, JournalRecord, JournalSequence};
use crate::test_support::{call_block, output_with_replay, reasoning_block, replay, step};
use crate::{ActiveTurnStatus, SkillActivation, SkillSource, ToolCall, ToolOutcome, UnixMillis};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

/// JRN-3: every canonical fact has a lossless tagged JSON representation.
#[test]
fn jrn_3_every_canonical_payload_variant_round_trips_inside_an_append() {
    let agent_a = id("agent-a", AgentId::new);
    let agent_b = id("agent-b", AgentId::new);
    let attention_id = id("attention-1", AttentionId::new);
    let call_id = id("call-1", ToolCallId::new);
    let call = ToolCall {
        call_id: call_id.clone(),
        name: "read_file".to_owned(),
        arguments: "{}".to_owned(),
    };
    let payloads = vec![
        JournalEntryPayload::AgentCreated {
            agent_id: agent_a.clone(),
            label: "Agent A".to_owned(),
            status: AgentStatus::Idle,
        },
        JournalEntryPayload::TurnStatusChanged {
            agent_id: agent_a.clone(),
            turn_id: id("turn-started", TurnId::new),
            status: ActiveTurnStatus::Running,
        },
        JournalEntryPayload::TurnStarted {
            agent_id: agent_a.clone(),
            item_id: id("turn-item", TranscriptItemId::new),
            turn_id: id("turn-started", TurnId::new),
            text: "hello".to_owned(),
            accepted_at: UnixMillis::new(100),
            opened_at: UnixMillis::new(120),
        },
        JournalEntryPayload::TurnRetried {
            agent_id: agent_a.clone(),
            source_turn_id: id("turn-started", TurnId::new),
            turn_id: id("turn-retried", TurnId::new),
            opened_at: UnixMillis::new(130),
        },
        JournalEntryPayload::SteeringAccepted {
            agent_id: agent_a.clone(),
            item_id: id("steering-item", TranscriptItemId::new),
            turn_id: id("turn-started", TurnId::new),
            text: "also inspect tests".to_owned(),
            accepted_at: UnixMillis::new(130),
        },
        JournalEntryPayload::SkillActivated {
            agent_id: agent_a.clone(),
            turn_id: id("turn-started", TurnId::new),
            activation: SkillActivation::new(
                "review".to_owned(),
                SkillSource::ProjectShared,
                "/workspace/.agents/skills/review/SKILL.md".to_owned(),
                "a".repeat(64),
                "exact instructions".to_owned(),
            )
            .unwrap_or_else(|error| panic!("skill fixture: {error}")),
        },
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_a.clone(),
            step_id: step("turn-started", 1),
            output: output_with_replay(
                vec![
                    reasoning_block("reasoning-item", "recovered"),
                    call_block("tool-item", call.clone()),
                ],
                [(0, replay("encrypted"))],
            ),
        },
        JournalEntryPayload::ToolCallRequested {
            agent_id: agent_a.clone(),
            call_id: call_id.clone(),
            presentation: ToolPresentation::default(),
        },
        JournalEntryPayload::ToolPermissionDecided {
            agent_id: agent_a.clone(),
            call_id: call_id.clone(),
            audit: permission_audit(),
        },
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent_a.clone(),
            call_id: call_id.clone(),
            item_revision: 1,
            status: ToolCallStatus::Failed,
            presentation: ToolPresentation::default(),
            outcome: Some(ToolOutcome::Failed {
                message: "failed".to_owned(),
            }),
        },
        JournalEntryPayload::AttentionRequested {
            agent_id: agent_a.clone(),
            attention_id: attention_id.clone(),
            request: AttentionRequest::Clarification {
                summary: "which file?".to_owned(),
            },
        },
        JournalEntryPayload::AttentionResolved {
            agent_id: agent_a.clone(),
            attention_id,
        },
        JournalEntryPayload::MailDelivered {
            item_id: id("mail-item", TranscriptItemId::new),
            mail_id: id("mail-1", MailId::new),
            from: agent_a.clone(),
            to: agent_b,
            summary: "done".to_owned(),
        },
        JournalEntryPayload::ArtifactAnnounced {
            agent_id: agent_a.clone(),
            item_id: id("artifact-item", TranscriptItemId::new),
            artifact_id: id("artifact-1", ArtifactId::new),
            label: "report".to_owned(),
            pointer: "artifact://report".to_owned(),
        },
        JournalEntryPayload::RuntimeWarning {
            agent_id: agent_a.clone(),
            item_id: id("warning-item", TranscriptItemId::new),
            message: "warning".to_owned(),
        },
        JournalEntryPayload::RuntimeError {
            agent_id: agent_a.clone(),
            item_id: id("error-item", TranscriptItemId::new),
            message: "error".to_owned(),
        },
        JournalEntryPayload::TurnInterruptedByRecovery {
            agent_id: agent_a,
            item_id: id("recovery-item", TranscriptItemId::new),
        },
    ];

    for (index, payload) in payloads.into_iter().enumerate() {
        let record = JournalRecord::AppendEntry {
            sequence: JournalSequence::new(1),
            record_id: id(&format!("record-{index}"), JournalRecordId::new),
            head: id("main", HeadName::new),
            expected_head_revision: HeadRevision::new(0),
            entry: Box::new(ConversationEntry {
                id: id(&format!("entry-{index}"), ConversationEntryId::new),
                parent_id: None,
                payload,
            }),
        };
        let json =
            serde_json::to_string(&record).unwrap_or_else(|error| panic!("encode append: {error}"));
        let decoded = serde_json::from_str::<JournalRecord>(&json)
            .unwrap_or_else(|error| panic!("decode append: {error}"));
        assert_eq!(decoded, record);
    }
}

fn permission_audit() -> Box<crate::PermissionDecisionAudit> {
    Box::new(crate::PermissionDecisionAudit {
        definition: crate::PermissionDefinition::new(
            id("native-read", plexmaton_core::ToolDefinitionId::new),
            crate::ToolDefinitionRevision::new(1).expect("revision"),
        ),
        command_context: None,
        revision: None,
        project: crate::PermissionProjectAudit::Disabled,
        evidence: crate::PermissionEvidence::Fallback {
            decision: crate::PolicyDecision::Allow,
        },
        user: None,
    })
}
