//! Immutable formula source and bounded native-text presentation over a pinned math engine.
//!
//! Preparation is synchronous and belongs outside the TUI event/render thread. Worker cancellation
//! and conversation integration are separate delivery gates; no upstream types escape this crate.
use std::sync::Arc;

mod engine;
mod native;
mod prepared;
mod source;
pub use prepared::NativeLayout;

#[cfg(test)]
mod tests;

/// Maximum original UTF-8 formula bytes, including delimiters.
pub const MAX_SOURCE_BYTES: usize = 8 * 1024;
/// Maximum admitted expanded syntax nodes.
pub const MAX_NODES: usize = 4096;
/// Maximum retained positioned primitives.
pub const MAX_ITEMS: usize = 4096;
/// Maximum terminal dimension of one prepared formula.
pub const MAX_DIMENSION: usize = 512;
/// Maximum reserved cells in one prepared formula.
pub const MAX_CELLS: usize = 32_768;
/// Maximum UTF-8 bytes in one native terminal operation.
pub const MAX_RUN_BYTES: usize = 4096;

/// A bound enforced at the first-party admission or presentation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    /// Original source bytes.
    SourceBytes,
    /// Expanded syntax depth.
    Depth,
    /// Expanded syntax nodes.
    Nodes,
    /// Positioned engine output.
    Items,
    /// Non-finite or excessive geometry.
    Geometry,
    /// Reserved terminal cells.
    Cells,
    /// Native terminal text payload.
    NativeTextBytes,
}

/// A presentation feature that this native adapter cannot preserve reliably.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unsupported {
    /// A syntax-level effect outside the admitted native subset.
    Construct,
    /// A font whose character codes have no admitted native mapping.
    Font,
    /// A glyph without a proven Unicode equivalent.
    Glyph,
    /// A scale outside the admitted terminal-size choices.
    Scale,
    /// An arbitrary vector path has no native-text projection.
    Path,
    /// Unsupported paint, including transparency or filled backgrounds.
    Paint,
}

/// Refusal never substitutes a simplified or truncated formula.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum MathError {
    /// Formula-only input must retain its original paired delimiters.
    #[error("a complete formula with matching delimiters is required")]
    Delimiters,
    /// The upstream parser refused syntax or its own complexity bound.
    #[error("the math parser refused this formula")]
    ParseRejected,
    /// Terminal controls cannot enter prepared native text.
    #[error("math source contains terminal controls")]
    Controls,
    /// A source-only representation is required for this feature.
    #[error("unsupported native math feature: {0:?}")]
    Unsupported(Unsupported),
    /// An owned resource bound was exceeded.
    #[error("math preparation limit: {0:?}")]
    Limited(Limit),
    /// The formula cannot fit this width without dropping terms.
    #[error("formula requires {required} columns; {available} available")]
    TooWide { required: usize, available: usize },
    /// Independent native text reservations would overwrite one another.
    #[error("native math cell reservations overlap")]
    Overlap,
    /// There is no visible mathematical content to present.
    #[error("formula has no visible mathematical content")]
    Empty,
}

/// The style implied by the original Markdown math delimiters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathMode {
    /// Dollar or parenthesized inline math.
    Inline,
    /// Double-dollar or bracketed display math.
    Display,
}

/// Terminal font treatment, separate from TeX source and a concrete terminal font.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontStyle {
    /// Upright ordinary text.
    Roman,
    /// Mathematical variable or italic text.
    Italic,
    /// Bold upright text.
    Bold,
    /// Bold mathematical variable.
    BoldItalic,
}

/// Native paint can inherit a later palette without changing mathematical geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Paint {
    /// The caller's mathematical foreground.
    Inherit,
    /// An explicitly requested opaque color.
    Rgb { red: u8, green: u8, blue: u8 },
}

/// The bounded sizes emitted by the current native adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextScale {
    /// Normal terminal font size.
    Full,
    /// The engine's 0.7 script style.
    Script,
    /// The engine's 0.5 scriptscript style.
    ScriptScript,
    /// A larger operator in a two-row reservation.
    Large,
}

/// Fractional text alignment inside its owned cell rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    /// Align fractional text to the top.
    Top,
    /// Align fractional text to the bottom.
    Bottom,
    /// Center fractional text.
    Center,
}

/// One immutable native text operation. Coordinates are relative to the complete formula.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlyphRun {
    /// First reserved column.
    pub x: u16,
    /// First reserved row.
    pub y: u16,
    /// Printable Unicode, never raw TeX or escape sequences.
    pub text: String,
    /// Reserved columns; scaled text may contain more characters.
    pub columns: u16,
    /// Reserved rows.
    pub rows: u16,
    /// Native font scaling.
    pub scale: TextScale,
    /// Alignment within the reservation.
    pub align: VerticalAlign,
    /// Font treatment.
    pub style: FontStyle,
    /// Foreground paint.
    pub paint: Paint,
}

struct FormulaData {
    source: source::FormulaSource,
    scene: engine::Scene,
}

/// One complete delimited formula. Every geometry view shares this exact source owner.
#[derive(Clone)]
pub struct Formula(Arc<FormulaData>);

impl Formula {
    /// Prepare the pinned engine's layout without involving terminal state or I/O.
    pub fn parse(original: &str) -> Result<Self, MathError> {
        let source = source::FormulaSource::new(original)?;
        let scene = engine::prepare(source.body(), source.mode())?;
        Ok(Self(Arc::new(FormulaData { source, scene })))
    }

    /// Exact original source, including its original opening and closing delimiters.
    #[must_use]
    pub fn source(&self) -> &str {
        self.0.source.original()
    }

    /// The style selected by the original delimiters.
    #[must_use]
    pub fn mode(&self) -> MathMode {
        self.0.source.mode()
    }

    /// Project retained engine geometry into native cells without reparsing.
    pub fn layout(&self, width: usize) -> Result<FormulaLayout, MathError> {
        let native = native::project(&self.0.scene, width)?;
        Ok(FormulaLayout {
            formula: self.clone(),
            width: native.width,
            height: native.height,
            axis: native.axis,
            runs: native.runs,
        })
    }
}

/// One retained native layout. Selection is atomic over its rectangle, including blank cells.
pub struct FormulaLayout {
    formula: Formula,
    width: u16,
    height: u16,
    axis: u16,
    runs: Vec<GlyphRun>,
}

impl FormulaLayout {
    /// Move only native geometry across the worker boundary; the engine scene stays behind.
    /// The caller retains the exact source in its semantic text map (MTH-1).
    #[must_use]
    pub fn into_native(self) -> NativeLayout {
        NativeLayout::from_layout(self)
    }

    /// Complete reserved width.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }
    /// Complete reserved height.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }
    /// Mathematical axis row relative to the retained origin.
    #[must_use]
    pub const fn axis(&self) -> u16 {
        self.axis
    }
    /// Prepared native operations; callers may not mutate retained occupancy.
    #[must_use]
    pub fn runs(&self) -> &[GlyphRun] {
        &self.runs
    }
    /// Every intersecting cell resolves to the whole delimited source, never a subexpression.
    #[must_use]
    pub fn source_at(&self, column: usize, row: usize) -> Option<&str> {
        (column < usize::from(self.width) && row < usize::from(self.height))
            .then(|| self.formula.source())
    }
    /// Exact source for formula-only click/copy.
    #[must_use]
    pub fn source(&self) -> &str {
        self.formula.source()
    }
}
