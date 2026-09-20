//! Narrow Unix process-operation seam used by supervision fault tests (CMD-5 and CMD-6).

use std::time::Duration;
use std::{io, process::ExitStatus};

use rustix::{
    io::Errno,
    process::{Pid, Signal, kill_process_group, test_kill_process_group},
};
use tokio::process::Child;

pub(crate) trait ProcessOperations {
    fn record_spawn(&self, _process_group: Pid) {}

    fn kill_process_group(&self, process_group: Pid, signal: Signal) -> Result<(), Errno>;

    fn test_kill_process_group(&self, process_group: Pid) -> Result<(), Errno>;

    fn before_child_wait(&self) -> io::Result<()> {
        Ok(())
    }

    fn before_child_try_wait(&self) -> io::Result<()> {
        Ok(())
    }

    /// How long SIGTERM is given before escalation (CMD-5).
    ///
    /// Production answers with the product's own grace. A test proving that a cooperative command
    /// is never escalated widens it instead of betting that a shell wins a fixed wall-clock race
    /// against every sibling test spawning processes beside it. Widening can only strengthen that
    /// test: with a grace no loaded machine can exhaust, a SIGKILL means a real hang.
    fn termination_grace(&self) -> Duration {
        crate::executor::TERMINATION_GRACE
    }
}

pub(crate) struct OsProcessOperations;

impl ProcessOperations for OsProcessOperations {
    fn kill_process_group(&self, process_group: Pid, signal: Signal) -> Result<(), Errno> {
        kill_process_group(process_group, signal)
    }

    fn test_kill_process_group(&self, process_group: Pid) -> Result<(), Errno> {
        test_kill_process_group(process_group)
    }
}

pub(crate) async fn wait_for_child(
    child: &mut Child,
    operations: &impl ProcessOperations,
) -> io::Result<ExitStatus> {
    operations.before_child_wait()?;
    child.wait().await
}

pub(crate) fn try_wait_child(
    child: &mut Child,
    operations: &impl ProcessOperations,
) -> io::Result<Option<ExitStatus>> {
    operations.before_child_try_wait()?;
    child.try_wait()
}
