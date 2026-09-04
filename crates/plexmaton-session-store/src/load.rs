use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use plexmaton_agent::{JournalRecord, SessionJournal};

use crate::codec::{decode_header, read_line};
use crate::{StoreError, secure_open_options};

/// Repair applied to a syntactically incomplete final file tail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalRecovery {
    /// Every line was complete and newline-terminated.
    Clean,
    /// The last complete JSON value was missing only its newline.
    AddedFinalNewline,
    /// A malformed final fragment was preserved separately and removed from the journal.
    IsolatedFinalTail {
        /// Sibling containing the exact bytes removed from the canonical file.
        path: PathBuf,
        /// Number of isolated bytes.
        bytes: u64,
    },
}

pub(crate) struct Loaded {
    pub(crate) journal: SessionJournal,
    pub(crate) recovery: JournalRecovery,
}

enum Repair {
    None,
    AddNewline,
    Isolate { valid_bytes: u64, tail: Vec<u8> },
}

pub(crate) fn load(file: &mut File, path: &Path) -> Result<Loaded, StoreError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| StoreError::io("seek start", source))?;
    let (journal, repair) = {
        let mut reader = BufReader::new(&mut *file);
        decode(&mut reader)?
    };
    let recovery = match repair {
        Repair::None => JournalRecovery::Clean,
        Repair::AddNewline => {
            file.seek(SeekFrom::End(0))
                .map_err(|source| StoreError::io("seek repair", source))?;
            file.write_all(b"\n")
                .map_err(|source| StoreError::io("repair newline", source))?;
            JournalRecovery::AddedFinalNewline
        }
        Repair::Isolate { valid_bytes, tail } => {
            let tail_path = isolate_tail(path, &tail)?;
            file.set_len(valid_bytes)
                .map_err(|source| StoreError::io("truncate invalid tail", source))?;
            JournalRecovery::IsolatedFinalTail {
                path: tail_path,
                bytes: u64::try_from(tail.len()).unwrap_or(u64::MAX),
            }
        }
    };
    Ok(Loaded { journal, recovery })
}

fn decode(reader: &mut impl BufRead) -> Result<(SessionJournal, Repair), StoreError> {
    let Some(header) = read_line(reader, 1)? else {
        return Err(StoreError::MissingHeader);
    };
    let header_json = without_newline(&header.bytes, header.terminated);
    let session_id = decode_header(header_json)?;
    let mut journal = SessionJournal::new(session_id);
    let mut valid_bytes = u64::try_from(header.bytes.len()).unwrap_or(u64::MAX);
    if !header.terminated {
        return Ok((journal, Repair::AddNewline));
    }

    let mut line_number = 2_u64;
    loop {
        let Some(line) = read_line(reader, line_number)? else {
            return Ok((journal, Repair::None));
        };
        let json = without_newline(&line.bytes, line.terminated);
        let record = match serde_json::from_slice::<JournalRecord>(json) {
            Ok(record) => record,
            Err(source) => {
                let final_line = reader
                    .fill_buf()
                    .map_err(|error| StoreError::io("inspect tail", error))?
                    .is_empty();
                let syntactically_incomplete = matches!(
                    source.classify(),
                    serde_json::error::Category::Eof | serde_json::error::Category::Syntax
                );
                if final_line && !line.terminated && syntactically_incomplete {
                    return Ok((
                        journal,
                        Repair::Isolate {
                            valid_bytes,
                            tail: line.bytes,
                        },
                    ));
                }
                return Err(StoreError::MalformedLine {
                    line: line_number,
                    source,
                });
            }
        };
        journal
            .apply(record)
            .map_err(|reason| StoreError::RejectedRecord {
                line: line_number,
                reason,
            })?;
        valid_bytes =
            valid_bytes.saturating_add(u64::try_from(line.bytes.len()).unwrap_or(u64::MAX));
        if !line.terminated {
            return Ok((journal, Repair::AddNewline));
        }
        line_number = line_number.saturating_add(1);
    }
}

fn without_newline(bytes: &[u8], terminated: bool) -> &[u8] {
    if terminated {
        &bytes[..bytes.len().saturating_sub(1)]
    } else {
        bytes
    }
}

fn isolate_tail(path: &Path, tail: &[u8]) -> Result<PathBuf, StoreError> {
    for ordinal in 0..32_u8 {
        let candidate = path.with_extension(format!("invalid-tail-{ordinal}"));
        let mut options = secure_open_options();
        match options.write(true).create_new(true).open(&candidate) {
            Ok(mut file) => {
                if let Err(source) = file.write_all(tail) {
                    drop(file);
                    let _cleanup = std::fs::remove_file(&candidate);
                    return Err(StoreError::io("isolate invalid tail", source));
                }
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(StoreError::io("create invalid-tail file", source)),
        }
    }
    Err(StoreError::TailStagingExhausted)
}
