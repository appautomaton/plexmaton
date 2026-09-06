//! Terminal capability is an explicit presentation input, never inferred by a widget.

use serde::{Deserialize, Serialize};

mod frame;
pub(crate) use frame::NativeFrame;
pub use frame::{NativeStage, NativeText};
pub use plexmaton_math::{FontStyle, GlyphRun, Paint as MathPaint, TextScale, VerticalAlign};

/// The output owner's measured native-text capability for this workspace generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MathPresentation {
    /// The owner has verified full OSC 66 sizing and will emit the prepared native frame.
    Native,
    /// Keep exact source visible with the reason native output cannot be used.
    Source(MathUnavailable),
}

impl Default for MathPresentation {
    fn default() -> Self {
        Self::Source(MathUnavailable::Unverified)
    }
}

/// Absence is visible and is never interpreted as successful terminal capability detection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MathUnavailable {
    /// No complete capability result was received.
    Unverified,
    /// The terminal did not demonstrate the full native sizing protocol.
    Unsupported,
    /// A multiplexer cannot safely preserve native cell occupancy on this route.
    Multiplexer,
}

impl MathUnavailable {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unverified => "Math source · terminal sizing unverified",
            Self::Unsupported => "Math source · terminal sizing unavailable",
            Self::Multiplexer => "Math source · native sizing unavailable through multiplexer",
        }
    }
}
