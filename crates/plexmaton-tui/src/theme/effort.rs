//! One explicit RGB palette for the effort selector and composer rules/labels.
//! Callers supply animation phase; resolving a color owns no clock and changes no layout.

use plexmaton_core::ReasoningEffort;
use ratatui::style::Color;

/// Number of discrete samples in one smooth rainbow cycle.
pub const EFFORT_COLOR_PHASES: u16 = 120;

/// Named effort colors. Keep the defaults here, never in individual renderers or scripts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffortPalette {
    none: [u8; 3],
    low: [u8; 3],
    medium: [u8; 3],
    high: [u8; 3],
    disabled: [u8; 3],
    rainbow: [[u8; 3]; 6],
}

impl Default for EffortPalette {
    fn default() -> Self {
        Self {
            none: [192, 198, 210],
            low: [242, 190, 151],
            medium: [153, 216, 226],
            high: [170, 218, 173],
            disabled: [77, 83, 99],
            rainbow: [
                [237, 171, 192],
                [232, 195, 159],
                [207, 215, 161],
                [164, 216, 194],
                [166, 198, 234],
                [200, 179, 234],
            ],
        }
    }
}

impl EffortPalette {
    /// Resolve one letter or marker. Disabled stops never animate; xhigh ignores phase.
    #[must_use]
    pub fn color(
        &self,
        effort: ReasoningEffort,
        available: bool,
        phase: u16,
        position: u16,
    ) -> Color {
        let [r, g, b] = self.rgb(effort, available, phase, position);
        Color::Rgb(r, g, b)
    }

    /// Static full-width rule aligned with the label palette, independent of animation phase.
    #[must_use]
    pub fn rule_color(&self, effort: ReasoningEffort, position: u16, width: u16) -> Color {
        let position = u32::from(position.min(width.saturating_sub(1))) * 119
            / u32::from(width.saturating_sub(1).max(1));
        let [r, g, b] = match effort {
            ReasoningEffort::Xhigh | ReasoningEffort::Max => self.rainbow_at(position as u16),
            _ => self.rgb(effort, true, 0, 0),
        };
        Color::Rgb(r, g, b)
    }

    fn rgb(&self, effort: ReasoningEffort, available: bool, phase: u16, position: u16) -> [u8; 3] {
        if !available {
            return self.disabled;
        }
        match effort {
            ReasoningEffort::Default | ReasoningEffort::None => self.none,
            ReasoningEffort::Low => self.low,
            ReasoningEffort::Medium => self.medium,
            ReasoningEffort::High => self.high,
            ReasoningEffort::Xhigh => self.rainbow_at((position % 6) * 23),
            ReasoningEffort::Max => self.rainbow_at(
                (phase % EFFORT_COLOR_PHASES + (position % 4) * 35) % EFFORT_COLOR_PHASES,
            ),
        }
    }

    fn rainbow_at(&self, phase: u16) -> [u8; 3] {
        let step = phase % EFFORT_COLOR_PHASES;
        let segment = usize::from(step / 20);
        let fraction = step % 20;
        let a = self.rainbow[segment];
        let b = self.rainbow[(segment + 1) % self.rainbow.len()];
        std::array::from_fn(|i| {
            ((u16::from(a[i]) * (20 - fraction) + u16::from(b[i]) * fraction) / 20) as u8
        })
    }
}
