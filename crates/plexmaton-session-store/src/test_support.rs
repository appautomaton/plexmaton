//! Isolated directory ownership shared by storage unit and integration tests.
use std::path::{Path, PathBuf};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "plexmaton-session-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir(&path).expect("reserve unique test directory");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    #[allow(clippy::print_stderr)] // Test cleanup diagnostics must not double-panic during unwind.
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!(
                    "test directory cleanup failed at {}: {error}",
                    self.0.display()
                );
            } else {
                panic!(
                    "test directory cleanup failed at {}: {error}",
                    self.0.display()
                );
            }
        }
    }
}
