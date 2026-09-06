//! The composer menu: what the composer's draft completes to, by the token it starts with.
//!
//! `$` lists Skills, `/` lists Commands, and `/resume ` lists saved conversations. The draft is
//! the query and nothing here writes it. A Skill binds into the message (SKP-2); a Command leaves
//! the workspace as a value for the composition root (CMD-1); a conversation leaves as a request
//! (SPK-2). The menu owns no filter of its own, so there is exactly one caret (COM-1).

use std::collections::BTreeMap;

use plexmaton_core::{AgentId, ConversationId};

use super::{ViewState, conversation_picker::ConversationPicker};
use crate::{Direction, surface::SurfaceId};

mod grammar;

pub use grammar::Command;
pub(super) use grammar::binding_matches;
use grammar::{Completion, completion, exact_command, initial_token};

/// Rows the menu shows before it scrolls.
pub(crate) const VISIBLE_ROWS: usize = 5;
const MAX_SKILL_CHOICES: usize = 256;
const MAX_SKILL_CATALOG_BYTES: usize = 64 * 1024;

/// Display origin for one skill completion. It carries no filesystem authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillChoiceSource {
    ProjectNative,
    ProjectShared,
    User,
}

impl SkillChoiceSource {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ProjectNative => "project",
            Self::ProjectShared => "shared",
            Self::User => "user",
        }
    }
}

/// One bounded catalog summary offered by the primary composer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillChoice {
    pub name: String,
    pub description: String,
    pub source: SkillChoiceSource,
}

/// What the draft asks the menu to list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Listing {
    Skills,
    Commands,
    Conversations,
}

impl Listing {
    /// The title on the menu's rule.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Skills => "Skills",
            Self::Commands => "Commands",
            Self::Conversations => "Conversations",
        }
    }

    /// The keys, on the menu's last row.
    #[must_use]
    pub const fn keys(self) -> &'static str {
        match self {
            Self::Skills => " ↑↓ choose · Tab/Enter insert · Esc close",
            Self::Commands => " ↑↓ choose · Tab complete · Enter accept · Esc close",
            Self::Conversations => " ↑↓ choose · Enter open · Esc close",
        }
    }
}

