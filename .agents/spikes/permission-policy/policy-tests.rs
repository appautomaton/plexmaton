use super::*;

fn call() -> Call {
    Call {
        id: CallId(1),
        conversation: ConversationId(1),
        session: CodingSessionId(1),
        head: HeadId(1),
        subject: Subject {
            workspace: WorkspaceId(1),
            definition: DefinitionId(1),
            definition_revision: 1,
            environment_revision: 1,
            arguments: ArgumentsId(1),
            action: Action::Write {
                path: "src/a.rs",
                area: FileArea::Ordinary,
            },
        },
    }
}
fn grant(call: Call) -> Grant {
    Grant {
        id: GrantId(1),
        scope: Scope::Session(call.session),
        matcher: Matcher::Exact(call.subject),
    }
}
fn record(policy: &mut PolicyLog, value: Record) {
    policy
        .commit(policy.revision(), value)
        .expect("valid fixture record");
}

#[test]
fn p1_rewind_and_project_reload_do_not_resurrect_a_revoked_grant() {
    let mut policy = PolicyLog::default();
    let original = call();
    record(
        &mut policy,
        Record::Granted(Grant {
            scope: Scope::Workspace,
            ..grant(original)
        }),
    );
    let obsolete_authority = policy.clone();
    record(&mut policy, Record::Revoked(GrantId(1)));
    // Counterexample: restoring authority at the old conversation position restores permission.
    assert!(matches!(
        obsolete_authority.evaluate(original),
        Verdict::Allow(_)
    ));
    let reloaded = PolicyLog {
        records: policy.records.clone(),
    };
    for head in [HeadId(0), HeadId(1), HeadId(2)] {
        let selected = Call { head, ..original };
        assert_eq!(reloaded.evaluate(selected), Verdict::Ask);
    }
    assert_eq!(policy.records, reloaded.records);
}

#[test]
fn p1_regrant_needs_a_new_identity_and_retains_revocation_history() {
    let mut policy = PolicyLog::default();
    record(&mut policy, Record::Granted(grant(call())));
    record(&mut policy, Record::Revoked(GrantId(1)));
    let before = policy.clone();
    assert_eq!(
        policy.commit(2, Record::Granted(grant(call()))),
        Err(Error::InvalidRecord)
    );
    assert_eq!(policy, before);
    record(
        &mut policy,
        Record::Granted(Grant {
            id: GrantId(2),
            ..grant(call())
        }),
    );
    assert_eq!(
        policy.evaluate(call()),
        Verdict::Allow(Authority::Grant(GrantId(2)))
    );
    assert_eq!(&policy.records[..2], before.records);
}

#[test]
fn p2_session_grants_follow_new_and_resumed_conversations_but_not_other_roots() {
    let mut policy = PolicyLog::default();
    record(&mut policy, Record::Granted(grant(call())));
    assert!(matches!(
        policy.evaluate(Call {
            head: HeadId(2),
            ..call()
        }),
        Verdict::Allow(_)
    ));
    // /new chooses another conversation; resume can return to the original. Neither ends the
    // coding session or reconstructs its permission state from conversation history (P1/P2).
    for conversation in [ConversationId(2), ConversationId(1)] {
        assert_eq!(
            policy.evaluate(Call {
                id: CallId(2),
                conversation,
                ..call()
            }),
            Verdict::Allow(Authority::Grant(GrantId(1)))
        );
    }
    let other_root = Call {
        subject: Subject {
            workspace: WorkspaceId(2),
            ..call().subject
        },
        ..call()
    };
    assert_eq!(policy.evaluate(other_root), Verdict::Ask);
    record(
        &mut policy,
        Record::Granted(Grant {
            id: GrantId(2),
            scope: Scope::Workspace,
            ..grant(call())
        }),
    );
    assert!(matches!(
        policy.evaluate(Call {
            conversation: ConversationId(2),
            session: CodingSessionId(2),
            ..call()
        }),
        Verdict::Allow(_)
    ));
    assert_eq!(policy.evaluate(other_root), Verdict::Ask);
}

