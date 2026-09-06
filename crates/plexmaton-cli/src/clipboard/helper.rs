//! The clipboard's single owned subprocess; no terminal handle or projection enters this module.

use std::{io, process::ExitStatus, time::Duration};

use tokio::{
    io::AsyncWriteExt as _,
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

const CLEANUP_DEADLINE: Duration = Duration::from_millis(500);

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Completion {
    Accepted,
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Failure {
    #[error("clipboard helper I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("clipboard helper exited with {0}")]
    Rejected(ExitStatus),
    #[error("clipboard helper exceeded its deadline")]
    TimedOut,
    #[error("clipboard helper cleanup failed: {0}")]
    Cleanup(io::Error),
}

impl From<Failure> for io::Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Io(error) | Failure::Cleanup(error) => error,
            Failure::TimedOut => Self::new(io::ErrorKind::TimedOut, failure),
            Failure::Rejected(_) => Self::other(failure),
        }
    }
}

/// One deadline covers stdin and acknowledgement; cancellation closes stdin, kills and reaps.
/// The caller retains this future until completion, including across losing select branches.
pub(super) async fn copy_through_command(
    mut command: Command,
    text: String,
    deadline: Duration,
    cancel: CancellationToken,
) -> Result<Completion, Failure> {
    if cancel.is_cancelled() {
        return Ok(Completion::Cancelled);
    }
    let mut child = command.spawn()?;
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("clipboard helper stdin was not piped"))?;
        stdin.write_all(text.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
        child.wait().await
    };
    let result = tokio::select! {
        biased;
        () = cancel.cancelled() => Ok(Completion::Cancelled),
        result = tokio::time::timeout(deadline, operation) => match result {
            Ok(Ok(status)) if status.success() => Ok(Completion::Accepted),
            Ok(Ok(status)) => Err(Failure::Rejected(status)),
            Ok(Err(error)) => Err(Failure::Io(error)),
            Err(_) => Err(Failure::TimedOut),
        },
    };
    if !matches!(result, Ok(Completion::Accepted) | Err(Failure::Rejected(_))) {
        reap(&mut child).await.map_err(Failure::Cleanup)?;
    }
    result
}

async fn reap(child: &mut Child) -> io::Result<()> {
    // The child may have exited between cancellation and the kill. Its wait result, not a raced
    // signal error, establishes that ownership has ended before the next helper can start.
    let _ = child.start_kill();
    match tokio::time::timeout(CLEANUP_DEADLINE, child.wait()).await {
        Ok(result) => result.map(|_| ()),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "clipboard helper could not be reaped",
        )),
    }
}
