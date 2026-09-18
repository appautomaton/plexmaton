//! What the collaboration log means on screen, decided in one place.
//!
//! Split from the composition root because the two answer different questions and fail in different
//! ways. This half is a pure rule over one record, so it can be checked without a runtime, a
//! provider or a child; the root owns opening the log, naming children and driving the pass.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context as _, ensure};
use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationItemRef, CollaborationRecord, MailEndpoint,
    ResolvedTurnAdmission,
};
use plexmaton_agent::{ConversationJournal, JournalProjection};
use plexmaton_core::{
    AgentId, CollaborationId, ConversationEntryId, ConversationEvent, DelegationId,
    TranscriptItemId,
};

use super::item_of;

/// Selected-session anchors for shared rows, derived only from durable references.
#[derive(Debug)]
pub(super) struct SessionPlacements {
    collaboration: CollaborationId,
    anchors: BTreeMap<CollaborationItemRef, Vec<ConversationEntryId>>,
}

impl SessionPlacements {
    pub(super) fn build(
        collaboration: CollaborationId,
        owner: &AgentId,
        journal: &ConversationJournal,
        resolved: &[Arc<ResolvedTurnAdmission>],
        records: &[CollaborationRecord],
    ) -> anyhow::Result<Self> {
        let mut anchors = BTreeMap::new();

        // A session link is the exact first durable position. It wins over any older compatibility
        // inclusion because ENT-1 fixes first appearance rather than moving an old row forward.
        for origin in journal
            .collaboration_links(journal.selected_head())
            .map_err(|error| {
                anyhow::anyhow!("read selected collaboration placement links: {error:?}")
            })?
        {
            ensure!(
                origin.agent() == owner,
                "collaboration placement link belongs to another session agent"
            );
            validate_reference(&collaboration, origin.reference(), records)?;
            anchors
                .entry(origin.reference().clone())
                .or_insert_with(Vec::new)
                .push(origin.entry().clone());
        }

        let inclusions = journal
            .collaboration_inclusions(journal.selected_head())
            .map_err(|error| {
                anyhow::anyhow!("read selected collaboration inclusion anchors: {error:?}")
            })?;
        ensure!(
            inclusions.len() == resolved.len(),
            "resolved collaboration inclusions do not match the selected session"
        );
        for (origin, turn) in inclusions.iter().zip(resolved) {
            ensure!(
                origin.reference() == turn.reference(),
                "resolved collaboration inclusion changed identity"
            );
            journal
                .validate_collaboration_source(origin.entry(), turn)
                .context("validate selected-session collaboration inclusion")?;
            for item in turn.items() {
                validate_reference(&collaboration, &item.reference, records)?;
                anchors
                    .entry(item.reference.clone())
                    .or_insert_with(Vec::new)
                    .push(origin.entry().clone());
            }
        }
        Ok(Self {
            collaboration,
            anchors,
        })
    }

