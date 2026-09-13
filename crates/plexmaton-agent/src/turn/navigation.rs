use plexmaton_core::{
    ConversationId, JournalRecordId, TreeNavigation, TreeNavigationTarget, TreeOrigin, TreeRevision,
};

use super::Agent;
use crate::interface::Reaction;
use crate::journal::{JournalRecord, JournalSequence};
use crate::{ReturnedDraft, TreeNavigationRefusal, TreeNavigationResult};

impl Agent {
    /// Captures the selected source and complete journal high-water token for a tree snapshot.
    #[must_use]
    pub fn tree_origin(&self) -> TreeOrigin {
        TreeOrigin {
            conversation_id: self.journal().conversation_id().clone(),
            agent_id: self.record.agent_id().clone(),
            selected_head: self.selected_head().clone(),
            revision: TreeRevision::new(self.journal().next_sequence().get()),
        }
    }

    /// Applies one addressed tree action without starting a turn or performing an effect (TRE-4, TRE-7).
    ///
    /// A successful reaction contains one durable head mutation (except an already-selected
    /// no-op), a projection reset built from destination ancestry, and an optional historical
    /// user draft. The runtime owns acknowledgement before it publishes that replacement.
    pub fn navigate(
        &mut self,
        navigation: &TreeNavigation,
    ) -> Result<Reaction, TreeNavigationRefusal> {
        if self.is_running() {
            return Err(TreeNavigationRefusal::Busy);
        }
        self.validate_tree_origin(&navigation.origin)?;

        let source = self.selected_head().clone();
        if let Some(turn_id) = self.journal().open_turn_on_path(&source) {
            return Err(TreeNavigationRefusal::SourceTurnOpen(turn_id));
        }

        let mut reaction = Reaction::default();
        match &navigation.target {
            TreeNavigationTarget::Rewind(entry_id) => {
                let resolved = self
                    .journal()
                    .resolve_rewind_target(self.record.agent_id(), entry_id)?;
                let destination = self.journal().fresh_rewind_head_name()?;
                let sequence = self.journal().next_sequence();
                let record = JournalRecord::ForkAndSelectHead {
                    sequence,
                    record_id: navigation_record_id(self.journal().conversation_id(), sequence),
                    source: source.clone(),
                    expected_source_revision: self
                        .journal()
                        .head_revision(&source)
                        .map_err(TreeNavigationRefusal::Journal)?,
                    destination: destination.clone(),
                    at: resolved.boundary.clone(),
                };
                self.journal()
                    .validate_record(&record)
                    .map_err(TreeNavigationRefusal::Journal)?;
                let projection = self
                    .journal()
                    .project_at(resolved.boundary.as_ref())
                    .map_err(TreeNavigationRefusal::Projection)?;
                let returned_draft =
                    self.journal()
                        .materialize_rewind_draft(&resolved)
                        .map(|draft| ReturnedDraft {
                            text: draft.text,
                            skill_name: draft.skill_name,
                        });
                self.record
                    .apply_tree_navigation(record, projection, &mut reaction)
                    .map_err(TreeNavigationRefusal::Journal)?;
                reaction.tree_navigation = Some(TreeNavigationResult {
                    selected_head: destination,
                    mutation_sequence: Some(sequence),
                    returned_draft,
                });
            }
            TreeNavigationTarget::SelectHead(destination) => {
                let destination_revision =
                    self.journal()
                        .head_revision(destination)
                        .map_err(|error| match error {
                            crate::JournalError::MissingHead(missing) => {
                                TreeNavigationRefusal::MissingHead(missing)
                            }
                            other => TreeNavigationRefusal::Journal(other),
                        })?;
                self.journal()
                    .validate_navigation_head_target(destination)?;
                if destination == &source {
                    reaction.tree_navigation = Some(TreeNavigationResult {
                        selected_head: source,
                        mutation_sequence: None,
                        returned_draft: None,
                    });
                    return Ok(reaction.into_output());
                }

                let sequence = self.journal().next_sequence();
                let record = JournalRecord::SelectHead {
                    sequence,
                    record_id: navigation_record_id(self.journal().conversation_id(), sequence),
                    expected_selected: source,
                    destination: destination.clone(),
                    expected_destination_revision: destination_revision,
                };
                self.journal()
                    .validate_record(&record)
                    .map_err(TreeNavigationRefusal::Journal)?;
                let projection = self
                    .journal()
                    .project(destination)
                    .map_err(TreeNavigationRefusal::Projection)?;
                self.record
                    .apply_tree_navigation(record, projection, &mut reaction)
                    .map_err(TreeNavigationRefusal::Journal)?;
                reaction.tree_navigation = Some(TreeNavigationResult {
                    selected_head: destination.clone(),
                    mutation_sequence: Some(sequence),
                    returned_draft: None,
                });
            }
        }
        Ok(reaction.into_output())
    }

