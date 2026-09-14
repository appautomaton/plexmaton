//! Semantic colour tokens and the palettes that assign them a style.
//!
//! Widgets name a [`Role`], never a terminal colour. A [`Palette`] is one complete
//! assignment of those tokens; [`Palette::pastel`] is the shipped one, not a closed
//! set. A
//! new colourway is a new assignment, not a change to a widget.

use plexmaton_core::{AgentStatus, ToolCallStatus};
use ratatui::style::{Modifier, Style};

mod effort;
mod markdown;
pub use effort::{EFFORT_COLOR_PHASES, EffortPalette};
pub(crate) use markdown::MarkdownStyles;

/// The named colours, as the user wrote them for the status line, on a dark terminal ground.
///
/// Names, not hex values, are what a widget or a document refers to. A colour separates what a
/// thing *is*; weight separates what reads first; italic separates what stays quiet.
pub(crate) mod tokens {
    use ratatui::style::Color;

    pub(crate) const BODY: Color = Color::Rgb(0xE6, 0xE9, 0xF0);
    pub(crate) const STEEL: Color = Color::Rgb(142, 162, 196);
    pub(crate) const LINE: Color = Color::Rgb(0x3D, 0x46, 0x64);
    pub(crate) const BAR: Color = Color::Rgb(0x1C, 0x22, 0x33);
    pub(crate) const SKY: Color = Color::Rgb(130, 180, 240);
    pub(crate) const TEAL: Color = Color::Rgb(120, 210, 205);
    pub(crate) const MINT: Color = Color::Rgb(140, 218, 165);
    pub(crate) const GOLD: Color = Color::Rgb(245, 208, 114);
    pub(crate) const ORANGE: Color = Color::Rgb(255, 196, 102);
    pub(crate) const CORAL: Color = Color::Rgb(255, 120, 120);
}
pub use markdown::MarkdownTheme;

/// A semantic colour token.
///
/// Widgets name a role, never a terminal colour. That keeps a palette change to one edit and
/// makes low-colour degradation a single implementation rather than a decision repeated at every
/// call site. The attention roles are deliberately separate from [`Role::Accent`]: the UI/UX
/// contract requires ambient activity, new information, action required, and failure to be
/// distinguishable from each other and from ordinary identity emphasis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Reversed key hint, such as the `⇥` in the collapsed composer's row.
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
    /// The row the next `Enter` acts on: a bar, weight and a hue together, so it is found at a
    /// glance, read first, and tied to the action colour.
    Chosen,
}

