use std::{path::Path, process::Stdio};

use plexmaton_tui::StatusLineText;
use rustix::process::{Pid, Signal, kill_process_group};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

use super::config::StatusLineConfig;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Failure {
    #[error("status line: command could not start")]
    Spawn,
    #[error("status line: command I/O failed")]
    Io,
    #[error("status line: command failed")]
    Exit,
    #[error("status line: command timed out")]
    Timeout,
    #[error("status line: output too large")]
    Overflow,
    #[error("status line: invalid styled output")]
    InvalidOutput,
    #[error("status line: cancelled")]
    Cancelled,
    #[error("status line: cleanup failed")]
    Cleanup,
    #[error("status line: snapshot unavailable")]
    Snapshot,
}

struct Process {
    child: Child,
    group: Option<Pid>,
}

impl Process {
    fn kill_group(&self) -> Result<(), Failure> {
        let Some(group) = self.group else {
            return Ok(());
        };
        match kill_process_group(group, Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(_) => Err(Failure::Cleanup),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Unwinding/owner cancellation kills descendants too; Child's kill_on_drop owns reaping.
        let _ = self.kill_group();
    }
}

pub(super) async fn execute(
    config: &StatusLineConfig,
    input: Vec<u8>,
    cwd: &Path,
    credential_env: &str,
    cancel: CancellationToken,
) -> Result<StatusLineText, Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    if input.len() > 64 * 1024 {
        return Err(Failure::Overflow);
    }
    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&config.command)
        .current_dir(cwd)
        .env_remove(credential_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Failure::Spawn)?;
    let group = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .ok_or(Failure::Spawn)?;
    let mut process = Process {
        child,
        group: Some(group),
    };
    let mut stdin = process.child.stdin.take().ok_or(Failure::Io)?;
    let stdout = process.child.stdout.take().ok_or(Failure::Io)?;
    let stderr = process.child.stderr.take().ok_or(Failure::Io)?;
    let operation = async {
        let write = async {
            stdin.write_all(&input).await.map_err(|_| Failure::Io)?;
            stdin.shutdown().await.map_err(|_| Failure::Io)?;
            drop(stdin);
            Ok::<_, Failure>(())
        };
        let wait = async { process.child.wait().await.map_err(|_| Failure::Io) };
        let (_, out, _, status) = tokio::try_join!(
            write,
            read_bounded(stdout, 16 * 1024),
            read_bounded(stderr, 4096),
            wait
        )?;
        if !status.success() {
            return Err(Failure::Exit);
        }
        StatusLineText::parse(&out).map_err(|_| Failure::InvalidOutput)
    };
    let result = tokio::select! {
        result = operation => result,
        () = cancel.cancelled() => Err(Failure::Cancelled),
        () = tokio::time::sleep(config.timeout()) => Err(Failure::Timeout),
    };
    process.kill_group()?;
    // This handle stays owned across every select cancellation; cleanup has its own bound.
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        process.child.wait().await.map_err(|_| Failure::Cleanup)?;
        loop {
            match rustix::process::test_kill_process_group(group) {
                Err(rustix::io::Errno::SRCH) => return Ok::<_, Failure>(()),
                Err(_) => return Err(Failure::Cleanup),
                Ok(()) => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            }
        }
    })
    .await
    .map_err(|_| Failure::Cleanup)??;
    process.group = None;
    result
}

async fn read_bounded(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = stream.read(&mut chunk).await.map_err(|_| Failure::Io)?;
        if count == 0 {
            return Ok(result);
        }
        if result.len() + count > limit {
            return Err(Failure::Overflow);
        }
        result.extend_from_slice(&chunk[..count]);
    }
}
