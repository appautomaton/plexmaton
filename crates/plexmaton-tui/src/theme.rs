//! Colour in two layers: the slots a theme fills, and the roles the product spends them on.
//!
//! A [`Slots`] is twelve colours named for where each sits — a ground ramp and eight hues around
//! the wheel — and is the only thing a theme supplies. A [`Role`] is what a colour *means* here,
//! and [`Palette::from_slots`] is the one place the eighteen roles are spent on those twelve slots.
//! Widgets name a role and never a colour, so a theme changes how the product looks and never what
//! it says. [`Palette::pastel`] is the shipped assignment, not a closed set.

use plexmaton_core::{AgentStatus, ToolCallStatus};
use ratatui::style::{Color, Modifier, Style};

pub(crate) mod code;
mod effort;
mod markdown;
#[cfg(test)]
mod tests;
pub use effort::{EFFORT_COLOR_PHASES, EffortPalette};
pub(crate) use markdown::MarkdownStyles;

/// The twelve colours a theme assigns, each named for where it sits rather than what it is.
///
/// A slot is a position, not a paint. `red` is the slot a failure points at; what sits in it today
/// happens to be a coral. A theme replaces the twelve values and never the twelve positions, so
/// `red` still reads true once a crimson or a brick is assigned to it.
///
/// Rejected: naming a slot for the colour it currently holds — `CORAL`, `MINT`, `GOLD` — which made
/// every name wrong the moment a theme changed the value, and hid two defects behind names that
/// merely sounded distinct. `orange` and `yellow` sat 6° apart on the wheel and nobody noticed
/// because "orange" and "gold" are different words; and between `blue` and `red` lay 147° of
/// nothing, so the effort rail and the status line each invented a purple of their own rather than
/// ask for one — landing, independently, within 2° of each other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slots {
    /// Panel ground, and the colour a hue is carried toward when it is at rest.
    pub ground: Color,
    /// Rules, dividers and borders that belong to no particular surface.
    pub line: Color,
    /// Secondary text: pointers, summaries, hints.
    pub muted: Color,
    /// Body text.
    pub text: Color,
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub cyan: Color,
    pub blue: Color,
    pub purple: Color,
    pub magenta: Color,
}

impl Slots {
    /// The shipped assignment: the status line's colours on a dark terminal ground.
    ///
    /// The ramp is one blue-tinted hue at four lightnesses rather than four greys, so panel
    /// structure recedes behind content instead of competing with it as neutral grey does.
    #[must_use]
    pub const fn designed() -> Self {
        Self {
            ground: Color::Rgb(0x1C, 0x22, 0x33),
            line: Color::Rgb(0x3D, 0x46, 0x64),
            muted: Color::Rgb(142, 162, 196),
            text: Color::Rgb(0xE6, 0xE9, 0xF0),
            red: Color::Rgb(255, 120, 120),
            orange: Color::Rgb(255, 196, 102),
            yellow: Color::Rgb(245, 208, 114),
            green: Color::Rgb(140, 218, 165),
            cyan: Color::Rgb(120, 210, 205),
            blue: Color::Rgb(130, 180, 240),
            // The value two separate places arrived at on their own while this slot did not exist.
            purple: Color::Rgb(0x8B, 0x5C, 0xF6),
            magenta: Color::Rgb(0xD9, 0x46, 0xEF),
        }
    }

    /// Every slot through one transformation, for a harness that needs a palette differing in
    /// colour alone. Roles are untouched by construction, which is the point: proving that a swap
    /// costs a repaint must not require an API that can also change what a colour means.
    #[must_use]
    pub fn map(self, mut colour: impl FnMut(Color) -> Color) -> Self {
        Self {
            ground: colour(self.ground),
            line: colour(self.line),
            muted: colour(self.muted),
            text: colour(self.text),
            red: colour(self.red),
            orange: colour(self.orange),
            yellow: colour(self.yellow),
            green: colour(self.green),
            cyan: colour(self.cyan),
            blue: colour(self.blue),
            purple: colour(self.purple),
            magenta: colour(self.magenta),
        }
    }
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
    /// Panel borders and section dividers that belong to no particular surface.
    Border,
    /// Border of a focused surface that has no identity hue of its own.
    BorderFocused,
    /// The roster of agents: the index of who exists, rather than anything anyone said.
    SurfaceRoster,
    /// The conversation the user owns.
    SurfacePrimary,
    /// A delegate's conversation, opened beside the user's own.
    SurfaceDelegate,
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
    /// Work a provider did on its own side, outside the fence: the marker of a server tool call
    /// that ended well. One that failed wears `Failure`, because a failure is one wherever it ran;
    /// the tool's name beside the marker still says which it was.
    ServerTool,
    /// Content the user has selected for copying.
    Selection,
    /// The row the next `Enter` acts on: a bar, weight and a hue together, so it is found at a
    /// glance, read first, and tied to the action colour.
    Chosen,
    /// The ground a user's message sits on: this palette's ground lifted toward its line, so the
    /// turn is told by its surface rather than by a hue. Background only; the text keeps `Body`.
    UserMessage,
}

