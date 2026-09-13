//! Bounded all-head semantic tree projection (TRE-2, TRE-6, TRE-7).

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{
    AgentId, ConversationEntryId, TreeHead, TreeOrigin, TreePreview, TreeRevision,
    TreeRewindEligibility, TreeRow, TreeRowKind, TreeSnapshot, TreeSnapshotError,
    TreeSnapshotLimit,
};

use super::{ConversationJournal, JournalEntryPayload, JournalRecord, PROCESS_RECOVERY_MESSAGE};
use crate::AssistantBlock;

const MAX_TREE_HEAD_COUNT: usize = 64;
const MAX_TREE_HEAD_NAME_BYTES: usize = plexmaton_core::MAX_TREE_LABEL_BYTES;
const MAX_TREE_ANCESTRY_ENTRY_COUNT: usize = 16_384;
const MAX_TREE_SCANNED_RECORD_COUNT: usize = 65_536;
const MAX_TREE_NODE_COUNT: usize = 2_048;
const MAX_TREE_PREVIEW_BYTES_PER_ROW: usize = 512;
const MAX_TREE_AGGREGATE_PREVIEW_BYTES: usize = 256 * 1024;
const PREVIEW_ELLIPSIS: &str = "…";

#[cfg(test)]
mod tests;

impl ConversationJournal {
    /// Builds a complete, bounded snapshot for one agent from every active named head.
    ///
    /// Shared ancestry is visited once. Rows follow journal append chronology, and rewind
    /// eligibility is delegated to the same stable-boundary resolver used by navigation (TRE-2,
    /// TRE-6, TRE-7). Any exceeded bound returns an explicit error rather than a partial tree.
    pub fn tree_snapshot(&self, agent_id: &AgentId) -> Result<TreeSnapshot, TreeSnapshotError> {
        let heads = self.snapshot_heads()?;
        let (ancestry, active_ancestry) = self.head_ancestry()?;
        self.check_record_limit()?;

        let mut rows = Vec::new();
        let mut nearest_row_by_entry =
            BTreeMap::<ConversationEntryId, Option<ConversationEntryId>>::new();
        let mut aggregate_preview_bytes = 0_usize;

        // Journal records are already ordered by their validated monotonic sequence. Scanning
        // this vector once supplies both row order and the stable ordinal without consulting IDs.
        for record in &self.records {
            let JournalRecord::AppendEntry { entry, .. } = record else {
                continue;
            };
            if !ancestry.contains(&entry.id) {
                continue;
            }

            let parent_row = entry
                .parent_id
                .as_ref()
                .and_then(|parent| nearest_row_by_entry.get(parent))
                .cloned()
                .flatten();
            if let Some((kind, preview)) = semantic_row(agent_id, &entry.payload) {
                if rows.len() == MAX_TREE_NODE_COUNT {
                    return Err(limit_error(
                        TreeSnapshotLimit::Nodes,
                        MAX_TREE_NODE_COUNT,
                        MAX_TREE_NODE_COUNT.saturating_add(1),
                    ));
                }
                aggregate_preview_bytes =
                    aggregate_preview_bytes.saturating_add(preview.text.len());
                if aggregate_preview_bytes > MAX_TREE_AGGREGATE_PREVIEW_BYTES {
                    return Err(limit_error(
                        TreeSnapshotLimit::AggregatePreviewBytes,
                        MAX_TREE_AGGREGATE_PREVIEW_BYTES,
                        aggregate_preview_bytes,
                    ));
                }
                let chronological_ordinal = u64::try_from(rows.len())
                    .unwrap_or_else(|_| unreachable!("tree node limit fits in a u64 ordinal"));
                let rewind = if self.resolve_rewind_target(agent_id, &entry.id).is_ok() {
                    TreeRewindEligibility::Eligible
                } else {
                    TreeRewindEligibility::Ineligible
                };
                rows.push(TreeRow {
                    entry_id: entry.id.clone(),
                    parent_id: parent_row,
                    chronological_ordinal,
                    kind,
                    preview,
                    label: self.tree_label(&entry.id).cloned(),
                    head_markers: Vec::new(),
                    active_ancestry: active_ancestry.contains(&entry.id),
                    rewind,
                });
                nearest_row_by_entry.insert(entry.id.clone(), Some(entry.id.clone()));
            } else {
                nearest_row_by_entry.insert(entry.id.clone(), parent_row);
            }
        }

        self.attach_head_markers(&mut rows, &nearest_row_by_entry);
        let origin = TreeOrigin {
            conversation_id: self.metadata.conversation_id().clone(),
            agent_id: agent_id.clone(),
            selected_head: self.selected.clone(),
            revision: TreeRevision::new(self.next_sequence.get()),
        };
        Ok(TreeSnapshot {
            origin,
            heads,
            rows,
        })
    }