    fn validate_tree_origin(&self, origin: &TreeOrigin) -> Result<(), TreeNavigationRefusal> {
        let conversation = self.journal().conversation_id();
        if &origin.conversation_id != conversation {
            return Err(TreeNavigationRefusal::ForeignConversation {
                expected: conversation.clone(),
                actual: origin.conversation_id.clone(),
            });
        }
        let agent = self.record.agent_id();
        if &origin.agent_id != agent {
            return Err(TreeNavigationRefusal::ForeignAgent {
                expected: agent.clone(),
                actual: origin.agent_id.clone(),
            });
        }
        let actual_revision = TreeRevision::new(self.journal().next_sequence().get());
        if origin.revision != actual_revision {
            return Err(TreeNavigationRefusal::StaleOrigin {
                expected: origin.revision,
                actual: actual_revision,
            });
        }
        let selected = self.selected_head();
        if &origin.selected_head != selected {
            return Err(TreeNavigationRefusal::SourceHeadChanged {
                expected: origin.selected_head.clone(),
                actual: selected.clone(),
            });
        }
        Ok(())
    }
}

fn navigation_record_id(
    conversation_id: &ConversationId,
    sequence: JournalSequence,
) -> JournalRecordId {
    JournalRecordId::new(format!("{conversation_id}-record-{}", sequence.get()))
        .unwrap_or_else(|error| unreachable!("formatted journal record identity is valid: {error}"))
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, HeadName, TreeNavigationTarget};

    use super::*;
    use crate::journal::JournalEntryPayload;
    use crate::{
        Agent, ApprovalPolicy, ConversationJournal, ConversationMetadata, Input, JournalRecord,
        SkillActivation, SkillSource, TurnBudget, UnixMillis,
    };

    fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
        build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
    }

    fn head(value: &str) -> HeadName {
        id(value, HeadName::new)
    }

    fn agent() -> Agent {
        let mut agent = Agent::new(id("agent-navigation", AgentId::new));
        let _ = agent.announce("navigation test agent");
        agent
    }

    fn activation(name: &str) -> SkillActivation {
        SkillActivation::new(
            name.to_owned(),
            SkillSource::ProjectShared,
            format!("/workspace/.agents/skills/{name}/SKILL.md"),
            "c".repeat(64),
            "historical skill body\r\n".to_owned(),
        )
        .unwrap_or_else(|error| panic!("skill fixture: {error}"))
    }

    fn user_entry(agent: &Agent) -> plexmaton_core::ConversationEntryId {
        agent
            .journal()
            .path(agent.selected_head())
            .unwrap_or_else(|error| panic!("selected path: {error:?}"))
            .into_iter()
            .find_map(|entry| {
                matches!(entry.payload, JournalEntryPayload::TurnStarted { .. })
                    .then(|| entry.id.clone())
            })
            .unwrap_or_else(|| panic!("turn started entry exists"))
    }

    fn finish_simple_turn(agent: &mut Agent, text: &str, skill: Option<SkillActivation>) {
        let input = match skill {
            Some(skill) => Input::SkillSubmitted {
                text: text.to_owned(),
                skill,
            },
            None => Input::Submitted {
                text: text.to_owned(),
            },
        };
        let started = agent.handle(input);
        assert_eq!(started.effects.len(), 1);
        let step_id = agent
            .active_model_step()
            .expect("submitted turn opened a step");
        let _ = agent.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: crate::ModelEvent::TextDelta {
                position: crate::ModelOutputPosition::new(0, 0),
                delta: "answer".to_owned(),
            },
        });
        let finished = agent.handle(Input::Streamed {
            step_id,
            event: crate::ModelEvent::Stopped(crate::StopReason::EndOfTurn),
        });
        assert!(!finished.records.is_empty());
    }

    fn tree_navigation(agent: &Agent, target: TreeNavigationTarget) -> TreeNavigation {
        TreeNavigation {
            origin: agent.tree_origin(),
            target,
        }
    }

    fn selected_target(agent: &Agent) -> Option<plexmaton_core::ConversationEntryId> {
        agent
            .journal()
            .head_target(agent.selected_head())
            .unwrap_or_else(|error| panic!("selected head target: {error:?}"))
            .cloned()
    }

    /// TRE-4/TRE-5: user rewind commits one fork/select record, preserves the source, and returns
    /// exact text plus the historical numeric explicit skill binding without starting work.
    #[test]
    fn tre_4_5_user_rewind_returns_exact_text_and_numeric_skill_without_effects() {
        let mut agent = agent();
        let text = "  $100 inspect\r\nexactly  ";
        finish_simple_turn(&mut agent, text, Some(activation("100")));
        let source_head = agent.selected_head().clone();
        let source_path: Vec<_> = agent
            .journal()
            .path(&source_head)
            .unwrap_or_else(|error| panic!("source path: {error:?}"))
            .into_iter()
            .cloned()
            .collect();
        let target = user_entry(&agent);
        let navigation = tree_navigation(&agent, TreeNavigationTarget::Rewind(target));

        let reaction = agent.navigate(&navigation).expect("valid user rewind");
        let [
            JournalRecord::ForkAndSelectHead {
                source,
                destination,
                at,
                sequence,
                ..
            },
        ] = reaction.records.as_slice()
        else {
            panic!("rewind is one fork-and-select mutation")
        };
        assert_eq!(source, &source_head);
        assert_eq!(at.as_ref(), source_path.first().map(|entry| &entry.id));
        assert_eq!(
            agent
                .journal()
                .path(&source_head)
                .expect("source path after rewind"),
            source_path.iter().collect::<Vec<_>>()
        );
        assert_eq!(agent.selected_head(), destination);
        assert!(reaction.effects.is_empty());
        assert!(reaction.events.is_empty());
        assert!(reaction.undelivered.is_empty());
        assert!(!agent.is_running());
        assert_eq!(reaction.projection_reset.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            reaction.tree_navigation,
            Some(TreeNavigationResult {
                selected_head: destination.clone(),
                mutation_sequence: Some(*sequence),
                returned_draft: Some(ReturnedDraft {
                    text: text.to_owned(),
                    skill_name: Some("100".to_owned()),
                }),
            })
        );
        assert_eq!(
            reaction.projection_reset.as_deref(),
            Some(
                agent
                    .journal()
                    .project(destination)
                    .expect("destination projection")
                    .events()
            )
        );
    }

    /// TRE-7: an assistant target resolves after its completed turn, retaining its complete tool
    /// batch and not replaying any of its recorded effects.
    #[test]
    fn tre_7_assistant_rewind_keeps_the_complete_tool_batch() {
        let mut journal = ConversationJournal::new(id("conversation-tools", ConversationId::new));
        let agent_id = id("agent-tools", AgentId::new);
        append(
            &mut journal,
            JournalEntryPayload::AgentCreated {
                agent_id: agent_id.clone(),
                label: "Tools".to_owned(),
                status: plexmaton_core::AgentStatus::Idle,
            },
        );
        let turn_id = id("turn-tools", plexmaton_core::TurnId::new);
        let user_id = id("user-entry", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut journal,
            user_id,
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: id("user-item", plexmaton_core::TranscriptItemId::new),
                turn_id: turn_id.clone(),
                text: "inspect".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
        );
        let call_id = id("call-one", plexmaton_core::ToolCallId::new);
        let assistant_target = id("assistant-tools", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut journal,
            assistant_target.clone(),
            JournalEntryPayload::AssistantOutput {
                agent_id: agent_id.clone(),
                step_id: crate::ModelStepId::new(turn_id.clone(), 1),
                output: crate::test_support::output(vec![crate::test_support::call_block(
                    "call-block",
                    crate::ToolCall {
                        call_id: call_id.clone(),
                        name: "read_file".to_owned(),
                        arguments: r#"{"path":"notes.txt"}"#.to_owned(),
                    },
                )]),
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::ToolCallRequested {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                presentation: plexmaton_core::ToolPresentation::default(),
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::ToolCallChanged {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                item_revision: 1,
                status: plexmaton_core::ToolCallStatus::Running,
                presentation: plexmaton_core::ToolPresentation::default(),
                outcome: None,
            },
        );
        append(
            &mut journal,
            JournalEntryPayload::ToolCallChanged {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                item_revision: 2,
                status: plexmaton_core::ToolCallStatus::Succeeded,
                presentation: plexmaton_core::ToolPresentation::default(),
                outcome: Some(crate::ToolOutcome::Succeeded {
                    output: "file contents".to_owned(),
                }),
            },
        );
        let final_assistant = id("assistant-final", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut journal,
            final_assistant.clone(),
            JournalEntryPayload::AssistantOutput {
                agent_id: agent_id.clone(),
                step_id: crate::ModelStepId::new(turn_id.clone(), 2),
                output: crate::test_support::output(vec![crate::test_support::text_block(
                    "final-item",
                    "done",
                )]),
            },
        );
        finish_journal_turn(&mut journal, &agent_id, &turn_id, &final_assistant);
        let mut agent = Agent::from_journal(
            agent_id,
            journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("journal is projectable");
        let before_interior_refusal = agent.journal().clone();
        let refusal = agent.navigate(&tree_navigation(
            &agent,
            TreeNavigationTarget::Rewind(assistant_target.clone()),
        ));
        assert_eq!(
            refusal,
            Err(TreeNavigationRefusal::InteriorAssistantTarget(
                assistant_target.clone()
            ))
        );
        assert_eq!(agent.journal(), &before_interior_refusal);
        let reaction = agent
            .navigate(&tree_navigation(
                &agent,
                TreeNavigationTarget::Rewind(final_assistant.clone()),
            ))
            .expect("final completed assistant target is eligible");
        assert_eq!(selected_target(&agent), Some(final_assistant));
        assert!(reaction.effects.is_empty());
        assert!(reaction.events.is_empty());
        assert!(reaction.undelivered.is_empty());
        assert!(
            reaction
                .tree_navigation
                .as_ref()
                .and_then(|result| result.returned_draft.as_ref())
                .is_none()
        );
        let projection = agent
            .journal()
            .project(agent.selected_head())
            .expect("rewound tool projection");
        assert!(matches!(
            projection.request().atoms.as_slice(),
            [user, batch, assistant]
                if matches!(user.value(), crate::ContextAtomValue::User { text } if text == "inspect")
                    && matches!(batch.value(), crate::ContextAtomValue::ToolBatch(tool_batch)
                        if tool_batch.results().len() == 1
                            && tool_batch.results()[0].call_id() == &call_id)
                    && matches!(assistant.value(), crate::ContextAtomValue::Assistant(output)
                        if output.blocks().iter().any(|block| matches!(block, crate::AssistantBlock::Text { text, .. } if text == "done")))
        ));
    }

    /// TRE-4: sequence fencing sees TurnFinished even though it does not advance HeadRevision.
    #[test]
    fn tre_4_turn_finish_stales_origin_without_advancing_head_revision() {
        let mut open = ConversationJournal::new(id("conversation-finish", ConversationId::new));
        let agent_id = id("agent-finish", AgentId::new);
        append(
            &mut open,
            JournalEntryPayload::AgentCreated {
                agent_id: agent_id.clone(),
                label: "Finish".to_owned(),
                status: plexmaton_core::AgentStatus::Idle,
            },
        );
        let turn_id = id("turn-finish", plexmaton_core::TurnId::new);
        append(
            &mut open,
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: id("user-finish", plexmaton_core::TranscriptItemId::new),
                turn_id: turn_id.clone(),
                text: "question".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
        );
        let assistant = id("assistant-finish", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut open,
            assistant.clone(),
            JournalEntryPayload::AssistantOutput {
                agent_id: agent_id.clone(),
                step_id: crate::ModelStepId::new(turn_id.clone(), 1),
                output: crate::test_support::output(vec![crate::test_support::text_block(
                    "finish-item",
                    "answer",
                )]),
            },
        );
        let head_revision = open.head_revision(&head("main")).expect("head revision");
        let open_agent = Agent::from_journal(
            agent_id.clone(),
            open.clone(),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("open projection is valid");
        let old_origin = open_agent.tree_origin();
        finish_journal_turn(&mut open, &agent_id, &turn_id, &assistant);
        assert_eq!(open.head_revision(&head("main")), Ok(head_revision));
        let mut finished_agent = Agent::from_journal(
            agent_id,
            open,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("finished projection is valid");
        let before = finished_agent.journal().clone();
        let result = finished_agent.navigate(&TreeNavigation {
            origin: old_origin,
            target: TreeNavigationTarget::Rewind(assistant),
        });
        assert!(matches!(
            result,
            Err(TreeNavigationRefusal::StaleOrigin { .. })
        ));
        assert_eq!(finished_agent.journal(), &before);
    }

    /// TRE-4/TRE-7: stale, foreign, missing, steering and interior addresses refuse without a
    /// head mutation; steering is never reinterpreted as a standalone rewind boundary.
    #[test]
    fn tre_4_7_invalid_targets_refuse_atomically_with_typed_reasons() {
        let mut agent = agent();
        finish_simple_turn(&mut agent, "question", None);
        let target = user_entry(&agent);
        let agent_id = id("agent-navigation", AgentId::new);
        let before = agent.journal().clone();
        let missing = id("missing-entry", plexmaton_core::ConversationEntryId::new);
        let result = agent.navigate(&tree_navigation(
            &agent,
            TreeNavigationTarget::Rewind(missing.clone()),
        ));
        assert_eq!(result, Err(TreeNavigationRefusal::MissingTarget(missing)));
        assert_eq!(agent.journal(), &before);

        let same_conversation_other_agent = Agent::for_conversation(
            id("agent-other", AgentId::new),
            ConversationMetadata::new(agent.journal().conversation_id().clone(), UnixMillis::EPOCH),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        );
        let foreign = agent.navigate(&TreeNavigation {
            origin: same_conversation_other_agent.tree_origin(),
            target: TreeNavigationTarget::Rewind(target.clone()),
        });
        assert!(matches!(
            foreign,
            Err(TreeNavigationRefusal::ForeignAgent { .. })
        ));
        assert_eq!(agent.journal(), &before);

        let other_conversation = Agent::new(id("agent-elsewhere", AgentId::new));
        let foreign_conversation = agent.navigate(&TreeNavigation {
            origin: other_conversation.tree_origin(),
            target: TreeNavigationTarget::Rewind(target.clone()),
        });
        assert!(matches!(
            foreign_conversation,
            Err(TreeNavigationRefusal::ForeignConversation { .. })
        ));
        assert_eq!(agent.journal(), &before);

        let mut forged_source = agent.tree_origin();
        forged_source.selected_head = head("other");
        let wrong_source = agent.navigate(&TreeNavigation {
            origin: forged_source,
            target: TreeNavigationTarget::Rewind(target.clone()),
        });
        assert!(matches!(
            wrong_source,
            Err(TreeNavigationRefusal::SourceHeadChanged { .. })
        ));
        assert_eq!(agent.journal(), &before);

        let mut journal = agent.journal().clone();
        let turn_id = id("steer-turn", plexmaton_core::TurnId::new);
        append(
            &mut journal,
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: id("steer-user", plexmaton_core::TranscriptItemId::new),
                turn_id: turn_id.clone(),
                text: "question with steering".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
        );
        let steering_id = id("steering-entry", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut journal,
            steering_id.clone(),
            JournalEntryPayload::SteeringAccepted {
                agent_id: agent_id.clone(),
                item_id: id("steering-item", plexmaton_core::TranscriptItemId::new),
                turn_id: turn_id.clone(),
                text: "interior steering".to_owned(),
                accepted_at: UnixMillis::EPOCH,
            },
        );
        let assistant = id("steer-assistant", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut journal,
            assistant.clone(),
            JournalEntryPayload::AssistantOutput {
                agent_id: agent_id.clone(),
                step_id: crate::ModelStepId::new(turn_id.clone(), 1),
                output: crate::test_support::output(vec![crate::test_support::text_block(
                    "steer-answer",
                    "answer",
                )]),
            },
        );
        finish_journal_turn(&mut journal, &agent_id, &turn_id, &assistant);
        let mut steering_agent = Agent::from_journal(
            agent_id.clone(),
            journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("steering journal is projectable");
        let before_steering = steering_agent.journal().clone();
        let steering_result = steering_agent.navigate(&tree_navigation(
            &steering_agent,
            TreeNavigationTarget::Rewind(steering_id.clone()),
        ));
        assert_eq!(
            steering_result,
            Err(TreeNavigationRefusal::SteeringTarget(steering_id))
        );
        assert_eq!(steering_agent.journal(), &before_steering);

        assert_eq!(agent.journal(), &before);
    }

    /// TRE-4/TRE-7: a present target belonging to a different agent refuses atomically.
    #[test]
    fn tre_4_7_foreign_agent_target_refuses_without_mutation() {
        let mut agent = agent();
        finish_simple_turn(&mut agent, "question", None);
        let agent_id = id("agent-navigation", AgentId::new);
        let before = agent.journal().clone();

        let other_agent_id = id("agent-other-target", AgentId::new);
        let mut foreign_journal = agent.journal().clone();
        append(
            &mut foreign_journal,
            JournalEntryPayload::AgentCreated {
                agent_id: other_agent_id.clone(),
                label: "Other".to_owned(),
                status: plexmaton_core::AgentStatus::Idle,
            },
        );
        let foreign_turn_id = id("foreign-turn", plexmaton_core::TurnId::new);
        let foreign_user_id = id("foreign-user", plexmaton_core::ConversationEntryId::new);
        append_with_id(
            &mut foreign_journal,
            foreign_user_id.clone(),
            JournalEntryPayload::TurnStarted {
                agent_id: other_agent_id.clone(),
                item_id: id("foreign-user-item", plexmaton_core::TranscriptItemId::new),
                turn_id: foreign_turn_id.clone(),
                text: "other agent question".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
        );
        let foreign_assistant = id(
            "foreign-assistant",
            plexmaton_core::ConversationEntryId::new,
        );
        append_with_id(
            &mut foreign_journal,
            foreign_assistant.clone(),
            JournalEntryPayload::AssistantOutput {
                agent_id: other_agent_id.clone(),
                step_id: crate::ModelStepId::new(foreign_turn_id.clone(), 1),
                output: crate::test_support::output(vec![crate::test_support::text_block(
                    "foreign-answer",
                    "answer",
                )]),
            },
        );
        finish_journal_turn(
            &mut foreign_journal,
            &other_agent_id,
            &foreign_turn_id,
            &foreign_assistant,
        );
        let mut foreign_target_agent = Agent::from_journal(
            agent_id,
            foreign_journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("foreign target journal is projectable");
        let before_foreign_target = foreign_target_agent.journal().clone();
        let foreign_target_result = foreign_target_agent.navigate(&tree_navigation(
            &foreign_target_agent,
            TreeNavigationTarget::Rewind(foreign_user_id.clone()),
        ));
        assert_eq!(
            foreign_target_result,
            Err(TreeNavigationRefusal::ForeignTargetAgent {
                expected: id("agent-navigation", AgentId::new),
                actual: other_agent_id,
            })
        );
        assert_eq!(foreign_target_agent.journal(), &before_foreign_target);
        assert_eq!(agent.journal(), &before);
    }

    /// TRE-4: a navigation is a no-op when its addressed destination is the selected head.
    #[test]
    fn tre_4_selecting_current_head_returns_no_mutation() {
        let mut agent = agent();
        let reaction = agent
            .navigate(&tree_navigation(
                &agent,
                TreeNavigationTarget::SelectHead(agent.selected_head().clone()),
            ))
            .expect("current head selection is valid");
        assert!(reaction.records.is_empty());
        assert!(reaction.projection_reset.is_none());
        assert!(reaction.effects.is_empty());
        assert_eq!(
            reaction.tree_navigation,
            Some(TreeNavigationResult {
                selected_head: head("main"),
                mutation_sequence: None,
                returned_draft: None,
            })
        );
    }

    /// TRE-7: a user target at the first turn returns the durable root and an empty request.
    #[test]
    fn tre_7_rewinding_first_user_selects_root_before_any_request_atom() {
        let mut agent = agent();
        finish_simple_turn(&mut agent, "first question", None);
        let root = agent
            .journal()
            .path(agent.selected_head())
            .expect("source path")
            .first()
            .expect("agent-created root")
            .id
            .clone();
        let reaction = agent
            .navigate(&tree_navigation(
                &agent,
                TreeNavigationTarget::Rewind(user_entry(&agent)),
            ))
            .expect("first user target is eligible");
        assert_eq!(selected_target(&agent), Some(root.clone()));
        assert_eq!(reaction.projection_reset.as_ref().map(Vec::len), Some(1));
        assert!(
            agent
                .journal()
                .project(agent.selected_head())
                .expect("root projection")
                .request()
                .atoms
                .is_empty()
        );
        assert!(matches!(
            reaction.records.as_slice(),
            [JournalRecord::ForkAndSelectHead { at: Some(boundary), .. }]
                if boundary == &root
        ));
    }

    /// TRE-3/TRE-7: selecting an existing stable head is one mutation, while an unfinished
    /// destination head is refused atomically before SelectHead can make it current.
    #[test]
    fn tre_3_7_existing_head_selection_and_unstable_destination() {
        let mut agent = agent();
        finish_simple_turn(&mut agent, "question", None);
        let agent_id = id("agent-navigation", AgentId::new);
        let main_path: Vec<_> = agent
            .journal()
            .path(&head("main"))
            .expect("main path")
            .into_iter()
            .cloned()
            .collect();
        let branch = head("experiment");
        let mut journal = agent.journal().clone();
        let sequence = journal.next_sequence();
        let branch_target = journal
            .head_target(&head("main"))
            .expect("main target")
            .cloned();
        journal
            .apply(JournalRecord::CreateHead {
                sequence,
                record_id: id(&format!("record-{}", sequence.get()), JournalRecordId::new),
                head: branch.clone(),
                at: branch_target,
            })
            .expect("create stable experiment head");
        let mut agent = Agent::from_journal(
            agent_id.clone(),
            journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("stable branch journal is projectable");
        let branch_path: Vec<_> = agent
            .journal()
            .path(&branch)
            .expect("branch path")
            .into_iter()
            .cloned()
            .collect();
        let selected = agent
            .navigate(&tree_navigation(
                &agent,
                TreeNavigationTarget::SelectHead(branch.clone()),
            ))
            .expect("existing stable head is selectable");
        assert!(matches!(
            selected.records.as_slice(),
            [JournalRecord::SelectHead { destination, .. }] if destination == &branch
        ));
        assert_eq!(selected.records.len(), 1);
        assert!(selected.effects.is_empty());
        assert_eq!(agent.selected_head(), &branch);
        assert_eq!(
            agent.journal().path(&head("main")).expect("main retained"),
            main_path.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            agent.journal().path(&branch).expect("branch retained"),
            branch_path.iter().collect::<Vec<_>>()
        );

        let returned = agent
            .navigate(&tree_navigation(
                &agent,
                TreeNavigationTarget::SelectHead(head("main")),
            ))
            .expect("source can be selected again");
        assert_eq!(returned.records.len(), 1);
        assert_eq!(agent.selected_head(), &head("main"));

        let turn_id = id("open-branch-turn", plexmaton_core::TurnId::new);
        let mut open_branch_journal = agent.journal().clone();
        append_on_head(
            &mut open_branch_journal,
            &branch,
            id("open-branch-user", plexmaton_core::ConversationEntryId::new),
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: id("open-branch-item", plexmaton_core::TranscriptItemId::new),
                turn_id: turn_id.clone(),
                text: "unfinished".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
        );
        let mut agent = Agent::from_journal(
            agent_id,
            open_branch_journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .expect("other branch may remain open");
        let before_unstable_refusal = agent.journal().clone();
        let unstable = agent.navigate(&tree_navigation(
            &agent,
            TreeNavigationTarget::SelectHead(branch.clone()),
        ));
        assert_eq!(
            unstable,
            Err(TreeNavigationRefusal::UnstableDestination {
                head: branch,
                turn_id,
            })
        );
        assert_eq!(agent.journal(), &before_unstable_refusal);
        assert_eq!(agent.selected_head(), &head("main"));
    }

    fn append(journal: &mut ConversationJournal, payload: JournalEntryPayload) {
        let sequence = journal.next_sequence();
        let main = head("main");
        let parent_id = journal
            .head_target(&main)
            .unwrap_or_else(|error| panic!("main target: {error:?}"))
            .cloned();
        append_with_id(
            journal,
            plexmaton_core::ConversationEntryId::new(format!("entry-{}", sequence.get()))
                .expect("entry id"),
            payload,
        );
        assert!(parent_id.is_some() || sequence.get() == 1);
    }

    fn append_with_id(
        journal: &mut ConversationJournal,
        entry_id: plexmaton_core::ConversationEntryId,
        payload: JournalEntryPayload,
    ) {
        let sequence = journal.next_sequence();
        let main = head("main");
        let parent_id = journal
            .head_target(&main)
            .unwrap_or_else(|error| panic!("main target: {error:?}"))
            .cloned();
        let revision = journal
            .head_revision(&main)
            .unwrap_or_else(|error| panic!("main revision: {error:?}"));
        journal
            .apply(JournalRecord::AppendEntry {
                sequence,
                record_id: id(&format!("record-{}", sequence.get()), JournalRecordId::new),
                head: main,
                expected_head_revision: revision,
                entry: Box::new(crate::ConversationEntry {
                    id: entry_id,
                    parent_id,
                    payload,
                }),
            })
            .unwrap_or_else(|error| panic!("append fixture: {error:?}"));
    }

    fn append_on_head(
        journal: &mut ConversationJournal,
        head: &HeadName,
        entry_id: plexmaton_core::ConversationEntryId,
        payload: JournalEntryPayload,
    ) {
        let sequence = journal.next_sequence();
        let parent_id = journal
            .head_target(head)
            .unwrap_or_else(|error| panic!("head target: {error:?}"))
            .cloned();
        let revision = journal
            .head_revision(head)
            .unwrap_or_else(|error| panic!("head revision: {error:?}"));
        journal
            .apply(JournalRecord::AppendEntry {
                sequence,
                record_id: id(&format!("record-{}", sequence.get()), JournalRecordId::new),
                head: head.clone(),
                expected_head_revision: revision,
                entry: Box::new(crate::ConversationEntry {
                    id: entry_id,
                    parent_id,
                    payload,
                }),
            })
            .unwrap_or_else(|error| panic!("append on selected test head: {error:?}"));
    }

    fn finish_journal_turn(
        journal: &mut ConversationJournal,
        agent_id: &AgentId,
        turn_id: &plexmaton_core::TurnId,
        boundary: &plexmaton_core::ConversationEntryId,
    ) {
        let sequence = journal.next_sequence();
        journal
            .apply(JournalRecord::TurnFinished {
                sequence,
                record_id: id(&format!("record-{}", sequence.get()), JournalRecordId::new),
                head: head("main"),
                expected_head_revision: journal
                    .head_revision(&head("main"))
                    .unwrap_or_else(|error| panic!("main revision: {error:?}")),
                fact: crate::TurnFinished {
                    agent_id: agent_id.clone(),
                    turn_id: turn_id.clone(),
                    semantic_boundary: boundary.clone(),
                    outcome: crate::TurnOutcome::Completed,
                    at: crate::TurnFinishedAt::Observed {
                        completed_at: UnixMillis::EPOCH,
                    },
                },
            })
            .unwrap_or_else(|error| panic!("finish fixture: {error:?}"));
    }
}
