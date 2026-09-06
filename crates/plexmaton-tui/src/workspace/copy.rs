//! Bounded selected-text assembly consumes prepared data, including entries no longer on screen.

use super::*;
use crate::{
    Selection,
    preparation::{Key, Refusal},
    state::CopyNote,
    text_layout::Layout,
};
use std::collections::VecDeque;

const MAX_COPY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct Assembly {
    selection: Selection,
    /// Semantic identities observed at release cancel changed-source work. Already painted
    /// fragments are captured separately because they may belong to an older source revision.
    keys: Vec<Key>,
    next: usize,
    text: String,
    entries: usize,
    key_bytes: usize,
    captured: VecDeque<Captured>,
    captured_bytes: usize,
}

#[derive(Debug)]
struct Captured {
    index: usize,
    text: Option<String>,
}

#[derive(Debug, Default)]
pub(super) enum CopyPreparation {
    #[default]
    Idle,
    Pending(Assembly),
    Ready(Assembly, Delivery),
}

#[derive(Debug)]
pub(super) enum Delivery {
    Pending,
    Taken,
}

impl Assembly {
    fn new(state: &ViewState, metrics: &TranscriptMetrics) -> Result<Option<Self>, CopyNote> {
        let Some(selection) = state.selection().filter(|selection| selection.is_text()) else {
            return Ok(None);
        };
        let (_, _, points, width) = state.text_selection_points().ok_or(CopyNote::Changed)?;
        if points[0] == points[1] && !points[0].is_atomic() {
            return Ok(None);
        }
        let agent = state.agent(&selection.agent).ok_or(CopyNote::Changed)?;
        let (first, last) = selection.bounds();
        let count = last.saturating_sub(first).saturating_add(1);
        if count > MAX_COPY_BYTES / size_of::<Key>() {
            return Err(CopyNote::Capacity);
        }
        let mut keys = Vec::with_capacity(count);
        let mut key_bytes = keys.capacity() * size_of::<Key>()
            + size_of::<Self>()
            + points
                .iter()
                .map(|point| point.allocation_bytes())
                .sum::<usize>()
            + selection.agent.as_str().len();
        for item in agent.entries().skip(first).take(count) {
            if !Key::fits(&agent.id, item) {
                return Err(CopyNote::Capacity);
            }
            let key = Key::new(
                &agent.id,
                item,
                width,
                state.disclosure().is_open(item.id()),
            )
            .with_math(metrics.math());
            key_bytes += key.allocation_bytes() - size_of::<Key>();
            if key_bytes > MAX_COPY_BYTES {
                return Err(CopyNote::Capacity);
            }
            keys.push(key);
        }
        if keys.len() != count {
            return Err(CopyNote::Changed);
        }
        let mut assembly = Self {
            selection: selection.clone(),
            keys,
            next: 0,
            text: String::new(),
            entries: 0,
            key_bytes,
            captured: VecDeque::new(),
            captured_bytes: 0,
        };
        assembly.capture_painted(state, metrics)?;
        Ok(Some(assembly))
    }

    /// Freeze visible fragments before preparing any off-screen gaps. A later frame may retire
    /// these layouts; the release must still copy the representation the user selected (FR-3).
    fn capture_painted(
        &mut self,
        state: &ViewState,
        metrics: &TranscriptMetrics,
    ) -> Result<(), CopyNote> {
        let (first, last) = self.selection.bounds();
        let painted =
            metrics.painted_sources(self.selection.surface, &self.selection.agent, first..=last);
        self.captured = VecDeque::with_capacity(painted.clone().count());
        self.key_bytes += self.captured.capacity() * size_of::<Captured>();
        if self.key_bytes > MAX_COPY_BYTES {
            return Err(CopyNote::Capacity);
        }
        for (index, key, layout) in painted {
            let offset = index - first;
            let Some(observed) = self.keys.get(offset) else {
                continue;
            };
            // Width belongs to the painted surface; plain selection text excludes soft wrapping.
            if key.item != observed.item
                || key.open != observed.open
                || key.math != observed.math
                || key.revision > observed.revision
            {
                continue;
            }
            let text = match self.selected_part(state, index, layout)? {
                None => None,
                Some(part) => {
                    if self.key_bytes + self.captured_bytes + part.len() > MAX_COPY_BYTES {
                        return Err(CopyNote::Capacity);
                    }
                    let mut text = String::new();
                    text.try_reserve_exact(part.len())
                        .map_err(|_| CopyNote::Capacity)?;
                    if self.key_bytes + self.captured_bytes + text.capacity() > MAX_COPY_BYTES {
                        return Err(CopyNote::Capacity);
                    }
                    text.push_str(part);
                    self.captured_bytes += text.capacity();
                    Some(text)
                }
            };
            self.captured.push_back(Captured {
                index: offset,
                text,
            });
        }
        self.captured
            .make_contiguous()
            .sort_by_key(|part| part.index);
        Ok(())
    }

