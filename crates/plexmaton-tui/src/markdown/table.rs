//! Tables keep every value: aligned cells when they fit, labelled rows on narrow viewports.
use super::*;
use crate::text_layout::Fragment;
use pulldown_cmark::Alignment;

fn parse_cells(
    events: Vec<Event<'_>>,
    columns: usize,
    palette: &Palette,
) -> Result<Vec<Vec<Line<'static>>>, PlainReason> {
    if columns == 0 || columns > 16 {
        return Err(PlainReason::Complexity);
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = Vec::new();
    for event in events {
        match event {
            Event::Start(Tag::TableCell | Tag::TableHead | Tag::TableRow) => {}
            Event::End(TagEnd::TableCell) => {
                let lines = render_events(std::mem::take(&mut cell), palette, 512)?;
                let mut spans = Vec::new();
                for (index, line) in lines.lines.into_iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::raw(" "));
                    }
                    spans.extend(line.spans);
                }
                row.push(Line::from(spans));
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if row.len() != columns || rows.len() >= 256 {
                    return Err(PlainReason::Complexity);
                }
                rows.push(std::mem::take(&mut row));
            }
            event => cell.push(event),
        }
    }
    Ok(rows)
}

pub(super) fn render(
    events: Vec<Event<'_>>,
    alignment: Vec<Alignment>,
    palette: &Palette,
    width: usize,
) -> Result<Layout, PlainReason> {
    let rows = parse_cells(events, alignment.len(), palette)?;
    let mut out = Renderer::new(palette, width);
    // Canonical plain text is row-major cell text, separated by tabs/newlines, at every width.
    let mut offsets = Vec::new();
    for row in &rows {
        let mut positions = Vec::new();
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                out.layout.text.push('\t');
            }
            let start = out.layout.text.len();
            out.layout.text.push_str(&cell.to_string());
            positions.push(start);
        }
        out.layout.text.push('\n');
        offsets.push(positions);
    }
    if width < alignment.len() * 8 + (alignment.len() - 1) * 3 {
        if let Some(headers) = rows.first() {
            for (row_index, values) in rows.iter().enumerate().skip(1) {
                for (index, (header, value)) in headers.iter().zip(values).enumerate() {
                    let label = if header.width() == 0 {
                        format!("Column {}", index + 1)
                    } else {
                        header.to_string()
                    };
                    let label_bytes = label.len() + 2;
                    let mut line = vec![Span::styled(
                        format!("{label}: "),
                        palette
                            .style(Role::SectionHeading)
                            .add_modifier(Modifier::BOLD),
                    )];
                    line.extend(value.spans.clone());
                    let combined = Line::from(line);
                    let text = combined.to_string();
                    for (line, range) in wrap::ranges(combined, width, false) {
                        let start = range.start.max(label_bytes);
                        let fragment = (start < range.end).then(|| Fragment {
                            column: text[range.start..start].width(),
                            text: offsets[row_index][index] + start - label_bytes
                                ..offsets[row_index][index] + range.end - label_bytes,
                        });
                        out.row(line)?;
                        if let Some(fragment) = fragment {
                            out.layout
                                .rows
                                .last_mut()
                                .expect("row just pushed")
                                .push(fragment);
                        }
                    }
                }
                out.blank()?;
            }
            if rows.len() == 1 {
                for (index, header) in headers.iter().enumerate() {
                    for (line, range) in wrap::ranges(header.clone(), width, false) {
                        out.row(line)?;
                        out.layout
                            .rows
                            .last_mut()
                            .expect("row just pushed")
                            .push(Fragment {
                                column: 0,
                                text: offsets[0][index] + range.start
                                    ..offsets[0][index] + range.end,
                            });
                    }
                }
            }
        }
        return Ok(out.layout);
    }
    let mut widths: Vec<_> = (0..alignment.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].width())
                .max()
                .unwrap_or(1)
                .clamp(8, 48)
        })
        .collect();
    while widths.iter().sum::<usize>() + (widths.len() - 1) * 3 > width {
        let widest = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, n)| **n)
            .map(|(i, _)| i)
            .ok_or(PlainReason::Complexity)?;
        widths[widest] -= 1;
    }
    for (index, row) in rows.into_iter().enumerate() {
        let cells: Vec<_> = row
            .into_iter()
            .zip(&widths)
            .map(|(line, width)| wrap::ranges(line, *width, false))
            .collect();
        for line_index in 0..cells.iter().map(Vec::len).max().unwrap_or(1) {
            let mut spans = Vec::new();
            let mut fragments = Vec::new();
            let mut x = 0;
            for (column, cell) in cells.iter().enumerate() {
                if column > 0 {
                    spans.push(Span::styled(" │ ", palette.style(Role::Border)));
                    x += 3;
                }
                let (line, range) = cell.get(line_index).cloned().unwrap_or_default();
                let padding = widths[column].saturating_sub(line.width());
                let left = match alignment[column] {
                    Alignment::Right => padding,
                    Alignment::Center => padding / 2,
                    _ => 0,
                };
                if !range.is_empty() {
                    fragments.push(Fragment {
                        column: x + left,
                        text: offsets[index][column] + range.start
                            ..offsets[index][column] + range.end,
                    });
                }
                spans.push(Span::raw(" ".repeat(left)));
                spans.extend(line.spans.into_iter().map(|mut span| {
                    if index == 0 {
                        span.style = span
                            .style
                            .patch(palette.style(Role::SectionHeading))
                            .add_modifier(Modifier::BOLD);
                    }
                    span
                }));
                spans.push(Span::raw(" ".repeat(padding - left)));
                x += widths[column];
            }
            out.row(Line::from(spans))?;
            if let Some(row) = out.layout.rows.last_mut() {
                *row = fragments;
            }
        }
        if index == 0 {
            out.row(Line::styled(
                widths
                    .iter()
                    .map(|n| "─".repeat(*n))
                    .collect::<Vec<_>>()
                    .join("─┼─"),
                palette.style(Role::Border),
            ))?;
        }
    }
    Ok(out.layout)
}
