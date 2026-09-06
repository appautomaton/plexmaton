//! Workspace request ownership and completion admission; no process, clock or terminal effects.

use std::sync::Arc;

use super::*;
use crate::preparation::{Key, PreparedText, Refusal, Request, Token, Work};

#[derive(Debug)]
struct Active {
    token: Token,
    keys: Vec<Key>,
}

#[derive(Debug)]
pub(super) struct Preparation {
    generation: Arc<()>,
    next: Option<u64>,
    active: Option<Active>,
    batch_limit: usize,
}

impl Default for Preparation {
    fn default() -> Self {
        Self {
            generation: Arc::new(()),
            next: Some(0),
            active: None,
            batch_limit: 16,
        }
    }
}

impl Workspace {
    /// Takes a bounded batch of reached snapshots, after a frame has declared its need. The
    /// caller must poll the owned process beside input and return the token with its completion.
    pub fn take_preparation(&mut self) -> Option<Work> {
        self.advance_copy();
        if self.preparation.active.as_ref().is_some_and(|active| {
            active
                .keys
                .iter()
                .any(|key| self.current_preparation_key(key) && self.wanted_preparation_key(key))
        }) {
            return None;
        }
        self.preparation.active = None;
        let keys = self.preparation_keys();
        if keys.is_empty() {
            return None;
        }
        let Some(sequence) = self.preparation.next else {
            for key in keys {
                self.metrics
                    .accept_prepared(PreparedText::unavailable(key, Refusal::Unavailable));
            }
            self.painted = None;
            return None;
        };
        self.preparation.next = sequence.checked_add(1);
        let requests = self.preparation_snapshots(keys);
        if requests.is_empty() {
            return None;
        }
        let token = Token {
            generation: self.preparation.generation.clone(),
            sequence,
        };
        self.preparation.active = Some(Active {
            token: token.clone(),
            keys: requests
                .iter()
                .map(|request| request.key().clone())
                .collect(),
        });
        Some(Work { token, requests })
    }

    /// False after session replacement, supersession or cancellation. The process owner uses
    /// this to cancel obsolete computation even when the new workspace has no text to prepare.
    pub fn owns_preparation(&self, token: &Token) -> bool {
        self.preparation
            .active
            .as_ref()
            .is_some_and(|active| &active.token == token)
    }

    /// Admits only an owned, structurally valid reply for the current semantic revision. New rows
    /// request a frame; they cannot replace the pinned pointer map until that draw succeeds.
    pub fn complete_preparation(&mut self, token: Token, prepared: Vec<PreparedText>) -> bool {
        if !self.owns_preparation(&token) {
            return false;
        }
        let active = self
            .preparation
            .active
            .take()
            .expect("owned token has an active request");
        let valid = prepared.len() == active.keys.len()
            && prepared
                .iter()
                .zip(&active.keys)
                .all(|(prepared, key)| prepared.validates(key))
            && prepared
                .iter()
                .map(PreparedText::allocation_bytes)
                .sum::<usize>()
                <= 2 * 1024 * 1024;
        let mut adopted = false;
        if valid {
            for (key, prepared) in active.keys.into_iter().zip(prepared) {
                if self.current_preparation_key(&key) && self.wanted_preparation_key(&key) {
                    self.metrics.accept_prepared(prepared);
                    adopted = true;
                }
            }
            self.preparation.batch_limit = 16;
        } else {
            for key in active.keys {
                if self.current_preparation_key(&key) && self.wanted_preparation_key(&key) {
                    self.metrics
                        .accept_prepared(PreparedText::unavailable(key, Refusal::Unavailable));
                    adopted = true;
                }
            }
        }
        self.validate_text_selection();
        self.advance_copy();
        if adopted {
            self.painted = None;
        }
        valid && adopted
    }

    /// A process failure is local, typed presentation feedback; it neither invents text nor
    /// repeatedly resubmits the same failed source revision on every frame.
    pub fn fail_preparation(&mut self, token: Token, reason: Refusal) {
        if !self.owns_preparation(&token) {
            return;
        }
        let active = self
            .preparation
            .active
            .take()
            .expect("owned token has an active request");
        if reason == Refusal::Capacity && active.keys.len() > 1 {
            self.preparation.batch_limit = active.keys.len() / 2;
            self.painted = None;
            return;
        }
        for key in active.keys {
            if self.current_preparation_key(&key) {
                self.metrics
                    .accept_prepared(PreparedText::unavailable(key, reason));
                self.painted = None;
            }
        }
        self.validate_text_selection();
        self.advance_copy();
    }