    fn valid(&self, state: &ViewState) -> bool {
        if state.selection() != Some(&self.selection) {
            return false;
        }
        let Some(agent) = state.agent(&self.selection.agent) else {
            return false;
        };
        let (first, _) = self.selection.bounds();
        let mut entries = agent.entries().skip(first);
        self.keys.iter().all(|key| {
            entries.next().is_some_and(|item| {
                key.matches(
                    &agent.id,
                    item,
                    key.width,
                    state.disclosure().is_open(item.id()),
                )
            })
        })
    }

    fn selected_part<'a>(
        &self,
        state: &ViewState,
        index: usize,
        layout: &'a Layout,
    ) -> Result<Option<&'a str>, CopyNote> {
        let (_, _, points, _) = state.text_selection_points().ok_or(CopyNote::Changed)?;
        let entry = state
            .agent(&self.selection.agent)
            .and_then(|agent| agent.entries().nth(index))
            .ok_or(CopyNote::Changed)?;
        if points
            .into_iter()
            .any(|point| point.index == index && !point.matches(entry, layout))
        {
            return Err(CopyNote::Changed);
        }
        state
            .selected_text_range(
                self.selection.surface,
                &self.selection.agent,
                index,
                layout.text.len(),
            )
            .map(|range| layout.text.get(range).ok_or(CopyNote::Changed))
            .transpose()
    }

    fn append(&mut self, part: Option<&str>) -> Result<(), CopyNote> {
        if let Some(part) = part {
            let separator = if self.entries == 0 { "" } else { "\n\n" };
            let length = self
                .text
                .len()
                .saturating_add(separator.len())
                .saturating_add(part.len());
            if self
                .key_bytes
                .saturating_add(self.captured_bytes)
                .saturating_add(length)
                > MAX_COPY_BYTES
            {
                return Err(CopyNote::Capacity);
            }
            self.text
                .try_reserve_exact(length - self.text.len())
                .map_err(|_| CopyNote::Capacity)?;
            if self.key_bytes + self.captured_bytes + self.text.capacity() > MAX_COPY_BYTES {
                return Err(CopyNote::Capacity);
            }
            self.text.push_str(separator);
            self.text.push_str(part);
            self.entries += 1;
        }
        self.next += 1;
        Ok(())
    }

    fn advance(&mut self, state: &ViewState, metrics: &TranscriptMetrics) -> Result<(), CopyNote> {
        while let Some(key) = self.keys.get(self.next) {
            if let Some(captured) = self.captured.pop_front_if(|part| part.index == self.next) {
                self.captured_bytes -= captured.text.as_ref().map_or(0, String::capacity);
                self.append(captured.text.as_deref())?;
                continue;
            }
            match metrics.prepared_source(key) {
                Some(Ok(layout)) => {
                    let part =
                        self.selected_part(state, self.selection.bounds().0 + self.next, &layout)?;
                    self.append(part)?;
                }
                Some(Err(reason)) => return Err(note(reason)),
                None => break,
            }
        }
        Ok(())
    }

    fn request(&self) -> Option<CopyRequest> {
        (self.next == self.keys.len() && self.entries != 0).then(|| CopyRequest {
            text: self.text.clone(),
            entries: self.entries,
        })
    }
}

fn note(reason: Refusal) -> CopyNote {
    match reason {
        Refusal::Capacity => CopyNote::Capacity,
        Refusal::InvalidRequest | Refusal::Unavailable => CopyNote::Unavailable,
    }
}