/// One row the menu can act on, by identity rather than by position (INV-1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MenuRow {
    Skill(String),
    Command(Command),
    Conversation(ConversationId),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ComposerMenu {
    skills: Vec<SkillChoice>,
    /// Saved conversations for `/resume`, once the composition root has listed them (SPK-1).
    pub(crate) conversations: Option<ConversationPicker>,
    state: MenuState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum MenuState {
    #[default]
    Closed,
    /// `None` while the listing has rows to come, or none at all: a status row stands in.
    Open {
        chosen: Option<MenuRow>,
    },
    Dismissed {
        token: String,
    },
}

impl ComposerMenu {
    pub(crate) fn set_choices(&mut self, choices: Vec<SkillChoice>) {
        let mut retained = Vec::new();
        let mut bytes = 0_usize;
        for choice in choices.into_iter().take(MAX_SKILL_CHOICES) {
            let Some(next) = bytes
                .checked_add(choice.name.len())
                .and_then(|total| total.checked_add(choice.description.len()))
            else {
                break;
            };
            if choice.name.is_empty() || next > MAX_SKILL_CATALOG_BYTES {
                break;
            }
            bytes = next;
            retained.push(choice);
        }
        self.skills = retained;
        self.state = MenuState::Closed;
    }

    pub(crate) fn choices(&self) -> &[SkillChoice] {
        &self.skills
    }

    pub(crate) const fn is_open(&self) -> bool {
        matches!(self.state, MenuState::Open { .. })
    }

    /// What the draft lists, whether or not the menu is showing it.
    pub(crate) fn listing(&self, text: &str, cursor: usize) -> Option<Listing> {
        completion(text, cursor).map(|completion| completion.listing)
    }

    pub(crate) fn dismiss(&mut self, text: &str, cursor: usize) -> bool {
        let changed = self.is_open();
        self.state =
            completion(text, cursor).map_or(MenuState::Closed, |completion| MenuState::Dismissed {
                token: completion.token.to_owned(),
            });
        changed
    }

    /// Forgets a dismissal, so the next sync shows the listing again.
    pub(crate) fn reopen(&mut self) {
        if matches!(self.state, MenuState::Dismissed { .. }) {
            self.state = MenuState::Closed;
        }
    }

    pub(crate) fn sync(&mut self, text: &str, cursor: usize) -> bool {
        let before = self.state.clone();
        let Some(completion) = completion(text, cursor) else {
            let dismissed_still_matches = matches!(
                &self.state,
                MenuState::Dismissed { token }
                    if initial_token(text).is_some_and(|current| current == token)
            );
            if !dismissed_still_matches {
                self.state = MenuState::Closed;
            }
            return before != self.state;
        };
        let rows = self.rows_for(&completion);
        if matches!(&self.state, MenuState::Dismissed { token } if token == completion.token) {
            // Dismissal survives caret and query edits until the initial token itself changes.
        } else if rows.is_empty() && completion.listing != Listing::Conversations {
            self.state = MenuState::Closed;
        } else {
            let chosen = match &self.state {
                MenuState::Open {
                    chosen: Some(chosen),
                } if rows.contains(chosen) => Some(chosen.clone()),
                _ => rows.first().cloned(),
            };
            self.state = MenuState::Open { chosen };
        }
        before != self.state
    }

    /// The rows the draft admits, in listing order.
    pub(crate) fn rows(&self, text: &str, cursor: usize) -> Vec<MenuRow> {
        completion(text, cursor).map_or_else(Vec::new, |completion| self.rows_for(&completion))
    }

    fn rows_for(&self, completion: &Completion<'_>) -> Vec<MenuRow> {
        match completion.listing {
            Listing::Skills => self
                .skills
                .iter()
                .filter(|choice| choice.name.starts_with(completion.query))
                .map(|choice| MenuRow::Skill(choice.name.clone()))
                .collect(),
            Listing::Commands => Command::ALL
                .into_iter()
                .filter(|command| command.name().starts_with(completion.query))
                .map(MenuRow::Command)
                .collect(),
            Listing::Conversations => self.conversations.as_ref().map_or_else(Vec::new, |picker| {
                picker
                    .matching(completion.query)
                    .into_iter()
                    .map(|choice| MenuRow::Conversation(choice.id.clone()))
                    .collect()
            }),
        }
    }

    /// The skill a name refers to, if the draft currently admits it.
    pub(crate) fn skill(&self, name: &str) -> Option<&SkillChoice> {
        self.skills.iter().find(|choice| choice.name == name)
    }

    pub(crate) fn chosen(&self) -> Option<&MenuRow> {
        match &self.state {
            MenuState::Open { chosen } => chosen.as_ref(),
            MenuState::Closed | MenuState::Dismissed { .. } => None,
        }
    }

    pub(crate) fn step(&mut self, text: &str, cursor: usize, direction: Direction) -> bool {
        let rows = self.rows(text, cursor);
        let Some(current) = self
            .chosen()
            .and_then(|chosen| rows.iter().position(|row| row == chosen))
        else {
            return false;
        };
        let next = match direction {
            Direction::Forward => current.saturating_add(1).min(rows.len().saturating_sub(1)),
            Direction::Backward => current.saturating_sub(1),
        };
        if next == current {
            return false;
        }
        if let Some(chosen) = rows.get(next).cloned() {
            self.state = MenuState::Open {
                chosen: Some(chosen),
            };
        }
        true
    }

    /// The rows on screen: at most `visible`, sliding so the chosen one stays inside.
    pub(crate) fn window(
        &self,
        text: &str,
        cursor: usize,
        visible: usize,
    ) -> std::ops::Range<usize> {
        let rows = self.rows(text, cursor);
        let chosen = self
            .chosen()
            .and_then(|chosen| rows.iter().position(|row| row == chosen))
            .unwrap_or(0);
        let visible = visible.max(1);
        let start = chosen
            .saturating_add(1)
            .saturating_sub(visible)
            .min(rows.len().saturating_sub(visible));
        start..rows.len().min(start.saturating_add(visible))
    }

    fn close(&mut self) {
        self.state = MenuState::Closed;
    }
}

impl ViewState {
    pub(crate) fn set_skills(&mut self, choices: Vec<SkillChoice>) {
        self.composer_menu.set_choices(choices);
        let known: Vec<_> = self
            .composer_menu
            .choices()
            .iter()
            .map(|choice| choice.name.clone())
            .collect();
        self.skill_bindings
            .retain(|_, name| known.iter().any(|known| known == name));
        self.touch();
    }

    pub(crate) const fn composer_menu(&self) -> &ComposerMenu {
        &self.composer_menu
    }

    /// What the composer's draft lists right now, whether or not the menu is open.
    #[must_use]
    pub fn menu_listing(&self) -> Option<Listing> {
        self.composer_menu
            .listing(self.composer().text(), self.composer().cursor())
    }

    /// The rows the draft admits, in listing order.
    pub(crate) fn menu_rows(&self) -> Vec<MenuRow> {
        self.composer_menu
            .rows(self.composer().text(), self.composer().cursor())
    }

    /// Rows the menu asks layout for: its titled rule, the rows, a status row when the listing
    /// has something to say instead of rows, and the key line. The composer's top rule closes
    /// it (SKP-4).
    pub(crate) fn composer_menu_rows(&self) -> u16 {
        if !self.composer_menu.is_open()
            || !self.focus.prefers(SurfaceId::Composer)
            || self.drawer.is_some()
        {
            return 0;
        }
        let listed = self.menu_rows().len().min(VISIBLE_ROWS);
        let status = u16::from(self.menu_status().is_some());
        u16::try_from(listed)
            .unwrap_or(0)
            .saturating_add(2)
            .saturating_add(status)
    }

    /// The Conversations listing's one status row, when it has no rows or an open in flight.
    pub(crate) fn menu_status(&self) -> Option<super::ConversationPickerStatus> {
        if self.menu_listing() != Some(Listing::Conversations) {
            return None;
        }
        let status = self
            .composer_menu
            .conversations
            .as_ref()
            .map_or(super::ConversationPickerStatus::Loading, |picker| {
                picker.status()
            });
        (status != super::ConversationPickerStatus::Ready || self.menu_rows().is_empty())
            .then_some(status)
    }

    pub(crate) fn sync_composer_menu(&mut self) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        let changed = self.composer_menu.sync(&text, cursor);
        self.retain_primary_skill_binding();
        if self.drop_unlisted_conversations() || changed {
            self.touch();
        }
    }

    /// The menu's titled rule: the listing's name, and for a partial listing how much of the
    /// directory it holds (SPK-1).
    pub(crate) fn menu_title(&self) -> String {
        let Some(listing) = self.menu_listing() else {
            return String::new();
        };
        let limited = listing == Listing::Conversations
            && self
                .composer_menu
                .conversations
                .as_ref()
                .is_some_and(ConversationPicker::limited);
        if limited {
            format!(
                "{} · newest {}",
                listing.title(),
                super::conversation_picker::MAX_CONVERSATION_CHOICES
            )
        } else {
            listing.title().to_owned()
        }
    }

    pub(crate) fn close_composer_menu(&mut self) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.composer_menu.dismiss(&text, cursor) {
            self.touch();
        }
        // A dismissed listing has no destination for what the composition root is loading.
        if self.composer_menu.conversations.take().is_some() {
            self.touch();
        }
    }

    pub(crate) fn step_composer_menu(&mut self, direction: Direction) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.composer_menu.step(&text, cursor, direction) {
            self.touch();
        }
    }

    /// The row `Enter` acts on.
    pub(crate) fn menu_chosen(&self) -> Option<MenuRow> {
        self.composer_menu.chosen().cloned()
    }

    /// The Command the whole draft is, if it is one (CMD-2).
    pub(crate) fn exact_command(&self) -> Option<Command> {
        exact_command(self.composer().text())
    }

    pub(crate) fn accept_skill(&mut self, name: Option<String>) -> bool {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return false;
        };
        let name = name.or_else(|| match self.composer_menu.chosen() {
            Some(MenuRow::Skill(name)) => Some(name.clone()),
            _ => None,
        });
        let Some(name) = name.filter(|name| {
            self.menu_rows()
                .iter()
                .any(|row| matches!(row, MenuRow::Skill(listed) if listed == name))
        }) else {
            return false;
        };
        self.inputs
            .entry(primary.clone())
            .or_default()
            .complete_initial_token(&name);
        self.skill_bindings.insert(primary, name);
        self.composer_menu.close();
        self.touch();
        true
    }

    /// Completes the draft to `/name ` without running it (CMD-2).
    pub(crate) fn complete_command(&mut self, command: Command) {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        let input = self.inputs.entry(primary).or_default();
        if !input.text().starts_with(&format!("/{} ", command.name())) {
            input.replace_all(&format!("/{} ", command.name()));
        }
        self.composer_menu.reopen();
        self.sync_composer_menu();
        self.touch();
    }

    /// Takes the composer's draft for a Command that consumed it, and closes the menu.
    pub(crate) fn take_command_draft(&mut self) {
        if let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) {
            self.inputs.entry(primary.clone()).or_default().take();
            self.take_skill_binding(&primary);
        }
        self.composer_menu.close();
        self.touch();
    }

    pub(crate) fn selected_skill(&self, agent: &AgentId) -> Option<&str> {
        self.skill_bindings.get(agent).map(String::as_str)
    }

    pub(crate) fn take_skill_binding(&mut self, agent: &AgentId) -> Option<String> {
        self.skill_bindings.remove(agent)
    }

    pub(crate) fn clear_skill_binding(&mut self, agent: &AgentId) {
        self.skill_bindings.remove(agent);
        if self
            .primary_agent()
            .is_some_and(|primary| &primary.id == agent)
        {
            self.composer_menu.close();
        }
    }

    pub(crate) fn retain_primary_skill_binding(&mut self) {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        let keep = self
            .skill_bindings
            .get(&primary)
            .is_some_and(|name| binding_matches(self.composer().text(), name));
        if !keep {
            self.skill_bindings.remove(&primary);
        }
    }
}

