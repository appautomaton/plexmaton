mod inclusion;
use plexmaton_core::{
    AgentId, ArtifactId, CollaborationId, CollaborationItemId, ConversationId, DelegationId, MailId,
};

use super::*;

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        conversation: ConversationId::new(format!("conversation-{name}"))
            .expect("fixture identity"),
        agent: AgentId::new(name).expect("fixture identity"),
    }
}

fn text(value: &str) -> CollaborationText {
    CollaborationText::new(value).expect("fixture text")
}
fn item(value: &str) -> CollaborationItemId {
    CollaborationItemId::new(value).expect("fixture identity")
}
fn delegation() -> DelegationId {
    DelegationId::new("task").expect("fixture identity")
}

fn creation() -> CollaborationEvent {
    CollaborationEvent::DelegationCreated {
        delegation: delegation(),
        delegator: endpoint("a"),
        worker: endpoint("b"),
        task: text("Inspect files"),
    }
}

fn mail(name: &str) -> CollaborationEvent {
    CollaborationEvent::MailAccepted {
        mail: MailEnvelope {
            id: MailId::new(name).expect("fixture identity"),
            from: endpoint("b"),
            to: endpoint("a"),
            summary: text("Found two relevant files"),
            artifacts: Vec::new(),
        },
    }
}

fn amendment(author: DelegationAuthor, expected: u64, task: &str) -> CollaborationEvent {
    CollaborationEvent::TaskAmended {
        delegation: delegation(),
        expected: DelegationRevision(expected),
        author,
        task: text(task),
    }
}

fn ledger(limits: CollaborationLimits) -> CollaborationLedger {
    let mut ledger = CollaborationLedger::new(
        CollaborationId::new("collaboration").expect("fixture identity"),
        limits,
    )
    .expect("valid limits");
    accept(&mut ledger, "create", creation());
    ledger
}

fn accept(ledger: &mut CollaborationLedger, id: &str, event: CollaborationEvent) -> ItemReceipt {
    match ledger.prepare(item(id), event).expect("fixture admission") {
        Preparation::Existing(receipt) => receipt,
        Preparation::Append(record) => ledger.apply(*record).expect("validated fixture"),
    }
}

fn refuses(ledger: &mut CollaborationLedger, event: CollaborationEvent, error: CollaborationError) {
    let before = ledger.clone();
    assert_eq!(ledger.prepare(item("refused"), event), Err(error));
    assert_eq!(*ledger, before, "refusal must not mutate any projection");
}

/// COL-1: retry reconciles the original event before revision/capacity checks; replay is equal.
#[test]
fn col_1_exact_retry_and_replay_preserve_original_admission() {
    let mut ledger = ledger(CollaborationLimits {
        items: 2,
        control_items: 0,
        ..CollaborationLimits::default()
    });
    let event = amendment(DelegationAuthor::User, 0, "Only inspect tests");
    let receipt = accept(&mut ledger, "amend", event.clone());
    assert_eq!(
        ledger.prepare(item("amend"), event),
        Ok(Preparation::Existing(receipt))
    );
    assert_eq!(
        ledger.prepare(
            item("amend"),
            amendment(DelegationAuthor::User, 0, "Change files")
        ),
        Err(CollaborationError::ItemIdentityConflict)
    );
    refuses(&mut ledger, mail("new"), CollaborationError::ItemCapacity);
    let mut replay =
        CollaborationLedger::new(ledger.id().clone(), ledger.limits()).expect("valid limits");
    for record in ledger.records() {
        let encoded = serde_json::to_vec(record).expect("serialize");
        replay
            .apply(serde_json::from_slice(&encoded).expect("decode"))
            .expect("replay");
    }
    assert_eq!(replay, ledger);
    let mut duplicate = ledger.records()[0].clone();
    duplicate.id = item("sequence-gap");
    duplicate.sequence = CollaborationSequence(4);
    assert_eq!(
        replay.apply(duplicate),
        Err(CollaborationError::UnexpectedSequence)
    );
    assert_eq!(replay, ledger);
}

/// COL-2: a mail flood cannot spend reserved control slots; retries still work at full capacity.
#[test]
fn col_2_mail_saturation_preserves_control_admission() {
    let mut ledger = ledger(CollaborationLimits {
        items: 3,
        control_items: 1,
        ..CollaborationLimits::default()
    });
    let event = mail("first");
    let receipt = accept(&mut ledger, "mail", event.clone());
    refuses(
        &mut ledger,
        mail("second"),
        CollaborationError::ItemCapacity,
    );
    accept(
        &mut ledger,
        "user",
        amendment(DelegationAuthor::User, 0, "Stop the search at tests"),
    );
    assert_eq!(
        ledger.prepare(item("mail"), event),
        Ok(Preparation::Existing(receipt))
    );
    assert_eq!(ledger.mail_for(&endpoint("a")).count(), 1);
    assert_eq!(
        ledger
            .delegation(&delegation())
            .expect("task exists")
            .task
            .as_str(),
        "Stop the search at tests"
    );
}

