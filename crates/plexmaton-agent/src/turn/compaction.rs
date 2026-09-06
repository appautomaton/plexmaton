use plexmaton_core::{ConversationEvent, TranscriptItemId};

use super::Agent;
use crate::{
    CompactionAttemptFinished, CompactionPlan, CompactionRefusal, CompactionSource, ModelCall,
    ModelStepId, Reaction, RequestAttemptId, UnixMillis,
};

impl Agent {
    /// Freezes the currently selected branch without starting work or mutating history (CPL-1).
    pub fn compaction_source(&self) -> Result<CompactionSource, CompactionRefusal> {
        self.record
            .compaction_source()
            .map_err(CompactionRefusal::Journal)
    }

    /// Stages one compaction-owned request authorization before its provider effect (CPL-6).
    pub fn authorize_compaction_attempt(
        &mut self,
        plan: &CompactionPlan,
        authorized_at: UnixMillis,
    ) -> Result<(RequestAttemptId, Reaction), CompactionRefusal> {
        let mut reaction = Reaction::at(authorized_at);
        let attempt_id = self.record.next_request_attempt_id();
        self.record
            .authorize_compaction_attempt(attempt_id.clone(), plan, authorized_at, &mut reaction)
            .map_err(CompactionRefusal::Journal)?;
        Ok((attempt_id, reaction.into_output()))
    }

    /// Stages one immutable terminal audit and exposes a typed failure without advancing the head.
    pub fn finish_compaction_attempt(
        &mut self,
        finished: CompactionAttemptFinished,
    ) -> Result<Reaction, CompactionRefusal> {
        let failure = finished.outcome().failure();
        let attempt_id = finished.attempt_id().clone();
        let mut reaction = Reaction::default();
        self.record
            .finish_compaction_attempt(finished, &mut reaction)
            .map_err(CompactionRefusal::Journal)?;
        if let Some(failure) = failure {
            let item_id = TranscriptItemId::new(format!("compaction-error-{attempt_id}"))
                .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
            self.record.emit(
                &mut reaction,
                ConversationEvent::RuntimeError {
                    agent_id: self.record.agent_id().clone(),
                    item_id,
                    message: failure.to_string(),
                },
            );
        }
        Ok(reaction.into_output())
    }

    /// Stages the checkpoint that points at an already-durable successful attempt (CPL-4).
    pub fn commit_compaction_checkpoint(
        &mut self,
        plan: CompactionPlan,
        successful_attempt_id: RequestAttemptId,
    ) -> Result<Reaction, CompactionRefusal> {
        let mut reaction = Reaction::default();
        self.record
            .commit_compaction_checkpoint(plan, successful_attempt_id, &mut reaction)
            .map_err(CompactionRefusal::Journal)?;
        Ok(reaction.into_output())
    }

    /// Rebuilds the exact still-active call after an acknowledged checkpoint changes its context.
    pub fn model_call_for_active_step(
        &self,
        step_id: &ModelStepId,
    ) -> Result<ModelCall, CompactionRefusal> {
        let Some(expected) = self.active_model_step() else {
            return Err(CompactionRefusal::NoActiveStep);
        };
        if &expected != step_id {
            return Err(CompactionRefusal::WrongStep { expected });
        }
        Ok(ModelCall {
            step_id: step_id.clone(),
            request: self.record.request(),
        })
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, ConversationEntryId, ConversationEvent, HeadName, JournalRecordId, TokenCounts,
        TokenUsage,
    };

    use super::Agent;
    use crate::test_support::{output_with_replay, reasoning_block, replay, text_block};
    use crate::{
        AdmissionOutcome, CompactionAttemptFinished, CompactionCut, CompactionFailure,
        CompactionInputMode, CompactionOutcome, CompactionPlan, CompactionRefusal,
        ContextAtomValue, ContextEpoch, DispatchedRequestTiming, Effect, ElapsedMillis, Input,
        JournalError, JournalRecord, ModelEvent, ModelOutputPosition, RequestAttemptId,
        RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome,
        RequestEnvironment, RequestEnvironmentFingerprint, RequestNotDispatchedOutcome, StopReason,
        ToolCall, ToolDefinitionRevision, ToolExecutionResult, ToolOutcome, UnixMillis,
    };

