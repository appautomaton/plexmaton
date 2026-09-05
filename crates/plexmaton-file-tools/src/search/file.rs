use std::{
    fs::File,
    io::{self, Write as _},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread::JoinHandle,
    time::Instant,
};

use crate::observation::FileVersion;

use super::{
    BoundedBytes, CHANNEL_CAPACITY, FileCancellation, MAX_RG_STDERR_BYTES, POLL_INTERVAL,
    PumpEvent, SearchCompletion, SearchError, SearchRequest, SearchResult, SearchRunner,
    collect_bounded, pump_stdout, reap, snapshot::prepare_search_file, successful_rg_status,
};

#[derive(Clone, Copy)]
pub(super) struct FileSearchLimits {
    pub(super) matches: u16,
    pub(super) retained_bytes: usize,
    pub(super) transport_bytes: usize,
    pub(super) validation_only: bool,
}

impl SearchRunner {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_file(
        &self,
        file: File,
        logical_path: &str,
        request: &SearchRequest,
        cancellation: &FileCancellation,
        deadline: Instant,
        limits: FileSearchLimits,
    ) -> Result<SearchResult, SearchError> {
        if cancellation.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SearchError::TimedOut);
        }
        if let Some(result) = bounded_before_start(limits) {
            return Ok(result);
        }
        let version_file = file
            .try_clone()
            .map_err(|error| SearchError::Io(error.kind()))?;
        let before =
            FileVersion::read(&version_file).map_err(|error| SearchError::Io(error.kind()))?;
        let prepared = prepare_search_file(file, cancellation, deadline)?;
        let after_snapshot =
            FileVersion::read(&version_file).map_err(|error| SearchError::Io(error.kind()))?;
        if before != after_snapshot {
            return Err(SearchError::ChangedDuringSearch);
        }
        if !prepared.searchable {
            return Ok(empty_bounded_result(prepared.completion));
        }
        let result = self.search_input(
            prepared.bytes,
            logical_path,
            request,
            cancellation,
            deadline,
            limits,
        )?;
        let after =
            FileVersion::read(&version_file).map_err(|error| SearchError::Io(error.kind()))?;
        if before != after {
            return Err(SearchError::ChangedDuringSearch);
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_input(
        &self,
        input: Vec<u8>,
        logical_path: &str,
        request: &SearchRequest,
        cancellation: &FileCancellation,
        deadline: Instant,
        limits: FileSearchLimits,
    ) -> Result<SearchResult, SearchError> {
        if cancellation.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SearchError::TimedOut);
        }
        if let Some(result) = bounded_before_start(limits) {
            return Ok(result);
        }
        let mut command = Command::new(&self.executable);
        command
            .env_clear()
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .args([
                "--json",
                "--no-config",
                "--no-require-git",
                "--no-follow",
                "--color=never",
                "--line-number",
                "--max-columns=2048",
                "--max-columns-preview",
                "--max-filesize=5M",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if limits.validation_only {
            command.arg("--files-with-matches");
        }
        command.arg("--").arg(&request.pattern).arg("-");
        let mut child = command
            .spawn()
            .map_err(|error| SearchError::Spawn(error.kind()))?;
        self.collect_file(
            logical_path,
            cancellation,
            deadline,
            limits,
            &mut child,
            input,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_file(
        &self,
        logical_path: &str,
        cancellation: &FileCancellation,
        deadline: Instant,
        limits: FileSearchLimits,
        child: &mut Child,
        input: Vec<u8>,
    ) -> Result<SearchResult, SearchError> {
        let Some(mut stdin) = child.stdin.take() else {
            reap(child);
            return Err(SearchError::ReaderFailed);
        };
        let Some(stdout) = child.stdout.take() else {
            reap(child);
            return Err(SearchError::ReaderFailed);
        };
        let Some(stderr) = child.stderr.take() else {
            reap(child);
            return Err(SearchError::ReaderFailed);
        };
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let byte_limit = limits.transport_bytes;
        let stdout_task = match std::thread::Builder::new()
            .name("plexmaton-rg-stdout".to_owned())
            .spawn(move || pump_stdout(stdout, sender, byte_limit))
        {
            Ok(task) => task,
            Err(_) => {
                reap(child);
                return Err(SearchError::ReaderFailed);
            }
        };
        let stderr_task = match std::thread::Builder::new()
            .name("plexmaton-rg-stderr".to_owned())
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
        let stdin_task = match std::thread::Builder::new()
            .name("plexmaton-rg-stdin".to_owned())
            .spawn(move || stdin.write_all(&input).map_err(|error| error.kind()))
        {
            Ok(task) => task,
            Err(_) => {
                reap(child);
                drop(receiver);
                let stdout_result = stdout_task.join();
                let stderr_result = stderr_task.join();
                stdout_result.map_err(|_| SearchError::ReaderFailed)?;
                stderr_result
                    .map_err(|_| SearchError::ReaderFailed)?
                    .map_err(SearchError::Io)?;
                return Err(SearchError::ReaderFailed);
            }
        };
        collect_file_events(
            logical_path,
            cancellation,
            deadline,
            limits,
            child,
            receiver,
            stdin_task,
            stdout_task,
            stderr_task,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_file_events(
    logical_path: &str,
    cancellation: &FileCancellation,
    deadline: Instant,
    limits: FileSearchLimits,
    child: &mut Child,
    receiver: Receiver<PumpEvent>,
    stdin_task: JoinHandle<Result<(), io::ErrorKind>>,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<Result<BoundedBytes, io::ErrorKind>>,
) -> Result<SearchResult, SearchError> {
    let mut matches = Vec::new();
    let mut seen = 0_u64;
    let mut acquired_bytes = 0_usize;
    let mut retained_bytes = 0_usize;
    let mut completion = SearchCompletion::Complete;
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
            Ok(PumpEvent::Match { mut found, bytes }) => {
                acquired_bytes = bytes;
                seen = seen.saturating_add(1);
                found.path.clear();
                found.path.push_str(logical_path);
                let found_bytes = found.path.len().saturating_add(found.preview.len());
                if retained_bytes.saturating_add(found_bytes) > limits.retained_bytes {
                    completion = SearchCompletion::RetainedByteLimit;
                    break Ok(acquired_bytes);
                }
                retained_bytes = retained_bytes.saturating_add(found_bytes);
                matches.push(found);
                if matches.len() == usize::from(limits.matches) {
                    completion = SearchCompletion::MatchLimit;
                    break Ok(acquired_bytes);
                }
            }
            Ok(PumpEvent::Finished { bytes }) => {
                acquired_bytes = bytes;
                stdout_finished = true;
            }
            Ok(PumpEvent::TransportLimit { bytes }) => {
                completion = SearchCompletion::TransportByteLimit;
                break Ok(bytes);
            }
            Ok(PumpEvent::Failed(error)) => break Err(error),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) if stdout_finished => {}
            Err(RecvTimeoutError::Disconnected) => break Err(SearchError::ReaderFailed),
        }
    };
    if terminal.as_ref().is_err() || completion != SearchCompletion::Complete {
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
    let stdin = stdin_task.join();
    let stdout = stdout_task.join();
    let stderr = stderr_task.join();
    let transport_bytes = terminal?;
    let status = status?;
    stdout.map_err(|_| SearchError::ReaderFailed)?;
    match stdin.map_err(|_| SearchError::ReaderFailed)? {
        Ok(()) | Err(io::ErrorKind::BrokenPipe) => {}
        Err(error) => return Err(SearchError::Io(error)),
    }
    let stderr = stderr
        .map_err(|_| SearchError::ReaderFailed)?
        .map_err(SearchError::Io)?;
    if completion == SearchCompletion::Complete && !successful_rg_status(status) {
        return Err(SearchError::ProcessFailed {
            code: status.code(),
            stderr: stderr.text(),
            omitted_bytes: stderr.omitted(),
        });
    }
    Ok(SearchResult {
        matches,
        completion,
        matches_seen: seen,
        transport_bytes,
        stderr: stderr.text(),
        stderr_omitted_bytes: stderr.omitted(),
    })
}

fn empty_bounded_result(completion: SearchCompletion) -> SearchResult {
    SearchResult {
        matches: Vec::new(),
        completion,
        matches_seen: 0,
        transport_bytes: 0,
        stderr: String::new(),
        stderr_omitted_bytes: 0,
    }
}

fn bounded_before_start(limits: FileSearchLimits) -> Option<SearchResult> {
    if limits.matches == 0 {
        Some(empty_bounded_result(SearchCompletion::MatchLimit))
    } else if limits.transport_bytes == 0 {
        Some(empty_bounded_result(SearchCompletion::TransportByteLimit))
    } else if limits.retained_bytes == 0 {
        Some(empty_bounded_result(SearchCompletion::RetainedByteLimit))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
        sync::mpsc::{self, TryRecvError},
        time::{Duration, Instant},
    };

    use super::super::MAX_SEARCH_FILE_BYTES;
    use super::{
        File, FileCancellation, FileSearchLimits, PumpEvent, SearchError, SearchRequest,
        SearchRunner, collect_bounded, collect_file_events,
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn a_cancelled_file_boundary_precedes_an_oversized_completion() {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plexmaton-search-cancel-{}-{suffix}",
            std::process::id()
        ));
        let file = File::create(&path).unwrap_or_else(|error| panic!("create fixture: {error}"));
        file.set_len(MAX_SEARCH_FILE_BYTES + 1)
            .unwrap_or_else(|error| panic!("size fixture: {error}"));
        let request = SearchRequest::new("value".to_owned(), None, None, None)
            .unwrap_or_else(|error| panic!("request: {error}"));
        let cancellation = FileCancellation::new();
        cancellation.cancel();

        assert_eq!(
            SearchRunner::new("/bin/false", "/bin/false").search_file(
                file,
                "file",
                &request,
                &cancellation,
                Instant::now() + Duration::from_secs(1),
                FileSearchLimits {
                    matches: 1,
                    retained_bytes: 1024,
                    transport_bytes: 1024,
                    validation_only: false,
                },
            ),
            Err(SearchError::Cancelled)
        );
        fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    #[cfg(unix)]
    #[test]
    fn a_cancelled_search_returns_only_after_reader_join() {
        let cancellation = FileCancellation::new();
        cancellation.cancel();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec /bin/sleep 30"])
            .spawn()
            .unwrap_or_else(|error| panic!("spawn fixture: {error}"));
        let (events, receiver) = mpsc::sync_channel(1);
        let (first_queued, first_ready) = mpsc::sync_channel(0);
        let (reader_waiting, waiting) = mpsc::sync_channel(0);
        let (release, released) = mpsc::sync_channel(0);
        let stdout_task = std::thread::spawn(move || {
            events
                .send(PumpEvent::Finished { bytes: 0 })
                .unwrap_or_else(|_| panic!("queue first event"));
            first_queued
                .send(())
                .unwrap_or_else(|_| panic!("signal first event"));
            assert!(events.send(PumpEvent::Finished { bytes: 0 }).is_err());
            reader_waiting
                .send(())
                .unwrap_or_else(|_| panic!("signal reader barrier"));
            released
                .recv()
                .unwrap_or_else(|_| panic!("release reader barrier"));
        });
        first_ready
            .recv()
            .unwrap_or_else(|_| panic!("first event was not queued"));
        let stderr_task = std::thread::spawn(|| collect_bounded(&b""[..], 64));
        let stdin_task = std::thread::spawn(|| Ok(()));
        let (finished, result) = mpsc::sync_channel(1);
        let collector = std::thread::spawn(move || {
            let collected = collect_file_events(
                ".",
                &cancellation,
                Instant::now() + Duration::from_secs(1),
                FileSearchLimits {
                    matches: 1,
                    retained_bytes: 1024,
                    transport_bytes: 1024,
                    validation_only: false,
                },
                &mut child,
                receiver,
                stdin_task,
                stdout_task,
                stderr_task,
            );
            finished
                .send(collected)
                .unwrap_or_else(|_| panic!("send collector result"));
        });

        waiting
            .recv()
            .unwrap_or_else(|_| panic!("reader did not reach barrier"));
        assert_eq!(result.try_recv(), Err(TryRecvError::Empty));
        release
            .send(())
            .unwrap_or_else(|_| panic!("release reader"));
        assert_eq!(
            result
                .recv()
                .unwrap_or_else(|_| panic!("collector did not finish")),
            Err(SearchError::Cancelled)
        );
        collector
            .join()
            .unwrap_or_else(|_| panic!("collector panicked"));
    }

    #[cfg(unix)]
    #[test]
    fn a_reader_panic_still_joins_every_sibling_before_returning() {
        let cancellation = FileCancellation::new();
        cancellation.cancel();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec /bin/sleep 30"])
            .spawn()
            .unwrap_or_else(|error| panic!("spawn fixture: {error}"));
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(sender);
        let stdin_task = std::thread::spawn(|| Ok(()));
        let stdout_task = std::thread::spawn(|| panic!("reader fixture"));
        let (stderr_waiting, waiting) = mpsc::sync_channel(0);
        let (release, released) = mpsc::sync_channel(0);
        let stderr_task = std::thread::spawn(move || {
            stderr_waiting
                .send(())
                .unwrap_or_else(|_| panic!("signal stderr barrier"));
            released
                .recv()
                .unwrap_or_else(|_| panic!("release stderr barrier"));
            collect_bounded(&b""[..], 64)
        });
        let (finished, result) = mpsc::sync_channel(1);
        let collector = std::thread::spawn(move || {
            let collected = collect_file_events(
                ".",
                &cancellation,
                Instant::now() + Duration::from_secs(1),
                FileSearchLimits {
                    matches: 1,
                    retained_bytes: 1024,
                    transport_bytes: 1024,
                    validation_only: false,
                },
                &mut child,
                receiver,
                stdin_task,
                stdout_task,
                stderr_task,
            );
            finished
                .send(collected)
                .unwrap_or_else(|_| panic!("send collector result"));
        });

        waiting
            .recv()
            .unwrap_or_else(|_| panic!("stderr did not reach barrier"));
        assert_eq!(result.try_recv(), Err(TryRecvError::Empty));
        release
            .send(())
            .unwrap_or_else(|_| panic!("release stderr"));
        assert_eq!(
            result
                .recv()
                .unwrap_or_else(|_| panic!("collector did not finish")),
            Err(SearchError::Cancelled)
        );
        collector
            .join()
            .unwrap_or_else(|_| panic!("collector panicked"));
    }
}
