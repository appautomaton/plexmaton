use serde::{Deserialize, Serialize};

use crate::{ConversationEntryId, HeadName, TreeOrigin};

/// Largest single-line annotation retained for one stable tree node (TRE-8).
pub const MAX_TREE_LABEL_BYTES: usize = 256;

/// A bounded, nonempty single-line annotation; never model context or a navigation endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct TreeLabel(String);

impl TreeLabel {
    /// Validates source bytes rather than display cells, including during journal decoding.
    pub fn new(text: String) -> Result<Self, TreeLabelError> {
        if text.trim().is_empty() {
            return Err(TreeLabelError::Empty);
        }
        if text.len() > MAX_TREE_LABEL_BYTES {
            return Err(TreeLabelError::TooLong);
        }
        if text.chars().any(char::is_control) {
            return Err(TreeLabelError::ControlCharacter);
        }
        Ok(Self(text))
    }

    /// Exact annotation text, without trimming or presentation decoration.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TreeLabel {
    type Error = TreeLabelError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<TreeLabel> for String {
    fn from(label: TreeLabel) -> Self {
        label.0
    }
}

/// Why a node annotation could not enter the semantic or durable boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TreeLabelError {
    /// Use an explicit clear operation rather than storing an invisible annotation.
    #[error("A label must contain text; clear the label to remove it.")]
    Empty,
    /// Bound storage independently of terminal width.
    #[error("A label must fit in 256 UTF-8 bytes.")]
    TooLong,
    /// Labels occupy one plain-text row and cannot carry terminal controls.
    #[error("A label cannot contain control characters or line breaks.")]
    ControlCharacter,
}

/// One metadata mutation addressed to the exact tree from which the user chose it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeEdit {
    /// Full acknowledged journal origin; any intervening record makes the request stale.
    pub origin: TreeOrigin,
    /// Explicit metadata operation, distinct from moving the conversation's context.
    pub action: TreeEditAction,
}

/// Tree metadata operations that do not append model messages or replay effects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeEditAction {
    /// Rename an existing branch, preserving its pointer and durable selection if selected.
    RenameHead {
        /// Existing branch identity.
        head: HeadName,
        /// Fresh name; active and retired names remain unavailable.
        renamed: HeadName,
    },
    /// Retire an inactive branch pointer, retaining all immutable history.
    AbandonHead {
        /// Existing branch; the selected branch cannot be abandoned.
        head: HeadName,
    },
    /// Set or explicitly clear an annotation on a stable semantic node identity.
    SetLabel {
        /// Stable entry to annotate.
        entry_id: ConversationEntryId,
        /// `None` clears an annotation without changing the entry itself.
        label: Option<TreeLabel>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TRE-8: exactly bounded Unicode is lossless, while one-over and invisible/control labels
    /// cannot enter through either constructors or persisted JSON.
    #[test]
    fn tre_8_labels_revalidate_utf8_bounds_and_single_line_semantics() {
        let text = "界".repeat(85) + "x";
        let label = TreeLabel::new(text.clone()).expect("256 bytes");
        assert_eq!(label.as_str(), text);
        assert_eq!(TreeLabel::new(text + "y"), Err(TreeLabelError::TooLong));
        for (text, error) in [
            ("", TreeLabelError::Empty),
            ("  ", TreeLabelError::Empty),
            ("two\nlines", TreeLabelError::ControlCharacter),
            ("escape\u{1b}", TreeLabelError::ControlCharacter),
        ] {
            assert_eq!(TreeLabel::new(text.into()), Err(error));
            assert!(
                serde_json::from_str::<TreeLabel>(&serde_json::to_string(text).expect("JSON"))
                    .is_err()
            );
        }
        let encoded = serde_json::to_string(&label).expect("encode");
        assert_eq!(
            serde_json::from_str::<TreeLabel>(&encoded).expect("decode"),
            label
        );
    }
}
