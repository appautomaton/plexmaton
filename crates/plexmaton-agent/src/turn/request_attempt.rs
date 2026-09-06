//! Durable request authorization and terminal transitions owned by the active agent step.

use super::Agent;
use crate::{
    ModelStepId, Reaction, RequestAttemptId, RequestAttemptOwner, RequestAttemptRefusal,
    RequestAttemptTerminal, RequestEnvironment, UnixMillis,
};

impl Agent {
    /// Authorizes the exact active model step before any request effect begins (TIM-2, JRN-7).
    pub fn authorize_request_attempt(
        &mut self,
        step_id: ModelStepId,
        environment: RequestEnvironment,
        authorized_at: UnixMillis,
    ) -> Result<(RequestAttemptId, Reaction), RequestAttemptRefusal> {
        let Some(expected) = self.active_model_step() else {
            return Err(RequestAttemptRefusal::NoActiveStep);
        };
        if expected != step_id {
            return Err(RequestAttemptRefusal::WrongStep { expected });
        }

        let mut reaction = Reaction::at(authorized_at);
        let attempt_id = self.record.next_request_attempt_id();
        self.record
            .authorize_request_attempt(
                attempt_id.clone(),
                RequestAttemptOwner::AgentStep { step_id },
                environment,
                authorized_at,
                &mut reaction,
            )
            .map_err(RequestAttemptRefusal::Journal)?;
        Ok((attempt_id, reaction.into_output()))
    }

    /// Appends one validated terminal fact and emits its journal-derived turn usage projection.
    ///
    /// A terminal remains legal after interruption because request ownership outlives the live
    /// step until the runtime observes the owned operation's exact effect boundary (TIM-5).
    pub fn finish_request_attempt(
        &mut self,
        terminal: &RequestAttemptTerminal,
    ) -> Result<Reaction, RequestAttemptRefusal> {
        let mut reaction = Reaction::default();
        self.record
            .finish_request_attempt(terminal, &mut reaction)
            .map_err(|error| match error {
                super::super::record::RequestAttemptCommitError::Journal(error) => {
                    RequestAttemptRefusal::Journal(error)
                }
                super::super::record::RequestAttemptCommitError::Projection(error) => {
                    RequestAttemptRefusal::Projection(error)
                }
            })?;
        Ok(reaction.into_output())
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, ConversationEvent, HeadName, TokenCounts, TokenUsage};

    use super::Agent;
    use crate::test_support::replay_compatibility;
    use crate::{
        DispatchedRequestTiming, Effect, ElapsedMillis, HeadRevision, Input, JournalError,
        JournalRecord, ModelDeliveryRefusal, ModelEvent, ModelStepId, RequestAttemptId,
        RequestAttemptRefusal, RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost,
        RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
        RequestNotDispatchedOutcome, StopReason, UndeliveredModelInput, UnixMillis, UsdCostTicks,
    };

    fn agent() -> Agent {
        let mut agent = Agent::new(
            AgentId::new("agent-attempt").unwrap_or_else(|error| panic!("agent fixture: {error}")),
        );
        let _announced = agent.announce("Attempt agent");
        agent
    }

    fn environment(seed: u8) -> RequestEnvironment {
        RequestEnvironment::new(
            replay_compatibility(),
            RequestEnvironmentFingerprint::new([seed; 32]),
        )
    }

    fn open_step(agent: &mut Agent) -> ModelStepId {
        let reaction = agent.handle_at(
            Input::Submitted {
                text: "hello".to_owned(),
            },
            UnixMillis::new(10),
        );
        let [Effect::CallModel(call)] = reaction.effects.as_slice() else {
            panic!("turn fixture opens one model step");
        };
        call.step_id.clone()
    }

