//! Physical writer ownership retained by live collaboration authority.

use std::fs::File;
use std::sync::{Arc, Mutex};

use super::AuthorityGate;

pub(crate) type WriterAuthority = (
    Arc<WriterOwner>,
    Arc<WriterLease>,
    Arc<Mutex<AuthorityGate>>,
);

pub(crate) struct WriterLease {
    lock_file: File,
}

impl WriterLease {
    pub(super) fn new(file: &File) -> std::io::Result<Self> {
        Ok(Self {
            lock_file: file.try_clone()?,
        })
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        let _release = self.lock_file.unlock();
    }
}

pub(crate) struct WriterOwner;
