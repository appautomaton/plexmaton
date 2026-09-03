//! Strict UTF-8 line windows that stop at local work and retention bounds.

use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
};

use thiserror::Error;

use crate::{
    ObservationId,
    observation::FileVersion,
    path::{PathError, WorkspaceRoot, validate_file_path},
};

pub const DEFAULT_READ_LINES: u16 = 200;
pub const MAX_READ_LINES: u16 = 1000;
pub const MAX_READ_BYTES: usize = 64 * 1024;
pub const MAX_LINE_BYTES: usize = 16 * 1024;
pub const MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;

/// Validated model request for one line-oriented read window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadRequest {
    path: String,
    offset: u64,
    limit: u16,
}

impl ReadRequest {
    pub fn new(path: String, offset: Option<u64>, limit: Option<u16>) -> Result<Self, ReadError> {
        validate_file_path(&path)?;
        let offset = offset.unwrap_or(1);
        let limit = limit.unwrap_or(DEFAULT_READ_LINES);
        if offset == 0 || limit == 0 || limit > MAX_READ_LINES {
            return Err(ReadError::InvalidWindow);
        }
        Ok(Self {
            path,
            offset,
            limit,
        })
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Why a successful bounded read stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadCompletion {
    EndOfFile,
    LineLimit,
    ByteLimit,
}

/// Exact model-facing read data and its session-local observation identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadResult {
    pub path: String,
    pub observation: ObservationId,
    pub start_line: u64,
    pub content: String,
    pub lines: u16,
    pub completion: ReadCompletion,
    pub next_offset: Option<u64>,
    pub bytes_examined: usize,
}

/// Typed read refusal or bounded-work failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error("the requested read window is outside its hard bounds")]
    InvalidWindow,
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("file I/O failed: {0:?}")]
    Io(io::ErrorKind),
    #[error("line {line} is not valid UTF-8")]
    InvalidUtf8 { line: u64 },
    #[error("line {line} contains a NUL byte and is treated as binary")]
    Binary { line: u64 },
    #[error("line {line} exceeds the {limit}-byte line bound")]
    LineTooLong { line: u64, limit: usize },
    #[error("reaching the requested line exceeded the {limit}-byte scan bound")]
    ScanLimit { limit: usize },
    #[error("the file changed while its window was being read")]
    ChangedDuringRead,
    #[error("read was cancelled")]
    Cancelled,
}

pub(crate) fn read(
    root: &WorkspaceRoot,
    request: &ReadRequest,
) -> Result<(ReadResult, FileVersion), ReadError> {
    read_before_version_check(root, request, || {})
}

fn read_before_version_check(
    root: &WorkspaceRoot,
    request: &ReadRequest,
    before_version_check: impl FnOnce(),
) -> Result<(ReadResult, FileVersion), ReadError> {
    let (path, file) = root.open_file(&request.path)?;
    let before = FileVersion::read(&file).map_err(|error| ReadError::Io(error.kind()))?;
    let mut reader = LineReader::new(BufReader::new(file));
    let mut line_number = 1_u64;
    let mut content = String::new();
    let mut lines = 0_u16;
    let mut completion = ReadCompletion::EndOfFile;

    while line_number < request.offset {
        if reader.next(line_number)?.is_none() {
            break;
        }
        line_number = line_number.checked_add(1).ok_or(ReadError::ScanLimit {
            limit: MAX_SCAN_BYTES,
        })?;
    }
    while line_number >= request.offset && lines < request.limit {
        let Some(line) = reader.next(line_number)? else {
            break;
        };
        if content.len().saturating_add(line.len()) > MAX_READ_BYTES {
            completion = ReadCompletion::ByteLimit;
            break;
        }
        content.push_str(&line);
        lines = lines.saturating_add(1);
        line_number = line_number.checked_add(1).ok_or(ReadError::ScanLimit {
            limit: MAX_SCAN_BYTES,
        })?;
    }
    if completion != ReadCompletion::ByteLimit && lines == request.limit && reader.has_more()? {
        completion = ReadCompletion::LineLimit;
    }
    let next_offset = (completion != ReadCompletion::EndOfFile).then_some(line_number);
    before_version_check();
    let after = FileVersion::read(reader.file()).map_err(|error| ReadError::Io(error.kind()))?;
    if before != after {
        return Err(ReadError::ChangedDuringRead);
    }
    Ok((
        ReadResult {
            path,
            observation: ObservationId::UNRECORDED,
            start_line: request.offset,
            content,
            lines,
            completion,
            next_offset,
            bytes_examined: reader.examined,
        },
        after,
    ))
}

struct LineReader {
    inner: BufReader<File>,
    examined: usize,
}

impl LineReader {
    fn new(inner: BufReader<File>) -> Self {
        Self { inner, examined: 0 }
    }

    fn next(&mut self, line: u64) -> Result<Option<String>, ReadError> {
        let scan_left = MAX_SCAN_BYTES.saturating_sub(self.examined);
        if scan_left == 0 {
            return Err(ReadError::ScanLimit {
                limit: MAX_SCAN_BYTES,
            });
        }
        let allowance = MAX_LINE_BYTES.saturating_add(1).min(scan_left);
        let mut bytes = Vec::with_capacity(allowance.min(8192));
        let read = self
            .inner
            .by_ref()
            .take(u64::try_from(allowance).unwrap_or(u64::MAX))
            .read_until(b'\n', &mut bytes)
            .map_err(|error| ReadError::Io(error.kind()))?;
        self.examined = self.examined.saturating_add(read);
        if read == 0 {
            return Ok(None);
        }
        if !bytes.ends_with(b"\n") && read == allowance {
            return Err(if scan_left <= MAX_LINE_BYTES {
                ReadError::ScanLimit {
                    limit: MAX_SCAN_BYTES,
                }
            } else {
                ReadError::LineTooLong {
                    line,
                    limit: MAX_LINE_BYTES,
                }
            });
        }
        if bytes.contains(&0) {
            return Err(ReadError::Binary { line });
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| ReadError::InvalidUtf8 { line })?;
        Ok(Some(text.to_owned()))
    }

    fn has_more(&mut self) -> Result<bool, ReadError> {
        self.inner
            .fill_buf()
            .map(|buffer| !buffer.is_empty())
            .map_err(|error| ReadError::Io(error.kind()))
    }

    fn file(&self) -> &File {
        self.inner.get_ref()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write as _,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{ReadError, ReadRequest, read_before_version_check};
    use crate::WorkspaceRoot;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn a_change_before_the_version_check_refuses_the_read() {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "plexmaton-read-version-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.join("file");
        fs::write(&path, b"before\n").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let root = WorkspaceRoot::open(&directory)
            .unwrap_or_else(|error| panic!("open workspace: {error}"));
        let request = ReadRequest::new("file".to_owned(), None, None)
            .unwrap_or_else(|error| panic!("request: {error}"));

        let result = read_before_version_check(&root, &request, || {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap_or_else(|error| panic!("open fixture: {error}"));
            file.write_all(b"after\n")
                .unwrap_or_else(|error| panic!("change fixture: {error}"));
        });

        assert_eq!(result, Err(ReadError::ChangedDuringRead));
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("remove test directory: {error}"));
    }
}
