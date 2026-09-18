//! Debug-only process boundaries exercised by the real CLI recovery journey.

use std::{io::Write as _, time::Duration};

use plexmaton_runtime::{OwnedChildControl, OwnedChildControlSnapshot, OwnedCollaborationActivity};

/// A pending Handoff captured before its activity is consumed by the collaboration owner.
pub(crate) struct PendingHandoffProcessCut(Option<OwnedChildControlSnapshot>);

impl PendingHandoffProcessCut {
    pub(crate) fn capture(activity: &OwnedCollaborationActivity) -> Self {
        let snapshot = match activity {
            OwnedCollaborationActivity::Control(snapshot)
                if snapshot.control() == OwnedChildControl::HandoffPending =>
            {
                Some(snapshot.clone())
            }
            _ => None,
        };
        Self(snapshot)
    }

    /// Park after process-local projection and before the corresponding durable mutation.
    pub(crate) fn wait(&self) {
        let Some(snapshot) = self.0.as_ref() else {
            return;
        };
        let Some(path) = std::env::var_os("PLEXMATON_TEST_PENDING_HANDOFF_READY") else {
            return;
        };
        let mut marker = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create pending-Handoff process marker");
        writeln!(
            marker,
            "{}\n{}",
            snapshot.worker().conversation,
            snapshot.revision()
        )
        .expect("write pending-Handoff process marker");
        marker
            .sync_all()
            .expect("persist pending-Handoff process marker");

        // The smoke owns and kills this process. The deadline prevents an orphaned fixture.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            std::thread::park_timeout(
                deadline.saturating_duration_since(std::time::Instant::now()),
            );
        }
        panic!("pending-Handoff fixture was not terminated by its owner");
    }
}
