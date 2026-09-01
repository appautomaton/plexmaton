use plexmaton_core::{AgentStatus, ToolActivityStatus};
use ratatui::style::{Color, Modifier, Style};

/// A semantic colour role.
///
/// Widgets name a role, never a terminal colour. That keeps a palette change to one edit and
/// makes low-colour degradation a single implementation rather than a decision repeated at every
/// call site. The attention roles are deliberately separate from [`Role::Accent`]: the UI/UX
/// contract requires ambient activity, new information, action required, and failure to be
/// distinguishable from each other and from ordinary identity emphasis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    /// Transcript and body text.
    Body,
    /// Secondary text: pointers, summaries, and empty-state hints.
    Muted,
    /// Panel borders and section dividers.
    Border,
    /// Border of the surface that holds keyboard focus.
    BorderFocused,
    /// Headings for sections inside a panel.
    SectionHeading,
    /// Identity emphasis, such as the selected agent marker.
    Accent,
    /// Reversed key hint in the footer.
    KeyHint,
    /// Background work in progress. Must never compete for attention.
    Ambient,
    /// Delivered mail or a published artifact.
    NewInformation,
    /// An approval or clarification waiting in the queue.
    ActionRequired,
    /// A failed agent, or a producer that broke the event contract.
    Failure,
    /// Content the user has selected for copying.
    Selection,
}

impl Role {
    /// Every role, used by tests and by palette completeness checks.
    pub const ALL: [Self; 12] = [
        Self::Body,
        Self::Muted,
        Self::Border,
        Self::BorderFocused,
        Self::SectionHeading,
        Self::Accent,
        Self::KeyHint,
        Self::Ambient,
        Self::NewInformation,
        Self::ActionRequired,
        Self::Failure,
        Self::Selection,
    ];

    /// The attention hierarchy, which must stay mutually distinguishable in every palette.
    pub const ATTENTION: [Self; 4] = [
        Self::Ambient,
        Self::NewInformation,
        Self::ActionRequired,
        Self::Failure,
    ];
}

/// Maps a tool lifecycle onto the attention hierarchy.
///
/// Tool state is not its own colour vocabulary: a running tool is ambient background work and a
/// failed one is a failure, exactly like any other source of those levels.
#[must_use]
pub const fn tool_role(status: ToolActivityStatus) -> Role {
    match status {
        ToolActivityStatus::Queued | ToolActivityStatus::Cancelled => Role::Muted,
        ToolActivityStatus::Running => Role::Ambient,
        ToolActivityStatus::Succeeded => Role::NewInformation,
        ToolActivityStatus::Failed => Role::Failure,
    }
}

/// Maps an agent lifecycle onto the attention hierarchy.
#[must_use]
pub const fn agent_role(status: AgentStatus) -> Role {
    match status {
        AgentStatus::Idle | AgentStatus::Cancelled => Role::Muted,
        AgentStatus::Running | AgentStatus::Waiting => Role::Ambient,
        AgentStatus::Completed => Role::NewInformation,
        AgentStatus::Failed => Role::Failure,
    }
}

/// The one style every palette shares.
///
/// Selection is reversal in all three, because reversal is what a terminal user reads as "selected"
/// regardless of theme, and because it carries the distinction through a modifier rather than a
/// colour — which is what the monochrome palette would have forced anyway.
const SELECTION: Style = Style::new().add_modifier(Modifier::REVERSED);

/// Resolved styles for every [`Role`].
///
/// Fields are private on purpose. Reaching past `style` to a concrete colour is how a design
/// system stops being one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    body: Style,
    muted: Style,
    border: Style,
    border_focused: Style,
    section_heading: Style,
    accent: Style,
    key_hint: Style,
    ambient: Style,
    new_information: Style,
    action_required: Style,
    failure: Style,
    selection: Style,
}

impl Palette {
    /// Palette built from the sixteen ANSI colours.
    ///
    /// This is the default because named colours resolve through the user's own terminal theme,
    /// so the workspace sits inside their configured environment instead of overriding it. It is
    /// also the only palette guaranteed to render on a terminal without truecolour.
    #[must_use]
    pub fn ansi() -> Self {
        Self {
            body: Style::new(),
            muted: Style::new().fg(Color::DarkGray),
            border: Style::new().fg(Color::DarkGray),
            border_focused: Style::new().fg(Color::Cyan),
            section_heading: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            accent: Style::new().fg(Color::Cyan),
            key_hint: Style::new().fg(Color::Black).bg(Color::Cyan),
            ambient: Style::new().fg(Color::Blue),
            new_information: Style::new().fg(Color::Green),
            action_required: Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            failure: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            selection: SELECTION,
        }
    }

