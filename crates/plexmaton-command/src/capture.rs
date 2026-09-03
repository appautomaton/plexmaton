use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::result::{CommandExecutionError, OutputStream};

const HEAD_BYTES: usize = 32 * 1024;
const TAIL_BYTES: usize = 32 * 1024;
/// Maximum raw bytes retained from either stdout or stderr.
pub const MAX_RETAINED_STREAM_BYTES: usize = HEAD_BYTES + TAIL_BYTES;

/// Bounded raw output retaining the exact head, tail and omitted-byte count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedStream {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: u64,
    complete: bool,
}

impl CapturedStream {
    pub(crate) const fn empty() -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            total_bytes: 0,
            complete: true,
        }
    }

    /// First bytes of the stream, up to the head bound.
    #[must_use]
    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// Last bytes after the retained head, up to the tail bound.
    #[must_use]
    pub fn tail(&self) -> &[u8] {
        &self.tail
    }

    /// Exact bytes read before EOF or an explicit bounded drain seal.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Whether the owned pipe reached EOF before its bounded drain deadline.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// Exact middle bytes discarded after bounded retention filled.
    #[must_use]
    pub fn omitted_bytes(&self) -> u64 {
        self.total_bytes
            .saturating_sub((self.head.len() + self.tail.len()) as u64)
    }

    /// Bounded UTF-8 presentation with replacement characters for invalid raw byte sequences.
    ///
    /// When bytes were omitted, an ASCII marker carrying the exact count separates head and tail.
    #[must_use]
    pub fn to_lossy_utf8(&self) -> String {
        if self.omitted_bytes() == 0 {
            let mut contiguous = Vec::with_capacity(self.head.len() + self.tail.len());
            contiguous.extend_from_slice(&self.head);
            contiguous.extend_from_slice(&self.tail);
            return String::from_utf8_lossy(&contiguous).into_owned();
        }
        let mut rendered = String::from_utf8_lossy(&self.head).into_owned();
        rendered.push_str(&format!(
            "\n...[{} bytes omitted]...\n",
            self.omitted_bytes()
        ));
        rendered.push_str(&String::from_utf8_lossy(&self.tail));
        rendered
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let mut capture = Capture::default();
        capture.push(bytes);
        capture.finish(true)
    }
}

#[derive(Default)]
struct Capture {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: u64,
}

impl Capture {
    fn push(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        let head_room = HEAD_BYTES.saturating_sub(self.head.len());
        let to_head = head_room.min(bytes.len());
        self.head.extend_from_slice(&bytes[..to_head]);
        self.push_tail(&bytes[to_head..]);
    }

    fn push_tail(&mut self, bytes: &[u8]) {
        if bytes.len() >= TAIL_BYTES {
            self.tail.clear();
            self.tail
                .extend_from_slice(&bytes[bytes.len() - TAIL_BYTES..]);
            return;
        }
        let overflow = self
            .tail
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(TAIL_BYTES);
        if overflow > 0 {
            self.tail.drain(..overflow);
        }
        self.tail.extend_from_slice(bytes);
    }

    fn finish(self, complete: bool) -> CapturedStream {
        CapturedStream {
            head: self.head,
            tail: self.tail,
            total_bytes: self.total_bytes,
            complete,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct DrainTracker {
    #[cfg(test)]
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl DrainTracker {
    fn enter(&self) -> ActiveDrainGuard {
        #[cfg(test)]
        self.active
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ActiveDrainGuard {
            #[cfg(test)]
            tracker: self.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn active(&self) -> usize {
        self.active.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub(crate) async fn drain<R>(
    mut reader: R,
    stream: OutputStream,
    tracker: DrainTracker,
    seal: CancellationToken,
) -> Result<CapturedStream, CommandExecutionError>
where
    R: AsyncRead + Unpin,
{
    let _guard = tracker.enter();
    let mut capture = Capture::default();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = tokio::select! {
            biased;
            () = seal.cancelled() => return Ok(capture.finish(false)),
            result = reader.read(&mut buffer) => result
                .map_err(|source| CommandExecutionError::StreamRead { stream, source })?,
        };
        if count == 0 {
            return Ok(capture.finish(true));
        }
        capture.push(&buffer[..count]);
    }
}

struct ActiveDrainGuard {
    #[cfg(test)]
    tracker: DrainTracker,
}

impl Drop for ActiveDrainGuard {
    fn drop(&mut self) {
        #[cfg(test)]
        self.tracker
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{Capture, HEAD_BYTES, MAX_RETAINED_STREAM_BYTES, TAIL_BYTES};

    #[test]
    fn cmd_3_capture_keeps_exact_raw_head_tail_and_omission_across_chunking() {
        let bytes: Vec<u8> = (0_u8..=255).cycle().take(1024 * 1024).collect();
        let mut capture = Capture::default();
        for chunk in bytes.chunks(7919) {
            capture.push(chunk);
        }
        let captured = capture.finish(true);
        assert_eq!(captured.total_bytes(), bytes.len() as u64);
        assert_eq!(captured.head(), &bytes[..HEAD_BYTES]);
        assert_eq!(captured.tail(), &bytes[bytes.len() - TAIL_BYTES..]);
        assert_eq!(
            captured.omitted_bytes(),
            (bytes.len() - MAX_RETAINED_STREAM_BYTES) as u64
        );
        assert!(captured.is_complete());
        assert!(captured.to_lossy_utf8().contains('\u{fffd}'));
    }

    #[test]
    fn cmd_3_utf8_projection_keeps_a_character_split_between_head_and_tail() {
        let mut bytes = vec![b'a'; HEAD_BYTES - 1];
        bytes.extend_from_slice("é".as_bytes());
        let mut capture = Capture::default();
        capture.push(&bytes);
        let captured = capture.finish(true);
        assert_eq!(captured.omitted_bytes(), 0);
        assert_eq!(captured.to_lossy_utf8(), String::from_utf8_lossy(&bytes));
    }
}
