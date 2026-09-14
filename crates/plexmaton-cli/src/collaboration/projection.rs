//! What the collaboration log means on screen, decided in one place.
//!
//! Split from the composition root because the two answer different questions and fail in different
//! ways. This half is a pure rule over one record, so it can be checked without a runtime, a
//! provider or a child; the root owns opening the log, naming children and driving the pass.

use plexmaton_agent::collaboration::{CollaborationEvent, CollaborationRecord, MailEndpoint};
use plexmaton_core::{AgentId, ConversationEvent, DelegationId, TranscriptItemId};

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
        CollaborationEvent::TurnAdmitted { .. } => Vec::new(),
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
        // An explicit change of controller, which `ui-ux.md` names as its own entry kind and
        // `ConversationEvent` has no variant for yet.
        CollaborationEvent::HandoffCompleted { .. } => Vec::new(),
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
        CollaborationEvent, CollaborationRecord, CollaborationSequence, CollaborationText,
        MailEndpoint, MailEnvelope,
    };
    use plexmaton_core::{AgentId, CollaborationItemId, ConversationEvent, DelegationId, MailId};

    use super::entries;

    fn endpoint(conversation: &str, agent: &str) -> MailEndpoint {
        MailEndpoint {
            conversation: plexmaton_core::ConversationId::new(conversation).expect("conversation"),
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
            expected: plexmaton_agent::collaboration::DelegationRevision(0),
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
}