    /// Designed truecolour palette matching the published screen-anatomy tokens.
    ///
    /// Opt in only when the terminal is known to support 24-bit colour; it overrides the user's
    /// theme, which is a trade for precision rather than a strict improvement.
    #[must_use]
    pub fn truecolor() -> Self {
        const INK: Color = Color::Rgb(0xDC, 0xE7, 0xEA);
        const MUTED: Color = Color::Rgb(0x8D, 0xA1, 0xA9);
        const LINE: Color = Color::Rgb(0x33, 0x47, 0x4E);
        const ACCENT: Color = Color::Rgb(0x45, 0xC6, 0xCF);
        const AMBIENT: Color = Color::Rgb(0x7D, 0x91, 0x9A);
        const INFO: Color = Color::Rgb(0x55, 0xC0, 0x8A);
        const ATTENTION: Color = Color::Rgb(0xDD, 0xA3, 0x3F);
        const FAILURE: Color = Color::Rgb(0xE8, 0x74, 0x6D);
        const GROUND: Color = Color::Rgb(0x0B, 0x11, 0x14);

        Self {
            body: Style::new().fg(INK),
            muted: Style::new().fg(MUTED),
            border: Style::new().fg(LINE),
            border_focused: Style::new().fg(ACCENT),
            section_heading: Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            accent: Style::new().fg(ACCENT),
            key_hint: Style::new().fg(GROUND).bg(ACCENT),
            ambient: Style::new().fg(AMBIENT),
            new_information: Style::new().fg(INFO),
            action_required: Style::new().fg(ATTENTION).add_modifier(Modifier::BOLD),
            failure: Style::new().fg(FAILURE).add_modifier(Modifier::BOLD),
            selection: SELECTION,
        }
    }

    /// Palette that carries every distinction through modifiers alone.
    ///
    /// The UI/UX contract requires the grammar to survive a monochrome terminal, so this is a
    /// supported mode rather than a fallback that nobody checks. Colour must never be the only
    /// carrier of a distinction that matters.
    #[must_use]
    pub fn monochrome() -> Self {
        Self {
            body: Style::new(),
            muted: Style::new().add_modifier(Modifier::DIM),
            border: Style::new().add_modifier(Modifier::DIM),
            border_focused: Style::new().add_modifier(Modifier::BOLD),
            section_heading: Style::new().add_modifier(Modifier::BOLD),
            accent: Style::new().add_modifier(Modifier::BOLD),
            key_hint: Style::new().add_modifier(Modifier::REVERSED),
            ambient: Style::new().add_modifier(Modifier::DIM),
            new_information: Style::new(),
            action_required: Style::new().add_modifier(Modifier::BOLD),
            failure: Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            selection: SELECTION,
        }
    }

    /// Resolves one role.
    #[must_use]
    pub fn style(&self, role: Role) -> Style {
        match role {
            Role::Body => self.body,
            Role::Muted => self.muted,
            Role::Border => self.border,
            Role::BorderFocused => self.border_focused,
            Role::SectionHeading => self.section_heading,
            Role::Accent => self.accent,
            Role::KeyHint => self.key_hint,
            Role::Ambient => self.ambient,
            Role::NewInformation => self.new_information,
            Role::ActionRequired => self.action_required,
            Role::Failure => self.failure,
            Role::Selection => self.selection,
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::ansi()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Palette, Role};

    fn palettes() -> [(&'static str, Palette); 3] {
        [
            ("ansi", Palette::ansi()),
            ("truecolor", Palette::truecolor()),
            ("monochrome", Palette::monochrome()),
        ]
    }

    #[test]
    fn attention_levels_are_distinguishable_in_every_palette() {
        for (name, palette) in palettes() {
            let styles: HashSet<_> = Role::ATTENTION
                .iter()
                .map(|role| format!("{:?}", palette.style(*role)))
                .collect();
            assert_eq!(
                styles.len(),
                Role::ATTENTION.len(),
                "{name} collapses two attention levels onto one style"
            );
        }
    }

    #[test]
    fn monochrome_carries_every_attention_level_without_colour() {
        let palette = Palette::monochrome();

        for role in Role::ATTENTION {
            let style = palette.style(role);
            assert!(
                style.fg.is_none() && style.bg.is_none(),
                "{role:?} uses colour in the monochrome palette"
            );
        }
    }

    #[test]
    fn focused_and_unfocused_borders_never_look_alike() {
        for (name, palette) in palettes() {
            assert_ne!(
                palette.style(Role::Border),
                palette.style(Role::BorderFocused),
                "{name} cannot show which surface has focus"
            );
        }
    }

    #[test]
    fn every_role_resolves_in_every_palette() {
        for (_, palette) in palettes() {
            for role in Role::ALL {
                let _ = palette.style(role);
            }
        }
    }
}
