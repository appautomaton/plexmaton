use super::*;
use crate::{
    AdmissionOutcome, AdmissionRequest, CapabilitySet, NativeFileChange, PolicyDecision, ToolCall,
};
use plexmaton_core::{ToolCallId, ToolCapability};

fn definition(id: &str) -> PermissionDefinition {
    PermissionDefinition::new(
        ToolDefinitionId::new(id).expect("definition"),
        ToolDefinitionRevision::new(1).expect("revision"),
    )
}
fn preset() -> PermissionMatcher {
    PermissionMatcher::NativeFileChanges {
        create: definition("native-create"),
        edit: definition("native-edit"),
    }
}
fn state(id: &str) -> SessionPermissions {
    SessionPermissions::new(CodingSessionId::new(id).expect("Session"))
}
fn admitted(id: &str, subject: PermissionSubject) -> AdmittedToolCall {
    let request = AdmissionRequest::new(ToolCall {
        call_id: ToolCallId::new("call").expect("call"),
        name: "untrusted-display-name".into(),
        arguments: "{}".into(),
    });
    let AdmissionOutcome::Admitted(call) = request
        .with_permission_subject(subject)
        .admit(
            ToolDefinitionId::new(id).expect("definition"),
            ToolDefinitionRevision::new(1).expect("revision"),
            [ToolCapability::FileWrite],
            "{}".into(),
            "display text has no authority".into(),
            None,
        )
        .expect("admission")
    else {
        panic!("admitted");
    };
    call
}
fn create(path: &str) -> AdmittedToolCall {
    admitted(
        "native-create",
        PermissionSubject::NativeFileChange(NativeFileChange::create(path.into()).expect("path")),
    )
}
fn decision(state: &SessionPermissions, call: &AdmittedToolCall) -> PolicyDecision {
    let mut policy = ApprovalPolicy::default();
    policy.use_snapshot(state.snapshot());
    policy.decide(call)
}
fn remember(
    state: &mut SessionPermissions,
    matcher: PermissionMatcher,
    call: &AdmittedToolCall,
) -> Result<PermissionGrantId, PermissionChangeError> {
    state.remember(
        state.snapshot().revision(),
        matcher,
        call,
        &ApprovalPolicy::default(),
    )
}

#[test]
fn per_1_memory_is_owned_by_the_coding_session_and_snapshots_cannot_mutate_it() {
    let call = create("src/new.rs");
    let mut session = state("coding-a");
    let old = session.snapshot();
    let grant = remember(&mut session, preset(), &call).expect("effective grant");
    assert_eq!(decision(&session, &call), PolicyDecision::Allow);
    assert!(
        old.grants().is_empty(),
        "a published view remains immutable"
    );
    assert_eq!(
        decision(&state("coding-restarted"), &call),
        PolicyDecision::RequireApproval
    );
    session
        .revoke(session.snapshot().revision(), &grant)
        .expect("revoke");
    assert_eq!(decision(&session, &call), PolicyDecision::RequireApproval);
}

/// PER-3/PER-7: the native setting is its own grant; revoking it never deletes a different Allow source.
#[test]
fn per_7_native_setting_is_a_named_grant_with_current_revision_controls() {
    use plexmaton_core::{NativeFilePreset, PermissionAction, PermissionIntent};
    let mut session = state("coding-settings")
        .with_native_file_changes(definition("native-create"), definition("native-edit"));
    let intent = PermissionIntent {
        expected: session.snapshot().revision().clone(),
        action: PermissionAction::EnableNativeFiles,
    };
    session.apply_control(&intent).expect("enable");
    let enabled = session.snapshot();
    let NativeFilePreset::Enabled(grant) = enabled.control_view().native_files else {
        panic!("setting derives from its grant");
    };
    assert_eq!(
        decision(&session, &create("src/new.rs")),
        PolicyDecision::Allow
    );
    assert_eq!(
        decision(
            &session,
            &admitted(
                "native-edit",
                PermissionSubject::NativeFileChange(
                    NativeFileChange::edit("src/old.rs".into()).expect("path")
                )
            )
        ),
        PolicyDecision::Allow
    );
    for excluded in [
        create(".git/config"),
        create(".plexmaton/config.toml"),
        create("AGENTS.md"),
        admitted("native-command", PermissionSubject::Opaque),
    ] {
        assert_eq!(
            decision(&session, &excluded),
            PolicyDecision::RequireApproval
        );
    }
    assert_eq!(
        session.apply_control(&intent),
        Err(PermissionChangeError::StaleRevision)
    );
    assert_eq!(session.snapshot(), enabled);
    session
        .apply_control(&PermissionIntent {
            expected: enabled.revision().clone(),
            action: PermissionAction::EnableNativeFiles,
        })
        .expect("idempotent enable");
    assert_eq!(session.snapshot(), enabled);
    remember(&mut session, preset(), &create("src/new.rs")).expect("independent remembered grant");
    session
        .apply_control(&PermissionIntent {
            expected: session.snapshot().revision().clone(),
            action: PermissionAction::Revoke(grant),
        })
        .expect("disable named setting");
    assert_eq!(
        session.snapshot().control_view().native_files,
        NativeFilePreset::Disabled
    );
    assert_eq!(
        decision(&session, &create("src/new.rs")),
        PolicyDecision::Allow,
        "another grant still permits native changes"
    );
}

