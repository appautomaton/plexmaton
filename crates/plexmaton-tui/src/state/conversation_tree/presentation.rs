//! Disposable message-tree projection. Journal ancestry and source metadata stay canonical.

use std::collections::BTreeMap;

use plexmaton_core::{ConversationEntryId, HeadName, TreeRewindEligibility, TreeRow, TreeRowKind};

/// Built once per snapshot, shared by drawing, folding and row lookup (TRE-2/TRE-6).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Presentation {
    pub(super) order: Vec<usize>,
    prefixes: BTreeMap<ConversationEntryId, String>,
    rails: BTreeMap<ConversationEntryId, String>,
    descendants: BTreeMap<ConversationEntryId, std::ops::Range<usize>>,
    parents: BTreeMap<ConversationEntryId, Option<ConversationEntryId>>,
    anchors: BTreeMap<ConversationEntryId, Option<ConversationEntryId>>,
    markers: BTreeMap<ConversationEntryId, Vec<HeadName>>,
}

impl Presentation {
    pub(super) fn new(rows: &[TreeRow]) -> Result<Self, &'static str> {
        let indices = rows
            .iter()
            .enumerate()
            .map(|(i, row)| (&row.entry_id, i))
            .collect::<BTreeMap<_, _>>();
        if indices.len() != rows.len() {
            return Err("History contains repeated branch identities.");
        }
        let mut children = BTreeMap::<Option<usize>, Vec<usize>>::new();
        for (index, row) in rows.iter().enumerate() {
            let parent = row
                .parent_id
                .as_ref()
                .map(|id| {
                    indices
                        .get(id)
                        .copied()
                        .ok_or("History contains disconnected branch ancestry.")
                })
                .transpose()?;
            children.entry(parent).or_default().push(index);
        }
        let mut result = Self::default();
        let mut retained = BTreeMap::<Option<usize>, Vec<usize>>::new();
        let mut stack = children
            .get(&None)
            .into_iter()
            .flatten()
            .map(|index| (*index, None))
            .collect::<Vec<_>>();
        while let Some((index, parent)) = stack.pop() {
            let row = &rows[index];
            let shown =
                row.kind != TreeRowKind::ToolBatch || row.rewind == TreeRewindEligibility::Eligible;
            let anchor = if shown {
                result.parents.insert(
                    row.entry_id.clone(),
                    parent.map(|p: usize| rows[p].entry_id.clone()),
                );
                retained.entry(parent).or_default().push(index);
                Some(index)
            } else {
                parent
            };
            result.anchors.insert(
                row.entry_id.clone(),
                anchor.map(|i| rows[i].entry_id.clone()),
            );
            if let Some(descendants) = children.get(&Some(index)) {
                stack.extend(descendants.iter().map(|child| (*child, anchor)));
            }
        }
        if result.anchors.len() != rows.len() {
            return Err("History contains disconnected branch ancestry.");
        }
        for row in rows {
            if let Some(anchor) = result.anchor(&row.entry_id).cloned() {
                result
                    .markers
                    .entry(anchor)
                    .or_default()
                    .extend(row.head_markers.iter().cloned());
            }
        }
        // Display siblings follow chronology; each complete subtree stays contiguous.
        for siblings in retained.values_mut() {
            siblings.sort_by_key(|i| rows[*i].chronological_ordinal);
        }
        let mut stack = Vec::new();
        if let Some(roots) = retained.get(&None) {
            push_children(&mut stack, roots, &[]);
        }
        while let Some((index, lanes, junction)) = stack.pop() {
            result.order.push(index);
            result
                .prefixes
                .insert(rows[index].entry_id.clone(), prefix(&lanes, junction));
            let mut next_lanes = lanes;
            if let Some(last) = junction {
                next_lanes.push(!last);
            }
            result
                .rails
                .insert(rows[index].entry_id.clone(), prefix(&next_lanes, None));
            if let Some(descendants) = retained.get(&Some(index)) {
                push_children(&mut stack, descendants, &next_lanes);
            }
        }
        let positions = result
            .order
            .iter()
            .enumerate()
            .map(|(position, index)| (&rows[*index].entry_id, position))
            .collect::<BTreeMap<_, _>>();
        let mut ends = (1..=result.order.len()).collect::<Vec<_>>();
        for position in (0..result.order.len()).rev() {
            if let Some(parent) = result.parent(&rows[result.order[position]].entry_id) {
                let parent_position = positions[parent];
                ends[parent_position] = ends[parent_position].max(ends[position]);
            }
        }
        for (position, index) in result.order.iter().enumerate() {
            result
                .descendants
                .insert(rows[*index].entry_id.clone(), position + 1..ends[position]);
        }
        Ok(result)
    }

    pub(super) fn prefix(&self, id: &ConversationEntryId) -> &str {
        self.prefixes.get(id).map_or("", String::as_str)
    }

    pub(super) fn rails(&self, id: &ConversationEntryId) -> &str {
        self.rails.get(id).map_or("", String::as_str)
    }

    pub(super) fn descendants(&self, id: &ConversationEntryId) -> &[usize] {
        self.descendants
            .get(id)
            .map_or(&[], |range| &self.order[range.clone()])
    }

    pub(super) fn parent(&self, id: &ConversationEntryId) -> Option<&ConversationEntryId> {
        self.parents.get(id).and_then(Option::as_ref)
    }

    pub(super) fn anchor(&self, id: &ConversationEntryId) -> Option<&ConversationEntryId> {
        self.anchors.get(id).and_then(Option::as_ref)
    }

    pub(super) fn has_children(&self, id: &ConversationEntryId) -> bool {
        self.parents
            .values()
            .any(|parent| parent.as_ref() == Some(id))
    }

    pub(super) fn markers(&self, id: &ConversationEntryId) -> &[HeadName] {
        self.markers.get(id).map_or(&[], Vec::as_slice)
    }
}

type PendingRow = (usize, Vec<bool>, Option<bool>);

fn push_children(stack: &mut Vec<PendingRow>, children: &[usize], lanes: &[bool]) {
    for (position, index) in children.iter().enumerate().rev() {
        let junction = (children.len() > 1).then_some(position + 1 == children.len());
        stack.push((*index, lanes.to_vec(), junction));
    }
}

fn prefix(lanes: &[bool], junction: Option<bool>) -> String {
    const MAX_LANES: usize = 8;
    let mut text = String::new();
    if lanes.len() > MAX_LANES {
        text.push_str("… ");
    }
    for open in lanes.iter().skip(lanes.len().saturating_sub(MAX_LANES)) {
        text.push_str(if *open { " │ " } else { "   " });
    }
    if let Some(last) = junction {
        text.push_str(if last { " └─" } else { " ├─" });
    }
    text
}

impl super::ConversationTree {
    pub(crate) fn ancestry_prefix(&self, id: &ConversationEntryId) -> &str {
        self.presentation.prefix(id)
    }

    pub(crate) fn connector_rails(&self, id: &ConversationEntryId) -> &str {
        self.presentation.rails(id)
    }

    pub(crate) fn folded_contents(&self, id: &ConversationEntryId) -> (usize, Vec<HeadName>) {
        let descendants = self.presentation.descendants(id);
        let heads = descendants
            .iter()
            .flat_map(|index| {
                self.presentation
                    .markers(&self.rows()[*index].entry_id)
                    .iter()
                    .cloned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        (descendants.len(), heads.into_iter().collect())
    }

    pub(crate) fn head_markers(&self, id: &ConversationEntryId) -> &[HeadName] {
        self.presentation.markers(id)
    }
}