    pub(super) fn anchors(&self, record: &CollaborationRecord) -> Vec<ConversationEntryId> {
        self.anchors
            .get(&record_reference(&self.collaboration, record))
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn reference(&self, record: &CollaborationRecord) -> CollaborationItemRef {
        record_reference(&self.collaboration, record)
    }
}

pub(super) struct PlacedEntry {
    pub(super) event: ConversationEvent,
    pub(super) reference: CollaborationItemRef,
    pub(super) anchors: Vec<ConversationEntryId>,
}

/// Projects one endpoint's side of every shared fact in canonical log order.
pub(super) fn session_entries(
    records: &[CollaborationRecord],
    owner: &AgentId,
    placements: &SessionPlacements,
    name: &impl Fn(&MailEndpoint) -> Option<AgentId>,
    worker: &impl Fn(&DelegationId) -> Option<MailEndpoint>,
) -> Vec<PlacedEntry> {
    records
        .iter()
        .flat_map(|record| {
            entries(record, name, worker)
                .into_iter()
                .filter(move |(agent, _)| agent == owner)
                .map(move |(_, event)| PlacedEntry {
                    event,
                    reference: placements.reference(record),
                    anchors: placements.anchors(record),
                })
        })
        .collect()
}

/// Merges canonical shared rows into one selected journal projection without consulting time.
///
/// Missing anchors are retained as a stable collaboration-sequence suffix. This is the lossless
/// compatibility policy for old or interrupted sender journals: absence of a link cannot prove an
/// earlier position, so replay never invents one.
pub(super) fn merge_session_entries(
    projection: &JournalProjection,
    shared: Vec<PlacedEntry>,
) -> Vec<ConversationEvent> {
    let base = projection.events();
    let mut before = vec![Vec::new(); base.len().saturating_add(1)];
    let mut tail = Vec::new();
    let existing: BTreeSet<_> = base
        .iter()
        .filter_map(|envelope| item_of(&envelope.event))
        .collect();
    for placed in shared {
        if item_of(&placed.event).is_some_and(|item| existing.contains(&item)) {
            continue;
        }
        match placed
            .anchors
            .iter()
            .find_map(|entry| projection.event_offset(entry))
        {
            Some(offset) => before[offset].push(placed.event),
            None => tail.push(placed.event),
        }
    }
    let mut merged = Vec::with_capacity(
        base.len()
            .saturating_add(before.iter().map(Vec::len).sum::<usize>())
            .saturating_add(tail.len()),
    );
    for (offset, at) in before.iter_mut().enumerate() {
        merged.append(at);
        if let Some(envelope) = base.get(offset) {
            merged.push(envelope.event.clone());
        }
    }
    merged.append(&mut tail);
    merged
}

fn validate_reference(
    collaboration: &CollaborationId,
    reference: &CollaborationItemRef,
    records: &[CollaborationRecord],
) -> anyhow::Result<()> {
    reference
        .validate()
        .context("validate collaboration placement reference")?;
    ensure!(
        &reference.collaboration == collaboration
            && records.iter().any(|record| {
                record.id == reference.item && record.sequence == reference.sequence
            }),
        "collaboration placement reference has no canonical source"
    );
    Ok(())
}

fn record_reference(
    collaboration: &CollaborationId,
    record: &CollaborationRecord,
) -> CollaborationItemRef {
    CollaborationItemRef {
        collaboration: collaboration.clone(),
        item: record.id.clone(),
        sequence: record.sequence,
    }
}

/// What one collaboration record means on screen: the conversations that show it, and what each
/// one says.
///
/// The log holds five kinds of fact between two sessions, and this is the single place that says
/// which of them a reader sees. Total over those kinds, so a sixth is a compile error rather than a
/// silent nothing — which is what the first five got, one at a time, each discovered by a user
/// asking where half of a conversation had gone.
///
/// `name` resolves an endpoint to the roster name its conversation carries, and `worker` resolves a
/// delegation to the session it was given to, because a task update names the delegation rather
/// than the worker. Anything either cannot resolve yields nothing: a row naming a stranger is worse
/// than no row.
pub(super) fn entries(
    record: &CollaborationRecord,
    name: &impl Fn(&MailEndpoint) -> Option<AgentId>,
    worker: &impl Fn(&DelegationId) -> Option<MailEndpoint>,
) -> Vec<(AgentId, ConversationEvent)> {
    let item = |side: &str| {
        TranscriptItemId::new(format!("{}-{side}", record.id.as_str()))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    };
    match &record.event {
        // An ordering point with no content of its own.
        CollaborationEvent::TurnAdmitted { .. }
        | CollaborationEvent::AttentionRequested { .. }
        | CollaborationEvent::AttentionResolved { .. } => Vec::new(),
        CollaborationEvent::DelegationCreated {
            delegator,
            worker: to,
            task,
            ..
        } => both(
            name(delegator),
            name(to),
            &item,
            &|from, to, owner, side| ConversationEvent::TaskAssigned {
                agent_id: owner,
                item_id: side,
                from,
                to,
                task: task.as_str().to_owned(),
            },
        ),
        CollaborationEvent::TaskUpdated {
            delegation,
            author,
            task,
            ..
        } => both(
            name(author),
            worker(delegation).as_ref().and_then(name),
            &item,
            &|from, to, owner, side| ConversationEvent::TaskAssigned {
                agent_id: owner,
                item_id: side,
                from,
                to,
                task: task.as_str().to_owned(),
            },
        ),
        CollaborationEvent::MailAccepted { mail } => both(
            name(&mail.from),
            name(&mail.to),
            &item,
            &|from, to, owner, side| ConversationEvent::MailDelivered {
                agent_id: owner,
                item_id: side,
                mail_id: mail.id.clone(),
                from,
                to,
                summary: mail.summary.as_str().to_owned(),
            },
        ),
        CollaborationEvent::HandoffCompleted {
            delegation, author, ..
        } => both(
            name(author),
            worker(delegation).as_ref().and_then(name),
            &item,
            &|_from, child, owner, side| ConversationEvent::HandoffCompleted {
                agent_id: owner,
                item_id: side,
                child,
            },
        ),
    }
}

/// One fact, in the conversation that sent it and the one that received it.
///
/// An item belongs to exactly one conversation, so each side is its own item over one shared
/// identity; `out` and `in` are which side, named from the sender's point of view.
fn both(
    from: Option<AgentId>,
    to: Option<AgentId>,
    item: &impl Fn(&str) -> TranscriptItemId,
    event: &impl Fn(AgentId, AgentId, AgentId, TranscriptItemId) -> ConversationEvent,
) -> Vec<(AgentId, ConversationEvent)> {
    let (Some(from), Some(to)) = (from, to) else {
        return Vec::new();
    };
    [("out", from.clone()), ("in", to.clone())]
        .into_iter()
        .map(|(side, owner)| {
            (
                owner.clone(),
                event(from.clone(), to.clone(), owner, item(side)),
            )
        })
        .collect()
}

/// CMP-1: what the log means on screen, decided once and checked without a runtime.
#[cfg(test)]
mod tests {
    use plexmaton_agent::collaboration::{
        CollaborationEvent, CollaborationItemRef, CollaborationLimits, CollaborationRecord,
        CollaborationSequence, CollaborationText, DelegationRevision, MailEndpoint, MailEnvelope,
    };
    use plexmaton_agent::{
        ConversationEntry, ConversationJournal, JournalEntryPayload, JournalRecord, UnixMillis,
    };
    use plexmaton_core::{
        AgentId, AgentStatus, CollaborationId, CollaborationItemId, ConversationEntryId,
        ConversationEvent, ConversationId, DelegationId, JournalRecordId, MailId, TranscriptItemId,
    };
    use plexmaton_session_store::collaboration::CollaborationFile;
    use plexmaton_session_store::{ConversationDirectory, RootJournalFile};

