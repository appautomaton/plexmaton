//! Extend CommonMark's dollar math with source-preserving TeX paired delimiters.
//!
//! The Markdown parser still owns dollar recognition, code and HTML exclusions. Replacement
//! markers protect the contents of backslash-delimited formulas from Markdown reinterpretation;
//! offsets map directly back to the unchanged source, including whitespace and delimiters.

use std::{borrow::Cow, ops::Range};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) const OPTIONS: Options = Options::ENABLE_STRIKETHROUGH
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_TABLES)
    .union(Options::ENABLE_MATH);

struct Replacement {
    source: Range<usize>,
    rewritten: Range<usize>,
    display: bool,
}

pub(super) struct Source<'a> {
    original: &'a str,
    rewritten: Cow<'a, str>,
    replacements: Vec<Replacement>,
}

impl<'a> Source<'a> {
    pub(super) fn new(source: &'a str) -> Result<Self, super::PlainReason> {
        if !source.contains(r"\(") && !source.contains(r"\[") {
            return Ok(Self {
                original: source,
                rewritten: Cow::Borrowed(source),
                replacements: Vec::new(),
            });
        }
        let mut protected = Vec::new();
        for (count, (event, range)) in Parser::new_ext(source, OPTIONS)
            .into_offset_iter()
            .enumerate()
        {
            if count >= super::MAX_EVENTS {
                return Err(super::PlainReason::Complexity);
            }
            if matches!(
                event,
                Event::Code(_)
                    | Event::Html(_)
                    | Event::InlineHtml(_)
                    | Event::InlineMath(_)
                    | Event::DisplayMath(_)
                    | Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
            ) {
                protected.push(range);
            }
        }
        let bytes = source.as_bytes();
        let mut replacements = Vec::new();
        let mut rewritten = String::with_capacity(source.len());
        let mut copied = 0;
        let mut cursor = 0;
        let mut protected_index = 0;
        while cursor + 1 < bytes.len() {
            while protected
                .get(protected_index)
                .is_some_and(|range| range.end <= cursor)
            {
                protected_index += 1;
            }
            if let Some(range) = protected
                .get(protected_index)
                .filter(|range| range.contains(&cursor))
            {
                cursor = range.end;
                continue;
            }
            if bytes[cursor] != b'\\' {
                cursor += 1;
                continue;
            }
            let close = match bytes[cursor + 1] {
                b'(' => b')',
                b'[' => b']',
                _ => {
                    cursor += 2;
                    continue;
                }
            };
            let mut end = cursor + 2;
            while end + 1 < bytes.len() {
                if bytes[end] == b'\\' {
                    if bytes[end + 1] == close {
                        break;
                    }
                    end += 2;
                } else {
                    end += 1;
                }
            }
            end = if end + 1 < bytes.len() {
                end + 2
            } else {
                bytes.len()
            };
            rewritten.push_str(&source[copied..cursor]);
            let start = rewritten.len();
            // Autolinks remain independent even when formulas abut. Dollar placeholders would
            // merge adjacent delimiter runs and could map one formula to another's source.
            rewritten.push_str("<plexmaton-math:span>");
            replacements.push(Replacement {
                source: cursor..end,
                rewritten: start..rewritten.len(),
                display: close == b']',
            });
            if replacements.len() > crate::text_layout::math::MAX_FORMULAS {
                return Err(super::PlainReason::Complexity);
            }
            copied = end;
            cursor = end;
        }
        rewritten.push_str(&source[copied..]);
        Ok(Self {
            original: source,
            rewritten: Cow::Owned(rewritten),
            replacements,
        })
    }