// P2: a new Plexmaton launch has fresh temporary authority even for the same saved conversation.
#[test]
fn p2_restart_clears_session_grants_while_project_grants_survive() {
    let mut policy = PolicyLog::default();
    record(&mut policy, Record::Granted(grant(call())));
    let reopened = Call {
        session: CodingSessionId(2),
        ..call()
    };
    assert_eq!(reopened.conversation, call().conversation);
    assert!(matches!(policy.evaluate(call()), Verdict::Allow(_)));
    assert_eq!(policy.evaluate(reopened), Verdict::Ask);
    record(
        &mut policy,
        Record::Granted(Grant {
            id: GrantId(2),
            scope: Scope::Workspace,
            ..grant(call())
        }),
    );
    assert_eq!(
        policy.evaluate(reopened),
        Verdict::Allow(Authority::Grant(GrantId(2)))
    );
}

#[test]
fn p3_deny_then_explicit_ask_dominate_allow_regardless_of_rule_order() {
    for effects in [
        [RuleEffect::Allow, RuleEffect::Ask, RuleEffect::Deny],
        [RuleEffect::Deny, RuleEffect::Allow, RuleEffect::Ask],
    ] {
        let mut policy = PolicyLog::default();
        record(&mut policy, Record::Granted(grant(call())));
        for (id, effect) in effects.into_iter().enumerate() {
            record(
                &mut policy,
                Record::RuleAdded(Rule {
                    id: RuleId(id as u8),
                    matcher: Matcher::Exact(call().subject),
                    effect,
                }),
            );
        }
        assert!(matches!(policy.evaluate(call()), Verdict::Deny(_)));
        let mut pending = Pending::new(call(), &policy);
        assert_eq!(
            pending.resolve(call(), &mut policy, Choice::Once),
            Err(Error::Forbidden)
        );
    }
}

#[test]
fn p3_a_mandatory_ask_offers_once_but_no_ineffective_remember_choice() {
    let mut policy = PolicyLog::default();
    record(&mut policy, Record::Granted(grant(call())));
    record(
        &mut policy,
        Record::RuleAdded(Rule {
            id: RuleId(1),
            matcher: Matcher::Exact(call().subject),
            effect: RuleEffect::Ask,
        }),
    );
    let mut pending = Pending::new(call(), &policy);
    assert_eq!(
        pending.resolve(
            call(),
            &mut policy,
            Choice::Remember(Grant {
                id: GrantId(2),
                ..grant(call())
            })
        ),
        Err(Error::IneffectiveGrant)
    );
    assert_eq!(policy.records.len(), 2);
    let permit = pending
        .resolve(call(), &mut policy, Choice::Once)
        .expect("once still available")
        .expect("permit");
    assert_eq!(
        permit.dispatch(call(), &policy, ExecutorCheck::Valid),
        Ok(())
    );
    assert!(matches!(policy.evaluate(call()), Verdict::MandatoryAsk(_)));
}

#[test]
fn p4_once_cannot_approve_another_call_or_be_resolved_twice() {
    let mut policy = PolicyLog::default();
    let mut pending = Pending::new(call(), &policy);
    let changed = Call {
        id: CallId(2),
        ..call()
    };
    for invalid in [
        changed,
        Call {
            session: CodingSessionId(2),
            ..call()
        },
        Call {
            conversation: ConversationId(2),
            ..call()
        },
        Call {
            head: HeadId(2),
            ..call()
        },
        Call {
            subject: Subject {
                environment_revision: 2,
                ..call().subject
            },
            ..call()
        },
        Call {
            subject: Subject {
                definition_revision: 2,
                ..call().subject
            },
            ..call()
        },
        Call {
            subject: Subject {
                action: Action::Write {
                    path: "src/b.rs",
                    area: FileArea::Ordinary,
                },
                ..call().subject
            },
            ..call()
        },
    ] {
        assert_eq!(
            pending.resolve(invalid, &mut policy, Choice::Once),
            Err(Error::WrongCall)
        );
    }
    let permit = pending
        .resolve(call(), &mut policy, Choice::Once)
        .expect("approve")
        .expect("permit");
    assert_eq!(
        pending.resolve(call(), &mut policy, Choice::Once),
        Err(Error::Settled)
    );
    assert_eq!(
        permit.dispatch(changed, &policy, ExecutorCheck::Valid),
        Err(Error::WrongCall)
    );
    assert!(policy.records.is_empty());
}

