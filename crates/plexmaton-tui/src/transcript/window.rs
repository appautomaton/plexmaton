//! A measured viewport slice and its conversion to Ratatui's bounded scroll coordinates.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Window {
    pub(crate) items: Range<usize>,
    pub(crate) skip_rows: usize,
    pub(super) width: u16,
}

impl Window {
    pub(super) const fn empty(len: usize, width: u16) -> Self {
        Self {
            items: len..len,
            skip_rows: 0,
            width,
        }
    }
}

/// The semantic offset stays usize; discard complete rows until the widget's u16 scroll fits.
pub(super) fn trim_scroll_prefix(
    lines: &mut Vec<Line<'static>>,
    mut skip_rows: usize,
    width: u16,
) -> u16 {
    if width == 0 {
        return 0;
    }
    let mut remove = 0_usize;
    while skip_rows > usize::from(u16::MAX) {
        let Some(line) = lines.get(remove) else {
            break;
        };
        let rows = Paragraph::new(line.clone())
            .wrap(Wrap { trim: false })
            .line_count(width)
            .max(1);
        if rows > skip_rows {
            break;
        }
        skip_rows = skip_rows.saturating_sub(rows);
        remove = remove.saturating_add(1);
    }
    if remove > 0 {
        lines.drain(..remove);
    }
    u16::try_from(skip_rows).unwrap_or(u16::MAX)
}