/// COL-2: UTF-8 bytes and decoded values enforce the same ceiling; identity/pointer limits fail closed.
#[test]
fn col_2_payload_boundaries_and_retained_bytes_are_enforced() {
    let exact = "中".repeat(MAX_COLLABORATION_TEXT_BYTES / 3) + "ab";
    assert_eq!(exact.len(), MAX_COLLABORATION_TEXT_BYTES);
    let value = CollaborationText::new(exact.clone()).expect("exact boundary");
    assert_eq!(
        serde_json::from_str::<CollaborationText>(&serde_json::to_string(&value).expect("encode"))
            .expect("decode"),
        value
    );
    assert_eq!(
        CollaborationText::new(exact + "x"),
        Err(CollaborationError::TextTooLarge)
    );
    assert!(serde_json::from_str::<CollaborationText>("\"   \"").is_err());
    let encoded =
        serde_json::to_string(&"x".repeat(MAX_COLLABORATION_TEXT_BYTES + 1)).expect("fixture");
    assert!(serde_json::from_str::<CollaborationText>(&encoded).is_err());
    let mut ledger = ledger(CollaborationLimits {
        mail_bytes: 1,
        ..CollaborationLimits::default()
    });
    refuses(
        &mut ledger,
        mail("too-large"),
        CollaborationError::MailCapacity,
    );
    let CollaborationEvent::MailAccepted { mut mail } = mail("pointers") else {
        panic!("fixture variant")
    };
    mail.artifacts = (0..=MAX_MAIL_ARTIFACTS)
        .map(|index| ArtifactReference {
            conversation: endpoint("b").conversation,
            artifact: ArtifactId::new(format!("artifact-{index}")).expect("fixture identity"),
        })
        .collect();
    refuses(
        &mut ledger,
        CollaborationEvent::MailAccepted { mail: mail.clone() },
        CollaborationError::TooManyArtifacts,
    );
    mail.artifacts.truncate(1);
    mail.artifacts.push(mail.artifacts[0].clone());
    refuses(
        &mut ledger,
        CollaborationEvent::MailAccepted { mail: mail.clone() },
        CollaborationError::DuplicateArtifact,
    );
    mail.artifacts.clear();
    mail.id =
        MailId::new("x".repeat(MAX_COLLABORATION_ID_BYTES + 1)).expect("nonempty core identity");
    refuses(
        &mut ledger,
        CollaborationEvent::MailAccepted { mail },
        CollaborationError::IdentityTooLarge,
    );
}

/// COL-1/COL-2: mail identity is sender-scoped and neither unknown endpoints nor reuse enter the log.
#[test]
fn col_1_mail_identity_and_endpoint_projection_are_canonical() {
    let mut ledger = ledger(CollaborationLimits::default());
    accept(&mut ledger, "mail", mail("first"));
    refuses(
        &mut ledger,
        mail("first"),
        CollaborationError::MailIdentityConflict,
    );
    let CollaborationEvent::MailAccepted { mut mail } = mail("new") else {
        panic!("fixture variant")
    };
    mail.from = endpoint("unknown");
    refuses(
        &mut ledger,
        CollaborationEvent::MailAccepted { mail: mail.clone() },
        CollaborationError::UnknownEndpoint,
    );
    mail.from = endpoint("a");
    mail.to = endpoint("b");
    mail.id = MailId::new("first").expect("fixture identity");
    accept(
        &mut ledger,
        "reverse",
        CollaborationEvent::MailAccepted { mail },
    );
    let recipient = endpoint("b");
    let projected: Vec<_> = ledger.mail_for(&recipient).collect();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].0.id, item("reverse"));
}

/// COL-3: the user wins either serialization order; observing the new revision is not permission to revert.
#[test]
fn col_3_user_wins_both_writer_orders_and_objections_preserve_task() {
    for agent_first in [false, true] {
        let mut ledger = ledger(CollaborationLimits::default());
        let agent = amendment(
            DelegationAuthor::Agent(endpoint("a")),
            0,
            "Inspect all files",
        );
        if agent_first {
            accept(&mut ledger, "agent", agent.clone());
        }
        accept(
            &mut ledger,
            "user",
            amendment(DelegationAuthor::User, 0, "Inspect tests only"),
        );
        if !agent_first {
            refuses(&mut ledger, agent, CollaborationError::StaleRevision);
        }
        let view = ledger
            .delegation(&delegation())
            .expect("task exists")
            .clone();
        assert_eq!(view.task.as_str(), "Inspect tests only");
        assert_eq!(view.author, DelegationAuthor::User);
        refuses(
            &mut ledger,
            amendment(
                DelegationAuthor::Agent(endpoint("a")),
                view.revision.0,
                "Inspect all files",
            ),
            CollaborationError::UserAuthority,
        );
        refuses(
            &mut ledger,
            amendment(DelegationAuthor::User, 0, "Unseen concurrent user edit"),
            CollaborationError::StaleRevision,
        );
        accept(
            &mut ledger,
            "objection",
            CollaborationEvent::ObjectionRaised {
                delegation: delegation(),
                revision: view.revision,
                author: endpoint("a"),
                summary: text("The dependency may be outside tests"),
            },
        );
        assert_eq!(ledger.delegation(&delegation()), Some(&view));
        assert!(
            matches!(&ledger.records().last().expect("objection exists").event, CollaborationEvent::ObjectionRaised { author, .. } if author == &endpoint("a"))
        );
    }
}

