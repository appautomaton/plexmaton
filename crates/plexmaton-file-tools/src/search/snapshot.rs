use std::{fs::File, io, time::Instant};

use super::{FileCancellation, MAX_SEARCH_FILE_BYTES, SearchCompletion, SearchError};

pub(super) struct PreparedFile {
    pub(super) bytes: Vec<u8>,
    pub(super) completion: SearchCompletion,
    pub(super) searchable: bool,
}

pub(super) fn prepare_search_file(
    mut file: File,
    cancellation: &FileCancellation,
    deadline: Instant,
) -> Result<PreparedFile, SearchError> {
    let length = file
        .metadata()
        .map_err(|error| SearchError::Io(error.kind()))?
        .len();
    if length > MAX_SEARCH_FILE_BYTES {
        return Ok(skipped(SearchCompletion::FileByteLimit));
    }
    let max_bytes = usize::try_from(MAX_SEARCH_FILE_BYTES).unwrap_or(usize::MAX);
    read_search_snapshot(&mut file, cancellation, deadline, max_bytes, length)
}

fn read_search_snapshot(
    reader: &mut impl io::Read,
    cancellation: &FileCancellation,
    deadline: Instant,
    max_bytes: usize,
    length_hint: u64,
) -> Result<PreparedFile, SearchError> {
    let capacity = usize::try_from(length_hint)
        .unwrap_or(max_bytes)
        .min(max_bytes);
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 8192];
    loop {
        if cancellation.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SearchError::TimedOut);
        }
        let remaining = max_bytes.saturating_sub(bytes.len());
        let allowance = remaining.saturating_add(1).min(buffer.len());
        let read = reader
            .read(&mut buffer[..allowance])
            .map_err(|error| SearchError::Io(error.kind()))?;
        if read == 0 {
            break;
        }
        if read > remaining {
            return Ok(skipped(SearchCompletion::FileByteLimit));
        }
        if buffer[..read].contains(&0) {
            return Ok(skipped(SearchCompletion::Complete));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(PreparedFile {
        bytes,
        completion: SearchCompletion::Complete,
        searchable: true,
    })
}

fn skipped(completion: SearchCompletion) -> PreparedFile {
    PreparedFile {
        bytes: Vec::new(),
        completion,
        searchable: false,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        time::{Duration, Instant},
    };

    use super::{FileCancellation, SearchCompletion, read_search_snapshot};

    #[test]
    fn a_growing_snapshot_reads_only_one_byte_past_its_hard_bound() {
        let mut source = Cursor::new(b"abcde".to_vec());
        let prepared = read_search_snapshot(
            &mut source,
            &FileCancellation::new(),
            Instant::now() + Duration::from_secs(1),
            4,
            1,
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));

        assert_eq!(prepared.completion, SearchCompletion::FileByteLimit);
        assert_eq!(source.position(), 5);
        assert!(prepared.bytes.is_empty());
    }
}
