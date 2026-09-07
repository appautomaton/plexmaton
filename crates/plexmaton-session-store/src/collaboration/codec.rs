use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

use plexmaton_agent::collaboration::{
    CollaborationLedger, CollaborationLimits, CollaborationRecord,
};
use plexmaton_core::CollaborationId;
use serde::{Deserialize, Serialize};

use super::CollaborationStoreError;
use crate::JournalRecovery;
use crate::codec::{encode_line, read_line};
use crate::load::{Repair, repair_tail, without_newline};

const FORMAT: &str = "plexmaton.collaboration";
const SCHEMA: &str = "2026-09-07";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: String,
    schema: String,
    collaboration_id: CollaborationId,
    limits: CollaborationLimits,
}

pub(super) fn header(ledger: &CollaborationLedger) -> Result<Vec<u8>, CollaborationStoreError> {
    Ok(encode_line(&Header {
        format: FORMAT.into(),
        schema: SCHEMA.into(),
        collaboration_id: ledger.id().clone(),
        limits: ledger.limits(),
    })?)
}

pub(super) fn load(
    file: &mut File,
    path: &Path,
) -> Result<(CollaborationLedger, JournalRecovery), CollaborationStoreError> {
    let (ledger, repair) = decode(&mut BufReader::new(&mut *file))?;
    let recovery = repair_tail(file, path, repair)?;
    Ok((ledger, recovery))
}

fn decode(
    reader: &mut impl BufRead,
) -> Result<(CollaborationLedger, Repair), CollaborationStoreError> {
    let first = read_line(reader, 1)?.ok_or(CollaborationStoreError::MissingHeader)?;
    let header: Header = serde_json::from_slice(without_newline(&first.bytes, first.terminated))
        .map_err(|source| CollaborationStoreError::Malformed { line: 1, source })?;
    if header.format != FORMAT || header.schema != SCHEMA {
        return Err(CollaborationStoreError::UnsupportedHeader);
    }
    let mut ledger = CollaborationLedger::new(header.collaboration_id, header.limits)?;
    let mut valid_bytes = first.bytes.len() as u64;
    if !first.terminated {
        return Ok((ledger, Repair::AddNewline));
    }
    let mut line_number = 2;
    loop {
        let Some(line) = read_line(reader, line_number)? else {
            return Ok((ledger, Repair::None));
        };
        let record = match serde_json::from_slice::<CollaborationRecord>(without_newline(
            &line.bytes,
            line.terminated,
        )) {
            Ok(record) => record,
            Err(source) => {
                let final_line = reader
                    .fill_buf()
                    .map_err(CollaborationStoreError::Io)?
                    .is_empty();
                if final_line && !line.terminated && incomplete_tail(&line.bytes, &source) {
                    return Ok((
                        ledger,
                        Repair::Isolate {
                            valid_bytes,
                            tail: line.bytes,
                        },
                    ));
                }
                return Err(CollaborationStoreError::Malformed {
                    line: line_number,
                    source,
                });
            }
        };
        ledger
            .apply(record)
            .map_err(|reason| CollaborationStoreError::InvalidRecord {
                line: line_number,
                reason,
            })?;
        valid_bytes += line.bytes.len() as u64;
        if !line.terminated {
            return Ok((ledger, Repair::AddNewline));
        }
        line_number += 1;
    }
}

pub(super) fn ensure_parent(path: &Path) -> Result<(), CollaborationStoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder
        .create(parent)
        .map_err(CollaborationStoreError::Io)?;
    check_parent(path)
}

pub(super) fn check_parent(path: &Path) -> Result<(), CollaborationStoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let metadata = std::fs::symlink_metadata(parent).map_err(CollaborationStoreError::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CollaborationStoreError::InsecureDirectory);
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(CollaborationStoreError::InsecureDirectory);
    }
    Ok(())
}

// COL-5: arbitrary Syntax errors are corruption, not an interrupted append. Serde reports a
// truncated multibyte character as Syntax, so recognize that one case only if its valid UTF-8
// prefix is itself incomplete JSON. A complete value followed by broken bytes is not repairable.
fn incomplete_tail(bytes: &[u8], source: &serde_json::Error) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => source.is_eof(),
        Err(utf8) => {
            (source.is_eof() || source.is_syntax())
                && utf8.error_len().is_none()
                && serde_json::from_slice::<serde_json::Value>(&bytes[..utf8.valid_up_to()])
                    .is_err_and(|error| error.is_eof())
        }
    }
}
