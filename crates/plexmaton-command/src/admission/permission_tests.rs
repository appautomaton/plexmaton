use super::tests::{TestDirectory, request};
use super::*;
use plexmaton_agent::{
    ApprovalPolicy, PermissionRule, PermissionRuleAction, PermissionRuleSource, PolicyDecision,
    SessionPermissions,
};
use plexmaton_core::CodingSessionId;

fn admit(tool: &CommandTool, source: &str) -> AdmittedToolCall {
    let AdmissionOutcome::Admitted(call) = tool.admit(request(
        COMMAND_TOOL_NAME,
        serde_json::json!({"cmd": source}).to_string(),
    )) else {
        panic!("admitted command");
    };
    call
}

fn prefix(tool: &CommandTool, words: &[&str]) -> plexmaton_agent::PermissionMatcher {
    tool.prefix_permission(words.iter().map(|word| (*word).to_owned()).collect())
        .expect("prefix")
}

/// PER-10/PER-4: the real catalog's prefix covers every command and no discarded context token.
#[test]
fn per_10_catalog_prefix_matches_whole_literal_calls_and_never_peels_wrappers() {
    let directory = TestDirectory::new();
    let tool = CommandTool::new(&directory.0, "TEST_KEY").expect("tool");
    let fetch = prefix(&tool, &["git", "fetch"]);
    for source in [
        "git fetch origin",
        "git fetch 'team origin'",
        "g'i't f\"etch\" origin",
        "git fetch a && git fetch b",
        "git fetch a; git fetch b",
    ] {
        assert!(fetch.matches(&admit(&tool, source)), "must cover {source}");
    }
    for source in [
        "git fetchx origin",
        "git status",
        "git fetch a && git status",
        "git -C ../other fetch",
        "env VAR=value git fetch",
        "sh -c 'git fetch'",
        "git fetch > file",
        "git fetch $(other)",
        "git fetch a | git fetch b",
        "git fetch foo\\\nbar",
    ] {
        assert!(
            !fetch.matches(&admit(&tool, source)),
            "must ask for {source}"
        );
    }
    let narrowed = prefix(&tool, &["git", "fetch", "team origin"]);
    assert!(narrowed.matches(&admit(&tool, "git fetch team\\ origin main")));
    assert!(!narrowed.matches(&admit(&tool, "git fetch team origin main")));
    assert!(!narrowed.matches(&admit(&tool, "git fetch 'team originx'")));
    let other = TestDirectory::new();
    let other_tool = CommandTool::new(&other.0, "TEST_KEY").expect("different root");
    assert!(!fetch.matches(&admit(&other_tool, "git fetch origin")));
    let other_environment = CommandTool::open(
        &directory.0,
        CommandEnvironment::from_pairs([("PATH".into(), "/different/bin".into())]),
    )
    .expect("different environment");
    assert!(!fetch.matches(&admit(&other_environment, "git fetch origin")));
}

/// PER-10/PER-2: a restrictive prefix on one sibling precedes an exact Allow for the whole script.
#[test]
fn per_10_prefix_ask_and_deny_cover_any_literal_sibling_before_exact_allow() {
    let directory = TestDirectory::new();
    let tool = CommandTool::new(&directory.0, "TEST_KEY").expect("tool");
    let source = "git fetch origin && git status";
    let call = admit(&tool, source);
    for (action, expected) in [
        (PermissionRuleAction::Deny, PolicyDecision::Forbidden),
        (PermissionRuleAction::Ask, PolicyDecision::RequireApproval),
    ] {
        for reverse in [false, true] {
            let mut rules = vec![
                PermissionRule {
                    source: PermissionRuleSource::UserConfiguration {
                        fingerprint: [1; 32],
                        index: 0,
                    },
                    action: PermissionRuleAction::Allow,
                    matcher: tool.exact_permission(source).expect("exact"),
                },
                PermissionRule {
                    source: PermissionRuleSource::UserConfiguration {
                        fingerprint: [1; 32],
                        index: 1,
                    },
                    action,
                    matcher: prefix(&tool, &["git", "status"]),
                },
            ];
            if reverse {
                rules.reverse();
            }
            let mut owner =
                SessionPermissions::new(CodingSessionId::new("prefix-rules").expect("Session"));
            owner
                .replace_rules(owner.snapshot().revision(), rules)
                .expect("rules");
            let mut policy = ApprovalPolicy::default();
            policy.use_snapshot(owner.snapshot());
            assert_eq!(policy.decide(&call), expected);
            assert_eq!(
                owner.remember(
                    owner.snapshot().revision(),
                    tool.exact_permission(source).expect("exact"),
                    &call,
                    &policy
                ),
                Err(plexmaton_agent::PermissionChangeError::Ineffective)
            );
        }
    }
}

/// PER-10/PER-9: persisted token scopes retain empty/quoted arguments and enforce decode bounds.
#[test]
fn per_10_prefix_wire_roundtrip_preserves_tokens_and_refuses_invalid_scopes() {
    let directory = TestDirectory::new();
    let tool = CommandTool::new(&directory.0, "TEST_KEY").expect("tool");
    let scope = prefix(&tool, &["git", "fetch", "team origin", ""]);
    let wire = serde_json::to_value(&scope).expect("encode");
    let restored: plexmaton_agent::PermissionMatcher =
        serde_json::from_value(wire.clone()).expect("scope");
    assert_eq!(restored, scope);
    assert!(restored.matches(&admit(&tool, "git fetch 'team origin' '' main")));
    for invalid in [
        serde_json::json!([]),
        serde_json::json!([""]),
        serde_json::json!(["git", "bad\0arg"]),
        serde_json::json!(vec!["git"; 33]),
        serde_json::json!(["x".repeat(4097)]),
    ] {
        let mut changed = wire.clone();
        changed["prefix"]["arguments"] = invalid;
        assert!(serde_json::from_value::<plexmaton_agent::PermissionMatcher>(changed).is_err());
    }
}

/// PER-10/CMD-1: cancellation observed during prefix work refuses admission rather than publishing scope.
#[test]
fn per_10_cancelled_prefix_admission_publishes_no_permission_subject() {
    for cancel_on in [0, 1] {
        let directory = TestDirectory::new();
        let tool = CommandTool::new(&directory.0, "TEST_KEY").expect("tool");
        let polls = std::cell::Cell::new(0);
        let result = tool.admit_with_cancellation(
            request(
                COMMAND_TOOL_NAME,
                serde_json::json!({"cmd":"git fetch origin"}).to_string(),
            ),
            &|| {
                let current = polls.get();
                polls.set(current + 1);
                current >= cancel_on
            },
        );
        assert!(matches!(
            result,
            AdmissionOutcome::Refused {
                reason: AdmissionRefusal::Cancelled,
                ..
            }
        ));
    }
}