/// COL-3: attribution, topology and revisions are checked before any effective task changes.
#[test]
fn col_3_wrong_authors_cycles_and_stale_tasks_are_refused() {
    let mut ledger = ledger(CollaborationLimits::default());
    refuses(
        &mut ledger,
        amendment(
            DelegationAuthor::Agent(endpoint("b")),
            0,
            "Worker cannot redefine task",
        ),
        CollaborationError::WrongAuthor,
    );
    refuses(
        &mut ledger,
        amendment(DelegationAuthor::User, 1, "Future revision"),
        CollaborationError::StaleRevision,
    );
    accept(
        &mut ledger,
        "agent-edit",
        amendment(
            DelegationAuthor::Agent(endpoint("a")),
            0,
            "Inspect Rust files",
        ),
    );
    refuses(
        &mut ledger,
        amendment(DelegationAuthor::Agent(endpoint("a")), 0, "Stale task"),
        CollaborationError::StaleRevision,
    );
    refuses(
        &mut ledger,
        CollaborationEvent::DelegationCreated {
            delegation: DelegationId::new("cycle").expect("fixture identity"),
            delegator: endpoint("b"),
            worker: endpoint("a"),
            task: text("Cycle"),
        },
        CollaborationError::DelegationCycle,
    );
    refuses(
        &mut ledger,
        CollaborationEvent::ObjectionRaised {
            delegation: delegation(),
            revision: DelegationRevision(1),
            author: endpoint("b"),
            summary: text("Wrong author"),
        },
        CollaborationError::WrongAuthor,
    );
}

/// COL-2: semantic-byte accounting accepts the exact cap and refuses overflow without eviction.
#[test]
fn col_2_exact_retention_limit_and_invalid_configuration() {
    // Five mail-ID bytes, two 15-byte endpoints, and a 24-byte summary; no artifact pointers.
    let mut bounded = ledger(CollaborationLimits {
        mail_bytes: 59,
        ..CollaborationLimits::default()
    });
    accept(&mut bounded, "exact", mail("first"));
    refuses(
        &mut bounded,
        mail("second"),
        CollaborationError::MailCapacity,
    );
    let mut too_small = ledger(CollaborationLimits {
        mail_bytes: 58,
        ..CollaborationLimits::default()
    });
    refuses(
        &mut too_small,
        mail("first"),
        CollaborationError::MailCapacity,
    );
    let mut one_task = ledger(CollaborationLimits {
        delegations: 1,
        ..CollaborationLimits::default()
    });
    refuses(
        &mut one_task,
        CollaborationEvent::DelegationCreated {
            delegation: DelegationId::new("second").expect("fixture identity"),
            delegator: endpoint("a"),
            worker: endpoint("c"),
            task: text("Another task"),
        },
        CollaborationError::DelegationCapacity,
    );
    for limits in [
        CollaborationLimits {
            items: 0,
            ..CollaborationLimits::default()
        },
        CollaborationLimits {
            control_items: MAX_COLLABORATION_ITEMS + 1,
            ..CollaborationLimits::default()
        },
    ] {
        assert_eq!(
            CollaborationLedger::new(
                CollaborationId::new("invalid").expect("fixture identity"),
                limits
            ),
            Err(CollaborationError::InvalidLimits)
        );
    }
}

/// COL-3: endpoint identity is fixed by creation; a worker cannot acquire a second owner.
#[test]
fn col_3_declared_endpoints_and_worker_ownership_cannot_be_rebound() {
    let mut ledger = ledger(CollaborationLimits::default());
    refuses(
        &mut ledger,
        CollaborationEvent::DelegationCreated {
            delegation: DelegationId::new("second").expect("fixture identity"),
            delegator: endpoint("c"),
            worker: endpoint("b"),
            task: text("Reassign worker"),
        },
        CollaborationError::WorkerAlreadyAssigned,
    );
    let mut alias = endpoint("a");
    alias.agent = AgentId::new("replacement").expect("fixture identity");
    refuses(
        &mut ledger,
        CollaborationEvent::DelegationCreated {
            delegation: DelegationId::new("alias").expect("fixture identity"),
            delegator: alias,
            worker: endpoint("c"),
            task: text("Conflicting endpoint"),
        },
        CollaborationError::EndpointIdentityConflict,
    );
}
