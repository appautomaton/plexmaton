//! One assistant output preserves block order and forms an indivisible pending tool batch.
use super::*;

impl Projector {
    pub(super) fn assistant_output(
        &mut self,
        source: ConversationEntryId,
        agent_id: AgentId,
        step_id: ModelStepId,
        output: AssistantOutput,
    ) -> Result<(), JournalProjectionError> {
        self.finish_batch(false)?;
        self.require_agent(&agent_id)?;
        let Some(expected) = self.turns.get(step_id.turn_id()) else {
            return Err(JournalProjectionError::MissingTurn(
                step_id.turn_id().clone(),
            ));
        };
        if expected != &agent_id {
            return Err(JournalProjectionError::WrongTurnAgent(
                step_id.turn_id().clone(),
            ));
        }
        if !self.steps.insert(step_id.clone()) {
            return Err(JournalProjectionError::DuplicateModelStep(step_id));
        }

        let mut calls = Vec::new();
        for block in output.blocks() {
            self.claim_entry(block.item_id(), &agent_id)?;
            match block {
                AssistantBlock::Text { item_id, text } if !text.is_empty() => {
                    self.emit_message(
                        agent_id.clone(),
                        item_id.clone(),
                        TranscriptRole::Assistant,
                        text.clone(),
                    )?;
                }
                AssistantBlock::Reasoning { item_id, text } if !text.is_empty() => {
                    self.emit_message(
                        agent_id.clone(),
                        item_id.clone(),
                        TranscriptRole::Reasoning,
                        text.clone(),
                    )?;
                }
                AssistantBlock::ToolCall { item_id, call } => {
                    if self.tools.contains_key(&call.call_id) {
                        return Err(JournalProjectionError::DuplicateToolCall(
                            call.call_id.clone(),
                        ));
                    }
                    let call_id = call.call_id.clone();
                    self.tools.insert(
                        call_id.clone(),
                        ToolProjection {
                            agent_id: agent_id.clone(),
                            item_id: item_id.clone(),
                            call: call.clone(),
                            requested: false,
                            permission_at: None,
                            status: ToolCallStatus::Queued,
                            revision: 0,
                            outcome: None,
                            presentation: ToolPresentation::default(),
                        },
                    );
                    calls.push(call_id);
                }
                AssistantBlock::Text { .. }
                | AssistantBlock::Reasoning { .. }
                | AssistantBlock::ReplayOnly { .. } => {}
            }
        }
        if calls.is_empty() {
            self.atoms.push(
                ContextAtom::assistant(source, output)
                    .map_err(JournalProjectionError::InvalidContext)?,
            );
        } else {
            self.pending = Some(PendingBatch {
                output,
                calls,
                source_entries: vec![source],
            });
        }
        Ok(())
    }
}
