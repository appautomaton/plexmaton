use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct TestWorkspace {
    path: PathBuf,
}

impl TestWorkspace {
    pub fn new() -> Self {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plexmaton-file-tools-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("create test workspace: {error}"));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create test parent: {error}"));
        }
        fs::write(path, bytes).unwrap_or_else(|error| panic!("write test file: {error}"));
    }

    #[cfg(unix)]
    #[allow(dead_code)] // This shared module is compiled once per integration-test binary.
    pub fn executable(&self, relative: &str, source: &str) -> PathBuf {
        let body = source
            .strip_prefix("#!/bin/sh\n")
            .unwrap_or_else(|| panic!("ripgrep fixture must be a /bin/sh script"));
        let source = format!(
            "#!/bin/sh\ncase \" $* \" in *\" --files-with-matches \"*) exit 1;; esac\n{body}"
        );
        self.raw_executable(relative, &source)
    }

    #[cfg(unix)]
    #[allow(dead_code)] // This shared module is compiled once per integration-test binary.
    pub fn raw_executable(&self, relative: &str, source: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        self.write(relative, source.as_bytes());
        let path = self.path.join(relative);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("make test helper executable: {error}"));
        path
    }
}

#[cfg(unix)]
#[allow(dead_code)] // This shared module is compiled once per integration-test binary.
pub fn search_driver() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_plexmaton-rg-driver"))
}

#[cfg(unix)]
#[allow(dead_code)] // This shared module is compiled once per integration-test binary.
pub fn rg_executable() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("rg");
        if candidate.is_file() {
            return fs::canonicalize(&candidate)
                .unwrap_or_else(|error| panic!("canonicalize ripgrep {candidate:?}: {error}"));
        }
    }
    panic!("ripgrep is required for file-tool integration tests");
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path)
            .unwrap_or_else(|error| panic!("remove test workspace: {error}"));
    }
}
