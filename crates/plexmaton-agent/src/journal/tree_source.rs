//! Exact source assembly from canonical entries, never tree previews or provider replay (TRE-8).

use std::collections::BTreeMap;

use plexmaton_core::{
    ConversationEntryId, MAX_TREE_SOURCE_BYTES, ToolDetail, TreeOrigin, TreeRevision,
    TreeSourceError as Error, TreeSourceRequest,
};

use super::{ConversationJournal, JournalEntryPayload, JournalRecord, PROCESS_RECOVERY_MESSAGE};
use crate::{AssistantBlock, AssistantOutput, ToolOutcome};

impl ConversationJournal {
    /// Copies one visible row's exact source. Multiple semantic blocks are separated by a blank
    /// line; within each block every source byte is preserved, including Markdown and CRLF.
    pub fn tree_source(&self, request: &TreeSourceRequest) -> Result<String, Error> {
        let actual = TreeOrigin {
            conversation_id: self.conversation_id().clone(),
            agent_id: request.origin.agent_id.clone(),
            selected_head: self.selected.clone(),
            revision: TreeRevision::new(self.next_sequence.get()),
        };
        if request.origin != actual {
            return Err(Error::StaleOrigin);
        }
        let snapshot = self
            .tree_snapshot(&request.origin.agent_id)
            .map_err(|_| Error::SnapshotLimit)?;
        if !snapshot
            .rows
            .iter()
            .any(|row| row.entry_id == request.entry_id)
        {
            return Err(Error::EntryUnavailable);
        }
        let entry = self
            .entries
            .get(&request.entry_id)
            .ok_or(Error::EntryUnavailable)?;
        let mut source = Source::default();
        match &entry.payload {
            JournalEntryPayload::TurnStarted { text, .. }
            | JournalEntryPayload::SteeringAccepted { text, .. } => source.push(text)?,
            JournalEntryPayload::AssistantOutput { output, .. } => {
                self.copy_assistant_source(&entry.id, output, &mut source)?
            }
            JournalEntryPayload::RuntimeWarning { message, .. }
            | JournalEntryPayload::RuntimeError { message, .. } => source.push(message)?,
            JournalEntryPayload::MailDelivered { summary, .. } => source.push(summary)?,
            JournalEntryPayload::ArtifactAnnounced { label, pointer, .. } => {
                source.push(label)?;
                source.push(pointer)?;
            }
            JournalEntryPayload::TurnInterruptedByRecovery { .. } => {
                source.push(PROCESS_RECOVERY_MESSAGE)?
            }
            JournalEntryPayload::CompactionCheckpoint { checkpoint, .. } => {
                let summary = self
                    .compaction_attempt(checkpoint.successful_attempt_id())
                    .and_then(crate::CompactionAttemptFinished::complete_summary_text)
                    .ok_or(Error::NoSource)?;
                source.push(&summary)?;
            }
            _ => return Err(Error::NoSource),
        }
        source.finish()
    }

    fn copy_assistant_source(
        &self,
        source_id: &ConversationEntryId,
        output: &AssistantOutput,
        source: &mut Source,
    ) -> Result<(), Error> {
        // The snapshot gate already bounded the record scan. Gather each terminal outcome once;
        // call order below, not completion order, defines the indivisible group's source order.
        let mut terminals = BTreeMap::new();
        if output.tool_calls().next().is_some() {
            let mut owners = BTreeMap::new();
            for record in &self.records {
                let JournalRecord::AppendEntry { entry, .. } = record else {
                    continue;
                };
                let owner = if matches!(entry.payload, JournalEntryPayload::AssistantOutput { .. })
                {
                    Some(&entry.id)
                } else {
                    entry
                        .parent_id
                        .as_ref()
                        .and_then(|parent| owners.get(parent).copied())
                };
                if let Some(owner) = owner {
                    owners.insert(&entry.id, owner);
                }
                // Call IDs can repeat on disjoint branches; canonical parent ownership, not the
                // last globally matching ID, determines which outcome belongs to this row.
                if owner == Some(source_id)
                    && let JournalEntryPayload::ToolCallChanged {
                        call_id,
                        outcome: Some(outcome),
                        presentation,
                        ..
                    } = &entry.payload
                {
                    terminals.insert(call_id, (outcome, presentation));
                }
            }
        }
        for block in output.blocks() {
            match block {
                AssistantBlock::Text { text, .. } | AssistantBlock::Reasoning { text, .. } => {
                    source.push(text)?
                }
                AssistantBlock::ToolCall { call, .. } => {
                    let (outcome, presentation) =
                        terminals.get(&call.call_id).ok_or(Error::IncompleteBatch)?;
                    source.push(&call.name)?;
                    source.push(&call.arguments)?;
                    match outcome {
                        ToolOutcome::Succeeded { output } => source.push(output)?,
                        ToolOutcome::Failed { message } => source.push(message)?,
                        _ => {
                            if let Some(detail) = &presentation.outcome {
                                match detail {
                                    ToolDetail::Command(invocation) => {
                                        source.push(&invocation.source)?
                                    }
                                    ToolDetail::Diff { patch } => source.push(patch)?,
                                    ToolDetail::Text {
                                        source: text,
                                        omitted_bytes: 0,
                                    } => source.push(text)?,
                                    ToolDetail::Text { .. } => return Err(Error::NoSource),
                                }
                            }
                        }
                    }
                }
                AssistantBlock::ReplayOnly { .. } => {}
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Source {
    text: String,
}

impl Source {
    fn push(&mut self, part: &str) -> Result<(), Error> {
        if part.is_empty() {
            return Ok(());
        }
        let separator = if self.text.is_empty() { "" } else { "\n\n" };
        let extra = separator
            .len()
            .checked_add(part.len())
            .ok_or(Error::TooLarge)?;
        if self
            .text
            .len()
            .checked_add(extra)
            .is_none_or(|total| total > MAX_TREE_SOURCE_BYTES)
        {
            return Err(Error::TooLarge);
        }
        self.text
            .try_reserve_exact(extra)
            .map_err(|_| Error::TooLarge)?;
        self.text.push_str(separator);
        self.text.push_str(part);
        Ok(())
    }

    fn finish(self) -> Result<String, Error> {
        if self.text.is_empty() {
            Err(Error::NoSource)
        } else {
            Ok(self.text)
        }
    }
}

#[cfg(test)]
mod tests;
