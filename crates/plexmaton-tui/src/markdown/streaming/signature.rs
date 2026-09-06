//! Stable semantic hashing for a parser event prefix.

use std::{fmt::Debug, ops::Range};

use pulldown_cmark::{CodeBlockKind, Event, Tag};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct EventSignature {
    first: u64,
    second: u64,
    events: u32,
}

pub(super) fn digest(events: &[(Event<'_>, Range<usize>)], cut: usize) -> EventSignature {
    let mut hasher = EventHasher::default();
    for (event, range) in events.iter().filter(|(_, range)| range.end <= cut) {
        hasher.event(event);
        hasher.number(range.start);
        hasher.number(range.end);
    }
    hasher.finish()
}

#[derive(Default)]
struct EventHasher {
    first: u64,
    second: u64,
    events: u32,
}

impl EventHasher {
    fn byte(&mut self, byte: u8) {
        self.first ^= u64::from(byte);
        self.first = self.first.wrapping_mul(0x100000001b3);
        self.second ^= u64::from(byte).wrapping_add(0x9d);
        self.second = self.second.rotate_left(7).wrapping_mul(0x517cc1b727220a95);
    }

    fn number(&mut self, value: usize) {
        self.byte(0xff);
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn string(&mut self, value: &str) {
        self.number(value.len());
        for byte in value.bytes() {
            self.byte(byte);
        }
    }

    fn debug<T: Debug>(&mut self, value: &T) {
        for byte in format!("{value:?}").bytes() {
            self.byte(byte);
        }
    }

    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.byte(1);
                self.string(value);
            }
            None => self.byte(0),
        }
    }

    fn event(&mut self, event: &Event<'_>) {
        self.events = self.events.saturating_add(1);
        match event {
            Event::Start(tag) => {
                self.byte(1);
                self.tag(tag);
            }
            Event::End(tag) => {
                self.byte(2);
                self.debug(tag);
            }
            Event::Text(value) => {
                self.byte(3);
                self.string(value);
            }
            Event::Code(value) => {
                self.byte(4);
                self.string(value);
            }
            Event::InlineMath(value) => {
                self.byte(5);
                self.string(value);
            }
            Event::DisplayMath(value) => {
                self.byte(6);
                self.string(value);
            }
            Event::Html(value) => {
                self.byte(7);
                self.string(value);
            }
            Event::InlineHtml(value) => {
                self.byte(8);
                self.string(value);
            }
            Event::FootnoteReference(value) => {
                self.byte(9);
                self.string(value);
            }
            Event::SoftBreak => self.byte(10),
            Event::HardBreak => self.byte(11),
            Event::Rule => self.byte(12),
            Event::TaskListMarker(checked) => {
                self.byte(13);
                self.byte(u8::from(*checked));
            }
        }
    }

    fn tag(&mut self, tag: &Tag<'_>) {
        match tag {
            Tag::Paragraph => self.byte(1),
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                self.byte(2);
                self.debug(level);
                self.optional_string(id.as_deref());
                self.number(classes.len());
                for class in classes {
                    self.string(class);
                }
                self.number(attrs.len());
                for (name, value) in attrs {
                    self.string(name);
                    self.optional_string(value.as_deref());
                }
            }
            Tag::BlockQuote(kind) => {
                self.byte(3);
                self.debug(kind);
            }
            Tag::CodeBlock(kind) => {
                self.byte(4);
                match kind {
                    CodeBlockKind::Indented => self.byte(0),
                    CodeBlockKind::Fenced(info) => {
                        self.byte(1);
                        self.string(info);
                    }
                }
            }
            Tag::HtmlBlock => self.byte(5),
            Tag::List(start) => {
                self.byte(6);
                self.number(start.unwrap_or_default() as usize);
                self.byte(u8::from(start.is_some()));
            }
            Tag::Item => self.byte(7),
            Tag::FootnoteDefinition(value) => {
                self.byte(8);
                self.string(value);
            }
            Tag::DefinitionList => self.byte(9),
            Tag::DefinitionListTitle => self.byte(10),
            Tag::DefinitionListDefinition => self.byte(11),
            Tag::Table(alignment) => {
                self.byte(12);
                self.number(alignment.len());
                for alignment in alignment {
                    self.debug(alignment);
                }
            }
            Tag::TableHead => self.byte(13),
            Tag::TableRow => self.byte(14),
            Tag::TableCell => self.byte(15),
            Tag::Emphasis => self.byte(16),
            Tag::Strong => self.byte(17),
            Tag::Strikethrough => self.byte(18),
            Tag::Superscript => self.byte(19),
            Tag::Subscript => self.byte(20),
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => {
                self.byte(21);
                self.debug(link_type);
                self.string(dest_url);
                self.string(title);
                self.string(id);
            }
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            } => {
                self.byte(22);
                self.debug(link_type);
                self.string(dest_url);
                self.string(title);
                self.string(id);
            }
            Tag::MetadataBlock(kind) => {
                self.byte(23);
                self.debug(kind);
            }
        }
    }

    fn finish(self) -> EventSignature {
        EventSignature {
            first: self.first,
            second: self.second,
            events: self.events,
        }
    }
}
