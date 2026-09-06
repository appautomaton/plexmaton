//! Same-build framed data. Length and aggregate admission happen before retaining payloads.

use std::io::{self, Read, Write};

pub use plexmaton_tui::preparation::BatchRefusal as Refusal;
use plexmaton_tui::preparation::{Key, PreparedText, Request};
use serde::{Deserialize, Serialize};

pub(super) const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub(super) const MAX_REPLY_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_ITEMS: usize = plexmaton_tui::preparation::MAX_BATCH_ITEMS;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Ticket(pub(super) u64);

#[derive(Serialize, Deserialize)]
struct Batch {
    ticket: Ticket,
    requests: Vec<Request>,
}

pub(super) struct Pending {
    pub ticket: Ticket,
    pub keys: Vec<Key>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Reply {
    pub ticket: Ticket,
    pub result: Result<Vec<PreparedText>, Refusal>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Truncated or broken framed pipes.
    #[error("preparation pipe I/O: {0}")]
    Io(#[from] io::Error),
    /// Data cannot be decoded as the same-build protocol.
    #[error("preparation payload codec: {0}")]
    Codec(#[from] serde_json::Error),
    /// Framing, item count or encoded bytes exceed admission.
    #[error("preparation payload exceeds its bound")]
    Capacity,
    /// Ticket, entry identity or copy-map structure differs from its request.
    #[error("preparation reply does not match its request")]
    Identity,
}

impl Pending {
    pub fn new(ticket: Ticket, requests: Vec<Request>) -> Result<Self, Error> {
        if requests.is_empty() || requests.len() > MAX_ITEMS {
            return Err(Error::Capacity);
        }
        let mut batch = Batch { ticket, requests };
        let bytes = match encode(&batch, MAX_REQUEST_BYTES) {
            Err(Error::Capacity) => {
                // PRE-1: an optional reuse hint must not make an admissible source refuse.
                for request in &mut batch.requests {
                    request.discard_prefix();
                }
                encode(&batch, MAX_REQUEST_BYTES)?
            }
            result => result?,
        };
        // Retain encoded input, not user String/Vec spare capacity or a second transcript.
        let keys = batch.requests.iter().map(|r| r.key().clone()).collect();
        Ok(Self {
            ticket,
            keys,
            bytes,
        })
    }

    pub fn decode_reply(&self, bytes: &[u8]) -> Result<Reply, Error> {
        let reply: Reply = serde_json::from_slice(bytes)?;
        if reply.ticket != self.ticket {
            return Err(Error::Identity);
        }
        if let Ok(results) = &reply.result
            && (results.len() != self.keys.len()
                || results
                    .iter()
                    .zip(&self.keys)
                    .any(|(result, key)| !result.validates(key))
                || results
                    .iter()
                    .map(PreparedText::allocation_bytes)
                    .sum::<usize>()
                    > MAX_REPLY_BYTES)
        {
            return Err(Error::Identity);
        }
        Ok(reply)
    }
}

pub(super) fn run(mut input: impl Read, mut output: impl Write) -> Result<(), Error> {
    loop {
        let mut header = [0; 4];
        match input.read(&mut header[..1])? {
            0 => return Ok(()),
            _ => input.read_exact(&mut header[1..])?,
        }
        let length = length(header, MAX_REQUEST_BYTES)?;
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes)?;
        let bytes = prepare(&bytes)?;
        output.write_all(&(bytes.len() as u32).to_be_bytes())?;
        output.write_all(&bytes)?;
        output.flush()?;
    }
}

fn prepare(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let batch: Batch = serde_json::from_slice(bytes)?;
    if batch.requests.is_empty() || batch.requests.len() > MAX_ITEMS {
        return Err(Error::Capacity);
    }
    let result = plexmaton_tui::preparation::prepare_batch(&batch.requests);
    match encode(
        &Reply {
            ticket: batch.ticket,
            result,
        },
        MAX_REPLY_BYTES,
    ) {
        Err(Error::Capacity) => refused(batch.ticket),
        result => result,
    }
}

fn refused(ticket: Ticket) -> Result<Vec<u8>, Error> {
    encode(
        &Reply {
            ticket,
            result: Err(Refusal::Capacity),
        },
        MAX_REPLY_BYTES,
    )
}

pub(super) fn length(header: [u8; 4], limit: usize) -> Result<usize, Error> {
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > limit {
        return Err(Error::Capacity);
    }
    Ok(size)
}

fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Error> {
    let mut buffer = Bounded {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut buffer, value);
    if buffer.exceeded {
        return Err(Error::Capacity);
    }
    result?;
    Ok(buffer.bytes)
}

struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let needed = self.bytes.len().saturating_add(bytes.len());
        if needed > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("preparation payload capacity"));
        }
        if needed > self.bytes.capacity() {
            let capacity = needed
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.limit);
            self.bytes.reserve_exact(capacity - self.bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexmaton_core::{AgentId, TranscriptItemId, TranscriptRole};
    use plexmaton_tui::{TranscriptEntryView, TranscriptItemView, TranscriptTextKind};

    fn request(source: String) -> Request {
        Request::new(
            AgentId::new("primary").expect("agent"),
            TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new("message").expect("item"),
                source,
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision: 2,
                finalized: true,
            }),
            60,
            false,
        )
    }

    /// PRE-1: all identity axes and copy-map byte boundaries are checked without a second parse.
    #[test]
    fn preparation_wire_rejects_mismatched_identity_and_invalid_copy_ranges() {
        let input = request("**中文** and code".into());
        let prepared = input.prepare();
        let pending = Pending::new(Ticket(3), vec![input]).expect("admitted");
        let reply = Reply {
            ticket: pending.ticket,
            result: Ok(vec![prepared]),
        };
        let original = serde_json::to_value(reply).expect("reply");
        pending
            .decode_reply(&serde_json::to_vec(&original).expect("bytes"))
            .expect("valid reply");
        for (pointer, value) in [
            ("/ticket", serde_json::json!(4)),
            ("/result/Ok/0/key/agent", serde_json::json!("other")),
            ("/result/Ok/0/key/item", serde_json::json!("other")),
            ("/result/Ok/0/key/revision", serde_json::json!(3)),
            ("/result/Ok/0/key/width", serde_json::json!(88)),
            ("/result/Ok/0/key/open", serde_json::json!(true)),
            (
                "/result/Ok/0/result/Ok/rows/0/0/text/start",
                serde_json::json!(1),
            ),
            (
                "/result/Ok/0/result/Ok/rows/0/0/text/end",
                serde_json::json!(1_000_000),
            ),
            (
                "/result/Ok/0/result/Ok/rows/0/0/column",
                serde_json::json!(61),
            ),
            ("/result/Ok/0/result/Ok/rows", serde_json::json!([])),
            ("/result/Ok", serde_json::json!([])),
            (
                "/result/Ok/0/result/Ok/lines/0/treatment",
                serde_json::json!({ "SelectionWidth": 1_000_000 }),
            ),
            (
                "/result/Ok/0/result/Ok/lines/0/spans/0/content",
                serde_json::json!("x".repeat(61)),
            ),
        ] {
            let mut corrupted = original.clone();
            *corrupted
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("fixture pointer {pointer}")) = value;
            assert!(
                matches!(
                    pending.decode_reply(&serde_json::to_vec(&corrupted).expect("bytes")),
                    Err(Error::Identity)
                ),
                "{pointer}"
            );
        }
    }

    /// PRE-1/MTH-1: native geometry and atomic fragments must describe the same complete rectangle.
    #[test]
    fn preparation_wire_rejects_mismatched_math_capability_geometry_and_atomic_maps() {
        let input =
            request(r"\(x_i\)".into()).with_math(plexmaton_tui::math::MathPresentation::Native);
        let prepared = input.prepare();
        let pending = Pending::new(Ticket(3), vec![input]).expect("request");
        let original = serde_json::to_value(Reply {
            ticket: pending.ticket,
            result: Ok(vec![prepared]),
        })
        .expect("native reply");
        pending
            .decode_reply(&serde_json::to_vec(&original).expect("bytes"))
            .expect("valid reply");
        for (pointer, value) in [
            (
                "/result/Ok/0/key/math",
                serde_json::json!({"Source": "Unsupported"}),
            ),
            (
                "/result/Ok/0/result/Ok/formulas/0/column",
                serde_json::json!(60),
            ),
            (
                "/result/Ok/0/result/Ok/formulas/0/width",
                serde_json::json!(0),
            ),
            (
                "/result/Ok/0/result/Ok/formulas/0/row",
                serde_json::json!(8192),
            ),
            (
                "/result/Ok/0/result/Ok/formulas/0/text/end",
                serde_json::json!(1),
            ),
            (
                "/result/Ok/0/result/Ok/rows/0/0/kind",
                serde_json::json!("Text"),
            ),
            ("/result/Ok/0/result/Ok/formulas", serde_json::json!([])),
        ] {
            let mut corrupted = original.clone();
            *corrupted
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("{pointer}")) = value;
            assert!(
                matches!(
                    pending.decode_reply(&serde_json::to_vec(&corrupted).expect("bytes")),
                    Err(Error::Identity)
                ),
                "{pointer}"
            );
        }
    }

    /// PRE-1: bounds cover escaped wire bytes, batch count and retained capacity, not source length.
    #[test]
    fn preparation_wire_bounds_requests_before_retaining_them() {
        assert!(matches!(
            Pending::new(Ticket(1), Vec::new()),
            Err(Error::Capacity)
        ));
        assert!(matches!(
            Pending::new(
                Ticket(1),
                (0..=MAX_ITEMS).map(|_| request("x".into())).collect()
            ),
            Err(Error::Capacity)
        ));
        assert!(matches!(
            Pending::new(Ticket(1), vec![request("\"".repeat(MAX_REQUEST_BYTES / 2))]),
            Err(Error::Capacity)
        ));
        let mut spare = String::with_capacity(MAX_REQUEST_BYTES * 4);
        spare.push('x');
        let pending =
            Pending::new(Ticket(1), vec![request(spare)]).expect("only encoded source is retained");
        assert!(pending.bytes.capacity() <= MAX_REQUEST_BYTES);
        assert!(pending.bytes.len() < 1024);
        assert!(matches!(
            length((MAX_REPLY_BYTES as u32 + 1).to_be_bytes(), MAX_REPLY_BYTES),
            Err(Error::Capacity)
        ));
    }

    /// PRE-1/MD-4: encoded prefix data is optional; source that fits alone must still be admitted.
    #[test]
    fn preparation_wire_drops_optional_prefixes_before_refusing_source() {
        let source = format!("# {}\n\n", "heading ".repeat(80));
        let mut input = serde_json::to_value(request(source)).expect("source snapshot");
        input["entry"]["Text"]["finalized"] = false.into();
        let first: Request = serde_json::from_value(input.clone()).expect("streaming source");
        let prepared = serde_json::to_value(first.prepare()).expect("prepared heading");
        let checkpoint = &prepared["checkpoint"];
        let rows = checkpoint["rows"].as_u64().expect("checkpoint rows") as usize;
        let text_bytes = checkpoint["visible_text_bytes"]
            .as_u64()
            .expect("checkpoint text") as usize;
        let mut layout = prepared["result"]["Ok"].clone();
        for field in ["lines", "rows"] {
            layout[field]
                .as_array_mut()
                .expect("prepared rows")
                .truncate(rows);
        }
        layout["text"] = layout["text"].as_str().expect("copy text")[..text_bytes].into();
        let hint = serde_json::json!({"checkpoint": checkpoint, "layout": layout});
        let base = serde_json::to_vec(&serde_json::json!({
            "ticket": 7, "requests": [&input]
        }))
        .expect("canonical batch");
        let padding = "\0".repeat((MAX_REQUEST_BYTES - base.len() - 1) / 6);
        let source = input["entry"]["Text"]["source"].as_str().expect("source");
        input["entry"]["Text"]["source"] = format!("{source}{padding}").into();
        let canonical = input.clone();
        input["prefix"] = hint;
        let with_hint: Request = serde_json::from_value(input).expect("request with hint");
        let batch = Batch {
            ticket: Ticket(7),
            requests: vec![with_hint],
        };
        assert!(matches!(
            encode(&batch, MAX_REQUEST_BYTES),
            Err(Error::Capacity)
        ));
        let pending = Pending::new(batch.ticket, batch.requests).expect("source still fits");
        assert!(pending.bytes.len() <= MAX_REQUEST_BYTES);
        let decoded: Batch = serde_json::from_slice(&pending.bytes).expect("canonical wire data");
        assert_eq!(decoded.ticket, Ticket(7));
        assert_eq!(decoded.requests.len(), 1);
        assert_eq!(pending.keys, vec![decoded.requests[0].key().clone()]);
        assert_eq!(
            serde_json::to_value(&decoded.requests[0]).expect("retained snapshot"),
            canonical
        );
    }

    /// PRE-1: a framed stream distinguishes clean EOF, truncated input and invalid length.
    #[test]
    fn preparation_driver_rejects_truncated_frames_and_unadmitted_modifiers() {
        for input in [
            vec![0],
            vec![0, 0, 0, 0],
            vec![0, 0, 0, 2, b'{'],
            vec![0xff; 4],
        ] {
            let mut output = Vec::new();
            assert!(run(input.as_slice(), &mut output).is_err());
            assert!(output.is_empty());
        }
        let input = request("**styled**".into());
        let reply = Reply {
            ticket: Ticket(1),
            result: Ok(vec![input.prepare()]),
        };
        let pending = Pending::new(Ticket(1), vec![input]).expect("request");
        let mut value = serde_json::to_value(reply).expect("reply");
        let patches = value
            .pointer_mut("/result/Ok/0/result/Ok/lines/0/spans/0/style/patches")
            .expect("styled span");
        *patches = serde_json::json!([{ "Add": 65535 }]);
        assert!(matches!(
            pending.decode_reply(&serde_json::to_vec(&value).expect("bytes")),
            Err(Error::Codec(_))
        ));
    }
}
