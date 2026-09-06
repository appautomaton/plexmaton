//! The process boundary retains no parser tree, engine scene, or source duplicate.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use unicode_width::UnicodeWidthStr as _;

use crate::{FormulaLayout, GlyphRun, MAX_CELLS, MAX_DIMENSION, TextScale};

/// Immutable, validated native geometry suitable for a bounded preparation reply.
/// Its source owner is the caller's semantic text range, never these presentation glyphs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NativeLayout {
    width: u16,
    height: u16,
    axis: u16,
    runs: Vec<GlyphRun>,
}

impl NativeLayout {
    pub(crate) fn from_layout(layout: FormulaLayout) -> Self {
        Self {
            width: layout.width,
            height: layout.height,
            axis: layout.axis,
            runs: layout.runs,
        }
    }

    /// Reserved columns, including unpainted source-hit cells.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Reserved rows relative to the unchanged logical origin.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Axis row used when composing inline formula and prose baselines.
    #[must_use]
    pub const fn axis(&self) -> u16 {
        self.axis
    }

    /// Disjoint operations with bounded native sizing and printable Unicode.
    #[must_use]
    pub fn runs(&self) -> &[GlyphRun] {
        &self.runs
    }

    /// Retained vector and string capacities, excluding this inline struct.
    #[must_use]
    pub fn allocation_bytes(&self) -> usize {
        self.runs.capacity() * size_of::<GlyphRun>()
            + self
                .runs
                .iter()
                .map(|run| run.text.capacity())
                .sum::<usize>()
    }

    fn validates(&self) -> bool {
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || width * height > MAX_CELLS
            || self.axis >= self.height
            || self.runs.is_empty()
            || self.runs.len() > MAX_CELLS
        {
            return false;
        }
        let mut occupied = vec![false; width * height];
        for run in &self.runs {
            let right = usize::from(run.x) + usize::from(run.columns);
            let bottom = usize::from(run.y) + usize::from(run.rows);
            let scale = match run.scale {
                TextScale::Full => run.rows == 1 && run.text.width() == usize::from(run.columns),
                TextScale::Script | TextScale::ScriptScript => {
                    run.rows == 1 && run.columns <= 7 && run.text.len() <= 4096
                }
                TextScale::Large => {
                    run.rows == 2
                        && run.columns <= 14
                        && run.columns.is_multiple_of(2)
                        && run.text.len() <= 4096
                }
            };
            if run.text.len() > crate::MAX_RUN_BYTES
                || !scale
                || run.columns == 0
                || run.text.is_empty()
                || run.text.chars().any(char::is_control)
                || right > width
                || bottom > height
            {
                return false;
            }
            for y in usize::from(run.y)..bottom {
                for x in usize::from(run.x)..right {
                    if std::mem::replace(&mut occupied[y * width + x], true) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl<'de> Deserialize<'de> for NativeLayout {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            width: u16,
            height: u16,
            axis: u16,
            runs: Vec<GlyphRun>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let layout = Self {
            width: wire.width,
            height: wire.height,
            axis: wire.axis,
            runs: wire.runs,
        };
        if !layout.validates() {
            return Err(D::Error::custom("invalid native math reservations"));
        }
        Ok(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MTH-2/MTH-4: the actual engine output survives IPC; forged commands or occupancy cannot.
    #[test]
    fn native_reply_roundtrip_validates_the_complete_corpus_and_rejects_forged_runs() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/attention-derivatives.json"))
                .expect("fixture");
        let source = fixture["text"].as_str().expect("source");
        for formula in fixture["math"].as_array().expect("ranges") {
            let start = formula["start"].as_u64().expect("start") as usize;
            let end = formula["end"].as_u64().expect("end") as usize;
            for width in [120, 88, 60] {
                let native = crate::Formula::parse(&source[start..end])
                    .expect("formula")
                    .layout(width)
                    .expect("layout")
                    .into_native();
                let encoded = serde_json::to_vec(&native).expect("encode");
                assert_eq!(
                    serde_json::from_slice::<NativeLayout>(&encoded)
                        .expect("validated native output"),
                    native
                );
            }
        }
        let native = crate::Formula::parse("$x$")
            .expect("formula")
            .layout(60)
            .expect("layout")
            .into_native();
        let original = serde_json::to_value(&native).expect("wire");
        for (field, value) in [
            ("text", serde_json::json!("\u{1b}]52;c;payload\u{7}")),
            ("rows", serde_json::json!(2)),
            ("x", serde_json::json!(65535)),
            ("columns", serde_json::json!(0)),
        ] {
            let mut wire = original.clone();
            wire["runs"][0][field] = value;
            assert!(
                serde_json::from_value::<NativeLayout>(wire).is_err(),
                "{field}"
            );
        }
        let mut wire = original;
        let run = wire["runs"][0].clone();
        wire["runs"].as_array_mut().expect("runs").push(run);
        assert!(
            serde_json::from_value::<NativeLayout>(wire).is_err(),
            "overlap"
        );
    }
}
