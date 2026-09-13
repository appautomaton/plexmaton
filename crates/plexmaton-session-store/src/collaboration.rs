//! Exclusive JSONL admission for a canonical collaboration ledger (COL-4/COL-5).
//!
//! This blocking component must be owned by a runtime writer worker, never the TUI event loop.
//! It acknowledges accepted facts only; it does not schedule recipients or claim consumption.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use plexmaton_agent::collaboration::{
    CollaborationError, CollaborationEvent, CollaborationLedger, CollaborationLimits, ItemReceipt,
    MailEndpoint, Preparation, ResolvedTurnAdmission,
};
use plexmaton_core::{CollaborationId, CollaborationItemId, DelegationId};

use crate::{
    JournalRecovery, WriterState, ensure_owner_only, lock_writer, reject_symlink,
    secure_open_options,
};

mod authority;
mod codec;
mod error;
#[cfg(test)]
mod tests;

pub use authority::{
    CollaborationControl, DelegatedConversationControl, DelegatedConversationProvenance,
    ExecutionPermit, ExecutionReservation, ExecutionTicket,
};
pub use error::{CollaborationAppendFailure, CollaborationAttempt, CollaborationStoreError};

use authority::{
    AuthorityGate, WriterLease, WriterOwner, begin_release, complete_release, freeze_authority,
    register_delegation, require_all_quiescent, writer_authority,
};

/// One locked collaboration file and its reduction; no independent mailbox or delegation file.
pub struct CollaborationFile {
    path: PathBuf,
    file: File,
    ledger: CollaborationLedger,
    recovery: JournalRecovery,
    state: WriterState,
    owner: Arc<WriterOwner>,
    lease: Arc<WriterLease>,
    authority: Arc<Mutex<AuthorityGate>>,
}