    fn current_preparation_key(&self, key: &Key) -> bool {
        key.math == self.metrics.math()
            && self
                .state
                .agent(&key.agent)
                .and_then(|agent| agent.entries().find(|item| item.id() == &key.item))
                .is_some_and(|item| {
                    key.matches(
                        &key.agent,
                        item,
                        key.width,
                        self.state.disclosure().is_open(item.id()),
                    )
                })
    }

    fn wanted_preparation_key(&self, key: &Key) -> bool {
        self.copy_preparation_keys().any(|wanted| wanted == key)
            || self.selection_preparation_key().as_ref() == Some(key)
            || self.metrics.preparation_needed().contains(key)
    }

    fn preparation_keys(&self) -> Vec<Key> {
        let validation = self.selection_preparation_key();
        let mut keys = Vec::new();
        // The newest reached entries are prepared first: tail-follow must not spend its first
        // batch on entries that the completed tail's own height will push off screen.
        for key in self
            .copy_preparation_keys()
            .chain(validation.as_ref())
            .chain(self.metrics.preparation_needed().iter().rev())
        {
            if self.current_preparation_key(key)
                && self.metrics.prepared(key).is_none()
                && !keys.contains(key)
            {
                keys.push(key.clone());
                if keys.len() == self.preparation.batch_limit {
                    break;
                }
            }
        }
        keys
    }

    fn preparation_snapshots(&mut self, keys: Vec<Key>) -> Vec<Request> {
        let mut requests = Vec::new();
        let mut bytes: usize = 0;
        for key in keys {
            let Some(entry) = self
                .state
                .agent(&key.agent)
                .and_then(|agent| agent.entries().find(|item| item.id() == &key.item))
            else {
                continue;
            };
            let base_needed = snapshot_bytes(entry).saturating_add(key.allocation_bytes());
            if base_needed > 192 * 1024 {
                self.metrics
                    .accept_prepared(PreparedText::unavailable(key, Refusal::Capacity));
                self.painted = None;
            } else if bytes.saturating_add(base_needed) > 192 * 1024 {
                break;
            } else {
                let prefix = self.metrics.prefix_hint_with_budget(
                    &key.agent,
                    entry,
                    key.width,
                    key.open,
                    192 * 1024 - bytes - base_needed,
                );
                let request = Request::new(key.agent.clone(), entry.clone(), key.width, key.open)
                    .with_math(key.math);
                let (request, retained) = match prefix {
                    Some(prefix) => {
                        let retained = base_needed
                            .saturating_add(crate::markdown::PrefixHint::allocation_bytes(&prefix));
                        (request.with_prefix(prefix), retained)
                    }
                    None => (request, base_needed),
                };
                requests.push(request);
                bytes = bytes.saturating_add(retained);
            }
        }
        requests
    }

    fn selection_preparation_key(&self) -> Option<Key> {
        let (_, agent, points, width) = self.state.text_selection_points()?;
        points.into_iter().find_map(|point| {
            let entry = self.state.agent(agent)?.entries().nth(point.index)?;
            let key = Key::new(
                agent,
                entry,
                width,
                self.state.disclosure().is_open(entry.id()),
            )
            .with_math(self.metrics.math());
            self.metrics.prepared_source(&key).is_none().then_some(key)
        })
    }
}

/// Bound owned snapshots before cloning. The transport independently bounds escaped wire bytes.
fn snapshot_bytes(entry: &crate::TranscriptEntryView) -> usize {
    use crate::TranscriptEntryView as Entry;
    use plexmaton_core::ToolDetail;
    let detail = |detail: &ToolDetail| match detail {
        ToolDetail::Text { source, .. } => source.len(),
        ToolDetail::Diff { patch } => patch.as_str().len(),
    };
    let source = match entry {
        Entry::Text(text) => text.source.len(),
        Entry::Tool(tool) => {
            tool.label.len()
                + tool.id.as_str().len()
                + tool.presentation.invocation.as_ref().map_or(0, detail)
                + tool.presentation.outcome.as_ref().map_or(0, detail)
        }
        Entry::Artifact(artifact) => {
            artifact.id.as_str().len() + artifact.label.len() + artifact.pointer.len()
        }
        Entry::Mail(mail) => {
            mail.id.as_str().len()
                + mail.summary.len()
                + mail.from.as_str().len()
                + mail.to.as_str().len()
        }
    };
    size_of::<crate::TranscriptEntryView>() + entry.id().as_str().len() + source
}
