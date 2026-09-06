//! Multi-row cells retain their prepared math boxes. Narrow tables label full-width values.

use super::*;

pub(super) fn render(
    events: Vec<Vec<Vec<Event<'_>>>>,
    alignment: Vec<Alignment>,
    width: usize,
    math: MathPresentation,
) -> Result<Layout, PlainReason> {
    let mut cells = Vec::new();
    let mut bytes = 0;
    let mut formula_count = 0;
    for row in events {
        let mut prepared = Vec::new();
        for cell in row {
            let layout = render_events(cell, width, math)?;
            bytes += layout.allocation_bytes();
            formula_count += layout.formulas.len();
            if bytes > crate::preparation::MAX_PREPARED_BYTES
                || formula_count > crate::text_layout::math::MAX_FORMULAS
            {
                return Err(PlainReason::Complexity);
            }
            prepared.push(layout);
        }
        cells.push(prepared);
    }
    let widths: Vec<_> = (0..alignment.len())
        .map(|column| {
            cells
                .iter()
                .flat_map(|row| &row[column].lines)
                .map(Line::width)
                .max()
                .unwrap_or(1)
                .max(8)
        })
        .collect();
    let mut out = Renderer::new(width, math);
    let (text, offsets) = canonical_text(&cells, |cell| std::borrow::Cow::Borrowed(&cell.text));
    out.layout.text = text;
    if widths.iter().sum::<usize>() + widths.len().saturating_sub(1) * 3 <= width {
        grid(&mut out, cells, &offsets, &widths, &alignment)?;
    } else {
        stacked(&mut out, cells, &offsets)?;
    }
    Ok(out.layout)
}

fn grid(
    out: &mut Renderer,
    cells: Vec<Vec<Layout>>,
    offsets: &[Vec<usize>],
    widths: &[usize],
    alignment: &[Alignment],
) -> Result<(), PlainReason> {
    for (row_index, row) in cells.into_iter().enumerate() {
        let start = out.layout.lines.len();
        let height = row
            .iter()
            .map(|cell| cell.lines.len())
            .max()
            .unwrap_or(1)
            .max(1);
        rows(&mut out.layout, start + height)?;
        let mut x = 0;
        for (column, cell) in row.into_iter().enumerate() {
            if column > 0 {
                for line in &mut out.layout.lines[start..start + height] {
                    pad(line, x);
                    line.spans.push(Span::styled(" │ ", MarkdownRole::Rule));
                }
                x += 3;
            }
            let natural = cell.lines.iter().map(Line::width).max().unwrap_or(0);
            let padding = widths[column].saturating_sub(natural);
            let left = match alignment[column] {
                Alignment::Right => padding,
                Alignment::Center => padding / 2,
                _ => 0,
            };
            place(
                &mut out.layout,
                cell,
                x + left,
                start,
                offsets[row_index][column],
                row_index == 0,
            )?;
            x += widths[column];
        }
        if row_index == 0 {
            out.row(Line::styled(
                widths
                    .iter()
                    .map(|width| "─".repeat(*width))
                    .collect::<Vec<_>>()
                    .join("─┼─"),
                MarkdownRole::Rule,
            ))?;
        }
        out.check()?;
    }
    Ok(())
}

fn stacked(
    out: &mut Renderer,
    cells: Vec<Vec<Layout>>,
    offsets: &[Vec<usize>],
) -> Result<(), PlainReason> {
    let Some(headers) = cells.first() else {
        return Ok(());
    };
    let math_header = headers.iter().any(|cell| !cell.formulas.is_empty());
    let labels: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            if cell.text.is_empty() || !cell.formulas.is_empty() {
                format!("Column {}", index + 1)
            } else {
                cell.text.clone()
            }
        })
        .collect();
    let first = usize::from(cells.len() > 1 && !math_header);
    for (row_index, row) in cells.into_iter().enumerate().skip(first) {
        for (column, cell) in row.into_iter().enumerate() {
            let label = format!("{}:", labels[column]);
            for (line, _) in wrap::ranges(
                Line::styled(label, MarkdownRole::Heading1),
                out.width,
                false,
            ) {
                out.row(line)?;
            }
            let y = out.layout.lines.len();
            place(
                &mut out.layout,
                cell,
                0,
                y,
                offsets[row_index][column],
                row_index == 0,
            )?;
            out.check()?;
        }
        // Decorations do not append semantic newlines: canonical tab/newline copy was built above.
        out.row(Line::default())?;
    }
    Ok(())
}

fn rows(layout: &mut Layout, end: usize) -> Result<(), PlainReason> {
    if end > MAX_LINES {
        return Err(PlainReason::Complexity);
    }
    layout
        .lines
        .resize_with(end.max(layout.lines.len()), Line::default);
    layout
        .rows
        .resize_with(end.max(layout.rows.len()), Vec::new);
    Ok(())
}

fn pad(line: &mut Line, column: usize) {
    if line.width() < column {
        line.spans
            .push(Span::raw(" ".repeat(column - line.width())));
    }
}

fn header(style: Style, is_header: bool) -> Style {
    if is_header {
        style
            .patch(MarkdownRole::Heading1.into())
            .add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn place(
    out: &mut Layout,
    cell: Layout,
    x: usize,
    y: usize,
    offset: usize,
    is_header: bool,
) -> Result<(), PlainReason> {
    rows(out, y + cell.lines.len())?;
    for (index, (line, mut fragments)) in cell.lines.into_iter().zip(cell.rows).enumerate() {
        let target = &mut out.lines[y + index];
        pad(target, x);
        target.spans.extend(line.spans.into_iter().map(|mut span| {
            span.style = header(line.style.clone().patch(span.style), is_header);
            span
        }));
        for fragment in &mut fragments {
            fragment.column += x;
            fragment.text = offset + fragment.text.start..offset + fragment.text.end;
        }
        out.rows[y + index].extend(fragments);
    }
    out.formulas
        .extend(cell.formulas.into_iter().map(|mut formula| {
            formula.column += x;
            formula.row += y;
            formula.text = offset + formula.text.start..offset + formula.text.end;
            formula.style = header(formula.style, is_header);
            formula
        }));
    Ok(())
}
