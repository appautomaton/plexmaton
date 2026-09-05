//! Bounded SSE framing that drives one explicit provider codec.

use eventsource_stream2::Eventsource;
use std::future::Future;

use futures_util::{Stream, StreamExt};
use plexmaton_agent::{ModelError, ModelEvent};
use thiserror::Error;

use crate::{DecodeError, DecodeLimits, ProviderCodec, ResolvedModel};

/// Failure while framing provider bytes or translating one framed event.
#[derive(Debug, Error)]
pub enum SseDecodeError<E> {
    #[error("provider transport failed: {0}")]
    Transport(E),
    #[error("provider SSE contains invalid UTF-8")]
    InvalidUtf8,
    #[error("one provider SSE event exceeded {limit} bytes")]
    EventTooLarge { limit: usize },
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

impl<E: std::fmt::Display> SseDecodeError<E> {
    /// Maps protocol failures at the adapter boundary, preserving a transport's retry hint.
    #[must_use]
    pub fn into_model_error(self, retry_after: Option<u64>) -> ModelError {
        let message = self.to_string();
        match self {
            Self::Transport(error) => ModelError::Transport {
                message: error.to_string(),
            },
            Self::Decode(DecodeError::ProviderFailed { code }) => match code.as_deref() {
                Some("rate_limit_error" | "rate_limit_exceeded" | "RESOURCE_EXHAUSTED") => {
                    ModelError::RateLimited { retry_after }
                }
                Some("context_window_exceeded" | "context_length_exceeded") => {
                    ModelError::ContextTooLong
                }
                _ => ModelError::ProviderFailed { message },
            },
            Self::InvalidUtf8 | Self::EventTooLarge { .. } | Self::Decode(_) => {
                ModelError::Malformed { message }
            }
        }
    }
}

/// Applies backpressure from `emit` while framing arbitrary byte chunks into semantic events.
///
/// The caller owns the byte stream, cancellation and any channel used by `emit`; this function
/// owns only bounded framing and dialect translation. A semantic stop is withheld until the codec
/// accepts the stream trailer, so a malformed close cannot become `Stopped` followed by `Failed`
/// in a caller (PRV-1, PRV-2, PRV-7).
pub async fn drive_sse<S, B, E, F, Fut>(
    scope: &plexmaton_agent::RequestAttemptId,
    model: &ResolvedModel,
    stream: S,
    limits: DecodeLimits,
    mut emit: F,
) -> Result<(), SseDecodeError<E>>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    F: FnMut(ModelEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut guard = EventSizeGuard::new(limits.max_sse_event_bytes);
    let guarded = stream.map(move |item| match item {
        Ok(chunk) => match guard.observe(chunk.as_ref()) {
            Ok(()) => Ok(chunk),
            Err(()) => Err(GuardedError::EventTooLarge),
        },
        Err(error) => Err(GuardedError::Transport(error)),
    });
    let framed = guarded.eventsource();
    futures_util::pin_mut!(framed);
    let mut codec = ProviderCodec::new(scope, model, limits);
    let mut pending_stop = None;

    while let Some(event) = framed.next().await {
        let event = match event {
            Ok(event) => event,
            Err(error) if error.is_utf8() => return Err(SseDecodeError::InvalidUtf8),
            Err(error) => {
                let Some(error) = error.into_transport() else {
                    return Err(SseDecodeError::InvalidUtf8);
                };
                match error {
                    GuardedError::Transport(error) => {
                        return Err(SseDecodeError::Transport(error));
                    }
                    GuardedError::EventTooLarge => {
                        return Err(SseDecodeError::EventTooLarge {
                            limit: limits.max_sse_event_bytes,
                        });
                    }
                }
            }
        };
        for model_event in codec.push_sse(&event.event, &event.data)? {
            match model_event {
                ModelEvent::Stopped(reason) => {
                    if pending_stop.replace(reason).is_some() {
                        return Err(SseDecodeError::Decode(DecodeError::DuplicateFinality));
                    }
                }
                event => emit(event).await,
            }
        }
    }
    codec.finish()?;
    let Some(reason) = pending_stop else {
        return Err(SseDecodeError::Decode(DecodeError::IncompleteStream));
    };
    emit(ModelEvent::Stopped(reason)).await;
    Ok(())
}

#[derive(Debug)]
enum GuardedError<E> {
    Transport(E),
    EventTooLarge,
}

/// Tracks bytes since the last empty SSE line before the framing crate can retain them.
struct EventSizeGuard {
    limit: usize,
    bytes: usize,
    current_line_has_content: bool,
    previous_was_cr: bool,
}

impl EventSizeGuard {
    const fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: 0,
            current_line_has_content: false,
            previous_was_cr: false,
        }
    }

    fn observe(&mut self, chunk: &[u8]) -> Result<(), ()> {
        // The framing crate copies a whole transport chunk before yielding its first event. Keep
        // that allocation bounded even when one chunk contains many individually small events.
        if chunk.len() > self.limit {
            return Err(());
        }
        for byte in chunk {
            self.bytes = self.bytes.checked_add(1).ok_or(())?;
            if self.bytes > self.limit {
                return Err(());
            }
            match *byte {
                b'\r' => {
                    if self.current_line_has_content {
                        self.current_line_has_content = false;
                    } else {
                        self.bytes = 0;
                    }
                    self.previous_was_cr = true;
                }
                b'\n' if self.previous_was_cr => {
                    self.previous_was_cr = false;
                }
                b'\n' => {
                    if self.current_line_has_content {
                        self.current_line_has_content = false;
                    } else {
                        self.bytes = 0;
                    }
                }
                _ => {
                    self.previous_was_cr = false;
                    self.current_line_has_content = true;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EventSizeGuard;

    /// PRV-7: a terminator split across transport chunks still resets the event bound.
    #[test]
    fn event_guard_recognizes_split_crlf_boundaries() {
        let mut guard = EventSizeGuard::new(16);
        assert!(guard.observe(b"data: a\r").is_ok());
        assert!(guard.observe(b"\n\r").is_ok());
        assert!(guard.observe(b"\ndata: b\n\n").is_ok());
    }

    /// PRV-7: a stream cannot make the framing library retain an unbounded unterminated event.
    #[test]
    fn event_guard_rejects_an_unterminated_event_at_the_bound() {
        let mut guard = EventSizeGuard::new(8);
        assert!(guard.observe(b"data: 12").is_ok());
        assert!(guard.observe(b"3").is_err());
    }

    #[test]
    fn event_guard_rejects_one_oversized_transport_chunk() {
        let mut guard = EventSizeGuard::new(8);
        assert!(guard.observe(b"a\n\nb\n\nc\n\n").is_err());
    }
}