    use super::{
        SessionPlacements, entries, merge_session_entries, record_reference, session_entries,
    };
    use crate::tests::FixtureWorkspace;

    fn endpoint(conversation: &str, agent: &str) -> MailEndpoint {
        MailEndpoint {
            conversation: ConversationId::new(conversation).expect("conversation"),
            agent: AgentId::new(agent).expect("agent"),
        }
    }

    fn root() -> MailEndpoint {
        endpoint("session-root", "agent-primary")
    }

    fn child() -> MailEndpoint {
        endpoint("conversation-child", "agent-child")
    }

    /// The roster names the root by its own agent and a child by the short name it was announced
    /// under, which is what every row on screen has to agree with.
    fn name(target: &MailEndpoint) -> Option<AgentId> {
        match target.conversation.as_str() {
            "session-root" => Some(AgentId::new("agent-primary").expect("agent")),
            "conversation-child" => Some(AgentId::new("delegated-1").expect("agent")),
            _ => None,
        }
    }

    fn record(id: &str, event: CollaborationEvent) -> CollaborationRecord {
        CollaborationRecord {
            id: CollaborationItemId::new(id).expect("item"),
            sequence: CollaborationSequence(1),
            event,
        }
    }

    fn delegation() -> DelegationId {
        DelegationId::new("delegation-1").expect("delegation")
    }

    fn worker(id: &DelegationId) -> Option<MailEndpoint> {
        (id == &delegation()).then(child)
    }