impl CollaborationFile {
    /// Creates an owner-only file under an owner-only parent, refusing an existing destination.
    pub fn create(
        path: impl AsRef<Path>,
        id: CollaborationId,
        limits: CollaborationLimits,
    ) -> Result<Self, CollaborationStoreError> {
        let path = path.as_ref();
        let ledger = CollaborationLedger::new(id, limits)?;
        let header = codec::header(&ledger)?;
        codec::ensure_parent(path)?;
        let mut file = secure_open_options()
            .read(true)
            .append(true)
            .create_new(true)
            .open(path)
            .map_err(CollaborationStoreError::Io)?;
        if let Err(error) = lock_writer(&file) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(error.into());
        }
        if let Err(error) = file.write_all(&header) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(CollaborationStoreError::Io(error));
        }
        let (owner, lease, authority) = match writer_authority(&file, &ledger) {
            Ok(authority) => authority,
            Err(error) => {
                drop(file);
                let _cleanup = std::fs::remove_file(path);
                return Err(error);
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            file,
            ledger,
            recovery: JournalRecovery::Clean,
            state: WriterState::Ready,
            owner,
            lease,
            authority,
        })
    }

    /// Locks and validates before repairing a syntactically incomplete final tail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CollaborationStoreError> {
        let path = path.as_ref();
        codec::check_parent(path)?;
        reject_symlink(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
            .map_err(CollaborationStoreError::Io)?;
        ensure_owner_only(&file)?;
        lock_writer(&file)?;
        let (ledger, recovery) = codec::load(&mut file, path)?;
        let (owner, lease, authority) = writer_authority(&file, &ledger)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            ledger,
            recovery,
            state: WriterState::Ready,
            owner,
            lease,
            authority,
        })
    }

    /// Only acknowledged records appear here, even after an uncertain write.
    #[must_use]
    pub const fn ledger(&self) -> &CollaborationLedger {
        &self.ledger
    }

    /// Projects acknowledged mail only while this file owner can prove its canonical prefix.
    pub fn project_mail(
        &self,
        endpoint: &MailEndpoint,
    ) -> Result<plexmaton_agent::collaboration::CollaborationMailProjection, CollaborationStoreError>
    {
        if self.state == WriterState::Poisoned {
            return Err(CollaborationStoreError::WriterPoisoned);
        }
        self.ledger.project_mail(endpoint).map_err(Into::into)
    }

    /// Resolves exact session references only while the canonical prefix remains provable.
    pub fn resolve_turns(
        &self,
        references: &[plexmaton_agent::collaboration::CollaborationItemRef],
    ) -> Result<Vec<Arc<ResolvedTurnAdmission>>, CollaborationStoreError> {
        if self.state == WriterState::Poisoned {
            return Err(CollaborationStoreError::WriterPoisoned);
        }
        references
            .iter()
            .map(|reference| self.ledger.resolve_turn(reference).map_err(Into::into))
            .collect()
    }

    /// Physical tail repair performed by this open; callers can surface the isolated evidence.
    #[must_use]
    pub const fn recovery(&self) -> &JournalRecovery {
        &self.recovery
    }

    /// Exact file whose writer lock is held until this owner drops.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Shared authority handle used by the owned runtime boundary, never by presentation.
    #[must_use]
    pub fn control(&self) -> CollaborationControl {
        authority::control(&self.owner, &self.lease, &self.authority)
    }

    /// Binds controller inspection and Main execution to one canonical child Conversation.
    pub fn delegated_control(
        &self,
        delegation: &DelegationId,
    ) -> Result<DelegatedConversationControl, CollaborationStoreError> {
        let (record, delegator, worker) = self
            .ledger
            .records()
            .iter()
            .find_map(|record| match &record.event {
                CollaborationEvent::DelegationCreated {
                    delegation: created,
                    delegator,
                    worker,
                    ..
                } if created == delegation => Some((record, delegator, worker)),
                _ => None,
            })
            .ok_or(CollaborationError::UnknownDelegation)?;
        let provenance = DelegatedConversationProvenance::new(
            self.ledger.id().clone(),
            delegation.clone(),
            self.ledger.item_reference(&record.id)?,
            delegator.clone(),
            worker.clone(),
        );
        Ok(authority::delegated_control(self.control(), provenance))
    }

    /// Validates one canonical worker admission without granting execution authority.
    pub fn execution_ticket(
        &self,
        delegation: &DelegationId,
        admission: &ResolvedTurnAdmission,
    ) -> Result<ExecutionTicket, CollaborationStoreError> {
        let canonical = self.ledger.resolve_turn(admission.reference())?;
        let view = self
            .ledger
            .delegation(delegation)
            .ok_or(CollaborationError::UnknownDelegation)?;
        if canonical.as_ref() != admission
            || admission.admission().boundary.recipient != view.worker
        {
            return Err(CollaborationStoreError::InvalidExecutionTicket);
        }
        authority::ensure_ticket_available(&self.authority, delegation, admission.reference())?;
        Ok(authority::ticket(
            &self.authority,
            delegation.clone(),
            Arc::new(admission.clone()),
        ))
    }

    /// Refuses owner shutdown while any reservation or permit still retains execution authority.
    pub fn require_quiescent(&self) -> Result<(), CollaborationStoreError> {
        if self.state == WriterState::Poisoned {
            return Err(CollaborationStoreError::WriterPoisoned);
        }
        require_all_quiescent(&self.authority)
    }

    /// Returns the canonical original receipt on exact retry, including after reopen.
    pub fn admit(
        &mut self,
        id: CollaborationItemId,
        event: CollaborationEvent,
    ) -> Result<ItemReceipt, CollaborationAppendFailure> {
        self.admit_with(CollaborationAttempt { id, event }, |file, bytes| {
            file.write_all(bytes)
        })
    }

    fn admit_with(
        &mut self,
        attempt: CollaborationAttempt,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<ItemReceipt, CollaborationAppendFailure> {
        let outcome = self.admit_inner(&attempt, write);
        outcome.map_err(|error| CollaborationAppendFailure::new(error, attempt))
    }

    fn admit_inner(
        &mut self,
        attempt: &CollaborationAttempt,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<ItemReceipt, CollaborationStoreError> {
        // Even a previously accepted retry cannot acknowledge through an uncertain writer.
        if self.state == WriterState::Poisoned {
            return Err(CollaborationStoreError::WriterPoisoned);
        }
        let record = match self
            .ledger
            .prepare(attempt.id.clone(), attempt.event.clone())?
        {
            Preparation::Existing(receipt) => return Ok(receipt),
            Preparation::Append(record) => record,
        };
        let bytes = crate::codec::encode_line(&record)?;
        if let CollaborationEvent::HandoffCompleted { delegation, .. } = &record.event {
            begin_release(&self.authority, delegation)?;
        }
        if let Err(error) = write(&mut self.file, &bytes) {
            self.state = WriterState::Poisoned;
            freeze_authority(&self.authority);
            return Err(CollaborationStoreError::WriteUncertain(error));
        }
        let event = record.event.clone();
        let receipt = self.ledger.apply(*record).map_err(|error| {
            self.state = WriterState::Poisoned;
            freeze_authority(&self.authority);
            CollaborationStoreError::ReductionUncertain(error)
        })?;
        match event {
            CollaborationEvent::DelegationCreated { delegation, .. } => {
                register_delegation(&self.authority, delegation)?;
            }
            CollaborationEvent::HandoffCompleted { delegation, .. } => {
                complete_release(&self.authority, &delegation)?;
            }
            _ => {}
        }
        Ok(receipt)
    }
}

impl Drop for CollaborationFile {
    fn drop(&mut self) {
        // COL-5: release an unretained writer before an inherited duplicate can prolong its flock.
        // A live reservation or permit owns another lease reference and must keep the lock instead.
        if Arc::strong_count(&self.lease) == 1 {
            let _release = self.file.unlock();
        }
    }
}
