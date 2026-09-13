//! UI-only addressed retry controls. The runtime remains authority for eligibility and execution.
use super::{TextInput, ViewState};
use crate::surface::SurfaceId;
use plexmaton_core::{TranscriptItemId, TurnId};

/// Actions on one eligible failed message, never global workspace commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryAction {
    Retry,
    EditRetry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Runtime-issued identity of the failed tail; the TUI never increments its revision.
pub struct RetryTarget {
    pub turn_id: TurnId,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Inline actions anchored to retained semantic entries, not reconstructed terminal cells.
pub struct RetryActions {
    pub target: RetryTarget,
    pub question_item: TranscriptItemId,
    pub error_item: TranscriptItemId,
    /// Deliberate skill binding retained beside the historical question, if any.
    pub skill: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// An explicit retry intent; absence of edited text means reuse the existing question.
pub struct RetrySubmission {
    pub target: RetryTarget,
    pub edited_text: Option<String>,
    /// Normalized skill deliberately chosen while editing, if its token remains exact.
    pub skill: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RetryEdit {
    target: RetryTarget,
    saved: TextInput,
    saved_skill: Option<String>,
}

impl ViewState {
    pub(crate) fn has_unsent_input(&self) -> bool {
        // A draft that is a whole Command, `/resume` and its query included, is a request, not
        // something a switch would lose (SPK-2, CMC-2).
        let primary = self.primary_agent().map(|agent| &agent.id);
        let command = self.exact_command().is_some();
        self.retry_edit.is_some()
            || self.inputs.iter().any(|(agent, input)| {
                !input.text().is_empty() && !(Some(agent) == primary && command)
            })
    }
    pub(crate) fn set_retry_actions(&mut self, actions: Option<RetryActions>) {
        let Some(id) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        let Ok(agent) = self.agents.get_mut(&id) else {
            return;
        };
        if agent.retry != actions {
            agent.retry = actions;
            self.touch();
        }
    }

    pub(crate) fn retry_actions(&self) -> Option<&RetryActions> {
        self.primary_agent()?.retry.as_ref()
    }

    pub(crate) fn begin_retry_edit(&mut self) {
        if self.retry_edit.is_some() {
            return;
        }
        let Some(actions) = self.retry_actions().cloned() else {
            return;
        };
        let Some(agent) = self.primary_agent() else {
            return;
        };
        let Some(text) = agent
            .transcript()
            .find(|item| item.id == actions.question_item)
            .map(|item| item.source.clone())
        else {
            return;
        };
        let id = agent.id.clone();
        let retry_skill = actions.skill.clone();
        let saved = self.inputs.remove(&id).unwrap_or_default();
        let saved_skill = self.skill_bindings.remove(&id);
        let input = self.inputs.entry(id.clone()).or_default();
        super::apply_text(input, crate::intent::TextIntent::Paste(text));
        if let Some(skill) = retry_skill
            && super::composer_menu::binding_matches(input.text(), &skill)
        {
            self.skill_bindings.insert(id, skill);
        }
        self.retry_edit = Some(RetryEdit {
            target: actions.target,
            saved,
            saved_skill,
        });
        self.close_drawer();
        self.focus.prefer(SurfaceId::Composer);
        self.touch();
    }

    pub(crate) fn retry_submission(&self) -> Option<RetrySubmission> {
        let edit = self.retry_edit.as_ref()?;
        let text = self.composer().text();
        if text.trim().is_empty() {
            return None;
        }
        Some(RetrySubmission {
            target: edit.target.clone(),
            edited_text: Some(text.to_owned()),
            skill: self
                .primary_agent()
                .and_then(|agent| self.selected_skill(&agent.id))
                .map(str::to_owned),
        })
    }

    pub(crate) fn finish_retry_edit(&mut self) {
        let Some(edit) = self.retry_edit.take() else {
            return;
        };
        if let Some(id) = self.primary_agent().map(|agent| agent.id.clone()) {
            self.skill_bindings.remove(&id);
            if let Some(skill) = edit.saved_skill {
                self.skill_bindings.insert(id.clone(), skill);
            }
            self.inputs.insert(id, edit.saved);
        }
        self.touch();
    }

    pub(crate) fn editing_retry(&self) -> bool {
        self.retry_edit.is_some()
    }

    pub(crate) fn replace_projection(
        &mut self,
        events: Vec<plexmaton_core::ConversationEventEnvelope>,
    ) {
        let status = self.status.clone();
        let inputs = self.inputs.clone();
        let skill_bindings = self.skill_bindings.clone();
        let composer_menu = self.composer_menu.clone();
        let conversation_tree = self.conversation_tree.clone();
        // TRE-4/INV-6: admitted navigation can finish beneath a newly opened Drawer. Keep that
        // workspace interaction and the unchanged resolved model until the receipt arrives. Ordinary retry
        // resets retain their existing policy; runtime admission excludes concurrent navigation.
        let (drawer, model) = if self
            .tree()
            .is_some_and(|tree| tree.pending() == Some(super::TreePending::Navigation))
        {
            (self.drawer.clone(), self.model.clone())
        } else {
            (None, None)
        };
        let focus = self.focus;
        *self = Self {
            status,
            inputs,
            skill_bindings,
            composer_menu,
            conversation_tree,
            drawer,
            model,
            focus,
            ..Self::default()
        };
        for event in events {
            self.apply(event);
        }
        self.touch();
    }
}