    fn drawn(event: CollaborationEvent) -> Vec<(String, String, String)> {
        entries(&record("item-1", event), &name, &worker)
            .into_iter()
            .map(|(owner, event)| match event {
                ConversationEvent::TaskAssigned { from, to, task, .. } => {
                    (owner.to_string(), format!("task {from}->{to}"), task)
                }
                ConversationEvent::MailDelivered {
                    from, to, summary, ..
                } => (owner.to_string(), format!("mail {from}->{to}"), summary),
                other => (owner.to_string(), format!("{other:?}"), String::new()),
            })
            .collect()
    }

    fn text(value: &str) -> CollaborationText {
        CollaborationText::new(value).expect("text")
    }

    /// Every fact between two sessions reaches both of them, attributed the same way in each.
    ///
    /// Written as one table because the bugs it replaces were each one kind quietly missing while
    /// its neighbours worked: mail the root sent, a task nobody projected, a task change that
    /// waited for a restart. A kind that stops drawing both sides fails here rather than in a
    /// screenshot.
    #[test]
    fn every_kind_of_fact_reaches_both_conversations() {
        let created = CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: root(),
            worker: child(),
            task: text("read the specs"),
        };
        let updated = CollaborationEvent::TaskUpdated {
            delegation: delegation(),
            expected: DelegationRevision(0),
            author: root(),
            task: text("read the standards"),
        };
        let mail = |from: MailEndpoint, to: MailEndpoint| CollaborationEvent::MailAccepted {
            mail: MailEnvelope {
                id: MailId::new("mail-1").expect("mail"),
                from,
                to,
                summary: text("the answer"),
                artifacts: Vec::new(),
            },
        };
        for (what, event, shape) in [
            ("delegate", created, "task agent-primary->delegated-1"),
            ("update_task", updated, "task agent-primary->delegated-1"),
            (
                "child mail",
                mail(child(), root()),
                "mail delegated-1->agent-primary",
            ),
            (
                "main mail",
                mail(root(), child()),
                "mail agent-primary->delegated-1",
            ),
        ] {
            let rows = drawn(event);
            assert_eq!(rows.len(), 2, "{what} must reach both conversations");
            let owners: Vec<_> = rows.iter().map(|row| row.0.as_str()).collect();
            assert!(
                owners.contains(&"agent-primary") && owners.contains(&"delegated-1"),
                "{what} reached {owners:?}"
            );
            for row in &rows {
                assert_eq!(row.1, shape, "{what} is attributed the same on both sides");
            }
        }
    }

    /// CCV-3: one durable Handoff becomes distinct semantic rows on both named sides.
    #[test]
    fn handoff_projects_distinct_entries_on_both_sides() {
        let rows = entries(
            &record(
                "handoff-item",
                CollaborationEvent::HandoffCompleted {
                    delegation: delegation(),
                    expected: DelegationRevision(0),
                    author: root(),
                },
            ),
            &name,
            &worker,
        );
        assert_eq!(rows.len(), 2);
        let mut projected = rows.into_iter().map(|(owner, event)| match event {
            ConversationEvent::HandoffCompleted {
                agent_id,
                item_id,
                child,
            } => (owner, agent_id, item_id, child),
            other => panic!("unexpected Handoff projection: {other:?}"),
        });
        let root_side = projected.next().expect("root side");
        let child_side = projected.next().expect("child side");
        assert_eq!(root_side.0, AgentId::new("agent-primary").expect("root"));
        assert_eq!(root_side.0, root_side.1);
        assert_eq!(child_side.0, AgentId::new("delegated-1").expect("child"));
        assert_eq!(child_side.0, child_side.1);
        assert_eq!(root_side.3, child_side.0);
        assert_eq!(child_side.3, child_side.0);
        assert_ne!(root_side.2, child_side.2);
    }

