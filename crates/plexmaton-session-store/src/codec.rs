use std::io::{self, BufRead, Write};

use plexmaton_agent::UnixMillis;
use plexmaton_core::SessionId;
use serde::{Deserialize, Serialize};

use crate::StoreError;

pub const SCHEMA_EPOCH: &str = "2026-09-05";
pub const MAX_JOURNAL_LINE_BYTES: usize = 16 * 1024 * 1024;
const HEADER_FORMAT: &str = "plexmaton.session";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeaderWire {
    format: String,
    schema: String,
    session_id: SessionId,
    created_at_unix_ms: UnixMillis,
}

pub(crate) struct DecodedHeader {
    pub(crate) session_id: SessionId,
    pub(crate) created_at_unix_ms: UnixMillis,
}

pub(crate) fn encode_header(
    session_id: &SessionId,
    created_at_unix_ms: UnixMillis,
) -> Result<Vec<u8>, StoreError> {
    encode_line(&HeaderWire {
        format: HEADER_FORMAT.to_owned(),
        schema: SCHEMA_EPOCH.to_owned(),
        session_id: session_id.clone(),
        created_at_unix_ms,
    })
}

pub(crate) fn decode_header(bytes: &[u8]) -> Result<DecodedHeader, StoreError> {
    let header: HeaderWire = serde_json::from_slice(bytes)
        .map_err(|source| StoreError::MalformedLine { line: 1, source })?;
    if header.format != HEADER_FORMAT {
        return Err(StoreError::UnsupportedHeader);
    }
    if header.schema != SCHEMA_EPOCH {
        return Err(StoreError::UnsupportedSchema(header.schema));
    }
    Ok(DecodedHeader {
        session_id: header.session_id,
        created_at_unix_ms: header.created_at_unix_ms,
    })
}

pub(crate) fn encode_line(value: &impl Serialize) -> Result<Vec<u8>, StoreError> {
    let mut writer = LimitedLine::new();
    let encoded = serde_json::to_writer(&mut writer, value);
    if writer.exceeded {
        return Err(StoreError::LineTooLarge {
            line: 0,
            limit: MAX_JOURNAL_LINE_BYTES,
        });
    }
    encoded.map_err(|source| StoreError::MalformedLine { line: 0, source })?;
    writer.bytes.push(b'\n');
    Ok(writer.bytes)
}

pub(crate) struct ReadLine {
    pub(crate) bytes: Vec<u8>,
    pub(crate) terminated: bool,
}

pub(crate) fn read_line(
    reader: &mut impl BufRead,
    line: u64,
) -> Result<Option<ReadLine>, StoreError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|source| StoreError::io("read", source))?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else if bytes.len().saturating_add(1) > MAX_JOURNAL_LINE_BYTES {
                Err(StoreError::LineTooLarge {
                    line,
                    limit: MAX_JOURNAL_LINE_BYTES,
                })
            } else {
                Ok(Some(ReadLine {
                    bytes,
                    terminated: false,
                }))
            };
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if bytes.len().saturating_add(take) > MAX_JOURNAL_LINE_BYTES {
            return Err(StoreError::LineTooLarge {
                line,
                limit: MAX_JOURNAL_LINE_BYTES,
            });
        }
        let terminated = available[take - 1] == b'\n';
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if terminated {
            return Ok(Some(ReadLine { bytes, terminated }));
        }
    }
}

struct LimitedLine {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl LimitedLine {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(1024),
            exceeded: false,
        }
    }
}

impl Write for LimitedLine {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .saturating_add(buffer.len())
            .saturating_add(1)
            > MAX_JOURNAL_LINE_BYTES
        {
            self.exceeded = true;
            return Err(io::Error::other("journal line byte limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MAX_JOURNAL_LINE_BYTES, read_line};
    use crate::StoreError;

    /// JRN-4: an unterminated line reserves one byte for the repaired newline.
    #[test]
    fn jrn_4_unterminated_line_cannot_grow_past_the_bound_when_repaired() {
        let mut oversized = Cursor::new(vec![b'x'; MAX_JOURNAL_LINE_BYTES]);
        assert!(matches!(
            read_line(&mut oversized, 2),
            Err(StoreError::LineTooLarge { line: 2, .. })
        ));

        let mut exact = Cursor::new(vec![b'x'; MAX_JOURNAL_LINE_BYTES - 1]);
        let line = read_line(&mut exact, 2)
            .unwrap_or_else(|error| panic!("read exact-bound line: {error}"))
            .unwrap_or_else(|| panic!("exact-bound line missing"));
        assert!(!line.terminated);
        assert_eq!(line.bytes.len() + 1, MAX_JOURNAL_LINE_BYTES);

        let mut terminated = vec![b'x'; MAX_JOURNAL_LINE_BYTES - 1];
        terminated.push(b'\n');
        let mut terminated = Cursor::new(terminated);
        assert!(
            read_line(&mut terminated, 2)
                .unwrap_or_else(|error| panic!("read terminated line: {error}"))
                .is_some()
        );
    }
}
