use std::{
    collections::BTreeSet,
    io::{Read as _, Write},
};

use plexmaton_agent::{MAX_PERMISSION_ENTRIES, PermissionGrant};
use plexmaton_core::{PermissionGrantId, ProjectPermissionRevision, ProjectPermissionStoreId};
use serde::{Deserialize, Serialize};

use crate::{
    MAX_RECORD_BYTES, MAX_STORE_BYTES, MAX_STORE_RECORDS, PermissionStoreError as Error,
    ProjectIdentity, ProjectPermissionSnapshot,
};

const FORMAT: u32 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Header {
    format: u32,
    project: ProjectIdentity,
    store: ProjectPermissionStoreId,
}

impl Header {
    pub fn fresh(project: &ProjectIdentity) -> Self {
        Self {
            format: FORMAT,
            project: project.clone(),
            store: ProjectPermissionStoreId::new(format!("project-{}", uuid::Uuid::now_v7()))
                .unwrap_or_else(|_| unreachable!("a UUID creates a nonempty store id")),
        }
    }
    pub fn empty(&self) -> Folded {
        Folded {
            snapshot: ProjectPermissionSnapshot {
                can_remember: true,
                revision: ProjectPermissionRevision::Present {
                    store: self.store.clone(),
                    sequence: 0,
                },
                grants: Vec::new(),
                trusted_config: None,
            },
            seen: BTreeSet::new(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub sequence: u64,
    pub change: Change,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Change {
    Grant { grant: PermissionGrant },
    Revoke { id: PermissionGrantId },
    Trust { fingerprint: Option<[u8; 32]> },
}

#[derive(Clone)]
pub(crate) struct Folded {
    pub snapshot: ProjectPermissionSnapshot,
    seen: BTreeSet<PermissionGrantId>,
}

impl Folded {
    pub fn absent() -> Self {
        Self {
            snapshot: ProjectPermissionSnapshot {
                can_remember: true,
                revision: ProjectPermissionRevision::Absent,
                grants: Vec::new(),
                trusted_config: None,
            },
            seen: BTreeSet::new(),
        }
    }

    pub fn sequence(&self) -> u64 {
        match &self.snapshot.revision {
            ProjectPermissionRevision::Absent => 0,
            ProjectPermissionRevision::Present { sequence, .. } => *sequence,
        }
    }

    pub fn apply(&mut self, record: Record) -> Result<(), Error> {
        if record.sequence != self.sequence().checked_add(1).ok_or(Error::Capacity)? {
            return Err(Error::Corrupt);
        }
        if record.sequence > MAX_STORE_RECORDS {
            return Err(Error::Capacity);
        }
        match record.change {
            Change::Grant { grant } => {
                if self.snapshot.grants.len() >= MAX_PERMISSION_ENTRIES {
                    return Err(Error::Capacity);
                }
                if grant.id.as_str().len() > 128 || !self.seen.insert(grant.id.clone()) {
                    return Err(Error::Corrupt);
                }
                self.snapshot.grants.push(grant);
            }
            Change::Revoke { id } => {
                let index = self
                    .snapshot
                    .grants
                    .iter()
                    .position(|grant| grant.id == id)
                    .ok_or(Error::NotFound)?;
                self.snapshot.grants.remove(index);
            }
            Change::Trust { fingerprint } => self.snapshot.trusted_config = fingerprint,
        }
        let ProjectPermissionRevision::Present { sequence, .. } = &mut self.snapshot.revision
        else {
            return Err(Error::Corrupt);
        };
        *sequence = record.sequence;
        self.snapshot.can_remember = record.sequence < MAX_STORE_RECORDS
            && self.snapshot.grants.len() < MAX_PERMISSION_ENTRIES;
        Ok(())
    }
}

pub(crate) fn read(file: &std::fs::File, project: &ProjectIdentity) -> Result<Folded, Error> {
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io("read permission source", e))?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(Error::Capacity);
    }
    if bytes.is_empty() || bytes.last() != Some(&b'\n') {
        return Err(Error::Corrupt);
    }
    let mut lines = bytes[..bytes.len() - 1].split(|byte| *byte == b'\n');
    let first = lines.next().ok_or(Error::Corrupt)?;
    check_line(first)?;
    let header: Header = serde_json::from_slice(first).map_err(|_| Error::Corrupt)?;
    if header.format != FORMAT {
        return Err(Error::UnsupportedFormat);
    }
    if header.project != *project {
        return Err(Error::IdentityChanged);
    }
    if header.store.as_str().len() > 128 {
        return Err(Error::Corrupt);
    }
    let mut folded = header.empty();
    for line in lines {
        check_line(line)?;
        let record = serde_json::from_slice(line).map_err(|_| Error::Corrupt)?;
        folded.apply(record).map_err(|error| match error {
            Error::NotFound => Error::Corrupt,
            other => other,
        })?;
    }
    folded.snapshot.can_remember &=
        bytes.len() as u64 <= MAX_STORE_BYTES.saturating_sub(MAX_RECORD_BYTES as u64);
    Ok(folded)
}

fn check_line(line: &[u8]) -> Result<(), Error> {
    if line.is_empty() {
        Err(Error::Corrupt)
    } else if line.len() >= MAX_RECORD_BYTES {
        Err(Error::Capacity)
    } else {
        Ok(())
    }
}

pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) >= MAX_RECORD_BYTES {
                return Err(std::io::Error::other("permission record capacity"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bounded(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|_| Error::Capacity)?;
    bytes.0.push(b'\n');
    Ok(bytes.0)
}