impl Role {
    /// Every role, used by tests and by palette completeness checks.
    pub const ALL: [Self; 18] = [
        Self::Body,
        Self::Muted,
        Self::Border,
        Self::BorderFocused,
        Self::SurfaceRoster,
        Self::SurfacePrimary,
        Self::SurfaceDelegate,
        Self::SectionHeading,
        Self::Accent,
        Self::KeyHint,
        Self::Ambient,
        Self::NewInformation,
        Self::ActionRequired,
        Self::Failure,
        Self::ServerTool,
        Self::Selection,
        Self::Chosen,
        Self::UserMessage,
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

/// One slot's opposite. A harness swapping every slot for this proves a repaint costs styling and
/// nothing else, without an API that could have changed a role's meaning instead.
#[must_use]
pub fn invert(colour: Color) -> Color {
    match colour {
        Color::Rgb(red, green, blue) => Color::Rgb(255 - red, 255 - green, 255 - blue),
        other => other,
    }
}

/// One hue at rest: the same colour carried most of the way to *this palette's* ground.
///
/// Blending toward the ground rather than toward grey keeps the hue legible at low intensity, so an
/// unfocused surface still says whose it is. The ground has to come from the palette: a hard-coded
/// dark one quiets a hue only while the theme is dark, and on a light ground the same arithmetic
/// brightens the resting border past the focused one — the signal inverts instead of fading.
fn quieted(hue: Color, ground: Color) -> Color {
    let (Color::Rgb(red, green, blue), Color::Rgb(gr, gg, gb)) = (hue, ground) else {
        return hue;
    };
    let mix = |value: u8, ground: u8| {
        u8::try_from((u16::from(value) * 45 + u16::from(ground) * 55) / 100).unwrap_or(value)
    };
    Color::Rgb(mix(red, gr), mix(green, gg), mix(blue, gb))
}

/// The user's band: this palette's ground carried a third of the way toward its line.
///
/// Toward the line rather than toward white for the reason [`quieted`] blends toward the ground: on
/// a dark theme the band lifts, on a light one it sinks, and either way it stays a surface beneath
/// the text rather than a colour competing with it.
fn lifted(ground: Color, line: Color) -> Color {
    let (Color::Rgb(red, green, blue), Color::Rgb(lr, lg, lb)) = (ground, line) else {
        return ground;
    };
    // Thirds, rounded to nearest: a third never lands on a half, so inverting both slots inverts
    // the band exactly and a theme still reaches every colour a role paints with.
    let mix = |value: u8, line: u8| {
        u8::try_from((u16::from(value) * 2 + u16::from(line) + 1) / 3).unwrap_or(value)
    };
    Color::Rgb(mix(red, lr), mix(green, lg), mix(blue, lb))
}

/// Resolved styles for every [`Role`].
///
/// Fields are private on purpose. Reaching past `style` to a concrete colour is how a design
/// system stops being one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    markdown: MarkdownTheme,
    /// The ground its slots were assigned, kept so a hue can be carried toward it at rest without
    /// any widget knowing what colour the ground is.
    ground: Color,
    body: Style,
    muted: Style,
    border: Style,
    border_focused: Style,
    surface_roster: Style,
    surface_primary: Style,
    surface_delegate: Style,
    section_heading: Style,
    accent: Style,
    key_hint: Style,
    ambient: Style,
    new_information: Style,
    action_required: Style,
    failure: Style,
    server_tool: Style,
    selection: Style,
    chosen: Style,
    user_message: Style,
}

impl Palette {
    /// Spends the eighteen roles on a theme's twelve slots.
    ///
    /// This mapping is the product's half of the palette and lives in exactly one place. A theme
    /// supplies colours and reaches no further, so no colourway can make a failure read as a
    /// success, take the weight off what needs the user, or leave work in progress competing for
    /// attention. Every role is assigned exactly once; a role left unset would force a widget to
    /// choose a colour, which is what [`Role`] exists to prevent.
    ///
    /// Rejected: a seam taking a function of `Role`, which handed every caller the power to
    /// redefine the meanings this contract reserves — the API said a theme may do the one thing
    /// the contract says it may never do.
    #[must_use]
    pub fn from_slots(slots: Slots) -> Self {
        Self {
            markdown: MarkdownTheme::Inherited,
            ground: slots.ground,
            body: Style::new().fg(slots.text),
            muted: Style::new().fg(slots.muted),
            border: Style::new().fg(slots.line),
            border_focused: Style::new().fg(slots.blue),
            // Each surface keeps the hue whose meaning it already carries: blue for where you are,
            // cyan for work someone else is doing, muted for structure rather than content.
            surface_roster: Style::new().fg(slots.muted),
            surface_primary: Style::new().fg(slots.blue),
            surface_delegate: Style::new().fg(slots.cyan),
            section_heading: Style::new().fg(slots.text).add_modifier(Modifier::BOLD),
            accent: Style::new().fg(slots.yellow),
            key_hint: SELECTION,
            ambient: Style::new().fg(slots.cyan),
            new_information: Style::new().fg(slots.green),
            action_required: Style::new().fg(slots.orange).add_modifier(Modifier::BOLD),
            failure: Style::new().fg(slots.red).add_modifier(Modifier::BOLD),
            // Purple was the wheel's unspent hue, so what the provider did on its side gets a
            // colour no other meaning shares; no weight, because it is finished work, not a call
            // on the user. The user chose the shade on 2026-09-21 against violet, magenta and the
            // slot's earlier lavender.
            server_tool: Style::new().fg(slots.purple),
            selection: SELECTION,
            chosen: Style::new()
                .fg(slots.yellow)
                .bg(slots.ground)
                .add_modifier(Modifier::BOLD),
            user_message: Style::new().bg(lifted(slots.ground, slots.line)),
        }
    }