impl Workspace {
    /// Reads a ready selection without parsing. Use the Copy intent to request missing data;
    /// `take_copy` receives the eventual result while ordinary input continues.
    pub fn copy_selection(&self) -> Option<CopyRequest> {
        if !self.state.selection().is_some_and(Selection::is_text) {
            return self.state.copy_entries();
        }
        if let CopyPreparation::Ready(assembly, _) = &self.copy
            && assembly.valid(&self.state)
        {
            return assembly.request();
        }
        let mut assembly = Assembly::new(&self.state, &self.metrics).ok()??;
        assembly.advance(&self.state, &self.metrics).ok()?;
        assembly.request()
    }

    pub(super) fn request_selection_copy(&mut self) -> Option<CopyRequest> {
        if !self.state.selection().is_some_and(Selection::is_text) {
            return self.state.copy_entries();
        }
        self.copy = match Assembly::new(&self.state, &self.metrics) {
            Ok(Some(assembly)) => CopyPreparation::Pending(assembly),
            Ok(None) => {
                self.state.clear_selection();
                CopyPreparation::Idle
            }
            Err(reason) => {
                self.state.set_copy_note(Some(reason));
                CopyPreparation::Idle
            }
        };
        self.advance_copy();
        self.take_copy()
    }

    /// Takes an auto-copy result exactly once. It becomes ready only after all selected entries
    /// have matching prepared data; it is not an acknowledgement of clipboard delivery.
    pub fn take_copy(&mut self) -> Option<CopyRequest> {
        self.reconcile_copy();
        let CopyPreparation::Ready(assembly, delivery @ Delivery::Pending) = &mut self.copy else {
            return None;
        };
        *delivery = Delivery::Taken;
        assembly.request()
    }

    pub(super) fn copy_preparation_keys(&self) -> impl Iterator<Item = &Key> {
        let pending = match &self.copy {
            CopyPreparation::Pending(assembly) => Some(assembly),
            _ => None,
        };
        pending.into_iter().flat_map(|assembly| {
            let mut captured = assembly.captured.iter().peekable();
            assembly
                .keys
                .iter()
                .enumerate()
                .skip(assembly.next)
                .filter_map(move |(index, key)| {
                    if captured.peek().is_some_and(|part| part.index == index) {
                        captured.next();
                        None
                    } else {
                        Some(key)
                    }
                })
        })
    }

    pub(super) fn advance_copy(&mut self) {
        self.reconcile_copy();
        let mut assembly = match std::mem::take(&mut self.copy) {
            CopyPreparation::Pending(assembly) => assembly,
            other => {
                self.copy = other;
                return;
            }
        };
        match assembly.advance(&self.state, &self.metrics) {
            Ok(()) if assembly.next == assembly.keys.len() => {
                self.state.set_copy_note(None);
                if assembly.entries == 0 {
                    self.state.clear_selection();
                } else {
                    self.copy = CopyPreparation::Ready(assembly, Delivery::Pending);
                }
            }
            Ok(()) => {
                self.state.set_copy_note(Some(CopyNote::Preparing));
                self.copy = CopyPreparation::Pending(assembly);
            }
            Err(reason) => self.state.set_copy_note(Some(reason)),
        }
    }

    pub(super) fn cancel_pending_copy(&mut self) {
        if matches!(self.copy, CopyPreparation::Pending(_)) {
            self.copy = CopyPreparation::Idle;
            self.state.set_copy_note(None);
        }
    }