    #[cfg(test)]
    pub(super) fn events(&self) -> impl Iterator<Item = Result<Event<'_>, super::PlainReason>> {
        self.events_with_ranges().map(|event| match event {
            Ok((event, _)) => Ok(event),
            Err(reason) => Err(reason),
        })
    }

    /// Events with source ranges retained for conservative append-only checkpoints.
    ///
    /// The ordinary renderer only needs event values. Checkpoint validation additionally needs
    /// to prove that a complete top-level block is unchanged after the parser has seen the whole
    /// source; ranges make that dependency explicit instead of treating raw prefix bytes as a
    /// semantic identity.
    pub(super) fn events_with_ranges(
        &self,
    ) -> impl Iterator<Item = Result<(Event<'_>, Range<usize>), super::PlainReason>> {
        Parser::new_ext(&self.rewritten, OPTIONS)
            .into_offset_iter()
            .filter_map(|(event, range)| {
                if let Some(replacement) = self.replacements.iter().find(|item| {
                    item.rewritten.start <= range.start && item.rewritten.end >= range.end
                }) {
                    match event {
                        Event::Start(Tag::Link { .. }) if range == replacement.rewritten => {
                            let source = self
                                .original
                                .get(replacement.source.clone())
                                .ok_or(super::PlainReason::Complexity);
                            return Some(source.map(|source| {
                                (
                                    if replacement.display {
                                        Event::DisplayMath(source.into())
                                    } else {
                                        Event::InlineMath(source.into())
                                    },
                                    replacement.source.clone(),
                                )
                            }));
                        }
                        Event::Text(_) | Event::End(TagEnd::Link) => return None,
                        _ => {}
                    }
                }
                Some(
                    match event {
                        Event::InlineMath(_) => self.original_math(range.clone()).map(|source| {
                            (Event::InlineMath(source.into()), self.original_range(range))
                        }),
                        Event::DisplayMath(_) => self.original_math(range.clone()).map(|source| {
                            (
                                Event::DisplayMath(source.into()),
                                self.original_range(range),
                            )
                        }),
                        event => Some((event, self.original_range(range))),
                    }
                    .ok_or(super::PlainReason::Complexity),
                )
            })
    }

    fn original_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.original_offset(range.start);
        let end = self.original_offset(range.end);
        start..end
    }

    fn original_offset(&self, offset: usize) -> usize {
        let mut delta = 0isize;
        for replacement in &self.replacements {
            if offset < replacement.rewritten.start {
                break;
            }
            if offset <= replacement.rewritten.end {
                return if offset == replacement.rewritten.end {
                    replacement.source.end
                } else {
                    replacement.source.start
                };
            }
            delta += replacement.source.len() as isize - replacement.rewritten.len() as isize;
        }
        (offset as isize + delta).max(0) as usize
    }

    fn original_math(&self, range: Range<usize>) -> Option<&str> {
        if self.replacements.iter().any(|replacement| {
            range.start < replacement.rewritten.end && range.end > replacement.rewritten.start
        }) {
            return None;
        }
        let mut original = 0;
        let mut rewritten = 0;
        for replacement in &self.replacements {
            if replacement.rewritten.end > range.start {
                break;
            }
            original = replacement.source.end;
            rewritten = replacement.rewritten.end;
        }
        self.original
            .get(original + range.start - rewritten..original + range.end - rewritten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn math(source: &str) -> Vec<String> {
        Source::new(source)
            .expect("bounded source")
            .events()
            .filter_map(|event| match event.expect("valid source mapping") {
                Event::InlineMath(source) | Event::DisplayMath(source) => Some(source.to_string()),
                _ => None,
            })
            .collect()
    }

    /// MTH-1: paired delimiters, interior whitespace and source offsets survive Markdown parsing.
    #[test]
    fn math_recognition_retains_original_delimiters_and_excludes_literal_regions() {
        let source = "中 **before** $x$ \\( y_i^2 \\) and $$\\frac{a}{b}$$\n\n\\[\n z & =1\r\n\\]\n after $m$";
        assert_eq!(
            math(source),
            [
                "$x$",
                r"\( y_i^2 \)",
                r"$$\frac{a}{b}$$",
                "\\[\n z & =1\r\n\\]",
                "$m$"
            ]
        );
        for source in [
            r"`\(code\)`",
            "```tex\n\\[code\\]\n```",
            r"\\(escaped\\)",
            r"[link](https://example.invalid/\(path\))",
            "<span title=\"\\(attribute\\)\">",
            r"$\text{\(nested\)}$",
        ] {
            let expected = if source.starts_with('$') {
                vec![source.to_owned()]
            } else {
                Vec::new()
            };
            assert_eq!(math(source), expected, "{source}");
        }
        assert_eq!(math("before \\[\n\\frac{1}{"), ["\\[\n\\frac{1}{"]);
    }

    /// MTH-1: neighboring formulas cannot merge delimiters or resolve into the middle of UTF-8.
    #[test]
    fn adjacent_formula_spans_and_literal_marker_text_remain_independent() {
        assert_eq!(
            math(r"\(α\)\(y\)\[z\]$q$"),
            [r"\(α\)", r"\(y\)", r"\[z\]", "$q$"]
        );
        assert_eq!(math(r"<plexmaton-math:span> \(x\)"), [r"\(x\)"]);
    }

    proptest::proptest! {
        /// MTH-1: original paired spans are the identity, independently of adjacency and UTF-8.
        #[test]
        fn adjacent_math_source_mapping_preserves_each_original_span(
            pieces in proptest::collection::vec(proptest::prop_oneof![
                proptest::strategy::Just(r"\(α\)"),
                proptest::strategy::Just(r"\(x_i\)"),
                proptest::strategy::Just(r"\[\frac{a}{b}\]"),
            ], 1..20)
        ) {
            let source = format!("中 {} plus $z$", pieces.concat());
            let mut expected: Vec<_> = pieces.into_iter().map(str::to_owned).collect();
            expected.push("$z$".into());
            proptest::prop_assert_eq!(math(&source), expected);
        }
    }
}