    fn snapshot_heads(&self) -> Result<Vec<TreeHead>, TreeSnapshotError> {
        if self.heads.len() > MAX_TREE_HEAD_COUNT {
            return Err(limit_error(
                TreeSnapshotLimit::Heads,
                MAX_TREE_HEAD_COUNT,
                self.heads.len(),
            ));
        }
        // Older/direct journal APIs accept longer names than today's metadata editor. Check
        // before cloning into heads, ancestry work lists, markers or the selected origin.
        for name in self.heads.keys() {
            if name.as_str().len() > MAX_TREE_HEAD_NAME_BYTES {
                return Err(limit_error(
                    TreeSnapshotLimit::HeadNameBytes,
                    MAX_TREE_HEAD_NAME_BYTES,
                    name.as_str().len(),
                ));
            }
        }
        Ok(self
            .heads
            .iter()
            .map(|(name, state)| TreeHead {
                name: name.clone(),
                target: state.target.clone(),
            })
            .collect())
    }

    fn head_ancestry(
        &self,
    ) -> Result<(BTreeSet<ConversationEntryId>, BTreeSet<ConversationEntryId>), TreeSnapshotError>
    {
        let mut ancestry = BTreeSet::new();
        let mut active_ancestry = BTreeSet::new();
        let mut ordered_heads = Vec::with_capacity(self.heads.len());
        ordered_heads.push(self.selected.clone());
        ordered_heads.extend(
            self.heads
                .keys()
                .filter(|head| *head != &self.selected)
                .cloned(),
        );

        // Traverse the selected path first so its complete active ancestry is marked even if
        // another head shares its ancestors. Every later path stops at its first visited node.
        for head_name in ordered_heads {
            let state = self.heads.get(&head_name).unwrap_or_else(|| {
                unreachable!("the selected head remains in the active head map")
            });
            let mut cursor = state.target.as_ref();
            while let Some(entry_id) = cursor {
                if head_name == self.selected {
                    active_ancestry.insert(entry_id.clone());
                }
                if !ancestry.insert(entry_id.clone()) {
                    break;
                }
                if ancestry.len() > MAX_TREE_ANCESTRY_ENTRY_COUNT {
                    return Err(limit_error(
                        TreeSnapshotLimit::AncestryEntries,
                        MAX_TREE_ANCESTRY_ENTRY_COUNT,
                        ancestry.len(),
                    ));
                }
                let entry = self.entries.get(entry_id).unwrap_or_else(|| {
                    unreachable!("validated active heads only point to present entries")
                });
                cursor = entry.parent_id.as_ref();
            }
        }
        Ok((ancestry, active_ancestry))
    }

    fn check_record_limit(&self) -> Result<(), TreeSnapshotError> {
        if self.records.len() > MAX_TREE_SCANNED_RECORD_COUNT {
            return Err(limit_error(
                TreeSnapshotLimit::ScannedRecords,
                MAX_TREE_SCANNED_RECORD_COUNT,
                self.records.len(),
            ));
        }
        Ok(())
    }

    fn attach_head_markers(
        &self,
        rows: &mut [TreeRow],
        nearest_row_by_entry: &BTreeMap<ConversationEntryId, Option<ConversationEntryId>>,
    ) {
        let row_indexes: BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.entry_id.clone(), index))
            .collect();
        for (head_name, state) in &self.heads {
            let Some(target) = state.target.as_ref() else {
                continue;
            };
            let Some(Some(row_id)) = nearest_row_by_entry.get(target) else {
                continue;
            };
            let index = row_indexes.get(row_id).unwrap_or_else(|| {
                unreachable!("every projected head marker names a row in the same snapshot")
            });
            rows[*index].head_markers.push(head_name.clone());
        }
    }
}

