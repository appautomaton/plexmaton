use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};

use plexmaton_agent::UnixMillis;
use plexmaton_core::SessionId;

use crate::{JournalFile, StoreError};

/// Maximum UTF-8 bytes in the portable session name used as a file stem.
const MAX_SESSION_FILE_NAME_BYTES: usize = 128;
const AUTOMATIC_NAME_ATTEMPTS: u16 = 1_000;

/// Owner-only home for the canonical per-session JSONL files.
pub struct SessionDirectory {
    path: PathBuf,
}

impl SessionDirectory {
    /// Creates or opens `PLEXMATON_HOME/sessions` without consulting process-global configuration.
    pub fn under(plexmaton_home: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = plexmaton_home.as_ref().join("sessions");
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(&path)
            .map_err(|source| StoreError::io("create sessions directory", source))?;
        ensure_owner_only_directory(&path)?;
        Ok(Self { path })
    }

    /// Creates and exclusively owns a new named session.
    pub fn create(
        &self,
        session_id: SessionId,
        created_at_unix_ms: UnixMillis,
    ) -> Result<JournalFile, StoreError> {
        JournalFile::create(self.path_for(&session_id)?, session_id, created_at_unix_ms)
    }

    /// Creates one collision-safe session whose generated identity remains a portable file name.
    pub fn create_automatic(
        &self,
        created_at_unix_ms: UnixMillis,
    ) -> Result<(SessionId, JournalFile), StoreError> {
        self.create_automatic_at(created_at_unix_ms)
    }