#[test]
fn per_2_deny_then_ask_precede_allow_and_memory_in_every_rule_order() {
    let call = create("src/new.rs");
    let mut session = state("coding");
    remember(&mut session, preset(), &call).expect("grant");
    for actions in [
        [
            PermissionRuleAction::Allow,
            PermissionRuleAction::Ask,
            PermissionRuleAction::Deny,
        ],
        [
            PermissionRuleAction::Deny,
            PermissionRuleAction::Allow,
            PermissionRuleAction::Ask,
        ],
    ] {
        session
            .replace_rules(
                session.snapshot().revision(),
                actions
                    .map(|action| PermissionRule {
                        source: PermissionRuleSource::Runtime,
                        matcher: preset(),
                        action,
                    })
                    .into(),
            )
            .expect("rules");
        assert_eq!(decision(&session, &call), PolicyDecision::Forbidden);
        assert_eq!(
            remember(&mut session, preset(), &call),
            Err(PermissionChangeError::Ineffective)
        );
    }
    session
        .replace_rules(
            session.snapshot().revision(),
            vec![PermissionRule {
                source: PermissionRuleSource::Runtime,
                matcher: preset(),
                action: PermissionRuleAction::Ask,
            }],
        )
        .expect("ask rule");
    assert_eq!(decision(&session, &call), PolicyDecision::RequireApproval);
    let before = session.snapshot();
    assert_eq!(
        remember(&mut session, preset(), &call),
        Err(PermissionChangeError::Ineffective)
    );
    assert_eq!(session.snapshot(), before);
}

#[test]
fn per_2_explicit_capability_ask_cannot_hide_a_matching_deny_or_be_remembered() {
    let call = create("src/new.rs");
    let mut session = state("coding");
    let mut policy = ApprovalPolicy::new(
        CapabilitySet::new([ToolCapability::FileWrite]),
        CapabilitySet::default(),
    );
    assert_eq!(
        session.remember(session.snapshot().revision(), preset(), &call, &policy),
        Err(PermissionChangeError::Ineffective)
    );
    session
        .replace_rules(
            session.snapshot().revision(),
            vec![PermissionRule {
                source: PermissionRuleSource::Runtime,
                matcher: preset(),
                action: PermissionRuleAction::Deny,
            }],
        )
        .expect("deny");
    policy.use_snapshot(session.snapshot());
    assert_eq!(policy.decide(&call), PolicyDecision::Forbidden);
}

#[test]
fn per_3_file_change_preset_pins_definitions_and_excludes_control_paths() {
    let mut session = state("coding");
    remember(&mut session, preset(), &create("src/new.rs")).expect("grant");
    let edit = admitted(
        "native-edit",
        PermissionSubject::NativeFileChange(
            NativeFileChange::edit("src/existing.rs".into()).expect("path"),
        ),
    );
    assert_eq!(decision(&session, &edit), PolicyDecision::Allow);
    for path in [
        ".git/config",
        ".plexmaton/config.toml",
        ".agents/skills/x",
        "nested/.GIT/config",
        "AGENTS.md",
        "nested/agents.md",
        ".codex/config.toml",
    ] {
        assert_eq!(
            decision(&session, &create(path)),
            PolicyDecision::RequireApproval,
            "{path}"
        );
    }
    for call in [
        admitted("shell", PermissionSubject::Opaque),
        admitted("native-edit-lookalike", edit.permission_subject().clone()),
        admitted("native-create", PermissionSubject::Opaque),
    ] {
        assert_eq!(decision(&session, &call), PolicyDecision::RequireApproval);
    }
    let changed_definition = PermissionDefinition::new(
        ToolDefinitionId::new("native-create").expect("id"),
        ToolDefinitionRevision::new(2).expect("revision"),
    );
    assert!(!changed_definition.matches(&create("src/new.rs")));
}

#[test]
fn per_4_exact_commands_preserve_source_and_context_without_reading_detail() {
    let command = CommandPermission::new("git fetch origin".into(), [1; 32]).expect("command");
    let call = admitted(
        "shell",
        PermissionSubject::Command {
            command: command.clone(),
            syntax: CommandSyntax::ExactOnly(PrefixUnavailable::UnsupportedSyntax),
        },
    );
    let matcher = PermissionMatcher::ExactCommand {
        definition: definition("shell"),
        command,
    };
    let mut session = state("coding");
    remember(&mut session, matcher, &call).expect("exact grant");
    assert_eq!(decision(&session, &call), PolicyDecision::Allow);
    for (source, context) in [
        ("git fetch upstream", [1; 32]),
        ("git fetch origin; echo side-effect", [1; 32]),
        ("git fetch origin", [2; 32]),
        ("git fetch 'origin'", [1; 32]),
    ] {
        let changed = admitted(
            "shell",
            PermissionSubject::Command {
                command: CommandPermission::new(source.into(), context).expect("command"),
                syntax: CommandSyntax::ExactOnly(PrefixUnavailable::UnsupportedSyntax),
            },
        );
        assert_eq!(
            decision(&session, &changed),
            PolicyDecision::RequireApproval,
            "{source}"
        );
    }
}