pub(super) type SkillBindings = BTreeMap<AgentId, String>;

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(index: usize, description: String) -> SkillChoice {
        SkillChoice {
            name: format!("skill-{index}"),
            description,
            source: SkillChoiceSource::User,
        }
    }

    /// SKP-1: caller mistakes cannot grow the retained TUI catalog past either local bound.
    #[test]
    fn catalog_retention_is_bounded_by_count_and_bytes() {
        let mut picker = ComposerMenu::default();
        picker.set_choices(
            (0..MAX_SKILL_CHOICES + 20)
                .map(|index| choice(index, "small".to_owned()))
                .collect(),
        );
        assert_eq!(picker.choices().len(), MAX_SKILL_CHOICES);

        picker.set_choices(vec![
            choice(0, "x".repeat(40 * 1024)),
            choice(1, "y".repeat(40 * 1024)),
        ]);
        assert_eq!(picker.choices().len(), 1);
        let retained: usize = picker
            .choices()
            .iter()
            .map(|choice| choice.name.len() + choice.description.len())
            .sum();
        assert!(retained <= MAX_SKILL_CATALOG_BYTES);
    }

    /// CMD-2: a whole draft is a Command only when nothing but the command is in it; `/resume`
    /// keeps its query, `/compact please` is text, and a slash inside prose is a character.
    #[test]
    fn a_whole_draft_is_a_command_only_when_nothing_else_is_in_it() {
        assert_eq!(exact_command("/compact"), Some(Command::Compact));
        assert_eq!(exact_command("/compact  "), Some(Command::Compact));
        assert_eq!(exact_command("/new"), Some(Command::New));
        assert_eq!(exact_command("/resume"), Some(Command::Resume));
        assert_eq!(exact_command("/resume proj"), Some(Command::Resume));
        assert_eq!(exact_command("/compact please"), None);
        assert_eq!(exact_command("/config"), None);
        assert_eq!(exact_command("see /compact"), None);
        assert_eq!(exact_command(""), None);
    }

    /// CMD-1/SKP-3: the listing follows the token the draft starts with, and the query is what
    /// was typed inside it.
    #[test]
    fn the_listing_follows_the_leading_token() {
        let menu = ComposerMenu::default();
        assert_eq!(menu.listing("/", 1), Some(Listing::Commands));
        assert_eq!(menu.listing("/co", 3), Some(Listing::Commands));
        assert_eq!(menu.listing("/compact please", 15), None);
        assert_eq!(menu.listing("/resume ", 8), Some(Listing::Conversations));
        assert_eq!(
            menu.listing("/resume proj", 3),
            Some(Listing::Conversations)
        );
        assert_eq!(menu.listing("$re", 3), Some(Listing::Skills));
        assert_eq!(menu.listing("hello /compact", 14), None);
        assert_eq!(
            menu.rows("/co", 3),
            vec![MenuRow::Command(Command::Compact)]
        );
        assert_eq!(
            menu.rows("/", 1),
            vec![
                MenuRow::Command(Command::New),
                MenuRow::Command(Command::Resume),
                MenuRow::Command(Command::Compact),
            ]
        );
        assert!(menu.rows("/zzz", 4).is_empty());
    }
}
