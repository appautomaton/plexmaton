use serde::{Deserialize, Serialize};

/// Canonical physical checkout identity. Git metadata does not merge linked worktrees.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectIdentity {
    path: Vec<u8>,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl ProjectIdentity {
    pub(crate) fn from_directory(
        path: &std::path::Path,
        directory: &std::fs::File,
    ) -> Result<Self, crate::PermissionStoreError> {
        use std::os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _};
        let metadata = directory
            .metadata()
            .map_err(|e| crate::PermissionStoreError::io("inspect project root", e))?;
        let bytes = path.as_os_str().as_bytes();
        if !metadata.is_dir() || bytes.len() > 4096 {
            return Err(crate::PermissionStoreError::UnsafePath);
        }
        Ok(Self {
            path: bytes.to_vec(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Opaque personal-store directory name, bound to canonical path and physical directory identity.
    #[must_use]
    pub fn key(&self) -> String {
        use sha2::{Digest as _, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"plexmaton-project-permissions-1");
        hash.update((self.path.len() as u64).to_le_bytes());
        hash.update(&self.path);
        hash.update(self.device.to_le_bytes());
        hash.update(self.inode.to_le_bytes());
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
