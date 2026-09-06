//! Finite permission model, not production policy or a filesystem/shell sandbox.
//! The caller supplies an admitted subject. `commit` models an accepted atomic policy change.
//! This in-memory trace is not a conversation journal or a session-grant persistence format.
//! Hypotheses P1–P6 and limits are in README.md; APV-2/APV-3/APV-4 and JRN-7 constrain the model.

use std::collections::{BTreeMap, BTreeSet};

macro_rules! identity {
    ($($name:ident),+) => {$(
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        struct $name(u8);
    )+};
}
identity!(
    ConversationId,
    CodingSessionId,
    WorkspaceId,
    HeadId,
    CallId,
    GrantId,
    RuleId,
    DefinitionId,
    ArgumentsId
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileArea {
    Ordinary,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Read,
    Write { path: &'static str, area: FileArea },
    Shell { script: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Subject {
    workspace: WorkspaceId,
    definition: DefinitionId,
    definition_revision: u8,
    environment_revision: u8,
    // Finite identity of ALL canonical arguments, including content, observations and timeout.
    // Actual canonical encoding / identity derivation remains an admission integration obligation.
    arguments: ArgumentsId,
    action: Action,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Call {
    id: CallId,
    conversation: ConversationId,
    session: CodingSessionId,
    head: HeadId,
    subject: Subject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    Session(CodingSessionId),
    Workspace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Matcher {
    Exact(Subject),
    WorkspaceEdits {
        workspace: WorkspaceId,
        definition: DefinitionId,
        definition_revision: u8,
    },
}

impl Matcher {
    fn matches(self, subject: Subject) -> bool {
        match self {
            Self::Exact(expected) => expected == subject,
            Self::WorkspaceEdits {
                workspace,
                definition,
                definition_revision,
            } => {
                subject.workspace == workspace
                    && subject.definition == definition
                    && subject.definition_revision == definition_revision
                    && matches!(
                        subject.action,
                        Action::Write {
                            area: FileArea::Ordinary,
                            ..
                        }
                    )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Grant {
    id: GrantId,
    scope: Scope,
    matcher: Matcher,
}

impl Grant {
    fn covers(self, call: Call) -> bool {
        let in_scope = match self.scope {
            Scope::Session(session) => session == call.session,
            Scope::Workspace => true,
        };
        in_scope && self.matcher.matches(call.subject)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RuleEffect {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rule {
    id: RuleId,
    matcher: Matcher,
    effect: RuleEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Record {
    Granted(Grant),
    Revoked(GrantId),
    RuleAdded(Rule),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PolicyLog {
    records: Vec<Record>,
}

#[derive(Default)]
struct Snapshot {
    grants: BTreeMap<GrantId, Grant>,
    issued: BTreeSet<GrantId>,
    rules: BTreeMap<RuleId, Rule>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Authority {
    ReadDefault,
    Rule(RuleId),
    Grant(GrantId),
    Once,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    Allow(Authority),
    Ask,
    MandatoryAsk(RuleId),
    Deny(RuleId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Error {
    StalePolicy,
    InvalidRecord,
    Limit,
    WrongCall,
    Settled,
    Forbidden,
    IneffectiveGrant,
    InvalidAtExecution,
}

impl PolicyLog {
    const LIMIT: usize = 32;
    fn revision(&self) -> usize {
        self.records.len()
    }

    // This finite in-memory fold belongs to the coding session, independent of conversation
    // replacement or ancestry. Replaying a conversation cannot create another session's authority.
    fn snapshot(&self) -> Snapshot {
        let mut state = Snapshot::default();
        for record in &self.records {
            match *record {
                Record::Granted(grant) => {
                    state.issued.insert(grant.id);
                    state.grants.insert(grant.id, grant);
                }
                Record::Revoked(id) => {
                    state.grants.remove(&id);
                }
                Record::RuleAdded(rule) => {
                    state.rules.insert(rule.id, rule);
                }
            }
        }
        state
    }

    fn commit(&mut self, expected_revision: usize, record: Record) -> Result<(), Error> {
        if expected_revision != self.revision() {
            return Err(Error::StalePolicy);
        }
        if self.records.len() == Self::LIMIT {
            return Err(Error::Limit);
        }
        let snapshot = self.snapshot();
        let valid = match record {
            Record::Granted(grant) => !snapshot.issued.contains(&grant.id),
            Record::Revoked(id) => snapshot.grants.contains_key(&id),
            Record::RuleAdded(rule) => !snapshot.rules.contains_key(&rule.id),
        };
        if !valid {
            return Err(Error::InvalidRecord);
        }
        self.records.push(record);
        Ok(())
    }

    fn evaluate(&self, call: Call) -> Verdict {
        let snapshot = self.snapshot();
        let strongest = snapshot
            .rules
            .values()
            .filter(|rule| rule.matcher.matches(call.subject))
            .max_by_key(|rule| (rule.effect, rule.id));
        match strongest {
            Some(rule) if rule.effect == RuleEffect::Deny => return Verdict::Deny(rule.id),
            Some(rule) if rule.effect == RuleEffect::Ask => return Verdict::MandatoryAsk(rule.id),
            _ => {}
        }
        if let Some(grant) = snapshot.grants.values().find(|grant| grant.covers(call)) {
            return Verdict::Allow(Authority::Grant(grant.id));
        }
        if let Some(rule) = strongest {
            return Verdict::Allow(Authority::Rule(rule.id));
        }
        if call.subject.action == Action::Read {
            Verdict::Allow(Authority::ReadDefault)
        } else {
            Verdict::Ask
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingState {
    Waiting,
    Resolved,
    Cancelled,
}

struct Pending {
    call: Call,
    revision: usize,
    state: PendingState,
}

#[derive(Clone, Copy)]
enum Choice {
    Once,
    Remember(Grant),
    Deny,
}

// Consumed by dispatch, not cloneable. No history replay manufactures a one-call permit.
#[derive(Debug, Eq, PartialEq)]
struct Permit {
    call: Call,
    revision: usize,
    authority: Authority,
}

impl Pending {
    fn new(call: Call, policy: &PolicyLog) -> Self {
        Self {
            call,
            revision: policy.revision(),
            state: PendingState::Waiting,
        }
    }
    fn resolve(
        &mut self,
        current: Call,
        policy: &mut PolicyLog,
        choice: Choice,
    ) -> Result<Option<Permit>, Error> {
        if self.state != PendingState::Waiting {
            return Err(Error::Settled);
        }
        if self.call != current {
            return Err(Error::WrongCall);
        }
        if self.revision != policy.revision() {
            return Err(Error::StalePolicy);
        }
        let verdict = policy.evaluate(current);
        if matches!(verdict, Verdict::Deny(_)) {
            return Err(Error::Forbidden);
        }
        let authority = match choice {
            Choice::Deny => {
                self.state = PendingState::Resolved;
                return Ok(None);
            }
            Choice::Once => Authority::Once,
            Choice::Remember(grant) => {
                // Only offer a remembered choice when this exact choice can suppress the prompt.
                if !grant.covers(current) || matches!(verdict, Verdict::MandatoryAsk(_)) {
                    return Err(Error::IneffectiveGrant);
                }
                policy.commit(self.revision, Record::Granted(grant))?;
                Authority::Grant(grant.id)
            }
        };
        self.state = PendingState::Resolved;
        Ok(Some(Permit {
            call: current,
            revision: policy.revision(),
            authority,
        }))
    }
}

#[derive(Clone, Copy)]
enum ExecutorCheck {
    Valid,
    StaleObservation,
}

impl Permit {
    fn dispatch(
        self,
        current: Call,
        policy: &PolicyLog,
        check: ExecutorCheck,
    ) -> Result<(), Error> {
        if self.call != current {
            return Err(Error::WrongCall);
        }
        if self.revision != policy.revision() {
            return Err(Error::StalePolicy);
        }
        if matches!(check, ExecutorCheck::StaleObservation) {
            return Err(Error::InvalidAtExecution);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "policy-tests.rs"]
mod tests;