#[test]
fn p4_cancellation_and_denial_cannot_mint_a_grant() {
    let mut policy = PolicyLog::default();
    let mut pending = Pending::new(call(), &policy);
    pending.state = PendingState::Cancelled;
    assert_eq!(
        pending.resolve(call(), &mut policy, Choice::Remember(grant(call()))),
        Err(Error::Settled)
    );
    let mut pending = Pending::new(call(), &policy);
    assert_eq!(pending.resolve(call(), &mut policy, Choice::Deny), Ok(None));
    assert_eq!(
        pending.resolve(call(), &mut policy, Choice::Once),
        Err(Error::Settled)
    );
    assert!(policy.records.is_empty());
}

// P4 / APV-1 / APV-3: retaining the same path or revision number cannot identify an operation.
#[test]
fn p4_pending_and_dispatch_bind_full_arguments_and_definition_identity() {
    for subject in [
        Subject {
            arguments: ArgumentsId(2),
            ..call().subject
        },
        Subject {
            definition: DefinitionId(2),
            ..call().subject
        },
    ] {
        let changed = Call { subject, ..call() };
        let mut policy = PolicyLog::default();
        let mut pending = Pending::new(call(), &policy);
        assert_eq!(
            pending.resolve(changed, &mut policy, Choice::Once),
            Err(Error::WrongCall)
        );
        let permit = pending
            .resolve(call(), &mut policy, Choice::Once)
            .expect("the unchanged pending call remains approvable")
            .expect("one-call permit");
        assert_eq!(
            permit.dispatch(changed, &policy, ExecutorCheck::Valid),
            Err(Error::WrongCall)
        );
        assert!(policy.records.is_empty());
    }
}

// P6 / APV-2: exact and workspace scopes intentionally differ on argument identity.
#[test]
fn p6_exact_grants_bind_arguments_while_workspace_edits_allow_new_content() {
    let changed = Call {
        subject: Subject {
            arguments: ArgumentsId(2),
            ..call().subject
        },
        ..call()
    };
    let mut exact = PolicyLog::default();
    record(&mut exact, Record::Granted(grant(call())));
    assert_eq!(exact.evaluate(changed), Verdict::Ask);

    let mut edits = PolicyLog::default();
    record(
        &mut edits,
        Record::Granted(Grant {
            matcher: Matcher::WorkspaceEdits {
                workspace: call().subject.workspace,
                definition: call().subject.definition,
                definition_revision: call().subject.definition_revision,
            },
            ..grant(call())
        }),
    );
    assert_eq!(
        edits.evaluate(changed),
        Verdict::Allow(Authority::Grant(GrantId(1)))
    );
    let other_definition = Call {
        subject: Subject {
            definition: DefinitionId(2),
            ..call().subject
        },
        ..call()
    };
    assert_eq!(edits.evaluate(other_definition), Verdict::Ask);
    assert_eq!(exact.evaluate(other_definition), Verdict::Ask);
}