    pub(super) fn reconcile_copy(&mut self) {
        let valid = match &self.copy {
            CopyPreparation::Idle => return,
            CopyPreparation::Pending(assembly) | CopyPreparation::Ready(assembly, _) => {
                assembly.valid(&self.state)
            }
        };
        if !valid {
            let same_selection = match &self.copy {
                CopyPreparation::Pending(assembly) | CopyPreparation::Ready(assembly, _) => {
                    self.state.selection() == Some(&assembly.selection)
                }
                CopyPreparation::Idle => false,
            };
            self.copy = CopyPreparation::Idle;
            self.state
                .set_copy_note(same_selection.then_some(CopyNote::Changed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SurfaceId, state::TextPoint};
    use plexmaton_core::{
        AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
        TranscriptItemId, TranscriptRole,
    };

    /// PRE-4: a selection larger than the LRU captures only the successful frame's bounded set;
    /// every retained fragment allocation and its container capacity stay in the copy budget.
    #[test]
    fn large_selection_capture_is_bounded_by_the_painted_set() {
        let (mut workspace, mut terminal, _) = preparation_tests::select_unprepared_history(60);
        let CopyPreparation::Pending(assembly) = &workspace.copy else {
            panic!("off-screen gaps need preparation");
        };
        let (first, last) = assembly.selection.bounds();
        let painted = workspace
            .metrics
            .painted_sources(
                assembly.selection.surface,
                &assembly.selection.agent,
                first..=last,
            )
            .count();
        assert!(assembly.keys.len() > 128);
        assert!(painted > 0 && painted <= 128);
        assert!(!assembly.captured.is_empty() && assembly.captured.len() <= painted);
        let bytes: usize = assembly
            .captured
            .iter()
            .map(|part| part.text.as_ref().map_or(0, String::capacity))
            .sum();
        assert_eq!(assembly.captured_bytes, bytes);
        assert!(
            assembly.key_bytes
                >= assembly.keys.capacity() * size_of::<Key>()
                    + assembly.captured.capacity() * size_of::<Captured>()
        );
        assert!(assembly.key_bytes + assembly.text.capacity() + bytes <= MAX_COPY_BYTES);
        assert!(
            workspace.copy_preparation_keys().count() < assembly.keys.len() - assembly.next,
            "captured tail fragments are not requested again"
        );
        workspace
            .settled_draw(&mut terminal)
            .expect("prepare missing gaps");
        assert_eq!(
            workspace.take_copy().expect("complete selection").entries,
            200
        );
    }

    /// PRE-4/SEL-2: complete individually admitted entries may exceed the selection budget;
    /// refusal produces no partial CopyRequest and accounts for the endpoint/member metadata.
    #[test]
    fn selected_text_capacity_refuses_a_complete_request_without_emitting_a_prefix() {
        let agent = AgentId::new("primary").expect("agent");
        let source = "x".repeat(128 * 1024);
        let mut state = ViewState::default();
        let mut sequence = 0;
        let mut emit = |state: &mut ViewState, event| {
            sequence += 1;
            assert_eq!(
                state.apply(ConversationEventEnvelope {
                    sequence: EventSequence::new(sequence),
                    event
                }),
                crate::ApplyOutcome::Accepted
            );
        };
        emit(
            &mut state,
            ConversationEvent::AgentCreated {
                agent_id: agent.clone(),
                label: "Plexmaton".into(),
                status: AgentStatus::Idle,
            },
        );
        for index in 0..64 {
            let item = TranscriptItemId::new(format!("item-{index}")).expect("item");
            emit(
                &mut state,
                ConversationEvent::TranscriptItemStarted {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    role: TranscriptRole::Assistant,
                },
            );
            emit(
                &mut state,
                ConversationEvent::TranscriptDelta {
                    agent_id: agent.clone(),
                    item_id: item,
                    item_revision: 1,
                    text: source.clone(),
                },
            );
        }
        state.begin_text_selection(
            SurfaceId::Transcript,
            agent.clone(),
            TextPoint::new(
                0,
                TranscriptItemId::new("item-0").expect("item"),
                0,
                &source,
            )
            .expect("anchor"),
            120,
        );
        state.extend_text_selection(
            SurfaceId::Transcript,
            &agent,
            TextPoint::new(
                63,
                TranscriptItemId::new("item-63").expect("item"),
                source.len(),
                &source,
            )
            .expect("focus"),
        );
        let entry = state
            .agent(&agent)
            .expect("agent")
            .entries()
            .next()
            .expect("entry")
            .clone();
        let layout = crate::preparation::Request::new(agent, entry, 120, false)
            .prepare()
            .result
            .expect("one prepared entry fits");
        let mut assembly = Assembly::new(&state, &TranscriptMetrics::default())
            .expect("metadata fits")
            .expect("selection");
        let mut refused = false;
        while assembly.next < assembly.keys.len() {
            let part = assembly
                .selected_part(&state, assembly.next, &layout)
                .expect("selected source");
            if let Err(reason) = assembly.append(part) {
                assert_eq!(reason, CopyNote::Capacity);
                refused = true;
                break;
            }
        }
        assert!(refused, "aggregate selected text must reach the limit");
        assert!(assembly.next > 1 && assembly.next < 64);
        assert!(assembly.key_bytes + assembly.text.capacity() <= MAX_COPY_BYTES);
        assert!(
            assembly.request().is_none(),
            "a retained prefix is not a complete clipboard value"
        );
    }
}
