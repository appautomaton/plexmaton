//! The mark's colours, spent from the same slots as every role so a theme reaches it too.

use ratatui::style::{Color, Style};

use super::{Palette, Slots};

/// A blue frame lit toward the text colour by a passing sheen, around a purple centre, both
/// fading from the ground; chosen in the conversation-chrome spike.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MarkColours {
    frame: Color,
    core: Color,
    lit: Color,
    ground: Color,
}

impl MarkColours {
    pub(super) const fn from_slots(slots: &Slots) -> Self {
        Self {
            frame: slots.blue,
            core: slots.purple,
            lit: slots.text,
            ground: slots.ground,
        }
    }
}

/// `from` carried `share` of the way to `to`; a colour that is not RGB switches at halfway.
fn toward(from: Color, to: Color, share: f32) -> Color {
    let share = share.clamp(0.0, 1.0);
    let (Color::Rgb(r, g, b), Color::Rgb(tr, tg, tb)) = (from, to) else {
        return if share < 0.5 { from } else { to };
    };
    let mix = |a: u8, b: u8| {
        let value = f32::from(a) + (f32::from(b) - f32::from(a)) * share;
        u8::try_from(value.round() as i32).unwrap_or(b)
    };
    Color::Rgb(mix(r, tr), mix(g, tg), mix(b, tb))
}

impl Palette {
    /// The frame at `level` of its presence, lit by `shine` of a passing sheen.
    #[must_use]
    pub fn mark_frame_at(&self, level: f32, shine: f32) -> Style {
        let colours = self.mark;
        let frame = toward(colours.frame, colours.lit, shine);
        Style::new().fg(toward(colours.ground, frame, level))
    }

    /// The centre at `level` of its presence.
    #[must_use]
    pub fn mark_core_at(&self, level: f32) -> Style {
        Style::new().fg(toward(self.mark.ground, self.mark.core, level))
    }

    /// The product's name beneath the mark, at `level` of its presence.
    #[must_use]
    pub fn mark_name_at(&self, level: f32) -> Style {
        Style::new().fg(toward(self.mark.ground, self.mark.lit, level))
    }
}
