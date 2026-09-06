use super::*;
use crate::{
    ConversationMetadata, NativeFileChange, PermissionChangeError, PermissionDefinition,
    PermissionPreparationRequest, PermissionSubject, SessionPermissions, ToolAuthorization,
};
use plexmaton_core::{ApprovalId, CodingSessionId, PermissionScope};

fn waiting(conversation: &str) -> (Agent, SessionPermissions, ApprovalId) {
    waiting_calls(conversation, 1)
}

fn waiting_calls(conversation: &str, count: u16) -> (Agent, SessionPermissions, ApprovalId) {
    let revision = ToolDefinitionRevision::new(1).expect("revision");
    let definition = |name| {
        PermissionDefinition::new(ToolDefinitionId::new(name).expect("definition"), revision)
    };
    let owner = SessionPermissions::new(CodingSessionId::new("coding-session").expect("Session"))
        .with_native_file_changes(definition("create-v1"), definition("edit-v1"));
    let mut agent = Agent::for_conversation(
        AgentId::new("agent").expect("agent"),
        ConversationMetadata::new(
            ConversationId::new(conversation).expect("Conversation"),
            UnixMillis::EPOCH,
        ),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    agent.use_permission_snapshot(owner.snapshot());
    submit(&mut agent, "create a file");
    for index in 1..=count {
        call_named(&mut agent, &format!("same-call-{index}"), "create");
    }
    for effect in stop_before_admission(&mut agent, StopReason::ToolCalls).effects {
        let Effect::AdmitTool(request) = effect else {
            panic!("admission effect");
        };
        let path = format!("src/{}.rs", request.requested().call_id);
        let outcome = request
            .with_permission_subject(PermissionSubject::NativeFileChange(
                NativeFileChange::create(path).expect("path"),
            ))
            .admit(
                ToolDefinitionId::new("create-v1").expect("definition"),
                revision,
                [ToolCapability::FileWrite],
                "{}".into(),
                "create a source file".into(),
                None,
            )
            .expect("admitted");
        let reaction = agent.handle(Input::ToolAdmissionResolved(outcome));
        assert!(reaction.effects.is_empty());
    }
    let id = agent
        .pending_approvals()
        .next()
        .expect("pending")
        .approval_id()
        .clone();
    (agent, owner, id)
}

fn decision(agent: &Agent) -> ApprovalDecision {
    let offer = agent
        .pending_approvals()
        .next()
        .expect("pending")
        .permission_offer()
        .expect("effective offer");
    ApprovalDecision::AllowAndRemember {
        offer: offer.id,
        scope: PermissionScope::Session,
    }
}
fn prepare(agent: &mut Agent, approval_id: &ApprovalId) -> PermissionPreparationRequest {
    let decision = decision(agent);
    let mut reaction = agent.handle(Input::ApprovalDecided {
        approval_id: approval_id.clone(),
        decision,
    });
    assert!(
        reaction.records.is_empty(),
        "PER-5: no execution audit before permission preparation"
    );
    assert_eq!(reaction.effects.len(), 1);
    let Effect::PreparePermission(request) = reaction.effects.pop().expect("effect") else {
        panic!("only permission preparation");
    };
    request
}
fn apply(
    owner: &mut SessionPermissions,
    request: PermissionPreparationRequest,
) -> crate::PermissionPreparationOutcome {
    let grant = owner
        .remember(
            request.revision(),
            request.matcher().clone(),
            request.admitted(),
            &ApprovalPolicy::default(),
        )
        .expect("remember");
    request.prepared(grant)
}

#[test]
fn per_5_remember_prepares_once_and_only_then_produces_the_audited_execution() {
    let (mut agent, mut owner, id) = waiting("conversation-a");
    let selected = decision(&agent);
    let request = prepare(&mut agent, &id);
    assert_eq!(
        agent.pending_approvals().count(),
        1,
        "request remains inspectable during preparation"
    );
    let repeated = agent.handle(Input::ApprovalDecided {
        approval_id: id.clone(),
        decision: selected,
    });
    assert!(repeated.effects.is_empty());
    assert_eq!(
        repeated.unresolved_approvals[0].reason,
        ApprovalDecisionRefusal::Preparing
    );
    let prepared = apply(&mut owner, request);
    agent.use_permission_snapshot(owner.snapshot());
    let reaction = agent.handle(Input::PermissionPrepared(prepared));
    assert!(
        !reaction.records.is_empty(),
        "the runtime owes this execution an acknowledged lifecycle audit"
    );
    assert!(matches!(
        reaction.effects.as_slice(),
        [Effect::RunTool {
            authorization: ToolAuthorization::Remembered { .. },
            ..
        }]
    ));
    assert_eq!(agent.pending_approvals().count(), 0);
    let repeated = agent.handle(Input::ApprovalDecided {
        approval_id: id,
        decision: selected,
    });
    assert!(repeated.effects.is_empty());
    assert_eq!(
        repeated.unresolved_approvals[0].reason,
        ApprovalDecisionRefusal::NotPending
    );
}

#[test]
fn per_5_preparation_failure_keeps_the_request_and_cancellation_refuses_late_completion() {
    let (mut agent, mut owner, id) = waiting("conversation-a");
    let request = prepare(&mut agent, &id);
    let refused = agent.handle(Input::PermissionPrepared(
        request.refused(PermissionChangeError::Capacity),
    ));
    assert!(refused.effects.is_empty());
    assert_eq!(agent.pending_approvals().count(), 1);
    assert_eq!(
        refused.unresolved_approvals[0].reason,
        ApprovalDecisionRefusal::Permission(PermissionChangeError::Capacity)
    );
    let request = prepare(&mut agent, &id);
    let prepared = apply(&mut owner, request);
    agent.handle(Input::Interrupted);
    agent.use_permission_snapshot(owner.snapshot());
    let late = agent.handle(Input::PermissionPrepared(prepared));
    assert!(late.effects.is_empty());
    assert_eq!(agent.pending_approvals().count(), 0);
    assert!(!agent.is_running());
}

#[test]
fn per_4_a_changed_policy_reissues_choices_without_applying_the_stale_decision() {
    let (mut agent, mut owner, id) = waiting("conversation-a");
    let old = decision(&agent);
    owner
        .replace_rules(owner.snapshot().revision(), Vec::new())
        .expect("policy revision");
    agent.use_permission_snapshot(owner.snapshot());
    let refused = agent.handle(Input::ApprovalDecided {
        approval_id: id,
        decision: old,
    });
    assert!(refused.effects.is_empty());
    assert_eq!(
        refused.unresolved_approvals[0].reason,
        ApprovalDecisionRefusal::PolicyChanged
    );
    assert!(refused.unresolved_approvals[0].current_offer.is_some());
    assert_ne!(decision(&agent), old);
    assert!(owner.snapshot().grants().is_empty());
}

#[test]
fn per_5_allow_once_cannot_cross_conversations_with_reused_provider_call_ids() {
    let (_, _, first) = waiting("conversation-a");
    let (mut other, _, second) = waiting("conversation-b");
    assert_ne!(first, second);
    let refused = other.handle(Input::ApprovalDecided {
        approval_id: first,
        decision: ApprovalDecision::AllowOnce,
    });
    assert!(refused.effects.is_empty());
    assert_eq!(
        refused.unresolved_approvals[0].reason,
        ApprovalDecisionRefusal::NotPending
    );
    assert_eq!(
        other
            .pending_approvals()
            .next()
            .expect("still pending")
            .approval_id(),
        &second
    );
}

/// PER-2/PER-5: a broad approved preset also releases already-waiting matching siblings through policy authority.
#[test]
fn per_5_remember_releases_covered_waiting_siblings_through_current_policy() {
    let (mut agent, mut owner, id) = waiting_calls("conversation-a", 2);
    let request = prepare(&mut agent, &id);
    let prepared = apply(&mut owner, request);
    agent.use_permission_snapshot(owner.snapshot());
    let reaction = agent.handle(Input::PermissionPrepared(prepared));
    assert_eq!(agent.pending_approvals().count(), 0);
    assert_eq!(reaction.effects.len(), 2);
    assert!(matches!(
        &reaction.effects[0],
        Effect::RunTool {
            authorization: ToolAuthorization::Remembered { .. },
            ..
        }
    ));
    assert!(matches!(
        &reaction.effects[1],
        Effect::RunTool {
            authorization: ToolAuthorization::Policy,
            ..
        }
    ));
}

/// PER-9/JRN-5: wire history preserves the decision but restores neither a grant nor model content.
#[test]
fn per_9_permission_history_replays_without_authority_or_model_content() {
    for selected in [ApprovalDecision::AllowOnce, ApprovalDecision::Deny] {
        let (mut agent, _, id) = waiting("audit-once");
        let call = agent
            .pending_approvals()
            .next()
            .expect("pending")
            .admitted()
            .clone();
        let reaction = agent.handle(Input::ApprovalDecided {
            approval_id: id.clone(),
            decision: selected,
        });
        let audit = reaction
            .records
            .iter()
            .find_map(|record| match record {
                JournalRecord::AppendEntry { entry, .. } => match &entry.payload {
                    JournalEntryPayload::ToolPermissionDecided { audit, .. } => Some(audit),
                    _ => None,
                },
                _ => None,
            })
            .expect("explicit decision");
        let user = audit.user.as_ref().expect("user decision");
        assert_eq!(user.approval_id, id);
        assert_eq!(user.decision, selected);
        assert_eq!(
            user.reason,
            plexmaton_core::ApprovalReason::NativeFileChange
        );
        assert_eq!(user.remembered, None);
        assert_eq!(
            audit.definition,
            PermissionDefinition::new(call.definition_id().clone(), call.definition_revision())
        );
    }
    let (mut agent, mut owner, id) = waiting("audit-replay");
    let call = agent
        .pending_approvals()
        .next()
        .expect("pending")
        .admitted()
        .clone();
    let request = prepare(&mut agent, &id);
    let prepared = apply(&mut owner, request);
    agent.use_permission_snapshot(owner.snapshot());
    let reaction = agent.handle(Input::PermissionPrepared(prepared));
    let audit = reaction
        .records
        .iter()
        .find_map(|record| match record {
            JournalRecord::AppendEntry { entry, .. } => match &entry.payload {
                JournalEntryPayload::ToolPermissionDecided { audit, .. } => Some(audit),
                _ => None,
            },
            _ => None,
        })
        .expect("remember audit");
    let snapshot = owner.snapshot();
    let grant = &snapshot.grants()[0];
    assert_eq!(
        audit.user.as_ref().expect("user").remembered.as_ref(),
        Some(&grant.id)
    );
    assert!(
        matches!(&audit.evidence, crate::PermissionEvidence::Grant { id, scope: PermissionScope::Session, .. } if id == &grant.id)
    );
    finish(&mut agent, "same-call-1", "created");
    stop(&mut agent, StopReason::EndOfTurn);
    let before = agent
        .journal()
        .project(&HeadName::new("main").expect("head"))
        .expect("projection");
    let mut decoded = ConversationJournal::with_metadata(agent.journal().metadata().clone());
    for record in agent.journal().records() {
        let wire = serde_json::to_string(record).expect("encode");
        decoded
            .apply(serde_json::from_str(&wire).expect("decode"))
            .expect("replay");
    }
    let mut restored = Agent::from_journal(
        AgentId::new("agent").expect("agent"),
        decoded,
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("restore");
    let after = restored.rebuild_projection().expect("idle");
    assert_eq!(before, after);
    assert_eq!(
        after.request().atoms.len(),
        2,
        "only the question and tool batch enter model context"
    );
    assert_eq!(
        restored.policy.decide(&call),
        crate::PolicyDecision::RequireApproval,
        "history never restores the saved grant"
    );
}
