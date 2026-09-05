//! Explicit skill preparation retains input ownership while a bounded file worker runs.

use plexmaton_agent::{
    Input, RetryTarget, SkillActivation, UndeliveredInput, UndeliveredReason, UnixMillis,
};

use super::{AfterCommit, LiveRuntime, PendingInput, rejected_user_input};
use crate::{
    RuntimeError,
    native::{ExplicitSkillError, SkillReadTask, explicit_skill_name},
};

pub(super) struct PreparingSkillInput {
    action: SkillInputAction,
    pub(super) read: SkillReadTask,
    state: PreparationState,
}

enum SkillInputAction {
    Submit(PendingInput),
    EditRetry {
        target: RetryTarget,
        text: String,
        name: String,
        observed_at: UnixMillis,
    },
}

enum PreparationState {
    Reading,
    Cancelled(UndeliveredReason),
}

impl PreparingSkillInput {
    pub(super) fn cancel(&mut self, reason: UndeliveredReason) {
        self.state = PreparationState::Cancelled(reason);
        self.read.cancel();
    }
}

impl LiveRuntime {
    /// Bounded user-invocable choices for a composition-root UI projection (SKP-1).
    #[must_use]
    pub fn user_skills(&self) -> Vec<crate::SkillSummary> {
        self.tools.skills().map_or_else(Vec::new, |catalog| {
            catalog
                .entries()
                .iter()
                .filter(|entry| entry.invocation.user)
                .map(|entry| crate::SkillSummary {
                    name: entry.name.as_str().to_owned(),
                    description: entry.description.clone(),
                    source: crate::native::skill_source(entry.origin),
                })
                .collect()
        })
    }

    pub(super) fn resolve_skill_request(
        &self,
        text: &str,
        selected: Option<&str>,
    ) -> Result<Option<String>, ExplicitSkillError> {
        if let Some(name) = selected {
            if !crate::native::selected_skill_matches(text, name) {
                return Err(ExplicitSkillError::SelectionChanged);
            }
            return Ok(Some(name.to_owned()));
        }
        let Some(name) = explicit_skill_name(text) else {
            return Ok(None);
        };
        let Ok(name) = plexmaton_skills::SkillName::new(name) else {
            return Ok(None);
        };
        Ok(self.tools.skills().and_then(|catalog| {
            catalog
                .entries()
                .iter()
                .find(|entry| entry.invocation.user && entry.name == name)
                .map(|entry| entry.name.as_str().to_owned())
        }))
    }

    /// Startup diagnostics are display-only, outside the lazy session's authoritative transcript.
    #[must_use]
    pub fn skill_diagnostics(&self) -> Vec<String> {
        self.tools.skills().map_or_else(Vec::new, |catalog| {
            catalog
                .diagnostics()
                .iter()
                .map(|diagnostic| {
                    display_message(&format!(
                        "{}: {}",
                        diagnostic.path,
                        diagnostic_message(&diagnostic.kind)
                    ))
                })
                .collect()
        })
    }

    pub(super) fn prepare_skill_input(
        &mut self,
        mut pending: PendingInput,
    ) -> Option<PendingInput> {
        let name = match &pending.input {
            Input::Submitted { text } | Input::Steered { text } => {
                self.resolve_skill_request(text, pending.selected_skill.as_deref())
            }
            _ => Ok(None),
        };
        let name = match name {
            Ok(Some(name)) => name,
            Ok(None) => return Some(pending),
            Err(error) => {
                self.return_skill_input(
                    &pending.input,
                    pending.selected_skill.as_deref(),
                    error,
                    UndeliveredReason::SkillUnavailable,
                );
                return None;
            }
        };
        pending.selected_skill = Some(name.clone());
        match SkillReadTask::start(self.tools.skills(), name) {
            Ok(read) => {
                self.preparing_input = Some(PreparingSkillInput {
                    action: SkillInputAction::Submit(pending),
                    read,
                    state: PreparationState::Reading,
                })
            }
            Err(_) => self.return_skill_input(
                &pending.input,
                pending.selected_skill.as_deref(),
                ExplicitSkillError::Worker,
                UndeliveredReason::SkillUnavailable,
            ),
        }
        None
    }

    pub(super) async fn complete_skill_input(
        &mut self,
        result: Result<SkillActivation, ExplicitSkillError>,
    ) -> Result<(), RuntimeError> {
        self.settle_skill_input(result)?;
        self.finish_pending_inputs().await
    }

    pub(super) fn prepare_skill_retry(&mut self, target: RetryTarget, text: String, name: String) {
        match SkillReadTask::start(self.tools.skills(), name.clone()) {
            Ok(read) => {
                self.report.accepted_retry_edit = Some(target.clone());
                self.preparing_input = Some(PreparingSkillInput {
                    action: SkillInputAction::EditRetry {
                        target,
                        text,
                        name,
                        observed_at: self.clock.now(),
                    },
                    read,
                    state: PreparationState::Reading,
                });
            }
            Err(_) => self.return_skill_text(
                text,
                Some(name),
                ExplicitSkillError::Worker,
                UndeliveredReason::SkillUnavailable,
            ),
        }
    }