#[test]
fn per_4_stale_foreign_and_full_mutations_preserve_current_authority() {
    let call = create("src/new.rs");
    let mut session = state("coding");
    let old = session.snapshot();
    let grant = remember(&mut session, preset(), &call).expect("grant");
    let current = session.snapshot();
    assert_eq!(
        session.revoke(old.revision(), &grant),
        Err(PermissionChangeError::StaleRevision)
    );
    assert_eq!(
        session.remember(
            state("other").snapshot().revision(),
            preset(),
            &call,
            &ApprovalPolicy::default()
        ),
        Err(PermissionChangeError::StaleRevision)
    );
    assert_eq!(session.snapshot(), current);
    for _ in 1..MAX_PERMISSION_ENTRIES {
        remember(&mut session, preset(), &call).expect("within bound");
    }
    let full = session.snapshot();
    assert_eq!(
        remember(&mut session, preset(), &call),
        Err(PermissionChangeError::Capacity)
    );
    assert_eq!(session.snapshot(), full);
}

#[test]
fn per_4_permission_wire_scopes_validate_the_same_constructors() {
    let matcher = PermissionMatcher::ExactCommand {
        definition: definition("command"),
        command: CommandPermission::new("git fetch".into(), [1; 32]).expect("command"),
    };
    let encoded = serde_json::to_value(&matcher).expect("encode matcher");
    assert_eq!(
        serde_json::from_value::<PermissionMatcher>(encoded.clone()).expect("decode matcher"),
        matcher
    );
    for source in [String::new(), "x".repeat(24 * 1024 + 1), "a\0b".into()] {
        let mut invalid = encoded.clone();
        invalid["command"]["source"] = source.into();
        assert!(serde_json::from_value::<PermissionMatcher>(invalid).is_err());
    }
    let mut invalid = encoded.clone();
    invalid["definition"]["revision"] = 0.into();
    assert!(serde_json::from_value::<PermissionMatcher>(invalid).is_err());
    let mut invalid = encoded;
    invalid["command"]["untrusted_extra"] = true.into();
    assert!(serde_json::from_value::<PermissionMatcher>(invalid).is_err());
}

#[test]
fn per_6_project_observations_invalidate_offers_and_unavailable_sources_never_allow() {
    use plexmaton_core::{ProjectPermissionRevision, ProjectPermissionStoreId};
    let mut session = state("project-session")
        .with_native_file_changes(definition("native-create"), definition("native-edit"));
    let call = create("src/new.rs");
    let revision = ProjectPermissionRevision::Present {
        store: ProjectPermissionStoreId::new("store-one").expect("store"),
        sequence: 1,
    };
    let grant = PermissionGrant {
        id: PermissionGrantId::new("project-grant").expect("grant"),
        matcher: preset(),
        origin: PermissionGrantOrigin::Approval,
    };
    let initial = session.snapshot().revision().clone();
    let project = ProjectPermissions::Ready {
        can_remember: true,
        configuration: None,
        trusted_config: None,
        revision,
        grants: vec![grant.clone()],
    };
    session
        .observe_project(project.clone())
        .expect("observation");
    assert_ne!(session.snapshot().revision(), &initial);
    assert!(session.snapshot().grants().is_empty());
    assert_eq!(session.snapshot().project_grants(), &[grant]);
    assert_eq!(decision(&session, &call), PolicyDecision::Allow);
    let current = session.snapshot();
    session
        .observe_project(project.clone())
        .expect("unchanged observation");
    assert_eq!(session.snapshot(), current);
    assert_eq!(
        current.remember_offer(&call).expect("offer").display.scopes,
        plexmaton_core::PermissionScopes::SessionAndProject
    );
    let mut exhausted = project;
    if let ProjectPermissions::Ready { can_remember, .. } = &mut exhausted {
        *can_remember = false;
    }
    session
        .observe_project(exhausted)
        .expect("capacity observation");
    assert_eq!(
        session
            .snapshot()
            .remember_offer(&call)
            .expect("Session still available")
            .display
            .scopes,
        plexmaton_core::PermissionScopes::Session
    );
    let current = session.snapshot();
    session
        .replace_rules(
            current.revision(),
            vec![PermissionRule {
                source: PermissionRuleSource::Runtime,
                matcher: preset(),
                action: PermissionRuleAction::Ask,
            }],
        )
        .expect("explicit ask");
    assert_eq!(decision(&session, &call), PolicyDecision::RequireApproval);
    assert!(session.snapshot().remember_offer(&call).is_none());
    session
        .observe_project(ProjectPermissions::Unavailable)
        .expect("invalid source");
    assert_eq!(decision(&session, &call), PolicyDecision::Unavailable);
    assert_eq!(
        decision(&session, &admitted("read", PermissionSubject::Opaque)),
        PolicyDecision::Unavailable
    );
    assert!(session.snapshot().remember_offer(&call).is_none());
}

#[path = "audit_tests.rs"]
mod audit_tests;
