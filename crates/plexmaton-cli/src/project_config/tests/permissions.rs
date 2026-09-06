use super::*;
use plexmaton_agent::{
    AdmissionOutcome, AdmittedToolCall, Agent, ApprovalPolicy, Effect, Input, ModelEvent,
    ModelOutputPosition, PolicyDecision, StopReason, ToolCall, UnixMillis,
};
use plexmaton_core::{AgentId, PermissionAction, PermissionIntent, ToolCallId};
use plexmaton_runtime::{CodingSessionPermissions, NativeToolCatalog};
use std::sync::Arc;

const ALLOW: &str =
    "[[permissions.rules]]\naction = 'allow'\nmatch = { kind = 'native_file_changes' }\n";

fn catalog(root: &Path) -> NativeToolCatalog {
    NativeToolCatalog::open(root, "TEST_KEY", "/bin/false", "/bin/false", Vec::new())
        .expect("catalog")
}

fn owner(root: &Path, home: &Path) -> CodingSessionPermissions {
    let tools = catalog(root);
    CodingSessionPermissions::new(&tools)
        .with_project_store(
            plexmaton_permission_store::ProjectPermissionStore::open(home, root).expect("store"),
        )
        .expect("valid source")
        .with_project_configuration(Arc::new(ProjectPermissionReader::new(
            root.to_owned(),
            tools.permission_compiler(),
        )))
        .expect("valid project configuration")
}

fn native_create(root: &Path) -> AdmittedToolCall {
    let mut agent = Agent::new(AgentId::new("config-test").expect("id"));
    agent.handle_at(
        Input::Submitted {
            text: "create".to_owned(),
        },
        UnixMillis::EPOCH,
    );
    let step_id = agent.active_model_step().expect("step");
    agent.handle_at(
        Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: ToolCallId::new("create").expect("id"),
                    name: "create_file".to_owned(),
                    arguments: r#"{"path":"new.txt","content":"hi"}"#.to_owned(),
                },
            },
        },
        UnixMillis::EPOCH,
    );
    let effects = agent
        .handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            UnixMillis::EPOCH,
        )
        .effects;
    let request = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::AdmitTool(request) => Some(request),
            _ => None,
        })
        .expect("admission request");
    let files =
        plexmaton_file_tools::FileTools::open(root, "/bin/false", "/bin/false").expect("files");
    let AdmissionOutcome::Admitted(call) = files.admit(request, &FileCancellation::new()) else {
        panic!("native admission");
    };
    call
}

fn decision(owner: &CodingSessionPermissions, call: &AdmittedToolCall) -> PolicyDecision {
    let mut policy = ApprovalPolicy::default();
    policy.use_snapshot(owner.snapshot().expect("snapshot"));
    policy.decide(call)
}

fn trust(owner: &CodingSessionPermissions) -> PermissionIntent {
    let view = owner.snapshot().expect("snapshot").control_view();
    PermissionIntent {
        expected: view.revision,
        action: PermissionAction::TrustProjectConfiguration(
            view.configuration.expect("configuration").fingerprint,
        ),
    }
}