    fn agent() -> Agent {
        let mut agent = Agent::new(AgentId::new("compaction-agent").expect("agent id"));
        let _announcement = agent.announce("Compaction agent");
        agent
    }

    fn environment() -> RequestEnvironment {
        RequestEnvironment::new(
            crate::test_support::replay_compatibility(),
            RequestEnvironmentFingerprint::new([7; 32]),
        )
    }

    fn complete_turn(agent: &mut Agent, question: &str, answer: &str) {
        let _opened = agent.handle(Input::Submitted {
            text: question.to_owned(),
        });
        let step_id = agent.active_model_step().expect("active step");
        let _text = agent.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: answer.to_owned(),
            },
        });
        let _finished = agent.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        });
    }

    fn open_compactable_turn() -> (Agent, crate::ModelStepId, CompactionPlan) {
        let mut agent = agent();
        complete_turn(&mut agent, "first question", "first answer");
        complete_turn(&mut agent, "second question", "second answer");
        let opened = agent.handle(Input::Submitted {
            text: "current question".to_owned(),
        });
        let step_id = agent.active_model_step().expect("current step");
        assert!(matches!(
            opened.effects.as_slice(),
            [Effect::CallModel(call)] if call.step_id == step_id
        ));
        let source = agent.compaction_source().expect("compaction source");
        let atoms = agent.record();
        let cut = CompactionCut::new(
            atoms[0].source_entries()[0].clone(),
            atoms[3].source_entries()[0].clone(),
            Some(atoms[4].source_entries()[0].clone()),
            None,
        );
        let plan = CompactionPlan::new(
            crate::CompactionId::new("compact-current").expect("compaction id"),
            source,
            cut,
            environment(),
            1024,
        )
        .expect("compaction plan");
        (agent, step_id, plan)
    }

    fn successful_terminal(attempt_id: RequestAttemptId) -> RequestAttemptTerminal {
        RequestAttemptTerminal::new(
            attempt_id,
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(20),
                    Some(ElapsedMillis::new(1)),
                    Some(ElapsedMillis::new(2)),
                    ElapsedMillis::new(3),
                )
                .expect("request timing"),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::EndOfTurn,
                },
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
            },
        )
        .expect("request terminal")
    }

    fn measured_terminal(attempt_id: RequestAttemptId, input: u64) -> RequestAttemptTerminal {
        RequestAttemptTerminal::new(
            attempt_id,
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(20),
                    Some(ElapsedMillis::new(1)),
                    Some(ElapsedMillis::new(2)),
                    ElapsedMillis::new(3),
                )
                .expect("request timing"),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::EndOfTurn,
                },
                usage: TokenUsage::Complete(TokenCounts {
                    input,
                    cached_input: Some(0),
                    cache_write_input: Some(0),
                    output: 1,
                    reasoning_output: Some(0),
                    total: input + 1,
                }),
                cost: RequestCost::Unavailable,
            },
        )
        .expect("measured terminal")
    }

    fn complete_attempt(attempt_id: RequestAttemptId, summary: &str) -> CompactionAttemptFinished {
        let reasoning_id = format!("{attempt_id}-reasoning");
        let text_id = format!("{attempt_id}-text");
        CompactionAttemptFinished::new(
            successful_terminal(attempt_id),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Complete {
                output: output_with_replay(
                    vec![
                        reasoning_block(&reasoning_id, "private reasoning"),
                        text_block(&text_id, summary),
                    ],
                    [(0, replay("opaque-summary-state"))],
                ),
            },
        )
        .expect("complete compaction attempt")
    }

    fn assert_compaction_reactions(
        authorization: &crate::Reaction,
        terminal: &crate::Reaction,
        plan: &CompactionPlan,
        attempt_id: &RequestAttemptId,
    ) {
        assert!(matches!(
            authorization.records.as_slice(),
            [JournalRecord::RequestAttemptAuthorized { fact, .. }]
                if matches!(fact.owner(), crate::RequestAttemptOwner::Compaction { compaction_id }
                    if compaction_id == plan.id())
        ));
        assert!(matches!(
            terminal.records.as_slice(),
            [JournalRecord::CompactionAttemptFinished { fact, .. }]
                if fact.attempt_id() == attempt_id
        ));
        assert!(terminal.events.is_empty());
    }

    fn assert_checkpoint_context(
        projection: &crate::JournalProjection,
        checkpoint_id: ConversationEntryId,
        events_before: &[plexmaton_core::ConversationEventEnvelope],
    ) {
        assert_eq!(projection.events(), events_before);
        assert_eq!(
            projection.context_epoch(),
            &ContextEpoch::Checkpoint(checkpoint_id)
        );
        assert_eq!(projection.base_atom_count(), 2);
        assert!(matches!(
            projection.request().atoms.as_slice(),
            [summary, current]
                if summary.value() == &ContextAtomValue::CompactionSummary {
                    text: "condensed history".to_owned()
                }
                && current.value() == &ContextAtomValue::User {
                    text: "current question".to_owned()
                }
        ));
    }

    /// CPL-1/CPL-3/CPL-4/CPL-5/CPL-6: one acknowledged checkpoint replaces only model context,
    /// retains its full audit output once, and refreshes the same active agent step.
    #[test]
    fn checkpoint_preserves_history_and_refreshes_the_active_step() {
        let (mut agent, step_id, plan) = open_compactable_turn();
        let head = agent.selected_head().clone();
        let (old_epoch_attempt, _) = agent
            .authorize_request_attempt(step_id.clone(), environment(), UnixMillis::new(5))
            .expect("authorize old-epoch request");
        let _old_terminal = agent
            .finish_request_attempt(&measured_terminal(old_epoch_attempt, 999))
            .expect("finish old-epoch request");
        let events_before = agent
            .journal()
            .project(&head)
            .expect("projection before checkpoint")
            .events()
            .to_vec();
        let target_before = agent.journal().head_target(&head).expect("target").cloned();
        let revision_before = agent.journal().head_revision(&head).expect("revision");

        let (attempt_id, authorization) = agent
            .authorize_compaction_attempt(&plan, UnixMillis::new(10))
            .expect("authorize compaction");
        let finished = complete_attempt(attempt_id.clone(), "condensed history");
        let expected_output = finished
            .outcome()
            .output()
            .expect("complete output")
            .clone();
        let terminal = agent
            .finish_compaction_attempt(finished)
            .expect("finish compaction");
        assert_compaction_reactions(&authorization, &terminal, &plan, &attempt_id);
        assert_eq!(
            agent.journal().head_target(&head).expect("target"),
            target_before.as_ref()
        );
        assert_eq!(agent.journal().head_revision(&head), Ok(revision_before));

        let committed = agent
            .commit_compaction_checkpoint(plan, attempt_id.clone())
            .expect("commit checkpoint");
        let [JournalRecord::AppendEntry { entry, .. }] = committed.records.as_slice() else {
            panic!("checkpoint is one semantic append")
        };
        let checkpoint_id = entry.id.clone();
        let projection = agent
            .journal()
            .project(&head)
            .expect("checkpoint projection");
        assert_checkpoint_context(&projection, checkpoint_id, &events_before);
        assert_eq!(
            agent
                .journal()
                .compaction_attempt(&attempt_id)
                .and_then(|finished| finished.outcome().output()),
            Some(&expected_output)
        );
        assert_eq!(
            expected_output
                .replay()
                .expect("audit replay")
                .attachments()[0]
                .payload(),
            "opaque-summary-state"
        );
        let unanchored = agent
            .journal()
            .budget_basis(&head, &environment())
            .expect("checkpoint budget basis");
        assert_eq!(unanchored.context_epoch, *projection.context_epoch());
        assert_eq!(unanchored.base_atom_count, 2);
        assert!(
            unanchored.anchor.is_none(),
            "original-epoch usage is excluded"
        );

        let (new_epoch_attempt, _) = agent
            .authorize_request_attempt(step_id.clone(), environment(), UnixMillis::new(30))
            .expect("authorize new-epoch request");
        let _new_terminal = agent
            .finish_request_attempt(&measured_terminal(new_epoch_attempt.clone(), 123))
            .expect("finish new-epoch request");
        let anchored = agent
            .journal()
            .budget_basis(&head, &environment())
            .expect("anchored checkpoint basis")
            .anchor
            .expect("new epoch usage anchor");
        assert_eq!(anchored.attempt_id(), &new_epoch_attempt);
        assert_eq!(anchored.context_epoch(), projection.context_epoch());
        assert_eq!(anchored.atom_count(), 2);
        assert_eq!(anchored.input_tokens(), 123);
        let refreshed = agent
            .model_call_for_active_step(&step_id)
            .expect("refresh active call");
        assert_eq!(refreshed.step_id, step_id);
        assert_eq!(refreshed.request, *projection.request());
    }

    /// CPL-3/CPL-5: repeated checkpoints and historical forks select only the nearest checkpoint
    /// on their own ancestry while the original head remains unchanged.
    #[test]
    fn repeated_checkpoints_and_historical_forks_keep_their_own_epochs() {
        let (mut agent, first_step, first_plan) = open_compactable_turn();
        let original_target = first_plan.cut().last_covered().clone();
        let (first_attempt, _) = agent
            .authorize_compaction_attempt(&first_plan, UnixMillis::new(10))
            .expect("authorize first compaction");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(
                first_attempt.clone(),
                "first checkpoint summary",
            ))
            .expect("finish first compaction");
        let first_checkpoint = agent
            .commit_compaction_checkpoint(first_plan, first_attempt)
            .expect("commit first checkpoint");
        let [JournalRecord::AppendEntry { entry, .. }] = first_checkpoint.records.as_slice() else {
            panic!("first checkpoint record")
        };
        let first_epoch = ContextEpoch::Checkpoint(entry.id.clone());

        let _text = agent.handle(Input::Streamed {
            step_id: first_step.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "current answer".to_owned(),
            },
        });
        let _finished = agent.handle(Input::Streamed {
            step_id: first_step,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        });
        let between_target = agent
            .journal()
            .head_target(agent.selected_head())
            .expect("between target")
            .cloned()
            .expect("between entry");
        complete_turn(&mut agent, "fourth question", "fourth answer");
        let _opened = agent.handle(Input::Submitted {
            text: "fifth question".to_owned(),
        });

        let second_source = agent.compaction_source().expect("second source");
        assert_eq!(second_source.epoch(), &first_epoch);
        let atoms = agent.record();
        let second_plan = CompactionPlan::new(
            crate::CompactionId::new("compact-again").expect("compaction id"),
            second_source,
            CompactionCut::new(
                atoms[0].source_entries()[0].clone(),
                atoms[3].source_entries()[0].clone(),
                Some(atoms[4].source_entries()[0].clone()),
                None,
            ),
            environment(),
            1024,
        )
        .expect("second plan");
        let (second_attempt, _) = agent
            .authorize_compaction_attempt(&second_plan, UnixMillis::new(20))
            .expect("authorize second compaction");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(
                second_attempt.clone(),
                "second checkpoint summary",
            ))
            .expect("finish second compaction");
        let second_checkpoint = agent
            .commit_compaction_checkpoint(second_plan, second_attempt)
            .expect("commit second checkpoint");
        let [JournalRecord::AppendEntry { entry, .. }] = second_checkpoint.records.as_slice()
        else {
            panic!("second checkpoint record")
        };
        let second_epoch = ContextEpoch::Checkpoint(entry.id.clone());
        let main_target = agent
            .journal()
            .head_target(agent.selected_head())
            .expect("main target")
            .cloned();

        let mut journal = agent.journal().clone();
        for (name, target) in [
            ("before-checkpoints", original_target),
            ("between-checkpoints", between_target),
        ] {
            let sequence = journal.next_sequence();
            journal
                .apply(JournalRecord::CreateHead {
                    sequence,
                    record_id: JournalRecordId::new(format!("create-{name}")).expect("record id"),
                    head: HeadName::new(name).expect("head id"),
                    at: Some(target),
                })
                .expect("create historical head");
        }
        assert_eq!(
            journal
                .head_target(agent.selected_head())
                .expect("main target"),
            main_target.as_ref()
        );
        let original = journal
            .project(&HeadName::new("before-checkpoints").expect("head"))
            .expect("original projection");
        let between = journal
            .project(&HeadName::new("between-checkpoints").expect("head"))
            .expect("between projection");
        let current = journal
            .project(agent.selected_head())
            .expect("current projection");
        assert_eq!(original.context_epoch(), &ContextEpoch::Original);
        assert_eq!(between.context_epoch(), &first_epoch);
        assert_eq!(current.context_epoch(), &second_epoch);
        assert_eq!(original.request().atoms.len(), 4);
        assert_eq!(between.request().atoms.len(), 3);
        assert_eq!(current.request().atoms.len(), 3);
        assert!(matches!(
            between.request().atoms[0].value(),
            ContextAtomValue::CompactionSummary { text } if text == "first checkpoint summary"
        ));
        assert!(matches!(
            current.request().atoms[0].value(),
            ContextAtomValue::CompactionSummary { text } if text == "second checkpoint summary"
        ));
    }

    fn huge_tool_context() -> (Agent, Vec<crate::ContextAtom>) {
        let mut agent = agent();
        let _opened = agent.handle(Input::Submitted {
            text: "latest user".to_owned(),
        });
        let first_step = agent.active_model_step().expect("first step");
        let call = ToolCall {
            call_id: plexmaton_core::ToolCallId::new("huge-call").expect("call id"),
            name: "large_result".to_owned(),
            arguments: "{}".to_owned(),
        };
        let _called = agent.handle(Input::Streamed {
            step_id: first_step.clone(),
            event: ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: call.clone(),
            },
        });
        let stopped = agent.handle(Input::Streamed {
            step_id: first_step,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        });
        let admission = stopped
            .effects
            .into_iter()
            .find_map(|effect| match effect {
                Effect::AdmitTool(request) => Some(request),
                Effect::CallModel(_) | Effect::RunTool { .. } | Effect::PreparePermission(_) => {
                    None
                }
            })
            .expect("tool admission request");
        let admitted = admission
            .admit(
                plexmaton_core::ToolDefinitionId::new("large-result-definition")
                    .expect("definition id"),
                ToolDefinitionRevision::new(1).expect("definition revision"),
                [],
                "{}".to_owned(),
                "large result".to_owned(),
                None,
            )
            .expect("admitted tool");
        assert!(matches!(admitted, AdmissionOutcome::Admitted(_)));
        let running = agent.handle(Input::ToolAdmissionResolved(admitted));
        assert!(matches!(
            running.effects.as_slice(),
            [Effect::RunTool { .. }]
        ));
        let _result = agent.handle(Input::ToolFinished {
            call_id: call.call_id,
            result: ToolExecutionResult::new(
                ToolOutcome::Succeeded {
                    output: "x".repeat(32 * 1024),
                },
                None,
            ),
        });
        let atoms = agent.record();
        assert_eq!(atoms.len(), 2);
        assert!(matches!(atoms[1].value(), ContextAtomValue::ToolBatch(_)));
        assert!(atoms[1].source_entries().len() > 1);
        (agent, atoms)
    }

    /// CPL-3/CPL-5: a huge indivisible batch can be replaced while pinning its latest user even
    /// when atom count stays equal, and a later equal-count checkpoint replaces that epoch again.
    #[test]
    fn equal_atom_count_checkpoints_replace_huge_batches_and_prior_summaries() {
        let (mut agent, atoms) = huge_tool_context();

        let first_plan = CompactionPlan::new(
            crate::CompactionId::new("compact-huge-batch").expect("compaction id"),
            agent.compaction_source().expect("source"),
            CompactionCut::new(
                atoms[0].source_entries()[0].clone(),
                atoms[1].source_entries()[0].clone(),
                None,
                Some(atoms[0].source_entries()[0].clone()),
            ),
            environment(),
            1024,
        )
        .expect("first equal-count plan");
        let (first_attempt, _) = agent
            .authorize_compaction_attempt(&first_plan, UnixMillis::new(10))
            .expect("authorize first equal-count attempt");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(
                first_attempt.clone(),
                "huge batch summary",
            ))
            .expect("finish first equal-count attempt");
        let split_batch = CompactionPlan::new(
            first_plan.id().clone(),
            first_plan.source().clone(),
            CompactionCut::new(
                atoms[0].source_entries()[0].clone(),
                atoms[1].source_entries()[1].clone(),
                None,
                Some(atoms[0].source_entries()[0].clone()),
            ),
            first_plan.environment().clone(),
            first_plan.max_summary_bytes(),
        )
        .expect("split-batch descriptor");
        let before_split = agent.journal().clone();
        assert_eq!(
            agent.commit_compaction_checkpoint(split_batch, first_attempt.clone()),
            Err(CompactionRefusal::Journal(
                JournalError::InvalidCompactionCut
            ))
        );
        assert_eq!(agent.journal(), &before_split);
        let first_checkpoint = agent
            .commit_compaction_checkpoint(first_plan, first_attempt)
            .expect("commit first equal-count checkpoint");
        let [JournalRecord::AppendEntry { entry, .. }] = first_checkpoint.records.as_slice() else {
            panic!("first checkpoint append")
        };
        let first_epoch = ContextEpoch::Checkpoint(entry.id.clone());
        let first_projection = agent
            .journal()
            .project(agent.selected_head())
            .expect("first equal-count projection");
        assert_eq!(first_projection.request().atoms.len(), 2);
        assert_eq!(first_projection.context_epoch(), &first_epoch);

        let atoms = first_projection.request().atoms.clone();
        let second_plan = CompactionPlan::new(
            crate::CompactionId::new("compact-prior-summary").expect("compaction id"),
            agent.compaction_source().expect("second source"),
            CompactionCut::new(
                atoms[0].source_entries()[0].clone(),
                atoms[1].source_entries()[0].clone(),
                None,
                Some(atoms[1].source_entries()[0].clone()),
            ),
            environment(),
            1024,
        )
        .expect("second equal-count plan");
        let (second_attempt, _) = agent
            .authorize_compaction_attempt(&second_plan, UnixMillis::new(20))
            .expect("authorize second equal-count attempt");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(
                second_attempt.clone(),
                "new epoch summary",
            ))
            .expect("finish second equal-count attempt");
        let _checkpoint = agent
            .commit_compaction_checkpoint(second_plan, second_attempt)
            .expect("commit second equal-count checkpoint");
        let projection = agent
            .journal()
            .project(agent.selected_head())
            .expect("second equal-count projection");
        assert_eq!(projection.request().atoms.len(), 2);
        assert!(matches!(
            projection.request().atoms.as_slice(),
            [summary, user]
                if summary.value() == &ContextAtomValue::CompactionSummary {
                    text: "new epoch summary".to_owned()
                }
                && user.value() == &ContextAtomValue::User {
                    text: "latest user".to_owned()
                }
        ));
    }

    /// CPL-6/CPL-8: a typed failed attempt is visible, advances no head, and cannot publish.
    #[test]
    fn failed_attempt_keeps_the_frozen_source_usable() {
        let (mut agent, _, plan) = open_compactable_turn();
        let before = agent.journal().clone();
        let (attempt_id, _) = agent
            .authorize_compaction_attempt(&plan, UnixMillis::new(10))
            .expect("authorize compaction");
        let after_authorization = agent.journal().clone();
        let failed = CompactionAttemptFinished::new(
            RequestAttemptTerminal::new(
                attempt_id.clone(),
                RequestAttemptTerminalState::NotDispatched {
                    outcome: RequestNotDispatchedOutcome::PreparationFailed,
                },
            )
            .expect("terminal"),
            CompactionInputMode::Verbatim,
            CompactionOutcome::Failed {
                kind: CompactionFailure::Unavailable,
                output: None,
            },
        )
        .expect("failed compaction attempt");
        let reaction = agent
            .finish_compaction_attempt(failed)
            .expect("finish failure");
        assert!(reaction.events.iter().any(|event| matches!(
            &event.event,
            ConversationEvent::RuntimeError { message, .. } if message == "compaction is unavailable"
        )));
        assert_eq!(
            agent.journal().head_target(agent.selected_head()),
            before.head_target(agent.selected_head())
        );
        assert_eq!(
            agent.journal().head_revision(agent.selected_head()),
            before.head_revision(agent.selected_head())
        );
        let terminal_state = agent.journal().clone();
        let expected_attempt = attempt_id.clone();
        assert_eq!(
            agent.commit_compaction_checkpoint(plan, attempt_id),
            Err(CompactionRefusal::Journal(
                JournalError::CompactionAttemptFailed(expected_attempt)
            ))
        );
        assert_eq!(agent.journal(), &terminal_state);
        assert_ne!(agent.journal(), &after_authorization);
    }

    /// CPL-3/CPL-4: owner, environment, whole-atom cut, summary bound and useful reduction are
    /// independently checked before a checkpoint append can mutate the source head.
    #[test]
    fn checkpoint_publication_rechecks_all_frozen_provenance() {
        let (mut agent, _, plan) = open_compactable_turn();
        let atoms = agent.record();
        let (attempt_id, _) = agent
            .authorize_compaction_attempt(&plan, UnixMillis::new(10))
            .expect("authorize compaction");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(attempt_id.clone(), "condensed history"))
            .expect("finish compaction");
        let cases = [
            (
                CompactionPlan::new(
                    crate::CompactionId::new("wrong-owner").expect("compaction id"),
                    plan.source().clone(),
                    plan.cut().clone(),
                    plan.environment().clone(),
                    plan.max_summary_bytes(),
                )
                .expect("owner plan"),
                JournalError::CompactionOwnerMismatch(attempt_id.clone()),
            ),
            (
                CompactionPlan::new(
                    plan.id().clone(),
                    plan.source().clone(),
                    plan.cut().clone(),
                    RequestEnvironment::new(
                        crate::test_support::replay_compatibility(),
                        RequestEnvironmentFingerprint::new([9; 32]),
                    ),
                    plan.max_summary_bytes(),
                )
                .expect("environment plan"),
                JournalError::CompactionEnvironmentMismatch(attempt_id.clone()),
            ),
            (
                CompactionPlan::new(
                    plan.id().clone(),
                    plan.source().clone(),
                    CompactionCut::new(
                        atoms[4].source_entries()[0].clone(),
                        atoms[3].source_entries()[0].clone(),
                        None,
                        None,
                    ),
                    plan.environment().clone(),
                    plan.max_summary_bytes(),
                )
                .expect("invalid cut plan"),
                JournalError::InvalidCompactionCut,
            ),
            (
                CompactionPlan::new(
                    plan.id().clone(),
                    plan.source().clone(),
                    plan.cut().clone(),
                    plan.environment().clone(),
                    5,
                )
                .expect("small summary plan"),
                JournalError::CompactionSummaryExceedsPlan,
            ),
        ];

        for (candidate, expected) in cases {
            let before = agent.journal().clone();
            assert_eq!(
                agent.commit_compaction_checkpoint(candidate, attempt_id.clone()),
                Err(CompactionRefusal::Journal(expected))
            );
            assert_eq!(agent.journal(), &before);
        }
    }

    /// CPL-1/CPL-8: changing the source after a successful attempt makes publication stale.
    #[test]
    fn stale_checkpoint_publication_mutates_nothing() {
        let (mut agent, step_id, plan) = open_compactable_turn();
        let (attempt_id, _) = agent
            .authorize_compaction_attempt(&plan, UnixMillis::new(10))
            .expect("authorize compaction");
        let _finished = agent
            .finish_compaction_attempt(complete_attempt(attempt_id.clone(), "condensed history"))
            .expect("finish compaction");
        let _text = agent.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "late answer".to_owned(),
            },
        });
        let _stopped = agent.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        });
        let before = agent.journal().clone();
        let selected = agent.selected_head().clone();
        let expected = plan.source().head_revision();
        let actual = before.head_revision(&selected).expect("revision");
        assert_eq!(
            agent.commit_compaction_checkpoint(plan, attempt_id),
            Err(CompactionRefusal::Journal(JournalError::StaleHead {
                head: selected,
                expected,
                actual,
            }))
        );
        assert_eq!(agent.journal(), &before);
    }
}
