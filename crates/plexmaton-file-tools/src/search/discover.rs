//! Bounded candidate discovery; names are never trusted as file authority.

use std::{
    fs::File,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::Instant,
};

use super::{
    CHANNEL_CAPACITY, DirectoryDriver, FileCancellation, MAX_RG_STDERR_BYTES, MAX_SEARCH_FILES,
    POLL_INTERVAL, SearchCompletion, SearchError,
    pump::{DiscoveryEvent, collect_bounded, pump_paths},
    reap, successful_rg_status,
};

pub(super) struct DiscoveryResult {
    pub(super) candidates: Vec<String>,
    pub(super) completion: Option<SearchCompletion>,
    pub(super) transport_bytes: usize,
}

pub(super) fn discover(
    executable: &Path,
    driver: &DirectoryDriver,
    directory: File,
    glob: Option<&str>,
    cancellation: &FileCancellation,
    deadline: Instant,
    byte_limit: usize,
) -> Result<DiscoveryResult, SearchError> {
    let mut command = discovery_command(executable, driver, glob);
    command
        .stdin(Stdio::from(directory))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| SearchError::Spawn(error.kind()))?;
    collect(&mut child, cancellation, deadline, byte_limit)
}

fn discovery_command(executable: &Path, driver: &DirectoryDriver, glob: Option<&str>) -> Command {
    let mut command = Command::new(&driver.program);
    command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .args(&driver.prefix)
        .arg(executable)
        .args([
            "--files",
            "--null",
            "--no-config",
            "--no-require-git",
            "--no-follow",
        ]);
    if let Some(glob) = glob {
        command.arg("--glob").arg(glob);
    }
    command.arg("--").arg(".");
    command
}

fn collect(
    child: &mut Child,
    cancellation: &FileCancellation,
    deadline: Instant,
    byte_limit: usize,
) -> Result<DiscoveryResult, SearchError> {
    let Some(stdout) = child.stdout.take() else {
        reap(child);
        return Err(SearchError::ReaderFailed);
    };
    let Some(stderr) = child.stderr.take() else {
        reap(child);
        return Err(SearchError::ReaderFailed);
    };
    let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
    let stdout_task = std::thread::Builder::new()
        .name("plexmaton-rg-files".to_owned())
        .spawn(move || pump_paths(stdout, sender, byte_limit))
        .map_err(|_| {
            reap(child);
            SearchError::ReaderFailed
        })?;
    let stderr_task = match std::thread::Builder::new()
        .name("plexmaton-rg-files-stderr".to_owned())
        .spawn(move || collect_bounded(stderr, MAX_RG_STDERR_BYTES))
    {
        Ok(task) => task,
        Err(_) => {
            reap(child);
            drop(receiver);
            stdout_task.join().map_err(|_| SearchError::ReaderFailed)?;
            return Err(SearchError::ReaderFailed);
        }
    };
    let mut candidates = Vec::new();
    let mut acquired_bytes = 0_usize;
    let mut completion = None;
    let mut stdout_finished = false;
    let mut process_status = None;
    let terminal = loop {
        if cancellation.is_cancelled() {
            break Err(SearchError::Cancelled);
        }
        if Instant::now() >= deadline {
            break Err(SearchError::TimedOut);
        }
        if process_status.is_none() {
            match child.try_wait() {
                Ok(status) => process_status = status,
                Err(error) => break Err(SearchError::Io(error.kind())),
            }
        }
        if stdout_finished && process_status.is_some() {
            break Ok(acquired_bytes);
        }
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(DiscoveryEvent::Candidate { path, bytes }) => {
                acquired_bytes = bytes;
                candidates.push(path);
                if candidates.len() == MAX_SEARCH_FILES {
                    completion = Some(SearchCompletion::FileLimit);
                    break Ok(acquired_bytes);
                }
            }
            Ok(DiscoveryEvent::Finished { bytes }) => {
                acquired_bytes = bytes;
                stdout_finished = true;
            }
            Ok(DiscoveryEvent::TransportLimit { bytes }) => {
                completion = Some(SearchCompletion::TransportByteLimit);
                break Ok(bytes);
            }
            Ok(DiscoveryEvent::Failed(error)) => break Err(error),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) if stdout_finished => {}
            Err(RecvTimeoutError::Disconnected) => break Err(SearchError::ReaderFailed),
        }
    };
    if terminal.as_ref().is_err() || completion.is_some() {
        let _already_exited = child.kill();
    }
    let status = match process_status {
        Some(status) => Ok(status),
        None => child.wait().map_err(|error| SearchError::Io(error.kind())),
    };
    if status.is_err() {
        reap(child);
    }
    drop(receiver);
    let stdout = stdout_task.join();
    let stderr = stderr_task.join();
    let transport_bytes = terminal?;
    let status = status?;
    stdout.map_err(|_| SearchError::ReaderFailed)?;
    let stderr = stderr
        .map_err(|_| SearchError::ReaderFailed)?
        .map_err(SearchError::Io)?;
    if completion.is_none() && !successful_rg_status(status) {
        return Err(SearchError::ProcessFailed {
            code: status.code(),
            stderr: stderr.text(),
            omitted_bytes: stderr.omitted(),
        });
    }
    Ok(DiscoveryResult {
        candidates,
        completion,
        transport_bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt as _, path::Path, path::PathBuf};

    use super::{DirectoryDriver, discovery_command};

    /// WFS-4: the trusted driver prefix is exact argv before ripgrep and model-derived arguments.
    #[test]
    fn directory_driver_prefix_precedes_rg_and_model_arguments_exactly() {
        let fixed_non_utf8_path = OsString::from_vec(b"/trusted/fixed-\xff-path".to_vec());
        let driver = DirectoryDriver {
            program: PathBuf::from("/trusted/bin/plexmaton"),
            prefix: vec![
                OsString::from("--internal-rg-driver"),
                fixed_non_utf8_path.clone(),
            ],
        };
        let command = discovery_command(
            Path::new("/trusted/bin/rg"),
            &driver,
            Some("*.rs; touch should-not-run"),
        );

        assert_eq!(command.get_program(), "/trusted/bin/plexmaton");
        assert_eq!(
            command.get_args().map(OsString::from).collect::<Vec<_>>(),
            vec![
                OsString::from("--internal-rg-driver"),
                fixed_non_utf8_path,
                OsString::from("/trusted/bin/rg"),
                OsString::from("--files"),
                OsString::from("--null"),
                OsString::from("--no-config"),
                OsString::from("--no-require-git"),
                OsString::from("--no-follow"),
                OsString::from("--glob"),
                OsString::from("*.rs; touch should-not-run"),
                OsString::from("--"),
                OsString::from("."),
            ]
        );
    }
}