    /// An arbitrary style per role, for the paint tests that must prove composition survives a
    /// palette which removes a modifier or leaves a foreground unset. Deliberately not public:
    /// outside a test, choosing per role is the thing [`Palette::from_slots`] exists to forbid.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_roles(mut style: impl FnMut(Role) -> Style) -> Self {
        Self {
            markdown: MarkdownTheme::Inherited,
            // No slots were supplied, so resting hues blend toward the designed ground. Only the
            // paint tests build a palette this way, and none of them draws a surface border.
            ground: Slots::designed().ground,
            body: style(Role::Body),
            muted: style(Role::Muted),
            border: style(Role::Border),
            border_focused: style(Role::BorderFocused),
            surface_roster: style(Role::SurfaceRoster),
            surface_primary: style(Role::SurfacePrimary),
            surface_delegate: style(Role::SurfaceDelegate),
            section_heading: style(Role::SectionHeading),
            accent: style(Role::Accent),
            key_hint: style(Role::KeyHint),
            ambient: style(Role::Ambient),
            new_information: style(Role::NewInformation),
            action_required: style(Role::ActionRequired),
            failure: style(Role::Failure),
            server_tool: style(Role::ServerTool),
            selection: style(Role::Selection),
            chosen: style(Role::Chosen),
            user_message: style(Role::UserMessage),
        }
    }

    /// The shipped palette: [`Slots::designed`] spent on the eighteen roles.
    ///
    /// A role says what a thing is — blue for where you are, cyan for work in progress, green for
    /// what finished, orange for what needs you, red for what failed, yellow for what `Enter` acts
    /// on, purple for what the provider did on its own side. Weight makes titles and the chosen
    /// row read first; a low-saturation hue with no weight on it is what keeps work in progress
    /// quiet, because a slant would draw the eye `Ambient` exists to spare.
    /// It differs from any other assignment of the same slots in one way only: its Markdown is
    /// designed for reading rather than inherited from the roles (MD-5).
    ///
    /// Rejected: Catppuccin's mauve-tinted colours, which were nobody's here; and resolving through
    /// the terminal's own ANSI theme, which let whatever the user happened to have configured
    /// decide implicitly what our semantics look like. A palette stated explicitly — by us or by
    /// the user — is the opposite of that, and is what [`Palette::from_slots`] exists for.
    #[must_use]
    pub fn pastel() -> Self {
        Self {
            markdown: MarkdownTheme::Pastel,
            ..Self::from_slots(Slots::designed())
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
        Self::from_slots(Slots::designed().map(invert))
    }

    /// The border of one surface: its own hue, at full strength while it holds focus.
    ///
    /// Hue says which surface this is and intensity says whether it is the one being operated, so
    /// the two questions a reader asks of a border are answered on one channel without colliding:
    /// a dimmed cyan is still the delegate's, just not where the keys are going. A surface with no
    /// identity of its own — a menu, a notice, an approval — passes `None` and keeps the neutral
    /// line. Rejected: one focus colour for every surface, which made two conversations side by
    /// side indistinguishable except by reading their titles.
    #[must_use]
    pub fn surface_border(&self, hue: Option<Role>, focused: bool) -> Style {
        let Some(hue) = hue else {
            return if focused {
                self.border_focused
            } else {
                self.border
            };
        };
        let style = self.style(hue);
        if focused {
            return style;
        }
        match style.fg {
            Some(hue) => style.fg(quieted(hue, self.ground)),
            None => self.border,
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
            Role::SurfaceRoster => self.surface_roster,
            Role::SurfacePrimary => self.surface_primary,
            Role::SurfaceDelegate => self.surface_delegate,
            Role::SectionHeading => self.section_heading,
            Role::Accent => self.accent,
            Role::KeyHint => self.key_hint,
            Role::Ambient => self.ambient,
            Role::NewInformation => self.new_information,
            Role::ActionRequired => self.action_required,
            Role::Failure => self.failure,
            Role::ServerTool => self.server_tool,
            Role::Selection => self.selection,
            Role::Chosen => self.chosen,
            Role::UserMessage => self.user_message,
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::pastel()
    }
}