    /// A fact naming a session the roster cannot name draws nothing rather than a row about a
    /// stranger — and is not marked seen, so it appears once the roster learns the name.
    #[test]
    fn a_fact_the_roster_cannot_name_draws_nothing() {
        let stranger = CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: root(),
            worker: endpoint("conversation-unknown", "agent-unknown"),
            task: text("nobody can name me"),
        };
        assert!(drawn(stranger).is_empty());
    }

    /// ENT-1/CIN-2/JRN-3/JRN-5: a reference-only sender link restores the shared row at its
    /// session position; an old unanchored row remains losslessly at the canonical suffix.
    #[test]
    fn durable_links_place_shared_rows_and_legacy_rows_keep_a_stable_suffix() {
        let collaboration = CollaborationId::new("collaboration").expect("collaboration");
        let task = CollaborationRecord {
            id: CollaborationItemId::new("task-item").expect("task item"),
            sequence: CollaborationSequence(1),
            event: CollaborationEvent::DelegationCreated {
                delegation: delegation(),
                delegator: root(),
                worker: child(),
                task: text("read the specs"),
            },
        };
        let mail = CollaborationRecord {
            id: CollaborationItemId::new("mail-item").expect("mail item"),
            sequence: CollaborationSequence(2),
            event: CollaborationEvent::MailAccepted {
                mail: MailEnvelope {
                    id: MailId::new("mail-1").expect("mail"),
                    from: child(),
                    to: root(),
                    summary: text("the answer"),
                    artifacts: Vec::new(),
                },
            },
        };
        let records = vec![task.clone(), mail];
        let mut journal =
            ConversationJournal::new(ConversationId::new("session-root").expect("conversation"));
        append(
            &mut journal,
            JournalEntryPayload::AgentCreated {
                agent_id: root().agent,
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::RuntimeWarning {
                agent_id: root().agent,
                item_id: TranscriptItemId::new("before").expect("item"),
                message: "before work".to_owned(),
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::CollaborationItemLinked {
                agent_id: root().agent,
                reference: record_reference(&collaboration, &task),
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::RuntimeWarning {
                agent_id: root().agent,
                item_id: TranscriptItemId::new("after").expect("item"),
                message: "after work".to_owned(),
            },
        );
        let projection = journal
            .project(journal.selected_head())
            .expect("selected journal projection");
        let placements =
            SessionPlacements::build(collaboration, &root().agent, &journal, &[], &records)
                .expect("durable placements");
        let shared = session_entries(&records, &root().agent, &placements, &name, &worker);
        let semantic: Vec<_> = merge_session_entries(&projection, shared)
            .into_iter()
            .filter_map(|event| match event {
                ConversationEvent::RuntimeWarning { message, .. } => Some(message),
                ConversationEvent::TaskAssigned { task, .. } => Some(format!("task:{task}")),
                ConversationEvent::MailDelivered { summary, .. } => Some(format!("mail:{summary}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            semantic,
            [
                "before work",
                "task:read the specs",
                "after work",
                "mail:the answer"
            ]
        );
    }

    /// ENT-1/JRN-3/JRN-5: the live file reductions and two independent reopens derive the same
    /// semantic order from one collaboration log plus reference-only session links.
    #[test]
    fn real_writers_reopen_durable_shared_rows_in_their_live_order_twice() {
        let fixture = FixtureWorkspace::new();
        let collaboration = CollaborationId::new("session-root").expect("collaboration");
        let collaboration_path = fixture.path().join("collaborations/session-root.jsonl");
        let mut shared = CollaborationFile::create(
            &collaboration_path,
            collaboration.clone(),
            CollaborationLimits::default(),
        )
        .expect("create collaboration file");
        let task = CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: root(),
            worker: child(),
            task: text("read the specs"),
        };
        let task_receipt = shared
            .admit(CollaborationItemId::new("task-item").expect("item"), task)
            .expect("append task");
        let mail = CollaborationEvent::MailAccepted {
            mail: MailEnvelope {
                id: MailId::new("mail-1").expect("mail"),
                from: child(),
                to: root(),
                summary: text("the answer"),
                artifacts: Vec::new(),
            },
        };
        let mail_receipt = shared
            .admit(CollaborationItemId::new("mail-item").expect("item"), mail)
            .expect("append mail");
        let records = shared.ledger().records().to_vec();

        let sessions = ConversationDirectory::under(fixture.path()).expect("session directory");
        let root_id = ConversationId::new("session-root").expect("conversation");
        let mut session = sessions
            .create(root_id, UnixMillis::EPOCH)
            .expect("create root session");
        append_file(
            &mut session,
            JournalEntryPayload::AgentCreated {
                agent_id: root().agent,
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        append_file(&mut session, warning_payload("before"));
        append_file(
            &mut session,
            JournalEntryPayload::CollaborationItemLinked {
                agent_id: root().agent,
                reference: CollaborationItemRef {
                    collaboration: collaboration.clone(),
                    item: task_receipt.id,
                    sequence: task_receipt.sequence,
                },
            },
        );
        append_file(&mut session, warning_payload("middle"));
        append_file(
            &mut session,
            JournalEntryPayload::CollaborationItemLinked {
                agent_id: root().agent,
                reference: CollaborationItemRef {
                    collaboration: collaboration.clone(),
                    item: mail_receipt.id,
                    sequence: mail_receipt.sequence,
                },
            },
        );
        append_file(&mut session, warning_payload("after"));

        let live = semantic_order(session.journal(), collaboration.clone(), &records);
        assert_eq!(
            live,
            [
                "before",
                "task:read the specs",
                "middle",
                "mail:the answer",
                "after"
            ]
        );
        drop(session);
        drop(shared);
        let session_bytes = std::fs::read(
            sessions
                .path_for(&ConversationId::new("session-root").expect("conversation"))
                .expect("session path"),
        )
        .expect("session bytes");
        let collaboration_bytes = std::fs::read(&collaboration_path).expect("collaboration bytes");

        for _ in 0..2 {
            let reopened_session = sessions
                .resume(&ConversationId::new("session-root").expect("conversation"))
                .expect("reopen root session");
            let reopened_shared =
                CollaborationFile::open(&collaboration_path).expect("reopen collaboration");
            let reopened = semantic_order(
                reopened_session.journal(),
                collaboration.clone(),
                reopened_shared.ledger().records(),
            );
            assert_eq!(reopened, live);
            drop(reopened_session);
            drop(reopened_shared);
            assert_eq!(
                std::fs::read(
                    sessions
                        .path_for(&ConversationId::new("session-root").expect("conversation"))
                        .expect("session path")
                )
                .expect("session bytes"),
                session_bytes,
                "reopen is effect-free"
            );
            assert_eq!(
                std::fs::read(&collaboration_path).expect("collaboration bytes"),
                collaboration_bytes,
                "reopen changes no collaboration fact"
            );
        }
    }

    /// ENT-1/CIN-2: an unselected branch cannot make the selected conversation unreopenable.
    #[test]
    fn selected_session_placement_ignores_an_off_branch_foreign_link() {
        let mut journal =
            ConversationJournal::new(ConversationId::new("session-root").expect("conversation"));
        append(
            &mut journal,
            JournalEntryPayload::AgentCreated {
                agent_id: root().agent,
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        let at = journal
            .head_target(journal.selected_head())
            .expect("head target")
            .cloned();
        let branch = plexmaton_core::HeadName::new("off-branch").expect("head");
        let sequence = journal.next_sequence();
        journal
            .apply(JournalRecord::CreateHead {
                sequence,
                record_id: JournalRecordId::new(format!("record-{}", sequence.get()))
                    .expect("record"),
                head: branch.clone(),
                at,
            })
            .expect("create sibling branch");
        let sequence = journal.next_sequence();
        journal
            .apply(JournalRecord::AppendEntry {
                sequence,
                record_id: JournalRecordId::new(format!("record-{}", sequence.get()))
                    .expect("record"),
                expected_head_revision: journal.head_revision(&branch).expect("branch revision"),
                entry: Box::new(ConversationEntry {
                    id: ConversationEntryId::new("off-branch-link").expect("entry"),
                    parent_id: journal
                        .head_target(&branch)
                        .expect("branch target")
                        .cloned(),
                    payload: JournalEntryPayload::CollaborationItemLinked {
                        agent_id: root().agent,
                        reference: CollaborationItemRef {
                            collaboration: CollaborationId::new("foreign").expect("collaboration"),
                            item: CollaborationItemId::new("missing").expect("item"),
                            sequence: CollaborationSequence(1),
                        },
                    },
                }),
                head: branch,
            })
            .expect("append off-branch link");

        SessionPlacements::build(
            CollaborationId::new("session-root").expect("collaboration"),
            &root().agent,
            &journal,
            &[],
            &[],
        )
        .expect("selected branch ignores foreign sibling evidence");
    }

    /// ENT-1/JRN-2: another announced agent cannot choose this session owner's placement.
    #[test]
    fn selected_session_placement_rejects_a_link_from_another_announced_agent() {
        let collaboration = CollaborationId::new("session-root").expect("collaboration");
        let task = CollaborationRecord {
            id: CollaborationItemId::new("task-item").expect("item"),
            sequence: CollaborationSequence(1),
            event: CollaborationEvent::DelegationCreated {
                delegation: delegation(),
                delegator: root(),
                worker: child(),
                task: text("read the specs"),
            },
        };
        let mut journal =
            ConversationJournal::new(ConversationId::new("session-root").expect("conversation"));
        append(
            &mut journal,
            JournalEntryPayload::AgentCreated {
                agent_id: root().agent,
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        let other = AgentId::new("other-agent").expect("agent");
        append(
            &mut journal,
            JournalEntryPayload::AgentCreated {
                agent_id: other.clone(),
                label: "Other".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::CollaborationItemLinked {
                agent_id: other,
                reference: record_reference(&collaboration, &task),
            },
        );

        let error = SessionPlacements::build(collaboration, &root().agent, &journal, &[], &[task])
            .expect_err("another announced agent cannot anchor this session");
        assert!(error.to_string().contains("another session agent"));
    }

    fn warning_payload(message: &str) -> JournalEntryPayload {
        JournalEntryPayload::RuntimeWarning {
            agent_id: root().agent,
            item_id: TranscriptItemId::new(message).expect("item"),
            message: message.to_owned(),
        }
    }

    fn append_file(file: &mut RootJournalFile, payload: JournalEntryPayload) {
        let journal = file.journal();
        let sequence = journal.next_sequence();
        let head = journal.selected_head().clone();
        let record = JournalRecord::AppendEntry {
            sequence,
            record_id: JournalRecordId::new(format!("record-{}", sequence.get())).expect("record"),
            expected_head_revision: journal.head_revision(&head).expect("head revision"),
            entry: Box::new(ConversationEntry {
                id: ConversationEntryId::new(format!("entry-{}", sequence.get())).expect("entry"),
                parent_id: journal.head_target(&head).expect("head target").cloned(),
                payload,
            }),
            head,
        };
        file.append(record).expect("append session record");
    }

    fn semantic_order(
        journal: &ConversationJournal,
        collaboration: CollaborationId,
        records: &[CollaborationRecord],
    ) -> Vec<String> {
        let projection = journal
            .project(journal.selected_head())
            .expect("selected projection");
        let placements =
            SessionPlacements::build(collaboration, &root().agent, journal, &[], records)
                .expect("session placements");
        let shared = session_entries(records, &root().agent, &placements, &name, &worker);
        merge_session_entries(&projection, shared)
            .into_iter()
            .filter_map(|event| match event {
                ConversationEvent::RuntimeWarning { message, .. } => Some(message),
                ConversationEvent::TaskAssigned { task, .. } => Some(format!("task:{task}")),
                ConversationEvent::MailDelivered { summary, .. } => Some(format!("mail:{summary}")),
                _ => None,
            })
            .collect()
    }

    fn append(journal: &mut ConversationJournal, payload: JournalEntryPayload) {
        let sequence = journal.next_sequence();
        let head = journal.selected_head().clone();
        let entry = ConversationEntryId::new(format!("entry-{}", sequence.get())).expect("entry");
        journal
            .apply(JournalRecord::AppendEntry {
                sequence,
                record_id: JournalRecordId::new(format!("record-{}", sequence.get()))
                    .expect("record"),
                expected_head_revision: journal.head_revision(&head).expect("head revision"),
                entry: Box::new(ConversationEntry {
                    id: entry,
                    parent_id: journal.head_target(&head).expect("head target").cloned(),
                    payload,
                }),
                head,
            })
            .unwrap_or_else(|error| panic!("append fixture: {error:?}"));
    }
}