fn semantic_row(
    agent_id: &AgentId,
    payload: &JournalEntryPayload,
) -> Option<(TreeRowKind, TreePreview)> {
    let (kind, preview) = match payload {
        JournalEntryPayload::TurnStarted {
            agent_id: owner,
            text,
            ..
        } if owner == agent_id => (TreeRowKind::User, PreviewBuilder::from_text(text)),
        JournalEntryPayload::SteeringAccepted {
            agent_id: owner,
            text,
            ..
        } if owner == agent_id => (TreeRowKind::Steering, PreviewBuilder::from_text(text)),
        JournalEntryPayload::AssistantOutput {
            agent_id: owner,
            output,
            ..
        } if owner == agent_id => {
            let has_tool_calls = output
                .blocks()
                .iter()
                .any(|block| matches!(block, AssistantBlock::ToolCall { .. }));
            let mut builder = PreviewBuilder::default();
            for block in output.blocks() {
                match block {
                    AssistantBlock::Text { text, .. } | AssistantBlock::Reasoning { text, .. } => {
                        builder.append(text)
                    }
                    AssistantBlock::ToolCall { call, .. } => builder.append(&call.name),
                    AssistantBlock::ReplayOnly { .. } => {}
                }
            }
            (
                if has_tool_calls {
                    TreeRowKind::ToolBatch
                } else {
                    TreeRowKind::Assistant
                },
                builder,
            )
        }
        JournalEntryPayload::CompactionCheckpoint {
            agent_id: owner, ..
        } if owner == agent_id => (TreeRowKind::Checkpoint, PreviewBuilder::default()),
        JournalEntryPayload::CollaborationTurnStarted {
            agent_id: owner, ..
        } if owner == agent_id => (TreeRowKind::Notice, PreviewBuilder::default()),
        JournalEntryPayload::MailDelivered {
            from, to, summary, ..
        } if from == agent_id || to == agent_id => {
            (TreeRowKind::Notice, PreviewBuilder::from_text(summary))
        }
        JournalEntryPayload::ArtifactAnnounced {
            agent_id: owner,
            label,
            ..
        } if owner == agent_id => (TreeRowKind::Notice, PreviewBuilder::from_text(label)),
        JournalEntryPayload::RuntimeWarning {
            agent_id: owner,
            message,
            ..
        }
        | JournalEntryPayload::RuntimeError {
            agent_id: owner,
            message,
            ..
        } if owner == agent_id => (TreeRowKind::Notice, PreviewBuilder::from_text(message)),
        JournalEntryPayload::TurnInterruptedByRecovery {
            agent_id: owner, ..
        } if owner == agent_id => (
            TreeRowKind::Notice,
            PreviewBuilder::from_text(PROCESS_RECOVERY_MESSAGE),
        ),
        _ => return None,
    };
    Some((kind, preview.finish()))
}

#[derive(Default)]
struct PreviewBuilder {
    text: String,
    truncated: bool,
}

impl PreviewBuilder {
    fn from_text(text: &str) -> Self {
        let mut builder = Self::default();
        builder.append(text);
        builder
    }

    fn append(&mut self, source: &str) {
        if self.truncated || source.is_empty() {
            return;
        }
        let separator = if self.text.is_empty() { "" } else { " " };
        let full_length = self
            .text
            .len()
            .saturating_add(separator.len())
            .saturating_add(source.len());
        if full_length <= MAX_TREE_PREVIEW_BYTES_PER_ROW {
            self.text.push_str(separator);
            self.text.push_str(source);
            return;
        }

        while self
            .text
            .len()
            .saturating_add(usize::from(!self.text.is_empty()))
            .saturating_add(PREVIEW_ELLIPSIS.len())
            > MAX_TREE_PREVIEW_BYTES_PER_ROW
        {
            let _ = self.text.pop();
        }
        let separator = if self.text.is_empty() { "" } else { " " };
        self.text.push_str(separator);
        for character in source.chars() {
            if self
                .text
                .len()
                .saturating_add(character.len_utf8())
                .saturating_add(PREVIEW_ELLIPSIS.len())
                > MAX_TREE_PREVIEW_BYTES_PER_ROW
            {
                break;
            }
            self.text.push(character);
        }
        self.text.push_str(PREVIEW_ELLIPSIS);
        self.truncated = true;
    }

    fn finish(self) -> TreePreview {
        TreePreview {
            text: self.text,
            truncated: self.truncated,
        }
    }
}

fn limit_error(limit: TreeSnapshotLimit, maximum: usize, observed: usize) -> TreeSnapshotError {
    TreeSnapshotError::LimitExceeded {
        limit,
        maximum,
        observed,
    }
}
