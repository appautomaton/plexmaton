use super::*;
use crate::{RetryCandidate, RetryTarget, SkillActivation};

impl Agent {
    /// Available only at an idle, unanswered rate-limited tail.
    pub fn retry_candidate(&self) -> Option<RetryCandidate> {
        if self.is_running() {
            return None;
        }
        self.journal().retry_candidate(self.selected_head())
    }

    /// Retry preserves model input; edited retry preserves the old branch before replacing input.
    pub fn retry_at(
        &mut self,
        target: &RetryTarget,
        edited: Option<String>,
        at: UnixMillis,
    ) -> Result<Reaction, crate::JournalError> {
        self.retry_transition(target, edited.map(|text| (text, None)), at)
    }

    /// Edited retry whose replacement input carries separately loaded explicit skill context.
    pub fn edit_retry_skill_at(
        &mut self,
        target: &RetryTarget,
        text: String,
        skill: SkillActivation,
        at: UnixMillis,
    ) -> Result<Reaction, crate::JournalError> {
        self.retry_transition(target, Some((text, Some(skill))), at)
    }

    fn retry_transition(
        &mut self,
        target: &RetryTarget,
        edited: Option<(String, Option<SkillActivation>)>,
        at: UnixMillis,
    ) -> Result<Reaction, crate::JournalError> {
        let candidate = self
            .retry_candidate()
            .filter(|candidate| &candidate.target == target)
            .ok_or(crate::JournalError::RetryUnavailable)?;
        let mut reaction = Reaction::at(at);
        if let Some((text, skill)) = edited {
            if text.trim().is_empty() {
                return Err(crate::JournalError::RetryUnavailable);
            }
            self.record.branch_before_retry(&candidate, &mut reaction)?;
            self.open_turn(text, skill, at, &mut reaction);
            reaction.events.clear();
            reaction.projection_reset = Some(
                self.record
                    .journal()
                    .project(self.record.selected_head())
                    .expect("new branch projects")
                    .events()
                    .to_vec(),
            );
        } else {
            let turn_id = self.record.next_turn_id();
            self.record.commit(
                JournalEntryPayload::TurnRetried {
                    agent_id: self.record.agent_id().clone(),
                    source_turn_id: target.turn_id.clone(),
                    turn_id: turn_id.clone(),
                    opened_at: at,
                },
                &mut reaction,
            );
            self.record.emit(
                &mut reaction,
                SessionEvent::AgentStatusChanged {
                    agent_id: self.record.agent_id().clone(),
                    status: AgentStatus::Running,
                },
            );
            self.open_step(turn_id, 1, &mut reaction);
        }
        Ok(reaction.into_output())
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, HeadName, TokenUsage, TurnId};

    use super::Agent;
    use crate::test_support::replay_compatibility;
    use crate::{
        ContextAtomValue, DispatchedRequestTiming, Effect, Input, JournalEntryPayload,
        JournalError, JournalRecord, ModelError, ModelRequest, RequestAttemptTerminal,
        RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome, RequestEnvironment,
        RequestEnvironmentFingerprint, RetryTarget, SkillActivation, SkillSource, UnixMillis,
    };

    fn activation(
        name: &str,
        source: SkillSource,
        location: &str,
        instructions: &str,
    ) -> SkillActivation {
        SkillActivation::new(
            name.to_owned(),
            source,
            location.to_owned(),
            "a".repeat(64),
            instructions.to_owned(),
        )
        .unwrap_or_else(|error| panic!("skill activation fixture: {error}"))
    }

