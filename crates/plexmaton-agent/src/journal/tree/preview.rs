//! Bounded tree summaries. Copy retains the canonical block order in tree_source.
use super::MAX_TREE_PREVIEW_BYTES_PER_ROW;
use crate::{AssistantBlock, AssistantOutput};
use plexmaton_core::{TreePreview, TreeRowKind};

const PREVIEW_ELLIPSIS: &str = "…";

pub(super) fn assistant(output: &AssistantOutput) -> (TreeRowKind, PreviewBuilder) {
    let has_text = output.blocks().iter().any(|block| {
        matches!(block,
        AssistantBlock::Text { text, .. } if !text.trim().is_empty())
    });
    let mut tools = PreviewBuilder {
        limit: if has_text {
            MAX_TREE_PREVIEW_BYTES_PER_ROW / 2
        } else {
            MAX_TREE_PREVIEW_BYTES_PER_ROW
        },
        ..PreviewBuilder::default()
    };
    let mut has_tools = false;
    for block in output.blocks() {
        if let AssistantBlock::ToolCall { call, .. } = block {
            has_tools = true;
            tools.append(&call.name);
        }
    }
    let mut answer = PreviewBuilder::default();
    if has_tools && has_text {
        // Reserve independent space for answer text and the combined truncation marker.
        answer.limit = MAX_TREE_PREVIEW_BYTES_PER_ROW
            - tools.text.len()
            - " · ".len()
            - if tools.truncated {
                PREVIEW_ELLIPSIS.len()
            } else {
                0
            };
    }
    for block in output.blocks() {
        if let AssistantBlock::Text { text, .. } = block
            && !text.trim().is_empty()
        {
            answer.append(text);
        }
    }
    if has_tools {
        if !has_text {
            return (TreeRowKind::ToolBatch, tools);
        }
        let mut text = format!("{} · {}", tools.text, answer.text);
        if tools.truncated && !answer.truncated {
            text.push_str(PREVIEW_ELLIPSIS);
        }
        return (
            TreeRowKind::ToolBatch,
            PreviewBuilder {
                text,
                truncated: tools.truncated || answer.truncated,
                ..PreviewBuilder::default()
            },
        );
    }
    if !has_text {
        for block in output.blocks() {
            if let AssistantBlock::Reasoning { text, .. } = block
                && !text.trim().is_empty()
            {
                if answer.text.is_empty() {
                    answer.append("reasoning:");
                }
                answer.append(text);
            }
        }
    }
    (TreeRowKind::Assistant, answer)
}

pub(super) struct PreviewBuilder {
    text: String,
    truncated: bool,
    limit: usize,
}

impl Default for PreviewBuilder {
    fn default() -> Self {
        Self {
            text: String::new(),
            truncated: false,
            limit: MAX_TREE_PREVIEW_BYTES_PER_ROW,
        }
    }
}

impl PreviewBuilder {
    pub(super) fn from_text(text: &str) -> Self {
        let mut builder = Self::default();
        builder.append(text);
        builder
    }

    fn append(&mut self, source: &str) {
        if self.truncated || source.is_empty() {
            return;
        }
        let separator = if self.text.is_empty() { "" } else { " " };
        let full_length = self
            .text
            .len()
            .saturating_add(separator.len())
            .saturating_add(source.len());
        if full_length <= self.limit {
            self.text.push_str(separator);
            self.text.push_str(source);
            return;
        }

        while self
            .text
            .len()
            .saturating_add(usize::from(!self.text.is_empty()))
            .saturating_add(PREVIEW_ELLIPSIS.len())
            > self.limit
        {
            let _ = self.text.pop();
        }
        let separator = if self.text.is_empty() { "" } else { " " };
        self.text.push_str(separator);
        for character in source.chars() {
            if self
                .text
                .len()
                .saturating_add(character.len_utf8())
                .saturating_add(PREVIEW_ELLIPSIS.len())
                > self.limit
            {
                break;
            }
            self.text.push(character);
        }
        self.text.push_str(PREVIEW_ELLIPSIS);
        self.truncated = true;
    }

    pub(super) fn finish(self) -> TreePreview {
        TreePreview {
            text: self.text,
            truncated: self.truncated,
        }
    }
}
