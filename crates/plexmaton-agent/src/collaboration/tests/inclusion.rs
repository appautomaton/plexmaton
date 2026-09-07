use std::sync::Arc;

use plexmaton_core::{ConversationEntryId, HeadName, JournalRecordId, TurnId};

use super::*;
use crate::{
    Agent, ApprovalPolicy, ContextAtomValue, ConversationMetadata, Effect, Input, JournalRecord,
    TurnBudget, UnixMillis,
};

fn agent() -> Agent {
    let endpoint = endpoint("a");
    let mut agent = Agent::for_conversation(
        endpoint.agent,
        ConversationMetadata::new(endpoint.conversation, UnixMillis::EPOCH),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    agent.announce("A");
    agent
}

fn turn(name: &str) -> TurnId {
    TurnId::new(name).expect("fixture identity")
}

fn admitted(
    ledger: &mut CollaborationLedger,
    agent: &Agent,
    name: &str,
) -> Arc<ResolvedTurnAdmission> {
    let boundary = agent
        .collaboration_boundary(turn(name))
        .expect("idle boundary");
    let previous = agent
        .journal()
        .previous_collaboration_inclusion(agent.selected_head())
        .expect("path");
    let preparation = ledger
        .prepare_turn(item(name), boundary, previous)
        .expect("prepare turn");
    if let Preparation::Append(record) = preparation {
        ledger.apply(*record).expect("admit");
    }
    ledger
        .resolve_turn(&ledger.item_reference(&item(name)).expect("reference"))
        .expect("resolve")
}

/// CIN-1: prepared-before-amendment is stale; admitted-before-amendment stays immutable on retry.
#[test]
fn cin_1_admission_orders_amendments_and_freezes_original_revision() {
    let mut ledger = ledger(CollaborationLimits::default());
    let mut agent = agent();
    let boundary = agent
        .collaboration_boundary(turn("first"))
        .expect("boundary");
    let Preparation::Append(prepared) = ledger
        .prepare_turn(item("first"), boundary.clone(), None)
        .expect("prepare")
    else {
        panic!("new admission")
    };
    accept(
        &mut ledger,
        "user-one",
        amendment(DelegationAuthor::User, 0, "Read tests only"),
    );
    assert_eq!(
        ledger.prepare(prepared.id, prepared.event),
        Err(CollaborationError::StaleTurnAdmission)
    );
    let first = admitted(&mut ledger, &agent, "first");
    assert_eq!(
        first.items().last().expect("amendment").task_revision,
        Some(DelegationRevision(1))
    );
    accept(
        &mut ledger,
        "user-two",
        amendment(DelegationAuthor::User, 1, "Read only Rust tests"),
    );
    assert!(matches!(
        ledger
            .prepare_turn(item("first"), boundary, None)
            .expect("retry"),
        Preparation::Existing(_)
    ));
    let reread = ledger
        .resolve_turn(first.reference())
        .expect("frozen source");
    assert_eq!(reread, first);
    agent
        .start_collaboration_turn(&first, UnixMillis::EPOCH)
        .expect("include admitted turn");
    agent.handle_at(Input::Interrupted, UnixMillis::EPOCH);
    let second = admitted(&mut ledger, &agent, "second");
    assert_eq!(second.items().len(), 1);
    assert_eq!(second.items()[0].reference.item, item("user-two"));
    assert_eq!(second.items()[0].task_revision, Some(DelegationRevision(2)));
}

/// CIN-1/CIN-2: a gate that never reached the session consumes nothing; branch ancestry owns the cursor.
#[test]
fn cin_2_unincluded_admission_and_branch_retain_pending_sources() {
    let mut ledger = ledger(CollaborationLimits::default());
    let mut agent = agent();
    let abandoned = admitted(&mut ledger, &agent, "abandoned");
    let explicit = admitted(&mut ledger, &agent, "explicit");
    assert_eq!(explicit.items(), abandoned.items());
    agent
        .start_collaboration_turn(&explicit, UnixMillis::EPOCH)
        .expect("include");
    agent.handle_at(Input::Interrupted, UnixMillis::EPOCH);
    let mut journal = agent.journal().clone();
    let first_entry = journal.path(agent.selected_head()).expect("path")[0]
        .id
        .clone();
    let fork = HeadName::new("before-mail").expect("head");
    journal
        .apply(JournalRecord::CreateHead {
            sequence: journal.next_sequence(),
            record_id: JournalRecordId::new("fork").expect("id"),
            head: fork.clone(),
            at: Some(first_entry),
        })
        .expect("branch before inclusion");
    assert_eq!(
        journal
            .previous_collaboration_inclusion(&fork)
            .expect("fork cursor"),
        None
    );
    assert_eq!(
        journal
            .previous_collaboration_inclusion(agent.selected_head())
            .expect("main cursor"),
        Some(explicit.reference().clone())
    );
    let boundary = journal
        .collaboration_boundary(&fork, &endpoint("a").agent, turn("fork-turn"))
        .expect("fork boundary");
    let Preparation::Append(record) = ledger
        .prepare_turn(item("fork-turn"), boundary, None)
        .expect("fork admission")
    else {
        panic!("new turn")
    };
    ledger.apply(*record).expect("admit fork");
    let fork_source = ledger
        .resolve_turn(
            &ledger
                .item_reference(&item("fork-turn"))
                .expect("reference"),
        )
        .expect("resolve fork");
    assert_eq!(fork_source.items(), explicit.items());
}

/// CIN-2/CIN-3: session bytes carry only a reference; resolution is typed and bound to its exact source.
#[test]
fn cin_3_session_reference_resolves_without_synthetic_user_content() {
    let mut ledger = ledger(CollaborationLimits::default());
    let mut agent = agent();
    accept(&mut ledger, "mail", mail("finding"));
    let source = admitted(&mut ledger, &agent, "turn");
    let reaction = agent
        .start_collaboration_turn(&source, UnixMillis::EPOCH)
        .expect("start");
    let bytes = serde_json::to_string(&reaction.records).expect("encode session records");
    assert!(!bytes.contains("Found two relevant files"));
    assert!(!bytes.contains("Inspect files"));
    assert!(!bytes.contains("\"text\""));
    let [Effect::CallModel(call)] = reaction.effects.as_slice() else {
        panic!("one model call")
    };
    let original = call.request.clone();
    assert!(
        original
            .atoms
            .iter()
            .all(|atom| !matches!(atom.value(), ContextAtomValue::User { .. }))
    );
    let mut request = original.clone();
    assert_eq!(
        ResolvedContext::default().resolve(&mut request, agent.journal()),
        Err(CollaborationError::UnresolvedContext)
    );
    assert_eq!(request, original);
    let mut cache = ResolvedContext::default();
    cache.insert(Arc::clone(&source)).expect("bounded source");
    cache
        .resolve(&mut request, agent.journal())
        .expect("resolve");
    assert!(
        matches!(request.atoms[0].value(), ContextAtomValue::Collaboration(CollaborationContext::Resolved(value)) if value == &source)
    );
    let mut forged = original.clone();
    forged.atoms[0] = crate::ContextAtom::collaboration(
        ConversationEntryId::new("missing-source").expect("id"),
        source.reference().clone(),
    );
    assert_eq!(
        cache.resolve(&mut forged, agent.journal()),
        Err(CollaborationError::InvalidReference)
    );
    let replay = Agent::from_journal(
        endpoint("a").agent,
        agent.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("replay");
    assert_eq!(
        replay
            .journal()
            .project(replay.selected_head())
            .expect("projection")
            .request(),
        &original
    );
    assert!(
        agent
            .start_collaboration_turn(&source, UnixMillis::EPOCH)
            .is_err(),
        "same admission cannot start twice"
    );
}

/// CIN-1: all required source items must fit; admission never truncates an amendment or mail tail.
#[test]
fn cin_1_source_capacity_holds_the_turn_without_advancing_log() {
    let mut ledger = ledger(CollaborationLimits::default());
    let agent = agent();
    for index in 0..MAX_TURN_SOURCE_ITEMS - 1 {
        accept(
            &mut ledger,
            &format!("mail-{index}"),
            mail(&format!("mail-{index}")),
        );
    }
    let boundary = agent
        .collaboration_boundary(turn("capacity"))
        .expect("boundary");
    assert!(
        ledger
            .prepare_turn(item("capacity"), boundary.clone(), None)
            .is_ok()
    );
    accept(&mut ledger, "overflow", mail("overflow"));
    let before = ledger.clone();
    assert_eq!(
        ledger.prepare_turn(item("capacity"), boundary, None),
        Err(CollaborationError::TurnSourceCapacity)
    );
    assert_eq!(ledger, before);
}

/// CIN-3: transient materialization has its own cap even when many unused admissions are durable.
#[test]
fn cin_3_resolved_cache_is_bounded_and_exact_reinsertion_is_free() {
    let mut ledger = ledger(CollaborationLimits::default());
    let agent = agent();
    let mut cache = ResolvedContext::default();
    for index in 0..MAX_RESOLVED_TURNS {
        cache
            .insert(admitted(&mut ledger, &agent, &format!("turn-{index}")))
            .expect("within capacity");
    }
    let first = ledger
        .resolve_turn(&ledger.item_reference(&item("turn-0")).expect("ref"))
        .expect("resolve");
    cache
        .insert(first)
        .expect("existing identity uses no capacity");
    assert_eq!(
        cache.insert(admitted(&mut ledger, &agent, "overflow")),
        Err(CollaborationError::ContextCapacity)
    );
}

fn large_authored_prefix() -> (CollaborationLedger, TurnBoundary, MailEndpoint) {
    let author = MailEndpoint {
        conversation: ConversationId::new("s".repeat(MAX_COLLABORATION_ID_BYTES))
            .expect("bounded id"),
        agent: AgentId::new("a".repeat(MAX_COLLABORATION_ID_BYTES)).expect("bounded id"),
    };
    let mut ledger = CollaborationLedger::new(
        CollaborationId::new("collaboration").expect("id"),
        CollaborationLimits::default(),
    )
    .expect("ledger");
    accept(
        &mut ledger,
        "create-max",
        CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: author.clone(),
            worker: endpoint("b"),
            task: text("t"),
        },
    );
    for index in 0..7 {
        accept(
            &mut ledger,
            &format!("amend-{index}"),
            amendment(
                DelegationAuthor::Agent(author.clone()),
                index,
                &"x".repeat(32600),
            ),
        );
    }
    let boundary = TurnBoundary {
        recipient: author.clone(),
        head: HeadName::new("main").expect("head"),
        head_revision: crate::HeadRevision::new(0),
        parent: None,
        turn: turn("large"),
    };
    (ledger, boundary, author)
}

/// CIN-1/CIN-3: variable-sized authorship counts against the source-byte cap, not just task text.
#[test]
fn cin_1_source_bytes_include_attributed_agent_identities() {
    let (mut ledger, boundary, author) = large_authored_prefix();
    assert!(
        ledger
            .prepare_turn(item("fits"), boundary.clone(), None)
            .is_ok()
    );
    // Without the eight 512-byte authors, these source records fit below 256 KiB.
    // Counting preserved attribution pushes the eighth amendment above the cap.
    accept(
        &mut ledger,
        "amend-7",
        amendment(DelegationAuthor::Agent(author), 7, &"x".repeat(32600)),
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.prepare_turn(item("too-large"), boundary, None),
        Err(CollaborationError::TurnSourceCapacity)
    );
    assert_eq!(ledger, before);
}

/// CIN-3: bytes saturate before count for large materializations, without rejecting an exact reinsertion.
#[test]
fn cin_3_resolved_cache_byte_cap_is_independent_of_turn_count() {
    let (mut ledger, mut boundary, _) = large_authored_prefix();
    let mut cache = ResolvedContext::default();
    let mut first = None;
    let mut stopped = false;
    for index in 0..MAX_RESOLVED_TURNS {
        let name = format!("large-{index}");
        boundary.turn = turn(&name);
        let Preparation::Append(record) = ledger
            .prepare_turn(item(&name), boundary.clone(), None)
            .expect("bounded source")
        else {
            panic!("new admission")
        };
        ledger.apply(*record).expect("admit");
        let source = ledger
            .resolve_turn(&ledger.item_reference(&item(&name)).expect("ref"))
            .expect("source");
        if first.is_none() {
            first = Some(Arc::clone(&source));
        }
        match cache.insert(source) {
            Ok(()) => {}
            Err(error) => {
                assert_eq!(error, CollaborationError::ContextCapacity);
                assert!(
                    index > 0 && index < MAX_RESOLVED_TURNS,
                    "byte cap must win before count"
                );
                stopped = true;
                break;
            }
        }
    }
    assert!(stopped, "retained bytes grew without their independent cap");
    cache
        .insert(first.expect("first source"))
        .expect("retry does not spend capacity");
}
