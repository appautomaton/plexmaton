//! Bounded skill choices and the semantic binding owned by the primary composer.

use std::collections::BTreeMap;

use plexmaton_core::AgentId;

use super::ViewState;
use crate::{Direction, surface::SurfaceId};

pub(crate) const VISIBLE_SKILLS: usize = 5;
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SkillPicker {
    choices: Vec<SkillChoice>,
    state: PickerState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum PickerState {
    #[default]
    Closed,
    Open {
        chosen: String,
    },
    Dismissed {
        token: String,
    },
}

impl SkillPicker {
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
        self.choices = retained;
        self.state = PickerState::Closed;
    }

    pub(crate) fn choices(&self) -> &[SkillChoice] {
        &self.choices
    }

    pub(crate) const fn is_open(&self) -> bool {
        matches!(self.state, PickerState::Open { .. })
    }

    pub(crate) fn dismiss(&mut self, text: &str, cursor: usize) -> bool {
        let changed = self.is_open();
        self.state = completion(text, cursor).map_or(PickerState::Closed, |completion| {
            PickerState::Dismissed {
                token: completion.token.to_owned(),
            }
        });
        changed
    }

    pub(crate) fn sync(&mut self, text: &str, cursor: usize) -> bool {
        let before = self.state.clone();
        let Some(completion) = completion(text, cursor) else {
            let dismissed_still_matches = matches!(
                &self.state,
                PickerState::Dismissed { token }
                    if initial_token(text).is_some_and(|current| current == token)
            );
            if !dismissed_still_matches {
                self.state = PickerState::Closed;
            }
            return before != self.state;
        };
        let matches: Vec<_> = self
            .matching(completion.query)
            .into_iter()
            .map(|choice| choice.name.clone())
            .collect();
        if matches.is_empty() {
            self.state = PickerState::Closed;
        } else if matches!(
            &self.state,
            PickerState::Dismissed { token } if token == completion.token
        ) {
            // Dismissal survives caret/request edits until the initial token itself changes.
        } else {
            let chosen = match &self.state {
                PickerState::Open { chosen } if matches.iter().any(|name| name == chosen) => {
                    chosen.clone()
                }
                PickerState::Closed | PickerState::Open { .. } | PickerState::Dismissed { .. } => {
                    matches[0].clone()
                }
            };
            self.state = PickerState::Open { chosen };
        }
        before != self.state
    }

    pub(crate) fn matching(&self, query: &str) -> Vec<&SkillChoice> {
        self.choices
            .iter()
            .filter(|choice| choice.name.starts_with(query))
            .collect()
    }

    pub(crate) fn current_matches(&self, text: &str, cursor: usize) -> Vec<&SkillChoice> {
        completion(text, cursor).map_or_else(Vec::new, |value| self.matching(value.query))
    }

    pub(crate) fn chosen(&self) -> Option<&str> {
        match &self.state {
            PickerState::Open { chosen } => Some(chosen),
            PickerState::Closed | PickerState::Dismissed { .. } => None,
        }
    }

    pub(crate) fn step(&mut self, text: &str, cursor: usize, direction: Direction) -> bool {
        let names: Vec<_> = self
            .current_matches(text, cursor)
            .into_iter()
            .map(|choice| choice.name.clone())
            .collect();
        let Some(current) = self
            .chosen()
            .and_then(|chosen| names.iter().position(|name| name == chosen))
        else {
            return false;
        };
        let next = match direction {
            Direction::Forward => current.saturating_add(1).min(names.len().saturating_sub(1)),
            Direction::Backward => current.saturating_sub(1),
        };
        if next == current {
            return false;
        }
        if let Some(chosen) = names.get(next).cloned() {
            self.state = PickerState::Open { chosen };
        }
        true
    }

    pub(crate) fn window(
        &self,
        text: &str,
        cursor: usize,
        visible: usize,
    ) -> std::ops::Range<usize> {
        let matches = self.current_matches(text, cursor);
        let chosen = self
            .chosen()
            .and_then(|chosen| matches.iter().position(|choice| choice.name == chosen))
            .unwrap_or(0);
        let visible = visible.max(1);
        let start = chosen
            .saturating_add(1)
            .saturating_sub(visible)
            .min(matches.len().saturating_sub(visible));
        start..matches.len().min(start.saturating_add(visible))
    }
}

struct Completion<'a> {
    query: &'a str,
    token: &'a str,
}

fn completion(text: &str, cursor: usize) -> Option<Completion<'_>> {
    let rest = text.strip_prefix('$')?;
    let token = initial_token(text)?;
    let token_end = token.len();
    if cursor == 0 || cursor.saturating_sub(1) > token_end {
        return None;
    }
    let query = rest.get(..cursor.saturating_sub(1))?;
    if token.chars().next().is_some_and(|character| {
        character.is_numeric() || character.is_ascii_uppercase() || character == '_'
    }) {
        return None;
    }
    Some(Completion { query, token })
}

fn initial_token(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('$')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest.get(..end)
}

pub(super) fn binding_matches(text: &str, name: &str) -> bool {
    text.strip_prefix('$')
        .and_then(|rest| rest.split_whitespace().next())
        == Some(name)
}

impl ViewState {
    pub(crate) fn set_skills(&mut self, choices: Vec<SkillChoice>) {
        self.skill_picker.set_choices(choices);
        let known: Vec<_> = self
            .skill_picker
            .choices()
            .iter()
            .map(|choice| choice.name.clone())
            .collect();
        self.skill_bindings
            .retain(|_, name| known.iter().any(|known| known == name));
        self.touch();
    }

    pub(crate) const fn skill_picker(&self) -> &SkillPicker {
        &self.skill_picker
    }

    pub(crate) fn skill_picker_rows(&self) -> u16 {
        if !self.skill_picker.is_open()
            || !self.focus.prefers(SurfaceId::Composer)
            || self.drawer.is_some()
        {
            return 0;
        }
        let listed = self
            .skill_picker
            .current_matches(self.composer().text(), self.composer().cursor())
            .len()
            .min(VISIBLE_SKILLS);
        u16::try_from(listed).unwrap_or(0).saturating_add(3)
    }

    pub(crate) fn sync_skill_picker(&mut self) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        let changed = self.skill_picker.sync(&text, cursor);
        self.retain_primary_skill_binding();
        if changed {
            self.touch();
        }
    }

    pub(crate) fn close_skill_picker(&mut self) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.skill_picker.dismiss(&text, cursor) {
            self.touch();
        }
    }

    pub(crate) fn step_skill_picker(&mut self, direction: Direction) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.skill_picker.step(&text, cursor, direction) {
            self.touch();
        }
    }

    pub(crate) fn accept_skill(&mut self, name: Option<String>) -> bool {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return false;
        };
        let name = name.or_else(|| self.skill_picker.chosen().map(str::to_owned));
        let cursor = self.composer().cursor();
        let Some(name) = name.filter(|name| {
            self.skill_picker
                .current_matches(self.composer().text(), cursor)
                .iter()
                .any(|choice| choice.name == *name)
        }) else {
            return false;
        };
        self.inputs
            .entry(primary.clone())
            .or_default()
            .complete_initial_token(&name);
        self.skill_bindings.insert(primary, name);
        self.skill_picker.state = PickerState::Closed;
        self.touch();
        true
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
            self.skill_picker.state = PickerState::Closed;
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
        let mut picker = SkillPicker::default();
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
}