    fn terminal(attempt_id: RequestAttemptId, usage: TokenUsage) -> RequestAttemptTerminal {
        let cost = if matches!(usage, TokenUsage::Complete(_)) {
            RequestCost::Known {
                usd_ticks: UsdCostTicks::new(25),
            }
        } else {
            RequestCost::Unavailable
        };
        RequestAttemptTerminal::new(
            attempt_id,
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(20),
                    Some(ElapsedMillis::new(1)),
                    None,
                    ElapsedMillis::new(2),
                )
                .unwrap_or_else(|error| panic!("timing fixture: {error}")),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::EndOfTurn,
                },
                usage,
                cost,
            },
        )
        .unwrap_or_else(|error| panic!("terminal fixture: {error}"))
    }

    fn counts(input: u64, output: u64) -> TokenCounts {
        TokenCounts {
            input,
            cached_input: Some(1),
            cache_write_input: Some(1),
            output,
            reasoning_output: Some(1),
            total: input + output,
        }
    }

    fn main() -> HeadName {
        HeadName::new("main").unwrap_or_else(|error| panic!("head fixture: {error}"))
    }

    /// TIM-2/TIM-4/JRN-7: only the exact active step gains a stable pre-effect attempt fact, and
    /// the audit transition advances neither semantic ancestry nor the selected head.
    #[test]
    fn tim_2_agent_authorizes_only_the_exact_active_step_without_advancing_context() {
        let mut idle = agent();
        let idle_before = idle.journal().clone();
        assert_eq!(
            idle.authorize_request_attempt(
                ModelStepId::new(
                    plexmaton_core::TurnId::new("stale")
                        .unwrap_or_else(|error| panic!("turn fixture: {error}")),
                    1,
                ),
                environment(0),
                UnixMillis::new(11),
            ),
            Err(RequestAttemptRefusal::NoActiveStep)
        );
        assert_eq!(idle.journal(), &idle_before);

        let mut agent = agent();
        let step_id = open_step(&mut agent);
        let wrong = ModelStepId::new(step_id.turn_id().clone(), 2);
        let before = agent.journal().clone();
        assert_eq!(
            agent.authorize_request_attempt(wrong, environment(1), UnixMillis::new(11)),
            Err(RequestAttemptRefusal::WrongStep {
                expected: step_id.clone(),
            })
        );
        assert_eq!(agent.journal(), &before);

        let target = agent
            .journal()
            .head_target(&main())
            .unwrap_or_else(|error| panic!("head target: {error:?}"))
            .cloned();
        let revision = agent
            .journal()
            .head_revision(&main())
            .unwrap_or_else(|error| panic!("head revision: {error:?}"));
        let atoms = agent.record();
        let environment = environment(2);
        let (attempt_id, reaction) = agent
            .authorize_request_attempt(step_id.clone(), environment.clone(), UnixMillis::new(12))
            .unwrap_or_else(|error| panic!("authorize request: {error:?}"));

        assert!(attempt_id.as_str().starts_with("request-attempt-j"));
        assert!(reaction.events.is_empty());
        assert!(reaction.effects.is_empty());
        let [
            JournalRecord::RequestAttemptAuthorized {
                expected_head_revision,
                fact,
                ..
            },
        ] = reaction.records.as_slice()
        else {
            panic!("authorization emits one audit record");
        };
        assert_eq!(*expected_head_revision, revision);
        assert_eq!(fact.attempt_id(), &attempt_id);
        assert!(matches!(
            fact.owner(),
            crate::RequestAttemptOwner::AgentStep { step_id: actual } if actual == &step_id
        ));
        assert_eq!(
            fact.semantic_boundary(),
            target.as_ref().expect("turn boundary")
        );
        assert_eq!(fact.environment(), &environment);
        assert_eq!(fact.authorized_at(), UnixMillis::new(12));
        assert_eq!(
            agent
                .journal()
                .head_revision(&main())
                .unwrap_or_else(|error| panic!("head revision after auth: {error:?}")),
            revision
        );
        assert_eq!(agent.record(), atoms, "attempts are not context atoms");
    }

    /// TIM-3: streamed provider usage has no attempt identity and cannot change accounting or
    /// leave deferred warnings behind. Only a correlated terminal may consume the report.
    #[test]
    fn tim_3_streamed_usage_is_refused_without_mutating_agent_state() {
        let mut agent = agent();
        let step_id = open_step(&mut agent);
        let _authorized = agent
            .authorize_request_attempt(step_id.clone(), environment(3), UnixMillis::new(12))
            .unwrap_or_else(|error| panic!("authorize first: {error:?}"));
        let before = agent.clone();
        for usage in [
            TokenUsage::Complete(counts(3, 2)),
            TokenUsage::Partial(counts(3, 2)),
            TokenUsage::Unavailable,
        ] {
            let streamed = agent.handle_at(
                Input::Streamed {
                    step_id: step_id.clone(),
                    event: ModelEvent::Usage(usage),
                },
                UnixMillis::new(13),
            );
            assert!(streamed.records.is_empty());
            assert!(streamed.events.is_empty());
            assert!(streamed.effects.is_empty());
            assert_eq!(
                streamed.undelivered_model,
                [UndeliveredModelInput {
                    step_id: step_id.clone(),
                    reason: ModelDeliveryRefusal::UsageRequiresAttemptTerminal,
                }]
            );
            assert_eq!(agent, before);
        }
    }

    /// TIM-3/TIM-5: retries are distinct immutable attempts; each dispatched terminal emits the
    /// same cumulative usage event live and on journal replay.
    #[test]
    fn reported_step_usage_is_aggregated_for_the_owning_turn() {
        let mut agent = agent();
        let step_id = open_step(&mut agent);
        let atoms = agent.record();
        let (first_id, _) = agent
            .authorize_request_attempt(step_id.clone(), environment(3), UnixMillis::new(12))
            .unwrap_or_else(|error| panic!("authorize first: {error:?}"));
        let first_terminal = terminal(first_id.clone(), TokenUsage::Complete(counts(10, 4)));
        let first = agent
            .finish_request_attempt(&first_terminal)
            .unwrap_or_else(|error| panic!("finish first: {error:?}"));
        assert!(matches!(
            first.events.as_slice(),
            [event] if matches!(
                &event.event,
                ConversationEvent::TurnUsageUpdated {
                    usage: TokenUsage::Complete(counts),
                    ..
                } if counts.total == 14
            )
        ));

        let (retry_id, _) = agent
            .authorize_request_attempt(step_id, environment(3), UnixMillis::new(14))
            .unwrap_or_else(|error| panic!("authorize retry: {error:?}"));
        assert_ne!(retry_id, first_id);
        let retry_terminal = terminal(retry_id, TokenUsage::Unavailable);
        let retry = agent
            .finish_request_attempt(&retry_terminal)
            .unwrap_or_else(|error| panic!("finish retry: {error:?}"));
        assert!(matches!(
            retry.events.as_slice(),
            [event] if matches!(
                &event.event,
                ConversationEvent::TurnUsageUpdated {
                    usage: TokenUsage::Partial(counts),
                    ..
                } if counts.total == 14
            )
        ));

        let replayed = agent
            .journal()
            .project(&main())
            .unwrap_or_else(|error| panic!("project attempts: {error:?}"));
        let replayed_usage = replayed
            .events()
            .iter()
            .filter_map(|event| match &event.event {
                event @ ConversationEvent::TurnUsageUpdated { .. } => Some(event.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            replayed_usage,
            first
                .events
                .iter()
                .chain(&retry.events)
                .map(|event| event.event.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(agent.record(), atoms, "attempt facts remain outside atoms");
    }

    /// TIM-3/TIM-5: late confirmation that a cancelled retry never dispatched removes its unknown
    /// coverage, without adding fabricated zero-token provider usage.
    #[test]
    fn tim_5_not_dispatched_terminal_restores_known_turn_coverage_after_interrupt() {
        let mut agent = agent();
        let step = open_step(&mut agent);
        let (first_id, _) = agent
            .authorize_request_attempt(step.clone(), environment(1), UnixMillis::new(11))
            .unwrap_or_else(|error| panic!("authorize first: {error:?}"));
        let first = agent
            .finish_request_attempt(&terminal(first_id, TokenUsage::Complete(counts(8, 2))))
            .unwrap_or_else(|error| panic!("finish first: {error:?}"));
        let (retry_id, _) = agent
            .authorize_request_attempt(step, environment(1), UnixMillis::new(12))
            .unwrap_or_else(|error| panic!("authorize retry: {error:?}"));
        let interrupted = agent.handle_at(Input::Interrupted, UnixMillis::new(13));
        assert!(interrupted.events.iter().any(|event| matches!(
            &event.event, ConversationEvent::TurnUsageUpdated { usage: TokenUsage::Partial(counts), .. }
                if counts.total == 10
        )));
        let cancelled = RequestAttemptTerminal::new(
            retry_id,
            RequestAttemptTerminalState::NotDispatched {
                outcome: RequestNotDispatchedOutcome::Cancelled,
            },
        )
        .unwrap_or_else(|error| panic!("cancelled terminal: {error}"));
        let final_report = agent
            .finish_request_attempt(&cancelled)
            .unwrap_or_else(|error| panic!("record cancellation: {error:?}"));
        assert!(
            matches!(final_report.events.as_slice(), [event] if matches!(
                &event.event, ConversationEvent::TurnUsageUpdated { usage: TokenUsage::Complete(counts), .. }
                    if counts.total == 10
            ))
        );
        let replayed = agent
            .journal()
            .project(&main())
            .unwrap_or_else(|error| panic!("replay coverage: {error:?}"));
        let usage_events = |events: &[plexmaton_core::ConversationEventEnvelope]| {
            events
                .iter()
                .filter(|event| matches!(event.event, ConversationEvent::TurnUsageUpdated { .. }))
                .map(|event| event.event.clone())
                .collect::<Vec<_>>()
        };
        let live = first
            .events
            .into_iter()
            .chain(interrupted.events)
            .chain(final_report.events)
            .collect::<Vec<_>>();
        assert_eq!(usage_events(replayed.events()), usage_events(&live));
    }

    /// TIM-5: an owned dispatched request may end after interruption, but unknown and duplicate
    /// terminal identities are typed refusals that leave the journal unchanged.
    #[test]
    fn tim_5_terminal_after_interrupt_is_retained_and_invalid_terminals_mutate_nothing() {
        let mut agent = agent();
        let step_id = open_step(&mut agent);
        let (attempt_id, _) = agent
            .authorize_request_attempt(step_id, environment(4), UnixMillis::new(12))
            .unwrap_or_else(|error| panic!("authorize request: {error:?}"));
        let _interrupted = agent.handle_at(Input::Interrupted, UnixMillis::new(13));
        let finished = terminal(attempt_id.clone(), TokenUsage::Complete(counts(8, 2)));
        let reaction = agent
            .finish_request_attempt(&finished)
            .unwrap_or_else(|error| panic!("finish after interrupt: {error:?}"));
        assert!(matches!(
            reaction.events.as_slice(),
            [event] if matches!(event.event, ConversationEvent::TurnUsageUpdated { .. })
        ));

        let finished_journal = agent.journal().clone();
        assert_eq!(
            agent.finish_request_attempt(&finished),
            Err(RequestAttemptRefusal::Journal(
                JournalError::DuplicateRequestAttemptTerminal(attempt_id)
            ))
        );
        assert_eq!(agent.journal(), &finished_journal);

        let missing_id = RequestAttemptId::new("missing-attempt")
            .unwrap_or_else(|error| panic!("attempt fixture: {error}"));
        let missing = RequestAttemptTerminal::new(
            missing_id.clone(),
            RequestAttemptTerminalState::NotDispatched {
                outcome: RequestNotDispatchedOutcome::PreparationFailed,
            },
        )
        .unwrap_or_else(|error| panic!("missing terminal fixture: {error}"));
        assert_eq!(
            agent.finish_request_attempt(&missing),
            Err(RequestAttemptRefusal::Journal(
                JournalError::MissingRequestAttempt(missing_id)
            ))
        );
        assert_eq!(agent.journal(), &finished_journal);
        assert_eq!(
            agent
                .journal()
                .head_revision(&main())
                .unwrap_or_else(|error| panic!("terminal head revision: {error:?}")),
            HeadRevision::new(2),
            "attempt and turn terminals do not advance the semantic head"
        );
    }
}