    fn create_automatic_at(
        &self,
        created_at_unix_ms: UnixMillis,
    ) -> Result<(SessionId, JournalFile), StoreError> {
        let unix_millis = created_at_unix_ms.get();
        for attempt in 0..AUTOMATIC_NAME_ATTEMPTS {
            let name = if attempt == 0 {
                format!("session-{unix_millis}")
            } else {
                format!("session-{unix_millis}-{attempt}")
            };
            let session_id = SessionId::new(name)
                .unwrap_or_else(|error| unreachable!("generated session id is valid: {error}"));
            match self.create(session_id.clone(), created_at_unix_ms) {
                Ok(journal) => return Ok((session_id, journal)),
                Err(StoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(StoreError::AutomaticSessionNameExhausted)
    }

    /// Opens and exclusively owns an existing named session.
    pub fn resume(&self, session_id: &SessionId) -> Result<JournalFile, StoreError> {
        let path = self.path_for(session_id)?;
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|source| StoreError::io("inspect session path", source))?;
        if metadata.file_type().is_symlink() {
            return Err(StoreError::SymlinkPath);
        }
        JournalFile::open(path)
    }

    /// Deterministic JSONL location for a portable session identity.
    pub fn path_for(&self, session_id: &SessionId) -> Result<PathBuf, StoreError> {
        let name = session_id.as_str();
        let valid = !name.is_empty()
            && name.len() <= MAX_SESSION_FILE_NAME_BYTES
            && name.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'-' | b'_'))
            });
        if !valid {
            return Err(StoreError::InvalidSessionFileName);
        }
        Ok(self.path.join(format!("{name}.jsonl")))
    }

    /// The owner-only directory containing session files.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn ensure_owner_only_directory(path: &Path) -> Result<(), StoreError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|source| StoreError::io("inspect sessions directory", source))?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::SymlinkPath);
    }
    if !metadata.is_dir() {
        return Err(StoreError::io(
            "open sessions directory",
            std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a directory"),
        ));
    }
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(StoreError::InsecureDirectoryPermissions(mode));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::UnixMillis;
    use plexmaton_core::SessionId;

    use super::SessionDirectory;
    use crate::{JournalFile, StoreError};

    fn test_home(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "plexmaton-session-path-{label}-{}",
            std::process::id()
        ));
        let _ignored = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn jrn_4_session_paths_stay_inside_an_owner_only_directory() {
        let home = test_home("safe");
        let sessions = SessionDirectory::under(&home)
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));
        let session =
            SessionId::new("work-01").unwrap_or_else(|error| panic!("session id: {error}"));
        let path = sessions
            .path_for(&session)
            .unwrap_or_else(|error| panic!("session path: {error}"));

        assert_eq!(path, home.join("sessions/work-01.jsonl"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(sessions.path())
                .unwrap_or_else(|error| panic!("sessions metadata: {error}"))
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
    }

    #[test]
    fn jrn_4_session_file_names_cannot_escape_the_sessions_directory() {
        let home = test_home("escape");
        let sessions = SessionDirectory::under(&home)
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));

        for value in ["../outside", ".hidden", "two words", "slash/name", "é"] {
            let id = SessionId::new(value)
                .unwrap_or_else(|error| panic!("semantic session id: {error}"));
            assert!(matches!(
                sessions.path_for(&id),
                Err(StoreError::InvalidSessionFileName)
            ));
        }
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
    }

    #[test]
    fn jrn_4_automatic_session_names_are_portable_and_collision_safe() {
        let home = test_home("automatic");
        let sessions = SessionDirectory::under(&home)
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));

        let (first_id, first) = sessions
            .create_automatic_at(UnixMillis::new(1_234))
            .unwrap_or_else(|error| panic!("create first automatic session: {error}"));
        let (second_id, second) = sessions
            .create_automatic_at(UnixMillis::new(1_234))
            .unwrap_or_else(|error| panic!("create second automatic session: {error}"));

        assert_eq!(first_id.as_str(), "session-1234");
        assert_eq!(second_id.as_str(), "session-1234-1");
        assert_eq!(first.path(), home.join("sessions/session-1234.jsonl"));
        assert_eq!(second.path(), home.join("sessions/session-1234-1.jsonl"));
        drop((first, second));
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
    }

    #[cfg(unix)]
    #[test]
    fn jrn_4_an_insecure_existing_sessions_directory_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;

        let home = test_home("insecure");
        let sessions = SessionDirectory::under(&home)
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));
        let mut permissions = std::fs::metadata(sessions.path())
            .unwrap_or_else(|error| panic!("sessions metadata: {error}"))
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(sessions.path(), permissions)
            .unwrap_or_else(|error| panic!("weaken fixture permissions: {error}"));

        assert!(matches!(
            SessionDirectory::under(&home),
            Err(StoreError::InsecureDirectoryPermissions(0o755))
        ));
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
    }

    #[cfg(unix)]
    #[test]
    fn jrn_4_symlinked_session_directory_and_file_are_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::fs::symlink;

        let home = test_home("symlink-directory");
        let target = test_home("symlink-target");
        std::fs::create_dir(&home).unwrap_or_else(|error| panic!("create home: {error}"));
        std::fs::create_dir(&target).unwrap_or_else(|error| panic!("create target: {error}"));
        let mut target_permissions = std::fs::metadata(&target)
            .unwrap_or_else(|error| panic!("target metadata: {error}"))
            .permissions();
        target_permissions.set_mode(0o700);
        std::fs::set_permissions(&target, target_permissions)
            .unwrap_or_else(|error| panic!("secure target: {error}"));
        symlink(&target, home.join("sessions"))
            .unwrap_or_else(|error| panic!("link sessions directory: {error}"));
        assert!(matches!(
            SessionDirectory::under(&home),
            Err(StoreError::SymlinkPath)
        ));
        std::fs::remove_file(home.join("sessions"))
            .unwrap_or_else(|error| panic!("remove sessions link: {error}"));
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
        std::fs::remove_dir_all(target)
            .unwrap_or_else(|error| panic!("remove test target: {error}"));

        let home = test_home("symlink-file");
        let sessions = SessionDirectory::under(&home)
            .unwrap_or_else(|error| panic!("open sessions directory: {error}"));
        let target = home.join("outside.jsonl");
        std::fs::write(&target, b"not a journal")
            .unwrap_or_else(|error| panic!("write target: {error}"));
        let mut target_permissions = std::fs::metadata(&target)
            .unwrap_or_else(|error| panic!("target metadata: {error}"))
            .permissions();
        target_permissions.set_mode(0o600);
        std::fs::set_permissions(&target, target_permissions)
            .unwrap_or_else(|error| panic!("secure target: {error}"));
        let session =
            SessionId::new("linked").unwrap_or_else(|error| panic!("session id: {error}"));
        let linked = sessions
            .path_for(&session)
            .unwrap_or_else(|error| panic!("linked path: {error}"));
        symlink(&target, &linked).unwrap_or_else(|error| panic!("link session file: {error}"));
        assert!(matches!(
            JournalFile::open(&linked),
            Err(StoreError::SymlinkPath)
        ));
        assert!(matches!(
            sessions.resume(&session),
            Err(StoreError::SymlinkPath)
        ));
        std::fs::remove_dir_all(home).unwrap_or_else(|error| panic!("remove test home: {error}"));
    }
}