impl Role {
    /// Every role, used by tests and by palette completeness checks.
    pub const ALL: [Self; 13] = [
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
        Self::Chosen,
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
/// Tool state is not its own colour vocabulary: a running tool is ambient background work, a call
/// awaiting approval requires action, and a failed one is a failure, exactly like any other source
/// of those levels. A denied call is quiet because it is a resolved user decision, not a failure.
#[must_use]
pub const fn tool_role(status: ToolCallStatus) -> Role {
    match status {
        ToolCallStatus::Queued | ToolCallStatus::Denied | ToolCallStatus::Cancelled => Role::Muted,
        ToolCallStatus::AwaitingApproval => Role::ActionRequired,
        ToolCallStatus::Running => Role::Ambient,
        ToolCallStatus::Succeeded => Role::NewInformation,
        ToolCallStatus::Failed => Role::Failure,
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
/// colour, so a selection reads as a selection on top of whatever role painted the run.
const SELECTION: Style = Style::new().add_modifier(Modifier::REVERSED);

/// Resolved styles for every [`Role`].
///
/// Fields are private on purpose. Reaching past `style` to a concrete colour is how a design
/// system stops being one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    markdown: MarkdownTheme,
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
    chosen: Style,
}

impl Palette {
    /// Builds a complete palette from a function of the colour tokens.
    ///
    /// Every role is assigned exactly once. A palette that left a role unset would force a widget
    /// to pick a colour, which is the thing this type exists to prevent.
    #[must_use]
    pub fn from_roles(mut style: impl FnMut(Role) -> Style) -> Self {
        Self {
            markdown: MarkdownTheme::Inherited,
            body: style(Role::Body),
            muted: style(Role::Muted),
            border: style(Role::Border),
            border_focused: style(Role::BorderFocused),
            section_heading: style(Role::SectionHeading),
            accent: style(Role::Accent),
            key_hint: style(Role::KeyHint),
            ambient: style(Role::Ambient),
            new_information: style(Role::NewInformation),
            action_required: style(Role::ActionRequired),
            failure: style(Role::Failure),
            selection: style(Role::Selection),
            chosen: style(Role::Chosen),
        }
    }

    /// The designed palette: the status line's named colours on a dark terminal ground.
    ///
    /// Each colour names what a thing is: sky for where you are, teal for work in progress, mint
    /// for what finished, orange for what needs you, coral for what failed, violet for who is
    /// speaking, gold for what `Enter` acts on. Weight makes titles and the chosen row read first;
    /// italic keeps work in progress quiet. Its Markdown is the same tokens, designed for reading.
    /// Rejected: Catppuccin's mauve-tinted tokens, which were nobody's here; and resolving through
    /// the user's own terminal theme, which let their configuration decide what our semantics
    /// look like.
    #[must_use]
    pub fn pastel() -> Self {
        use tokens::{BAR, BODY, CORAL, GOLD, LINE, MINT, ORANGE, SKY, STEEL, TEAL};
        Self {
            markdown: MarkdownTheme::Pastel,
            body: Style::new().fg(BODY),
            muted: Style::new().fg(STEEL),
            border: Style::new().fg(LINE),
            border_focused: Style::new().fg(SKY),
            section_heading: Style::new().fg(BODY).add_modifier(Modifier::BOLD),
            accent: Style::new().fg(GOLD),
            key_hint: Style::new().add_modifier(Modifier::REVERSED),
            ambient: Style::new().fg(TEAL).add_modifier(Modifier::ITALIC),
            new_information: Style::new().fg(MINT),
            action_required: Style::new().fg(ORANGE).add_modifier(Modifier::BOLD),
            failure: Style::new().fg(CORAL).add_modifier(Modifier::BOLD),
            selection: SELECTION,
            chosen: Style::new().fg(GOLD).bg(BAR).add_modifier(Modifier::BOLD),
        }
    }

    /// Designed truecolour palette matching the published screen-anatomy tokens.
    ///
    /// Opt in only when the terminal is known to support 24-bit colour; it overrides the user's
    /// A second palette, owned by the tests that need one. A palette swap must change
    /// styling and nothing else, and proving that needs two assignments — not a second
    /// preset in the product that no user can select.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn inverted() -> Self {
        let base = Self::pastel();
        Self::from_roles(|role| {
            // Only a role that already names a colour gets a different one. A role that
            // carries its meaning in modifiers alone keeps them, so inverting cannot
            // invent a colour where the design deliberately has none.
            let style = base.style(role);
            match style.fg {
                Some(ratatui::style::Color::Rgb(r, g, b)) => {
                    style.fg(ratatui::style::Color::Rgb(255 - r, 255 - g, 255 - b))
                }
                _ => style,
            }
        })
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
            Role::Chosen => self.chosen,
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::pastel()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::style::Modifier;

    use plexmaton_core::ToolCallStatus;

    use super::{Palette, Role, tool_role};

    fn palettes() -> [(&'static str, Palette); 2] {
        [
            ("pastel", Palette::pastel()),
            ("inverted", Palette::inverted()),
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
    fn approval_tool_states_map_to_attention_without_treating_denial_as_failure() {
        assert_eq!(
            tool_role(ToolCallStatus::AwaitingApproval),
            Role::ActionRequired
        );
        assert_eq!(tool_role(ToolCallStatus::Denied), Role::Muted);
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
    fn key_hints_are_reversed_without_a_named_colour() {
        for (name, palette) in palettes() {
            let style = palette.style(Role::KeyHint);
            assert!(
                style.add_modifier.contains(Modifier::REVERSED),
                "{name} key hints are not reversed"
            );
            assert!(
                style.fg.is_none() && style.bg.is_none(),
                "{name} key hints inject a named colour"
            );
            assert_ne!(
                style,
                palette.style(Role::Accent),
                "{name} paints keys as identity emphasis"
            );
        }
    }

    #[test]
    fn a_palette_is_a_complete_assignment_of_roles() {
        let base = Palette::pastel();
        let rebuilt = Palette::from_roles(|role| base.style(role));
        for role in Role::ALL {
            assert_eq!(
                rebuilt.style(role),
                base.style(role),
                "{role:?} did not round-trip through from_roles"
            );
        }
    }
}
