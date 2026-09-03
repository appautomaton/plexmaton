//! Bounded JSON-line and stderr readers for one owned ripgrep child.

use std::{
    collections::VecDeque,
    io::{self, Read},
    sync::mpsc::SyncSender,
};

use serde::Deserialize;

use super::{
    MAX_PREVIEW_BYTES, MAX_RG_RECORD_BYTES, MAX_SEARCH_PATH_BYTES, SearchError, SearchMatch,
};

pub(super) enum PumpEvent {
    Match { found: SearchMatch, bytes: usize },
    Finished { bytes: usize },
    TransportLimit { bytes: usize },
    Failed(SearchError),
}

pub(super) fn pump_stdout(mut stdout: impl Read, sender: SyncSender<PumpEvent>, byte_limit: usize) {
    let mut chunk = [0_u8; 8192];
    let mut line = Vec::new();
    let mut total = 0_usize;
    loop {
        let read = match stdout.read(&mut chunk) {
            Ok(0) => {
                if !line.is_empty() && !send_record(&sender, &line, total) {
                    return;
                }
                let _sent = sender.send(PumpEvent::Finished { bytes: total });
                return;
            }
            Ok(read) => read,
            Err(error) => {
                let _sent = sender.send(PumpEvent::Failed(SearchError::Io(error.kind())));
                return;
            }
        };
        let accepted = read.min(byte_limit.saturating_sub(total));
        total = total.saturating_add(accepted);
        for byte in &chunk[..accepted] {
            if *byte == b'\n' {
                if !send_record(&sender, &line, total) {
                    return;
                }
                line.clear();
            } else if line.len() == MAX_RG_RECORD_BYTES {
                let _sent = sender.send(PumpEvent::Failed(SearchError::RecordTooLarge {
                    limit: MAX_RG_RECORD_BYTES,
                }));
                return;
            } else {
                line.push(*byte);
            }
        }
        if accepted < read || total == byte_limit {
            let _sent = sender.send(PumpEvent::TransportLimit { bytes: total });
            return;
        }
    }
}

pub(super) enum DiscoveryEvent {
    Candidate { path: String, bytes: usize },
    Finished { bytes: usize },
    TransportLimit { bytes: usize },
    Failed(SearchError),
}

pub(super) fn pump_paths(
    mut stdout: impl Read,
    sender: SyncSender<DiscoveryEvent>,
    byte_limit: usize,
) {
    let mut chunk = [0_u8; 8192];
    let mut path = Vec::new();
    let mut total = 0_usize;
    loop {
        let read = match stdout.read(&mut chunk) {
            Ok(0) => {
                if !path.is_empty() && !send_path(&sender, &path, total) {
                    return;
                }
                let _sent = sender.send(DiscoveryEvent::Finished { bytes: total });
                return;
            }
            Ok(read) => read,
            Err(error) => {
                let _sent = sender.send(DiscoveryEvent::Failed(SearchError::Io(error.kind())));
                return;
            }
        };
        let accepted = read.min(byte_limit.saturating_sub(total));
        total = total.saturating_add(accepted);
        for byte in &chunk[..accepted] {
            if *byte == 0 {
                if !send_path(&sender, &path, total) {
                    return;
                }
                path.clear();
            } else if path.len() == MAX_SEARCH_PATH_BYTES {
                let _sent = sender.send(DiscoveryEvent::Failed(SearchError::CandidateTooLong {
                    limit: MAX_SEARCH_PATH_BYTES,
                }));
                return;
            } else {
                path.push(*byte);
            }
        }
        if accepted < read || total == byte_limit {
            let _sent = sender.send(DiscoveryEvent::TransportLimit { bytes: total });
            return;
        }
    }
}

fn send_path(sender: &SyncSender<DiscoveryEvent>, bytes: &[u8], acquired: usize) -> bool {
    let Ok(path) = std::str::from_utf8(bytes) else {
        let _sent = sender.send(DiscoveryEvent::Failed(SearchError::InvalidProtocol));
        return false;
    };
    sender
        .send(DiscoveryEvent::Candidate {
            path: path.to_owned(),
            bytes: acquired,
        })
        .is_ok()
}