    fn failed_turn(
        text: &str,
        skill: Option<SkillActivation>,
    ) -> (Agent, RetryTarget, ModelRequest) {
        let mut agent = Agent::new(
            AgentId::new("agent-retry-skill")
                .unwrap_or_else(|error| panic!("agent fixture: {error}")),
        );
        let _ = agent.announce("Retry skill agent");
        let input = skill.map_or_else(
            || Input::Submitted {
                text: text.to_owned(),
            },
            |skill| Input::SkillSubmitted {
                text: text.to_owned(),
                skill,
            },
        );
        let started = agent.handle_at(input, UnixMillis::new(10));
        let [Effect::CallModel(call)] = started.effects.as_slice() else {
            panic!("skill submission opens a model step");
        };
        let request = call.request.clone();
        let step_id = call.step_id.clone();
        let environment = RequestEnvironment::new(
            replay_compatibility(),
            RequestEnvironmentFingerprint::new([7; 32]),
        );
        let (attempt_id, _) = agent
            .authorize_request_attempt(step_id.clone(), environment, UnixMillis::new(11))
            .unwrap_or_else(|error| panic!("authorize retry fixture: {error:?}"));
        let terminal = RequestAttemptTerminal::new(
            attempt_id,
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(12),
                    None,
                    None,
                    crate::ElapsedMillis::new(1),
                )
                .unwrap_or_else(|error| panic!("timing fixture: {error}")),
                outcome: RequestDispatchedOutcome::RateLimited,
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
            },
        )
        .unwrap_or_else(|error| panic!("terminal fixture: {error}"));
        let _ = agent
            .finish_request_attempt(&terminal)
            .unwrap_or_else(|error| panic!("finish retry attempt: {error:?}"));
        let _ = agent.handle_at(
            Input::Failed {
                step_id,
                error: ModelError::RateLimited { retry_after: None },
            },
            UnixMillis::new(13),
        );
        let target = agent
            .retry_candidate()
            .unwrap_or_else(|| panic!("failed skill turn is retryable"))
            .target;
        (agent, target, request)
    }

    fn original_skill() -> SkillActivation {
        activation(
            "review",
            SkillSource::ProjectNative,
            "/workspace/.plexmaton/skills/review/SKILL.md",
            "ORIGINAL_SKILL_BODY\n",
        )
    }

    /// SKL-5/JRN-8: an ordinary retry adds no atom and reuses the exact recorded activation.
    #[test]
    fn ordinary_retry_keeps_the_previous_skill_exact() {
        let exact = original_skill();
        let (mut agent, target, original_request) =
            failed_turn("$review inspect", Some(exact.clone()));

        let retried = agent
            .retry_at(&target, None, UnixMillis::new(20))
            .unwrap_or_else(|error| panic!("ordinary retry: {error:?}"));

        let [Effect::CallModel(call)] = retried.effects.as_slice() else {
            panic!("retry opens one model request");
        };
        assert_eq!(call.request, original_request);
        assert!(matches!(
            call.request.atoms.as_slice(),
            [user, skill]
                if matches!(user.value(), ContextAtomValue::User { text } if text == "$review inspect")
                    && skill.value() == &ContextAtomValue::Skill(exact)
        ));
        assert!(retried.records.iter().any(|record| matches!(
            record,
            JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, JournalEntryPayload::TurnRetried { .. })
        )));
        assert!(!retried.records.iter().any(|record| matches!(
            record,
            JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, JournalEntryPayload::SkillActivated { .. })
        )));
    }

    /// SKL-5/JRN-8: edited explicit retry replaces activation only on main and retains the old
    /// source and body on the archived branch, with one exact rebuilt projection.
    #[test]
    fn edited_explicit_retry_changes_only_the_new_branch_skill() {
        let original = original_skill();
        let (mut agent, target, original_request) =
            failed_turn("$review inspect", Some(original.clone()));
        let replacement = activation(
            "security",
            SkillSource::User,
            "/home/user/.plexmaton/skills/security/SKILL.md",
            "REPLACEMENT_SKILL_BODY\r\n",
        );
        let edited_text = "$security inspect again";

        let edited = agent
            .edit_retry_skill_at(
                &target,
                edited_text.to_owned(),
                replacement.clone(),
                UnixMillis::new(21),
            )
            .unwrap_or_else(|error| panic!("skill edit retry: {error:?}"));

        let archive = edited.records.iter().find_map(|record| match record {
            JournalRecord::CreateHead { head, .. } => Some(head.clone()),
            _ => None,
        });
        let archive = archive.unwrap_or_else(|| panic!("edit retry archives old branch"));
        let main = HeadName::new("main").unwrap_or_else(|error| panic!("main head: {error}"));
        let main_projection = agent
            .journal()
            .project(&main)
            .unwrap_or_else(|error| panic!("main projection: {error:?}"));
        assert_eq!(
            edited.projection_reset.as_deref(),
            Some(main_projection.events())
        );
        assert!(matches!(
            main_projection.request().atoms.as_slice(),
            [user, skill]
                if matches!(user.value(), ContextAtomValue::User { text } if text == edited_text)
                    && skill.value() == &ContextAtomValue::Skill(replacement.clone())
        ));
        let archived = agent
            .journal()
            .project(&archive)
            .unwrap_or_else(|error| panic!("archive projection: {error:?}"));
        assert_eq!(archived.request(), &original_request);
        assert!(matches!(
            archived.request().atoms.as_slice(),
            [_, skill] if skill.value() == &ContextAtomValue::Skill(original)
        ));
        assert!(matches!(
            edited
                .records
                .iter()
                .filter_map(|record| match record {
                    JournalRecord::AppendEntry { entry, .. } => Some(&entry.payload),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .as_slice(),
            [
                JournalEntryPayload::TurnStarted { text, turn_id: started, .. },
                JournalEntryPayload::SkillActivated { turn_id: activated, activation, .. }
            ] if text == edited_text && started == activated && activation == &replacement
        ));
    }

    /// JRN-8: skill-aware edit preflights the exact target before any branch or input mutation.
    #[test]
    fn invalid_skill_edit_retry_target_leaves_agent_unchanged() {
        let (mut agent, target, _) = failed_turn("$review inspect", Some(original_skill()));
        let wrong = RetryTarget {
            turn_id: TurnId::new("wrong-turn")
                .unwrap_or_else(|error| panic!("wrong turn fixture: {error}")),
            head_revision: target.head_revision,
        };
        let before = agent.clone();

        assert_eq!(
            agent.edit_retry_skill_at(
                &wrong,
                "$security changed".to_owned(),
                activation(
                    "security",
                    SkillSource::User,
                    "/home/user/.plexmaton/skills/security/SKILL.md",
                    "changed",
                ),
                UnixMillis::new(22),
            ),
            Err(JournalError::RetryUnavailable)
        );
        assert_eq!(agent, before);
    }

    /// SKP-2/SKL-5: retry restoration uses the adjacent activation fact, never ambiguous dollars.
    #[test]
    fn numeric_retry_candidate_retains_only_typed_skill_selection() {
        let numeric = activation(
            "100",
            SkillSource::User,
            "/home/user/.plexmaton/skills/100/SKILL.md",
            "NUMERIC_SKILL",
        );
        let (selected, _, _) = failed_turn("$100 request", Some(numeric));
        let selected = selected
            .retry_candidate()
            .unwrap_or_else(|| panic!("selected numeric question is retryable"));
        assert_eq!(selected.question, "$100 request");
        assert_eq!(selected.skill.as_deref(), Some("100"));

        let (literal, _, _) = failed_turn("$100 request", None);
        let literal = literal
            .retry_candidate()
            .unwrap_or_else(|| panic!("literal dollar question is retryable"));
        assert_eq!(literal.question, "$100 request");
        assert_eq!(literal.skill, None);
    }
}