/// PER-8/PGR-5: real readers, catalog scopes and personal storage compose; model loading confers no trust.
#[test]
fn per_8_project_allow_requires_exact_personal_trust_and_edits_invalidate_the_review() {
    let root = TempDirectory::new();
    let home = TempDirectory::new();
    root.write(
        ".plexmaton/config.toml",
        format!("active_model = {{ provider = 'project', model = 'chosen' }}\n{ALLOW}"),
    );
    assert_eq!(
        load(root.path())
            .expect("read")
            .select_model(&registry())
            .expect("model")
            .model_name(),
        "chosen"
    );
    let first = owner(root.path(), home.path());
    let call = native_create(root.path());
    assert_eq!(decision(&first, &call), PolicyDecision::RequireApproval);
    let intent = trust(&first);
    let before = first.snapshot().expect("snapshot").revision().clone();
    first.refresh(&|| false).expect("unchanged read");
    assert_eq!(first.snapshot().expect("snapshot").revision(), &before);
    first
        .apply_control(&intent, &|| false)
        .expect("explicit activation");
    assert_eq!(decision(&first, &call), PolicyDecision::Allow);
    assert!(
        !home.path().join("sessions").exists(),
        "trust is not Conversation history"
    );
    drop(first);
    let restarted = owner(root.path(), home.path());
    assert_eq!(decision(&restarted, &call), PolicyDecision::Allow);

    let stale = trust(&restarted);
    root.write(".plexmaton/config.toml", format!("# edited bytes\n{ALLOW}"));
    assert_eq!(
        restarted.apply_control(&stale, &|| false),
        Err(PermissionChangeError::StaleRevision)
    );
    assert_eq!(decision(&restarted, &call), PolicyDecision::RequireApproval);
    restarted
        .apply_control(&trust(&restarted), &|| false)
        .expect("review new bytes");
    assert_eq!(decision(&restarted, &call), PolicyDecision::Allow);
    fs::remove_file(root.path().join(".plexmaton/config.toml")).expect("remove project config");
    restarted.refresh(&|| false).expect("absence");
    assert_eq!(decision(&restarted, &call), PolicyDecision::RequireApproval);
    let view = restarted.snapshot().expect("snapshot").control_view();
    assert!(view.configuration.is_none() && view.trusted_config.is_some());
    restarted
        .apply_control(
            &PermissionIntent {
                expected: view.revision,
                action: PermissionAction::RevokeProjectTrust,
            },
            &|| false,
        )
        .expect("withdraw absent-file trust");
    root.write(".plexmaton/config.toml", format!("# edited bytes\n{ALLOW}"));
    assert_eq!(
        decision(&owner(root.path(), home.path()), &call),
        PolicyDecision::RequireApproval
    );
}

/// PER-2/PER-8: an untrusted project may restrict user Allow; no invalid suffix becomes an empty source.
#[test]
fn per_8_untrusted_project_ask_and_deny_precede_user_allow_and_bad_sources_refuse() {
    let root = TempDirectory::new();
    let home = TempDirectory::new();
    root.write(".plexmaton/config.toml", "");
    let tools = catalog(root.path());
    let config =
        crate::user_config::parse(&format!("{REGISTRY}\n{ALLOW}")).expect("single user parse");
    let rules = config
        .permissions
        .compile(&tools.permission_compiler(), |index| {
            PermissionRuleSource::UserConfiguration {
                fingerprint: config.fingerprint,
                index,
            }
        })
        .expect("rules");
    let current = owner(root.path(), home.path())
        .with_user_rules(rules)
        .expect("user rules");
    let call = native_create(root.path());
    assert_eq!(decision(&current, &call), PolicyDecision::Allow);
    for (action, expected) in [
        ("ask", PolicyDecision::RequireApproval),
        ("deny", PolicyDecision::Forbidden),
    ] {
        root.write(
            ".plexmaton/config.toml",
            ALLOW.replace("'allow'", &format!("'{action}'")),
        );
        current
            .refresh(&|| false)
            .expect("restriction needs no trust");
        assert_eq!(decision(&current, &call), expected);
    }
    root.write(
        ".plexmaton/config.toml",
        format!("{ALLOW}\n[[permissions.rules]]\naction = 'bad'\n"),
    );
    assert_eq!(
        current.refresh(&|| false),
        Err(PermissionChangeError::Unavailable)
    );
    assert_eq!(decision(&current, &call), PolicyDecision::Unavailable);
    root.write(".plexmaton/config.toml", ALLOW);
    current.refresh(&|| false).expect("repaired source");
    assert_eq!(
        decision(&current, &call),
        PolicyDecision::Allow,
        "independent user Allow remains effective"
    );
}