    fn settle_skill_input(
        &mut self,
        result: Result<SkillActivation, ExplicitSkillError>,
    ) -> Result<(), RuntimeError> {
        let Some(preparing) = self.preparing_input.take() else {
            return Ok(());
        };
        if let PreparationState::Cancelled(reason) = preparing.state {
            self.return_skill_action(preparing.action, ExplicitSkillError::Cancelled, reason);
            return Ok(());
        }
        match result {
            Ok(skill) => match preparing.action {
                SkillInputAction::Submit(mut pending) => {
                    pending.input = match pending.input {
                        Input::Submitted { text } => Input::SkillSubmitted { text, skill },
                        Input::Steered { text } => Input::SkillSteered { text, skill },
                        _ => unreachable!("only explicit user input owns a skill read"),
                    };
                    pending.selected_skill = None;
                    self.pending_inputs.push_front(pending);
                }
                SkillInputAction::EditRetry {
                    target,
                    text,
                    name,
                    observed_at,
                } => {
                    let reaction =
                        self.agent
                            .edit_retry_skill_at(&target, text.clone(), skill, observed_at);
                    match reaction {
                        Ok(reaction) => self.begin_transition(
                            reaction,
                            vec![UndeliveredInput {
                                text,
                                skill: Some(name),
                                reason: UndeliveredReason::PersistenceFailed,
                            }],
                            AfterCommit::None,
                        )?,
                        Err(_) => self.return_skill_text(
                            text,
                            Some(name),
                            ExplicitSkillError::RetryUnavailable,
                            UndeliveredReason::SkillUnavailable,
                        ),
                    }
                }
            },
            Err(error) => self.return_skill_action(
                preparing.action,
                error,
                UndeliveredReason::SkillUnavailable,
            ),
        }
        Ok(())
    }

    fn return_skill_action(
        &mut self,
        action: SkillInputAction,
        error: ExplicitSkillError,
        reason: UndeliveredReason,
    ) {
        match action {
            SkillInputAction::Submit(pending) => self.return_skill_input(
                &pending.input,
                pending.selected_skill.as_deref(),
                error,
                reason,
            ),
            SkillInputAction::EditRetry { text, name, .. } => {
                self.return_skill_text(text, Some(name), error, reason)
            }
        }
    }

    fn return_skill_text(
        &mut self,
        text: String,
        skill: Option<String>,
        error: ExplicitSkillError,
        reason: UndeliveredReason,
    ) {
        self.report.undelivered.push(UndeliveredInput {
            text,
            skill,
            reason,
        });
        self.report.skill_errors.push(display_message(&format!(
            "{error}. Your input was returned."
        )));
    }

    fn return_skill_input(
        &mut self,
        input: &Input,
        selected: Option<&str>,
        error: ExplicitSkillError,
        reason: UndeliveredReason,
    ) {
        if let Some(input) = rejected_user_input(input, selected, reason) {
            self.return_skill_text(input.text, input.skill, error, reason);
        }
    }

    pub(super) async fn cancel_skill_inputs(&mut self, reason: UndeliveredReason) {
        if let Some(preparing) = &mut self.preparing_input {
            preparing.cancel(reason);
            let result = preparing.read.finish().await;
            self.settle_skill_input(result)
                .unwrap_or_else(|_| unreachable!("a cancelled input performs no journal mutation"));
        }
        let mut retained = std::collections::VecDeque::new();
        while let Some(pending) = self.pending_inputs.pop_front() {
            let is_skill = match &pending.input {
                Input::Submitted { text } | Input::Steered { text } => {
                    pending.selected_skill.is_some()
                        || self
                            .resolve_skill_request(text, None)
                            .ok()
                            .flatten()
                            .is_some()
                }
                _ => false,
            };
            if is_skill {
                self.return_skill_input(
                    &pending.input,
                    pending.selected_skill.as_deref(),
                    ExplicitSkillError::Cancelled,
                    reason,
                );
            } else {
                retained.push_back(pending);
            }
        }
        self.pending_inputs = retained;
    }

    /// Control input cannot be rejected by the queue whose work it is cancelling.
    pub(super) fn interrupt_prepared_inputs(&mut self) {
        if let Some(preparing) = &mut self.preparing_input {
            preparing.cancel(UndeliveredReason::Interrupted);
        }
        let mut retained = std::collections::VecDeque::new();
        while let Some(pending) = self.pending_inputs.pop_front() {
            if let Some(input) = rejected_user_input(
                &pending.input,
                pending.selected_skill.as_deref(),
                UndeliveredReason::Interrupted,
            ) {
                self.report.undelivered.push(input);
            } else if !matches!(&pending.input, Input::Interrupted) {
                retained.push_back(pending);
            }
        }
        self.pending_inputs = retained;
    }
}

fn diagnostic_message(kind: &plexmaton_skills::SkillDiagnosticKind) -> String {
    use plexmaton_skills::{SkillDiagnosticKind, SkillOrigin};
    match kind {
        SkillDiagnosticKind::RootUnavailable => "skill directory could not be opened".to_owned(),
        SkillDiagnosticKind::Unreadable => "SKILL.md could not be read".to_owned(),
        SkillDiagnosticKind::InvalidMetadata { error } => error.to_string(),
        SkillDiagnosticKind::Shadowed { winner } => format!(
            "overridden by {}",
            match winner {
                SkillOrigin::ProjectPlexmaton => "project .plexmaton/skills",
                SkillOrigin::ProjectAgents => "project .agents/skills",
                SkillOrigin::User => "user skills",
            }
        ),
    }
}

fn display_message(message: &str) -> String {
    let mut end = message.len().min(1024);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}
