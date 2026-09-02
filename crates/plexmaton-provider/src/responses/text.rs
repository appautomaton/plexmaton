//! Exact finality checks for one streamed Responses text content part.

use crate::codec::DecodeError;

#[derive(Debug, Default)]
pub(super) struct TextAssembly {
    text: String,
    done: bool,
}

impl TextAssembly {
    pub(super) fn append(
        &mut self,
        delta: &str,
        output_index: usize,
        content_index: usize,
    ) -> Result<(), DecodeError> {
        if self.done {
            return Err(DecodeError::ConflictingOutputText {
                output_index,
                content_index,
                field: "delta_after_done",
            });
        }
        self.text.push_str(delta);
        Ok(())
    }

    /// Returns text only when `done` supplied the whole part instead of confirming deltas.
    pub(super) fn finish(
        &mut self,
        complete: &str,
        output_index: usize,
        content_index: usize,
    ) -> Result<Option<String>, DecodeError> {
        if self.done {
            return Err(DecodeError::ConflictingOutputText {
                output_index,
                content_index,
                field: "done",
            });
        }
        self.done = true;
        if self.text.is_empty() {
            self.text.push_str(complete);
            return Ok((!complete.is_empty()).then(|| complete.to_owned()));
        }
        if self.text != complete {
            return Err(DecodeError::ConflictingOutputText {
                output_index,
                content_index,
                field: "text",
            });
        }
        Ok(None)
    }

    pub(super) const fn is_done(&self) -> bool {
        self.done
    }
}