/// PER-8/PER-4: bounded strict declarations cannot inject stored matchers or relax command admission.
#[test]
fn per_8_rule_grammar_is_strict_bounded_and_compiles_complete_sources() {
    let root = TempDirectory::new();
    let tools = catalog(root.path());
    for bad in [
        "[[permissions.rules]]\naction='allow'\nmatch={kind='all_tools'}",
        "[[permissions.rules]]\naction='allow'\nmatch={kind='native_file_changes',create='forged'}",
        "[[permissions.rules]]\naction='allow'\ntrust=true\nmatch={kind='native_file_changes'}",
        "[permissions]\ntrusted=true",
    ] {
        root.write(".plexmaton/config.toml", bad);
        assert!(
            matches!(
                load(root.path()),
                Err(ProjectConfigError::InvalidConfiguration)
            ),
            "accepted forbidden rule: {bad}"
        );
    }
    root.write(".plexmaton/config.toml", ALLOW.repeat(129));
    assert!(matches!(
        load(root.path()),
        Err(ProjectConfigError::InvalidConfiguration)
    ));
    root.write(".plexmaton/config.toml", ALLOW.repeat(128));
    assert_eq!(
        load(root.path())
            .expect("max count")
            .permissions(&tools.permission_compiler())
            .expect("compile")
            .expect("present")
            .rules()
            .len(),
        128
    );
    for source in ["", " ", "\\u0000"] {
        root.write(".plexmaton/config.toml", format!("[[permissions.rules]]\naction='allow'\nmatch={{kind='exact_command',source=\"{source}\"}}"));
        assert_eq!(
            load(root.path())
                .expect("syntax")
                .permissions(&tools.permission_compiler()),
            Err(PermissionChangeError::Unavailable)
        );
    }
    root.write(".plexmaton/config.toml", "[[permissions.rules]]\naction='ask'\nmatch={kind='exact_command',source=\"git fetch 'two words'\"}");
    let configured = load(root.path())
        .expect("syntax")
        .permissions(&tools.permission_compiler())
        .expect("compile")
        .expect("present");
    assert_eq!(
        configured.rules()[0].matcher,
        tools
            .permission_compiler()
            .exact_command("git fetch 'two words'")
            .expect("exact scope")
    );
}

/// PER-10/PER-8: configuration preserves explicit argument vectors and cannot forge context bindings.
#[test]
fn per_10_configured_prefix_tokens_compile_with_catalog_context_and_strict_bounds() {
    let root = TempDirectory::new();
    let tools = catalog(root.path());
    root.write(".plexmaton/config.toml", "[[permissions.rules]]\naction='ask'\nmatch={kind='command_prefix',arguments=['git','fetch','team origin','']}");
    let configuration = load(root.path())
        .expect("declaration")
        .permissions(&tools.permission_compiler())
        .expect("compile")
        .expect("present");
    assert_eq!(
        configuration.rules()[0].matcher,
        tools
            .permission_compiler()
            .command_prefix(vec![
                "git".into(),
                "fetch".into(),
                "team origin".into(),
                "".into()
            ])
            .expect("scope")
    );
    for arguments in [
        vec![],
        vec!["".to_owned()],
        vec!["x".repeat(4097)],
        vec!["git".to_owned(); 33],
    ] {
        let words = serde_json::to_string(&arguments).expect("tokens");
        root.write(".plexmaton/config.toml", format!("[[permissions.rules]]\naction='allow'\nmatch={{kind='command_prefix',arguments={words}}}"));
        assert_eq!(
            load(root.path())
                .expect("syntax")
                .permissions(&tools.permission_compiler()),
            Err(PermissionChangeError::Unavailable)
        );
    }
    for extra in ["source='git fetch'", "context=[0,0]", "definition='forged'"] {
        root.write(".plexmaton/config.toml", format!("[[permissions.rules]]\naction='allow'\nmatch={{kind='command_prefix',arguments=['git','fetch'],{extra}}}"));
        assert!(load(root.path()).is_err());
    }
}
