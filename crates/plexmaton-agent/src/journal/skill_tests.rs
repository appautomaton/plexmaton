use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId, TranscriptItemId,
    TurnId,
};

use super::{JournalEntryPayload, JournalError, JournalRecord, SessionEntry, SessionJournal};
use crate::{ContextAtomValue, SkillActivation, SkillSource, UnixMillis};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).expect("fixture identity")
}

fn agent(value: &str) -> AgentId {
    id(value, AgentId::new)
}

fn head() -> HeadName {
    id("main", HeadName::new)
}

fn activation() -> SkillActivation {
    SkillActivation::new(
        "review".to_owned(),
        SkillSource::ProjectNative,
        "/workspace/.plexmaton/skills/review/SKILL.md".to_owned(),
        "c".repeat(64),
        "retain exactly\r\n".to_owned(),
    )
    .expect("activation")
}

fn record(journal: &SessionJournal, label: &str, payload: JournalEntryPayload) -> JournalRecord {
    let head = head();
    JournalRecord::AppendEntry {
        sequence: journal.next_sequence(),
        record_id: id(&format!("record-{label}"), JournalRecordId::new),
        expected_head_revision: journal.head_revision(&head).expect("revision"),
        entry: Box::new(SessionEntry {
            id: id(&format!("entry-{label}"), SessionEntryId::new),
            parent_id: journal.head_target(&head).expect("target").cloned(),
            payload,
        }),
        head,
    }
}

fn apply(journal: &mut SessionJournal, label: &str, payload: JournalEntryPayload) {
    let next = record(journal, label, payload);
    journal.apply(next).expect("valid fixture record");
}

fn started_journal() -> (SessionJournal, TurnId) {
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    apply(
        &mut journal,
        "agent",
        JournalEntryPayload::AgentCreated {
            agent_id: agent("agent-a"),
            label: "Agent A".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    let turn_id = id("turn-a", TurnId::new);
    apply(
        &mut journal,
        "turn",
        JournalEntryPayload::TurnStarted {
            agent_id: agent("agent-a"),
            item_id: id("item-a", TranscriptItemId::new),
            turn_id: turn_id.clone(),
            text: "$review inspect".to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        },
    );
    (journal, turn_id)
}

/// SKL-5: activation ownership is checked and an intervening journal fact invalidates attachment.
#[test]
fn skl_5_skill_activation_rejects_wrong_ownership_and_order_without_mutation() {
    let (journal, turn_id) = started_journal();
    let wrong_agent = agent("agent-b");
    let wrong_owner = record(
        &journal,
        "wrong-owner",
        JournalEntryPayload::SkillActivated {
            agent_id: wrong_agent.clone(),
            turn_id: turn_id.clone(),
            activation: activation(),
        },
    );
    let mut attempted = journal.clone();
    assert_eq!(
        attempted.apply(wrong_owner),
        Err(JournalError::WrongTurnAgent {
            turn_id: turn_id.clone(),
            expected: agent("agent-a"),
            actual: wrong_agent,
        })
    );
    assert_eq!(attempted, journal);

    let mut interrupted_order = journal;
    apply(
        &mut interrupted_order,
        "warning",
        JournalEntryPayload::RuntimeWarning {
            agent_id: agent("agent-a"),
            item_id: id("warning", TranscriptItemId::new),
            message: "intervening fact".to_owned(),
        },
    );
    let invalid = record(
        &interrupted_order,
        "wrong-order",
        JournalEntryPayload::SkillActivated {
            agent_id: agent("agent-a"),
            turn_id: turn_id.clone(),
            activation: activation(),
        },
    );
    let unchanged = interrupted_order.clone();
    assert_eq!(
        interrupted_order.apply(invalid),
        Err(JournalError::InvalidSkillActivationOrder(turn_id))
    );
    assert_eq!(interrupted_order, unchanged);
}

/// SKL-5/JRN-5: replay adds one exact skill atom and no synthetic visible transcript event.
#[test]
fn skl_5_skill_projection_is_exact_and_invisible() {
    let (mut journal, turn_id) = started_journal();
    let before = journal.project(&head()).expect("projection before skill");
    let exact = activation();
    apply(
        &mut journal,
        "skill",
        JournalEntryPayload::SkillActivated {
            agent_id: agent("agent-a"),
            turn_id,
            activation: exact.clone(),
        },
    );
    let projection = journal.project(&head()).expect("projection with skill");
    assert_eq!(projection.events(), before.events());
    assert!(matches!(
        projection.request().atoms.as_slice(),
        [user, skill]
            if matches!(user.value(), ContextAtomValue::User { text } if text == "$review inspect")
                && skill.value() == &ContextAtomValue::Skill(exact)
    ));
}
