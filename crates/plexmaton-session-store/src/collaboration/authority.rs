use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use plexmaton_agent::collaboration::{
    CollaborationError, CollaborationEvent, CollaborationItemRef, CollaborationLedger,
    DelegationController, MailEndpoint, ResolvedTurnAdmission,
};
use plexmaton_core::{CollaborationId, DelegationId};

use super::CollaborationStoreError;

mod lease;
pub(super) use lease::{WriterAuthority, WriterLease, WriterOwner};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlState {
    Main,
    Releasing,
    User,
    Frozen,
}

pub(super) struct AuthorityGate {
    collaboration: CollaborationId,
    delegations: BTreeMap<DelegationId, ControlState>,
    active: BTreeMap<DelegationId, u64>,
    issued: BTreeSet<CollaborationItemRef>,
    next_permit: u64,
}

impl AuthorityGate {
    fn from_ledger(ledger: &CollaborationLedger) -> Self {
        let mut delegations = BTreeMap::new();
        let mut issued = BTreeSet::new();
        for record in ledger.records() {
            match &record.event {
                CollaborationEvent::DelegationCreated { delegation, .. } => {
                    let state = match ledger
                        .delegation(delegation)
                        .expect("canonical creation has a delegation view")
                        .controller
                    {
                        DelegationController::Main => ControlState::Main,
                        DelegationController::User => ControlState::User,
                    };
                    delegations.insert(delegation.clone(), state);
                }
                CollaborationEvent::TurnAdmitted { .. } => {
                    issued.insert(
                        ledger
                            .item_reference(&record.id)
                            .expect("canonical turn record has its own reference"),
                    );
                }
                _ => {}
            }
        }
        Self {
            collaboration: ledger.id().clone(),
            delegations,
            active: BTreeMap::new(),
            issued,
            next_permit: 0,
        }
    }
}

/// Read-only admission identity. It carries no authority to execute a child turn.
pub struct ExecutionTicket {
    authority: Weak<Mutex<AuthorityGate>>,
    delegation: DelegationId,
    admission: Arc<ResolvedTurnAdmission>,
}

impl ExecutionTicket {
    /// Exact admitted turn this ticket describes.
    #[must_use]
    pub fn admission(&self) -> &CollaborationItemRef {
        self.admission.reference()
    }
}

/// Shared control handle; it does not keep an idle file or writer lock alive.
#[derive(Clone)]
pub struct CollaborationControl {
    owner: Weak<WriterOwner>,
    authority: Weak<Mutex<AuthorityGate>>,
    lease: Weak<WriterLease>,
}

/// File-scoped controller query and Main execution boundary for one delegated Conversation.
#[derive(Clone)]
pub struct DelegatedConversationControl {
    control: CollaborationControl,
    provenance: DelegatedConversationProvenance,
}

/// Immutable child origin reconstructed from the canonical delegation-creation item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegatedConversationProvenance {
    collaboration: CollaborationId,
    delegation: DelegationId,
    creation: CollaborationItemRef,
    delegator: MailEndpoint,
    worker: MailEndpoint,
}

impl DelegatedConversationProvenance {
    pub(super) fn new(
        collaboration: CollaborationId,
        delegation: DelegationId,
        creation: CollaborationItemRef,
        delegator: MailEndpoint,
        worker: MailEndpoint,
    ) -> Self {
        Self {
            collaboration,
            delegation,
            creation,
            delegator,
            worker,
        }
    }

    /// Collaboration whose canonical log owns this origin.
    #[must_use]
    pub const fn collaboration(&self) -> &CollaborationId {
        &self.collaboration
    }

    /// Delegation created by the referenced canonical item.
    #[must_use]
    pub const fn delegation(&self) -> &DelegationId {
        &self.delegation
    }

    /// Exact immutable item that created this child.
    #[must_use]
    pub const fn creation(&self) -> &CollaborationItemRef {
        &self.creation
    }

    /// Exact parent endpoint fixed at delegation creation.
    #[must_use]
    pub const fn delegator(&self) -> &MailEndpoint {
        &self.delegator
    }

    /// Exact child endpoint fixed at delegation creation.
    #[must_use]
    pub const fn worker(&self) -> &MailEndpoint {
        &self.worker
    }
}

impl DelegatedConversationControl {
    /// Canonical origin retained alongside controller authority.
    #[must_use]
    pub const fn provenance(&self) -> &DelegatedConversationProvenance {
        &self.provenance
    }