fn send_record(sender: &SyncSender<PumpEvent>, bytes: &[u8], acquired: usize) -> bool {
    match parse_match(bytes) {
        Ok(Some(found)) => sender
            .send(PumpEvent::Match {
                found,
                bytes: acquired,
            })
            .is_ok(),
        Ok(None) => true,
        Err(error) => {
            let _sent = sender.send(PumpEvent::Failed(error));
            false
        }
    }
}

#[derive(Deserialize)]
struct RgMessage {
    #[serde(rename = "type")]
    kind: String,
    data: RgData,
}

#[derive(Deserialize)]
struct RgData {
    path: Option<RgText>,
    lines: Option<RgText>,
    line_number: Option<u64>,
}

#[derive(Deserialize)]
struct RgText {
    text: Option<String>,
    bytes: Option<String>,
}

fn parse_match(bytes: &[u8]) -> Result<Option<SearchMatch>, SearchError> {
    let message: RgMessage =
        serde_json::from_slice(bytes).map_err(|_| SearchError::InvalidProtocol)?;
    if message.kind != "match" {
        return Ok(None);
    }
    let path = strict_text(message.data.path)?;
    let path = path.strip_prefix("./").unwrap_or(&path).to_owned();
    let mut preview = strict_text(message.data.lines)?;
    while matches!(preview.as_bytes().last(), Some(b'\n' | b'\r')) {
        preview.pop();
    }
    let preview_truncated = preview.len() > MAX_PREVIEW_BYTES;
    if preview_truncated {
        let mut end = MAX_PREVIEW_BYTES;
        while !preview.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        preview.truncate(end);
    }
    Ok(Some(SearchMatch {
        path,
        line: message
            .data
            .line_number
            .ok_or(SearchError::InvalidProtocol)?,
        preview,
        preview_truncated,
    }))
}

fn strict_text(value: Option<RgText>) -> Result<String, SearchError> {
    let value = value.ok_or(SearchError::InvalidProtocol)?;
    if value.bytes.is_some() {
        return Err(SearchError::InvalidProtocol);
    }
    value.text.ok_or(SearchError::InvalidProtocol)
}

pub(super) struct BoundedBytes {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: usize,
}

impl BoundedBytes {
    pub(super) fn text(&self) -> String {
        let mut bytes = self.head.clone();
        if self.omitted() > 0 {
            bytes.extend_from_slice(
                format!("\n... {} bytes omitted ...\n", self.omitted()).as_bytes(),
            );
        }
        bytes.extend(self.tail.iter());
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub(super) fn omitted(&self) -> usize {
        self.total
            .saturating_sub(self.head.len().saturating_add(self.tail.len()))
    }
}

pub(super) fn collect_bounded(
    mut reader: impl Read,
    limit: usize,
) -> Result<BoundedBytes, io::ErrorKind> {
    let half = limit / 2;
    let tail_limit = limit.saturating_sub(half);
    let mut head = Vec::with_capacity(half);
    let mut tail = VecDeque::with_capacity(tail_limit);
    let mut total = 0_usize;
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).map_err(|error| error.kind())?;
        if read == 0 {
            return Ok(BoundedBytes { head, tail, total });
        }
        total = total.saturating_add(read);
        for byte in &chunk[..read] {
            if head.len() < half {
                head.push(*byte);
            } else {
                if tail.len() == tail_limit {
                    tail.pop_front();
                }
                tail.push_back(*byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bounded_bytes_keep_head_tail_and_exact_omission() {
        let bytes = super::collect_bounded(&b"0123456789"[..], 6)
            .unwrap_or_else(|error| panic!("collect: {error:?}"));
        assert_eq!(bytes.head, b"012");
        assert_eq!(bytes.tail.iter().copied().collect::<Vec<_>>(), b"789");
        assert_eq!(bytes.omitted(), 4);
    }
}
