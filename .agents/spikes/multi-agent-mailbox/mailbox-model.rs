//! Bounded, dependency-free mailbox research model; not a production implementation.
//! Durable vectors represent acknowledged atomic records, not filesystem behavior.
//! Run from the repository root using the command in the adjacent README.md.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MailId(u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wake {
    QueueOnly,
    StartIfIdle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mail {
    id: MailId,
    payload: &'static str,
    wake: Wake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Full,
    IdentityConflict,
    Closed,
    UnknownMail,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Admission {
    #[default]
    Open,
    Closed,
}

#[derive(Clone, Default)]
struct ItemLog {
    accepted: Vec<Mail>,
    admission: Admission,
}

impl ItemLog {
    fn accept(&mut self, mail: Mail) -> Result<(), Refusal> {
        if let Some(existing) = self.accepted.iter().find(|item| item.id == mail.id) {
            return if *existing == mail {
                Ok(())
            } else {
                Err(Refusal::IdentityConflict)
            };
        }
        if self.admission == Admission::Closed {
            return Err(Refusal::Closed);
        }
        // A hard lifetime bound keeps this finite model honest; production needs retention.
        if self.accepted.len() == 2 {
            return Err(Refusal::Full);
        }
        self.accepted.push(mail);
        Ok(())
    }

    fn close(&mut self) {
        self.admission = Admission::Closed;
    }
}

#[derive(Clone, Default)]
struct RecipientJournal {
    // One entry stands for an atomic context-boundary record containing the mail reference.
    boundaries: Vec<MailId>,
}

impl RecipientJournal {
    fn include(&mut self, log: &ItemLog, id: MailId) -> Result<(), Refusal> {
        if !log.accepted.iter().any(|mail| mail.id == id) {
            return Err(Refusal::UnknownMail);
        }
        if !self.boundaries.contains(&id) {
            self.boundaries.push(id);
        }
        Ok(())
    }

    fn context<'a>(&self, log: &'a ItemLog) -> Vec<&'a str> {
        self.boundaries
            .iter()
            .map(|id| {
                log.accepted
                    .iter()
                    .find(|mail| mail.id == *id)
                    .expect("included references resolve to the retained canonical item log")
                    .payload
            })
            .collect()
    }
}

fn pending<'a>(log: &'a ItemLog, journal: &RecipientJournal) -> Vec<&'a Mail> {
    log.accepted
        .iter()
        .filter(|mail| !journal.boundaries.contains(&mail.id))
        .collect()
}

fn ready(log: &ItemLog, journal: &RecipientJournal, idle: bool, stopped: bool) -> bool {
    idle && !stopped
        && pending(log, journal)
            .iter()
            .any(|mail| mail.wake == Wake::StartIfIdle)
}

fn mail(id: u8, wake: Wake) -> Mail {
    Mail {
        id: MailId(id),
        payload: "bounded finding; artifact reference omitted in this model",
        wake,
    }
}

#[test]
fn every_handoff_crash_cut_recovers_one_context_reference() {
    // JRN-7 motivates the ordering; this model does not prove the production invariant.
    // Cuts: before accept, after accept, after include, after a volatile reply.
    for cut in 0..=3 {
        let message = mail(1, Wake::StartIfIdle);
        let mut log = ItemLog::default();
        let mut journal = RecipientJournal::default();
        if cut >= 1 {
            log.accept(message).unwrap();
        }
        if cut >= 2 {
            journal.include(&log, message.id).unwrap();
        }
        // Restart retains only acknowledged records. Retry uses the same identity.
        let mut reopened_log = log.clone();
        let mut reopened_journal = journal.clone();
        reopened_log.accept(message).unwrap();
        let ids: Vec<_> = pending(&reopened_log, &reopened_journal)
            .iter()
            .map(|mail| mail.id)
            .collect();
        for id in ids {
            reopened_journal.include(&reopened_log, id).unwrap();
        }
        // A courier may retry an already included item after losing its acknowledgement.
        reopened_journal.include(&reopened_log, message.id).unwrap();
        assert_eq!(reopened_log.accepted.len(), 1, "crash cut {cut}");
        assert_eq!(reopened_journal.context(&reopened_log), [message.payload]);
        assert!(pending(&reopened_log, &reopened_journal).is_empty());
    }
}

