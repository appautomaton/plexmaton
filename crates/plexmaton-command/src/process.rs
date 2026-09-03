//! Narrow Unix process-operation seam used by supervision fault tests (CMD-5 and CMD-6).

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
