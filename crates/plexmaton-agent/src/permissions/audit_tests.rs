use super::*;
use plexmaton_core::{PermissionScope, ProjectPermissionRevision, ProjectPermissionStoreId};

fn audit(owner: &SessionPermissions, call: &AdmittedToolCall) -> PermissionDecisionAudit {
    let mut policy = ApprovalPolicy::default();
    policy.use_snapshot(owner.snapshot());
    policy.audit(call, None)
}

/// PER-9: history names the evaluator's winning source and exact observed store/Session revisions.
#[test]
fn per_9_decision_evidence_tracks_precedence_and_exact_source_revisions() {
    let mut owner = state("audit-session");
    let call = create("src/new.rs");
    assert_eq!(
        audit(&owner, &call).evidence,
        PermissionEvidence::Fallback {
            decision: PolicyDecision::RequireApproval
        }
    );
    let id = remember(&mut owner, preset(), &call).expect("remember");
    assert_eq!(
        audit(&owner, &call).evidence,
        PermissionEvidence::Grant {
            id: id.clone(),
            scope: PermissionScope::Session,
            matcher: preset()
        }
    );
    owner
        .revoke(owner.snapshot().revision(), &id)
        .expect("revoke");
    let revision = ProjectPermissionRevision::Present {
        store: ProjectPermissionStoreId::new("project-store").expect("store"),
        sequence: 7,
    };
    owner
        .observe_project(ProjectPermissions::Ready {
            revision: revision.clone(),
            can_remember: true,
            grants: vec![PermissionGrant {
                id: id.clone(),
                matcher: preset(),
                origin: PermissionGrantOrigin::Approval,
            }],
            configuration: None,
            trusted_config: None,
        })
        .expect("project");
    let observed = audit(&owner, &call);
    assert_eq!(
        observed.revision.as_ref(),
        Some(owner.snapshot().revision())
    );
    assert_eq!(
        observed.project,
        PermissionProjectAudit::Available { revision }
    );
    assert_eq!(
        observed.evidence,
        PermissionEvidence::Grant {
            id,
            scope: PermissionScope::Project,
            matcher: preset()
        }
    );
    for action in [
        PermissionRuleAction::Allow,
        PermissionRuleAction::Ask,
        PermissionRuleAction::Deny,
    ] {
        let source = PermissionRuleSource::UserConfiguration {
            fingerprint: [7; 32],
            index: 3,
        };
        owner
            .replace_rules(
                owner.snapshot().revision(),
                vec![PermissionRule {
                    source: source.clone(),
                    action,
                    matcher: preset(),
                }],
            )
            .expect("rules");
        assert_eq!(
            audit(&owner, &call).evidence,
            PermissionEvidence::Rule {
                action,
                source,
                matcher: preset()
            }
        );
    }
    owner
        .replace_rules(owner.snapshot().revision(), Vec::new())
        .expect("clear user");
    let config = PermissionConfiguration::new(
        [8; 32],
        vec![PermissionRule {
            source: PermissionRuleSource::Runtime,
            action: PermissionRuleAction::Allow,
            matcher: preset(),
        }],
    )
    .expect("config");
    owner
        .observe_project(ProjectPermissions::Ready {
            revision: ProjectPermissionRevision::Absent,
            can_remember: true,
            grants: Vec::new(),
            configuration: Some(config),
            trusted_config: Some([8; 32]),
        })
        .expect("trust");
    assert_eq!(
        audit(&owner, &call).evidence,
        PermissionEvidence::Rule {
            action: PermissionRuleAction::Allow,
            source: PermissionRuleSource::ProjectConfiguration {
                fingerprint: [8; 32],
                index: 0
            },
            matcher: preset()
        }
    );
    owner
        .observe_project(ProjectPermissions::Unavailable)
        .expect("unavailable");
    assert_eq!(
        audit(&owner, &call).evidence,
        PermissionEvidence::Unavailable
    );
}

/// PER-9/JRN-3: a maximal exact scope round-trips and decoding cannot bypass its bound.
#[test]
fn per_9_historical_command_scopes_preserve_context_and_recheck_wire_bounds() {
    let command = CommandPermission::new("x".repeat(24 * 1024), [9; 32]).expect("bound");
    let call = admitted(
        "command",
        PermissionSubject::Command {
            command: command.clone(),
            syntax: CommandSyntax::ExactOnly(PrefixUnavailable::UnsupportedSyntax),
        },
    );
    let mut owner = state("audit-command");
    let matcher = PermissionMatcher::ExactCommand {
        definition: definition("command"),
        command,
    };
    remember(&mut owner, matcher, &call).expect("remember");
    let fact = audit(&owner, &call);
    assert_eq!(fact.command_context, Some([9; 32]));
    let encoded = serde_json::to_string(&fact).expect("encode");
    assert_eq!(
        serde_json::from_str::<PermissionDecisionAudit>(&encoded).expect("decode"),
        fact
    );
    let mut wire: serde_json::Value = serde_json::from_str(&encoded).expect("wire");
    wire["evidence"]["matcher"]["command"]["source"] =
        serde_json::Value::String("x".repeat(24 * 1024 + 1));
    assert!(serde_json::from_value::<PermissionDecisionAudit>(wire).is_err());
}