#[test]
fn accepted_mail_survives_restart_without_a_sender_retry() {
    let mut log = ItemLog::default();
    log.accept(mail(1, Wake::StartIfIdle)).unwrap();
    let reopened = log.clone();
    let mut journal = RecipientJournal::default();
    assert!(ready(&reopened, &journal, true, false));
    journal.include(&reopened, MailId(1)).unwrap();
    assert_eq!(journal.context(&reopened).len(), 1);
}

#[test]
fn duplicate_receipt_is_available_when_full_or_closed() {
    let mut log = ItemLog::default();
    log.accept(mail(1, Wake::QueueOnly)).unwrap();
    log.accept(mail(2, Wake::QueueOnly)).unwrap();
    assert_eq!(log.accept(mail(3, Wake::QueueOnly)), Err(Refusal::Full));
    log.accept(mail(1, Wake::QueueOnly)).unwrap();
    log.close();
    log.accept(mail(1, Wake::QueueOnly)).unwrap();
    assert_eq!(log.accept(mail(3, Wake::QueueOnly)), Err(Refusal::Closed));
    assert_eq!(log.accepted.len(), 2);
}

#[test]
fn reused_identity_cannot_change_payload_or_wake_policy() {
    let mut log = ItemLog::default();
    log.accept(mail(1, Wake::QueueOnly)).unwrap();
    assert_eq!(
        log.accept(mail(1, Wake::StartIfIdle)),
        Err(Refusal::IdentityConflict)
    );
    let mut changed = mail(1, Wake::QueueOnly);
    changed.payload = "different finding";
    assert_eq!(log.accept(changed), Err(Refusal::IdentityConflict));
}

#[test]
fn admission_and_wake_are_separate_and_stop_wins() {
    let mut log = ItemLog::default();
    let mut journal = RecipientJournal::default();
    log.accept(mail(1, Wake::QueueOnly)).unwrap();
    assert!(!ready(&log, &journal, true, false));
    log.accept(mail(2, Wake::StartIfIdle)).unwrap();
    assert!(ready(&log, &journal, true, false));
    assert!(!ready(&log, &journal, false, false));
    assert!(!ready(&log, &journal, true, true));
    for id in [MailId(1), MailId(2)] {
        journal.include(&log, id).unwrap();
    }
    assert!(!ready(&log, &journal, true, false));
}

#[test]
fn unaccepted_mail_cannot_enter_context() {
    // JRN-7 ordering, with an atomic-record assumption rather than real I/O.
    let log = ItemLog::default();
    let mut journal = RecipientJournal::default();
    assert_eq!(journal.include(&log, MailId(1)), Err(Refusal::UnknownMail));
    assert!(journal.context(&log).is_empty());
}

#[test]
fn close_and_accept_have_explicit_linearized_outcomes() {
    let mut send_first = ItemLog::default();
    send_first.accept(mail(1, Wake::QueueOnly)).unwrap();
    send_first.close();
    assert_eq!(pending(&send_first, &RecipientJournal::default()).len(), 1);
    let mut close_first = ItemLog::default();
    close_first.close();
    assert_eq!(
        close_first.accept(mail(1, Wake::QueueOnly)),
        Err(Refusal::Closed)
    );
}

#[test]
fn lost_notification_does_not_erase_durable_readiness() {
    let mut log = ItemLog::default();
    let journal = RecipientJournal::default();
    let ready_before_subscription = ready(&log, &journal, true, false);
    log.accept(mail(1, Wake::StartIfIdle)).unwrap();
    // A send between check and subscribe can lose an edge. Recheck canonical state.
    assert!(!ready_before_subscription);
    assert!(ready(&log, &journal, true, false));
}