    /// Collaboration identity the child provenance names.
    #[must_use]
    pub const fn collaboration(&self) -> &CollaborationId {
        self.provenance.collaboration()
    }

    /// Delegation whose child Conversation this handle controls.
    #[must_use]
    pub const fn delegation(&self) -> &DelegationId {
        self.provenance.delegation()
    }

    /// Exact child Conversation and Agent identity fixed at delegation creation.
    #[must_use]
    pub const fn worker(&self) -> &MailEndpoint {
        self.provenance.worker()
    }

    /// Current durable input owner; a Handoff in flight remains Main until acknowledged.
    pub fn controller(&self) -> Result<DelegationController, CollaborationStoreError> {
        self.control.controller(self.delegation())
    }

    /// Reserves Main authority before accepting input for this child.
    pub fn reserve_execution(&self) -> Result<ExecutionReservation, CollaborationStoreError> {
        self.control.reserve_execution(self.delegation())
    }

    /// Issues one exact child execution without accepting unreserved input first.
    pub fn issue_execution(
        &self,
        ticket: ExecutionTicket,
    ) -> Result<ExecutionPermit, CollaborationStoreError> {
        self.validate_ticket(&ticket, ticket.admission.as_ref())?;
        self.control.issue_execution(ticket)
    }

    /// Checks a still-inspectable ticket before the caller consumes its held reservation.
    pub fn validate_ticket(
        &self,
        ticket: &ExecutionTicket,
        admission: &ResolvedTurnAdmission,
    ) -> Result<(), CollaborationStoreError> {
        if !Weak::ptr_eq(&self.control.authority, &ticket.authority)
            || self.delegation() != &ticket.delegation
            || self.worker() != &admission.admission().boundary.recipient
            || ticket.admission.as_ref() != admission
        {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        Ok(())
    }

    /// Validates that a retained permit owns this physical child admission.
    pub fn validate_execution(
        &self,
        permit: &ExecutionPermit,
        admission: &ResolvedTurnAdmission,
    ) -> Result<(), CollaborationStoreError> {
        if !Weak::ptr_eq(&self.control.authority, &Arc::downgrade(&permit.authority))
            || self.delegation() != &permit.delegation
            || self.worker() != &admission.admission().boundary.recipient
            || permit.admission.as_ref() != admission
        {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        let gate = lock_gate(&permit.authority)?;
        require_main(gate.delegations.get(self.delegation()))?;
        if gate.active.get(self.delegation()) != Some(&permit.permit) {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        Ok(())
    }
}

impl CollaborationControl {
    fn controller(
        &self,
        delegation: &DelegationId,
    ) -> Result<DelegationController, CollaborationStoreError> {
        let _owner = self
            .owner
            .upgrade()
            .ok_or(CollaborationStoreError::ControlOwnerClosed)?;
        let authority = self
            .authority
            .upgrade()
            .ok_or(CollaborationStoreError::ControlOwnerClosed)?;
        let gate = lock_gate(&authority)?;
        match gate.delegations.get(delegation) {
            Some(ControlState::Main | ControlState::Releasing) => Ok(DelegationController::Main),
            Some(ControlState::User) => Ok(DelegationController::User),
            Some(ControlState::Frozen) => Err(CollaborationStoreError::WriterPoisoned),
            None => Err(CollaborationStoreError::InvalidExecutionTicket),
        }
    }

    /// Reserves the sole Main-owned slot before accepting input or starting a session write.
    pub fn reserve_execution(
        &self,
        delegation: &DelegationId,
    ) -> Result<ExecutionReservation, CollaborationStoreError> {
        let _owner = self
            .owner
            .upgrade()
            .ok_or(CollaborationStoreError::ControlOwnerClosed)?;
        let authority = self
            .authority
            .upgrade()
            .ok_or(CollaborationStoreError::ControlOwnerClosed)?;
        let lease = self
            .lease
            .upgrade()
            .ok_or(CollaborationStoreError::ControlOwnerClosed)?;
        let mut gate = lock_gate(&authority)?;
        require_main(gate.delegations.get(delegation))?;
        let permit = reserve_slot(&mut gate, delegation)?;
        let collaboration = gate.collaboration.clone();
        drop(gate);
        Ok(ExecutionReservation {
            authority,
            _lease: lease,
            collaboration,
            delegation: delegation.clone(),
            permit,
            armed: true,
        })
    }

    /// Converts an inspectable ticket into one bounded queued-or-active execution permit.
    pub fn issue_execution(
        &self,
        ticket: ExecutionTicket,
    ) -> Result<ExecutionPermit, CollaborationStoreError> {
        let delegation = ticket.delegation.clone();
        self.reserve_execution(&delegation)?.bind(ticket)
    }
}

/// Main-owned input or queued work that has not yet bound a canonical turn admission.
pub struct ExecutionReservation {
    authority: Arc<Mutex<AuthorityGate>>,
    _lease: Arc<WriterLease>,
    collaboration: CollaborationId,
    delegation: DelegationId,
    permit: u64,
    armed: bool,
}

impl ExecutionReservation {
    /// Binds retained Main authority to one exact, previously unissued durable admission.
    pub fn bind(
        mut self,
        ticket: ExecutionTicket,
    ) -> Result<ExecutionPermit, CollaborationStoreError> {
        if !Weak::ptr_eq(&ticket.authority, &Arc::downgrade(&self.authority)) {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        let mut gate = lock_gate(&self.authority)?;
        if ticket.admission().collaboration != self.collaboration
            || ticket.delegation != self.delegation
            || gate.active.get(&self.delegation) != Some(&self.permit)
        {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        require_main(gate.delegations.get(&self.delegation))?;
        if !gate.issued.insert(ticket.admission().clone()) {
            return Err(CollaborationStoreError::ExecutionAlreadyIssued);
        }
        drop(gate);
        self.armed = false;
        Ok(ExecutionPermit {
            authority: Arc::clone(&self.authority),
            _lease: Arc::clone(&self._lease),
            delegation: self.delegation.clone(),
            admission: ticket.admission,
            permit: self.permit,
        })
    }
}

impl Drop for ExecutionReservation {
    fn drop(&mut self) {
        if self.armed {
            release_slot(&self.authority, &self.delegation, self.permit);
        }
    }
}

/// Non-cloneable authority retained from input acceptance through owned execution.
pub struct ExecutionPermit {
    authority: Arc<Mutex<AuthorityGate>>,
    _lease: Arc<WriterLease>,
    delegation: DelegationId,
    admission: Arc<ResolvedTurnAdmission>,
    permit: u64,
}

impl ExecutionPermit {
    /// Exact admitted turn authorized by this permit.
    #[must_use]
    pub fn admission(&self) -> &CollaborationItemRef {
        self.admission.reference()
    }
}

impl Drop for ExecutionPermit {
    fn drop(&mut self) {
        release_slot(&self.authority, &self.delegation, self.permit);
    }
}

pub(super) fn writer_authority(
    file: &File,
    ledger: &CollaborationLedger,
) -> Result<WriterAuthority, CollaborationStoreError> {
    let owner = Arc::new(WriterOwner);
    let lease = Arc::new(WriterLease::new(file).map_err(CollaborationStoreError::Io)?);
    let authority = Arc::new(Mutex::new(AuthorityGate::from_ledger(ledger)));
    Ok((owner, lease, authority))
}

pub(super) fn control(
    owner: &Arc<WriterOwner>,
    lease: &Arc<WriterLease>,
    authority: &Arc<Mutex<AuthorityGate>>,
) -> CollaborationControl {
    CollaborationControl {
        owner: Arc::downgrade(owner),
        authority: Arc::downgrade(authority),
        lease: Arc::downgrade(lease),
    }
}

pub(super) fn delegated_control(
    control: CollaborationControl,
    provenance: DelegatedConversationProvenance,
) -> DelegatedConversationControl {
    DelegatedConversationControl {
        control,
        provenance,
    }
}

pub(super) fn ticket(
    authority: &Arc<Mutex<AuthorityGate>>,
    delegation: DelegationId,
    admission: Arc<ResolvedTurnAdmission>,
) -> ExecutionTicket {
    ExecutionTicket {
        authority: Arc::downgrade(authority),
        delegation,
        admission,
    }
}

pub(super) fn ensure_ticket_available(
    authority: &Arc<Mutex<AuthorityGate>>,
    delegation: &DelegationId,
    admission: &CollaborationItemRef,
) -> Result<(), CollaborationStoreError> {
    let gate = lock_gate(authority)?;
    require_main(gate.delegations.get(delegation))?;
    if gate.issued.contains(admission) {
        return Err(CollaborationStoreError::ExecutionAlreadyIssued);
    }
    Ok(())
}

pub(super) fn require_all_quiescent(
    authority: &Arc<Mutex<AuthorityGate>>,
) -> Result<(), CollaborationStoreError> {
    let gate = lock_gate(authority)?;
    if gate
        .delegations
        .values()
        .any(|state| *state == ControlState::Frozen)
    {
        return Err(CollaborationStoreError::WriterPoisoned);
    }
    if !gate.active.is_empty() {
        return Err(CollaborationStoreError::ControlNotQuiescent);
    }
    Ok(())
}

pub(super) fn begin_release(
    authority: &Arc<Mutex<AuthorityGate>>,
    delegation: &DelegationId,
) -> Result<(), CollaborationStoreError> {
    let mut gate = lock_gate(authority)?;
    if gate.active.contains_key(delegation) {
        return Err(CollaborationStoreError::ControlNotQuiescent);
    }
    let state = gate
        .delegations
        .get_mut(delegation)
        .ok_or(CollaborationStoreError::InvalidExecutionTicket)?;
    match state {
        ControlState::Main => {
            *state = ControlState::Releasing;
            Ok(())
        }
        ControlState::Releasing => Err(CollaborationStoreError::ControlNotQuiescent),
        ControlState::User => Err(CollaborationError::HandoffCompleted.into()),
        ControlState::Frozen => Err(CollaborationStoreError::WriterPoisoned),
    }
}

pub(super) fn register_delegation(
    authority: &Arc<Mutex<AuthorityGate>>,
    delegation: DelegationId,
) -> Result<(), CollaborationStoreError> {
    let mut gate = lock_gate(authority)?;
    gate.delegations.insert(delegation, ControlState::Main);
    Ok(())
}

pub(super) fn complete_release(
    authority: &Arc<Mutex<AuthorityGate>>,
    delegation: &DelegationId,
) -> Result<(), CollaborationStoreError> {
    let mut gate = lock_gate(authority)?;
    let state = gate
        .delegations
        .get_mut(delegation)
        .ok_or(CollaborationStoreError::InvalidExecutionTicket)?;
    if *state != ControlState::Releasing {
        return Err(CollaborationStoreError::ControlPoisoned);
    }
    *state = ControlState::User;
    Ok(())
}

pub(super) fn freeze_authority(authority: &Arc<Mutex<AuthorityGate>>) {
    let Ok(mut gate) = authority.lock() else {
        return;
    };
    for state in gate.delegations.values_mut() {
        *state = ControlState::Frozen;
    }
}

fn lock_gate(
    authority: &Arc<Mutex<AuthorityGate>>,
) -> Result<MutexGuard<'_, AuthorityGate>, CollaborationStoreError> {
    authority
        .lock()
        .map_err(|_| CollaborationStoreError::ControlPoisoned)
}

fn require_main(state: Option<&ControlState>) -> Result<(), CollaborationStoreError> {
    match state {
        Some(ControlState::Main) => Ok(()),
        Some(ControlState::Releasing) => Err(CollaborationStoreError::ControlNotQuiescent),
        Some(ControlState::User) => Err(CollaborationError::HandoffCompleted.into()),
        Some(ControlState::Frozen) => Err(CollaborationStoreError::WriterPoisoned),
        None => Err(CollaborationStoreError::InvalidExecutionTicket),
    }
}

fn reserve_slot(
    gate: &mut AuthorityGate,
    delegation: &DelegationId,
) -> Result<u64, CollaborationStoreError> {
    if gate.active.contains_key(delegation) {
        return Err(CollaborationStoreError::ExecutionBusy);
    }
    gate.next_permit = gate
        .next_permit
        .checked_add(1)
        .ok_or(CollaborationStoreError::ExecutionBusy)?;
    let permit = gate.next_permit;
    gate.active.insert(delegation.clone(), permit);
    Ok(permit)
}

fn release_slot(authority: &Arc<Mutex<AuthorityGate>>, delegation: &DelegationId, permit: u64) {
    let Ok(mut gate) = authority.lock() else {
        return;
    };
    if gate.active.get(delegation) == Some(&permit) {
        gate.active.remove(delegation);
    }
}