#[test]
fn p4_policy_changes_invalidate_both_pending_answers_and_unspent_permits() {
    let mut policy = PolicyLog::default();
    let mut old_prompt = Pending::new(call(), &policy);
    let mut current_prompt = Pending::new(call(), &policy);
    let permit = current_prompt
        .resolve(call(), &mut policy, Choice::Remember(grant(call())))
        .expect("grant commit")
        .expect("permit");
    record(&mut policy, Record::Revoked(GrantId(1)));
    assert_eq!(
        old_prompt.resolve(call(), &mut policy, Choice::Once),
        Err(Error::StalePolicy)
    );
    assert_eq!(
        permit.dispatch(call(), &policy, ExecutorCheck::Valid),
        Err(Error::StalePolicy)
    );
}

#[test]
fn p5_uncommitted_and_stale_policy_writes_change_no_authority() {
    let mut policy = PolicyLog::default();
    let draft = Record::Granted(grant(call()));
    assert_eq!(policy.evaluate(call()), Verdict::Ask);
    assert_eq!(policy.commit(1, draft), Err(Error::StalePolicy));
    assert!(policy.records.is_empty());
    policy.commit(0, draft).expect("acknowledged record");
    assert!(matches!(policy.evaluate(call()), Verdict::Allow(_)));
}

#[test]
fn p5_policy_log_bound_rejects_without_partial_mutation() {
    let mut policy = PolicyLog::default();
    for id in 0..PolicyLog::LIMIT {
        record(
            &mut policy,
            Record::Granted(Grant {
                id: GrantId(id as u8),
                ..grant(call())
            }),
        );
    }
    let before = policy.clone();
    assert_eq!(
        policy.commit(PolicyLog::LIMIT, Record::Revoked(GrantId(1))),
        Err(Error::Limit)
    );
    assert_eq!(policy, before);
}

#[test]
fn p6_exact_shell_grants_bind_the_whole_script_environment_and_definition() {
    let original = Call {
        subject: Subject {
            action: Action::Shell {
                script: "cargo test",
            },
            ..call().subject
        },
        ..call()
    };
    let mut policy = PolicyLog::default();
    record(&mut policy, Record::Granted(grant(original)));
    assert!(matches!(policy.evaluate(original), Verdict::Allow(_)));
    for subject in [
        Subject {
            action: Action::Shell {
                script: "cargo test && publish",
            },
            ..original.subject
        },
        Subject {
            action: Action::Shell {
                script: "env TARGET=other cargo test",
            },
            ..original.subject
        },
        Subject {
            environment_revision: 2,
            ..original.subject
        },
        Subject {
            definition_revision: 2,
            ..original.subject
        },
    ] {
        assert_eq!(
            policy.evaluate(Call {
                subject,
                ..original
            }),
            Verdict::Ask
        );
    }
}

#[test]
fn p6_workspace_edit_grants_preserve_control_path_and_executor_checks() {
    let mut policy = PolicyLog::default();
    let matcher = Matcher::WorkspaceEdits {
        workspace: WorkspaceId(1),
        definition: DefinitionId(1),
        definition_revision: 1,
    };
    record(
        &mut policy,
        Record::Granted(Grant {
            matcher,
            ..grant(call())
        }),
    );
    assert!(matches!(policy.evaluate(call()), Verdict::Allow(_)));
    let control = Call {
        subject: Subject {
            action: Action::Write {
                path: ".git/config",
                area: FileArea::Control,
            },
            ..call().subject
        },
        ..call()
    };
    assert_eq!(policy.evaluate(control), Verdict::Ask);
    let mut pending = Pending::new(call(), &policy);
    let permit = pending
        .resolve(call(), &mut policy, Choice::Once)
        .expect("approve")
        .expect("permit");
    assert_eq!(
        permit.dispatch(call(), &policy, ExecutorCheck::StaleObservation),
        Err(Error::InvalidAtExecution)
    );
    assert_eq!(
        policy.evaluate(Call {
            subject: Subject {
                action: Action::Read,
                ..call().subject
            },
            ..call()
        }),
        Verdict::Allow(Authority::ReadDefault)
    );
}
