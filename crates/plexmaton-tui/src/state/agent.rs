use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, MailId, TokenUsage, ToolCallId, ToolCallStatus,
    ToolPresentation, TranscriptItemId, TranscriptRole, TurnId,
};

use super::{
    ArtifactView, MailView, ReduceError, ToolCallView, TranscriptEntryView, TranscriptItemView,
    TranscriptTextKind, ordered::OrderedById,
};

/// Per-agent projection consumed by the renderer (ENT-1).
///
/// This type owns the invariants that are local to one agent — transcript item identity and
/// per-item revision continuity — so the workspace reducer is left owning only what is genuinely
/// cross-agent: stream ordering, selection, and the notice log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentView {
    pub id: AgentId,
    pub label: String,
    pub status: AgentStatus,
    entries: OrderedById<TranscriptItemId, TranscriptEntryView>,
    usage: Option<(TurnId, TokenUsage)>,
}

impl AgentView {
    pub(super) fn new(id: AgentId, label: String, status: AgentStatus) -> Self {
        Self {
            id,
            label,
            status,
            entries: OrderedById::default(),
            usage: None,
        }
    }

    /// Iterates transcript items in arrival order.
    pub fn transcript(&self) -> impl Iterator<Item = &TranscriptItemView> {
        self.entries.iter().filter_map(|entry| match entry {
            TranscriptEntryView::Text(item) => Some(item),
            TranscriptEntryView::Tool(_)
            | TranscriptEntryView::Artifact(_)
            | TranscriptEntryView::Mail(_) => None,
        })
    }

    /// Iterates every semantic entry in first-appearance order.
    pub fn entries(&self) -> impl Iterator<Item = &TranscriptEntryView> {
        self.entries.iter()
    }

    /// Iterates tool calls in arrival order.
    pub fn tool_activity(&self) -> impl Iterator<Item = &ToolCallView> {
        self.entries.iter().filter_map(|entry| match entry {
            TranscriptEntryView::Tool(tool) => Some(tool),
            _ => None,
        })
    }

    /// Iterates announced artifacts in arrival order.
    pub fn artifacts(&self) -> impl Iterator<Item = &ArtifactView> {
        self.entries.iter().filter_map(|entry| match entry {
            TranscriptEntryView::Artifact(artifact) => Some(artifact),
            _ => None,
        })
    }

    /// Iterates delivered mail in arrival order.
    pub fn mail(&self) -> impl Iterator<Item = &MailView> {
        self.entries.iter().filter_map(|entry| match entry {
            TranscriptEntryView::Mail(mail) => Some(mail),
            _ => None,
        })
    }

    /// Latest turn usage reported for this agent, retained for on-demand diagnostics.
    #[must_use]
    pub fn usage(&self) -> Option<(&TurnId, &TokenUsage)> {
        self.usage.as_ref().map(|(turn_id, usage)| (turn_id, usage))
    }

    pub(super) fn set_usage(&mut self, turn_id: TurnId, usage: TokenUsage) {
        self.usage = Some((turn_id, usage));
    }

    /// Opens a transcript item that later deltas will append to.
    pub(super) fn start_item(
        &mut self,
        item_id: TranscriptItemId,
        role: TranscriptRole,
    ) -> Result<(), ReduceError> {
        if self.entries.contains(&item_id) {
            return Err(ReduceError::DuplicateTranscriptItem(item_id));
        }
        let _added = self.entries.upsert(
            item_id.clone(),
            TranscriptEntryView::Text(TranscriptItemView {
                id: item_id,
                role,
                kind: TranscriptTextKind::Message,
                source: String::new(),
                revision: 0,
                finalized: false,
            }),
        );
        Ok(())
    }

    /// Appends streamed text to an open transcript item.
    ///
    /// `finalized` is checked before the revision, and it is checked at all because
    /// `TranscriptItemFinalized` is specified as "will receive no further deltas" — a claim the
    /// projection was recording and not enforcing. A producer that streams after finalizing had its
    /// text land silently, which is the one contract violation the notice log could not report.
    /// Refused first because "this item is closed" is the useful answer even when the revision is
    /// also wrong.
    pub(super) fn append_delta(
        &mut self,
        item_id: &TranscriptItemId,
        item_revision: u64,
        text: &str,
    ) -> Result<(), ReduceError> {
        let item = self.item_mut(item_id)?;
        if item.finalized {
            return Err(ReduceError::ItemAlreadyFinalized(item.id.clone()));
        }
        item.advance_revision(item_revision)?;
        item.source.push_str(text);
        Ok(())
    }

    /// Closes a transcript item to further deltas.
    ///
    /// Closing a closed item is refused rather than absorbed. It is not harmless: a second
    /// finalization consumes a revision, so every later event for the item is judged against a
    /// number the producer did not intend, and what the user would see is an unexplained gap.
    pub(super) fn finalize_item(
        &mut self,
        item_id: &TranscriptItemId,
        item_revision: u64,
    ) -> Result<(), ReduceError> {
        let item = self.item_mut(item_id)?;
        if item.finalized {
            return Err(ReduceError::ItemAlreadyFinalized(item.id.clone()));
        }
        item.advance_revision(item_revision)?;
        item.finalized = true;
        Ok(())
    }

    /// Creates or updates one tool call, keeping its arrival position.
    ///
    /// Creation requires queued revision zero; an update must carry the next revision and a valid
    /// lifecycle transition (ENT-2).
    pub(super) fn set_tool_activity(
        &mut self,
        entry_id: TranscriptItemId,
        item_revision: u64,
        id: ToolCallId,
        label: String,
        status: ToolCallStatus,
        presentation: ToolPresentation,
    ) -> Result<bool, ReduceError> {
        if let Some(entry) = self.entries.get_mut(&entry_id) {
            let TranscriptEntryView::Tool(tool) = entry else {
                return Err(ReduceError::EntryKindChanged(entry_id));
            };
            if tool.id != id || tool.label != label {
                return Err(ReduceError::ToolCorrelationChanged(entry_id));
            }
            let expected = tool.revision.saturating_add(1);
            if item_revision != expected {
                return Err(ReduceError::ItemRevisionGap {
                    item_id: entry_id,
                    expected,
                    received: item_revision,
                });
            }
            if !tool.status.can_transition_to(status) {
                return Err(ReduceError::InvalidToolTransition {
                    call_id: id,
                    from: tool.status,
                    to: status,
                });
            }
            tool.status = status;
            tool.presentation = presentation;
            tool.revision = item_revision;
            return Ok(true);
        }
        if item_revision != 0 || status != ToolCallStatus::Queued {
            return Err(ReduceError::UnknownTranscriptItem(entry_id));
        }
        if self.tool_activity().any(|tool| tool.id == id) {
            return Err(ReduceError::DuplicateToolCall(id));
        }
        let _added = self.entries.upsert(
            entry_id.clone(),
            TranscriptEntryView::Tool(ToolCallView {
                entry_id,
                id,
                label,
                status,
                presentation,
                revision: 0,
            }),
        );
        Ok(true)
    }

    /// Records a published artifact.
    pub(super) fn announce_artifact(
        &mut self,
        entry_id: TranscriptItemId,
        id: ArtifactId,
        label: String,
        pointer: String,
    ) -> Result<bool, ReduceError> {
        self.insert_terminal(
            entry_id.clone(),
            TranscriptEntryView::Artifact(ArtifactView {
                entry_id,
                id,
                label,
                pointer,
                revision: 0,
            }),
        )
    }

    /// Adds one outgoing mail item to its producer's transcript.
    pub(super) fn deliver_mail(
        &mut self,
        entry_id: TranscriptItemId,
        id: MailId,
        from: AgentId,
        to: AgentId,
        summary: String,
    ) -> Result<bool, ReduceError> {
        self.insert_terminal(
            entry_id.clone(),
            TranscriptEntryView::Mail(MailView {
                entry_id,
                id,
                from,
                to,
                summary,
                revision: 0,
            }),
        )
    }

    pub(super) fn runtime_message(
        &mut self,
        entry_id: TranscriptItemId,
        message: String,
        error: bool,
    ) -> Result<bool, ReduceError> {
        let item = TranscriptItemView {
            id: entry_id.clone(),
            role: TranscriptRole::System,
            kind: if error {
                TranscriptTextKind::Error
            } else {
                TranscriptTextKind::Warning
            },
            source: message,
            revision: 0,
            finalized: true,
        };
        self.insert_terminal(entry_id, TranscriptEntryView::Text(item))
    }

    fn insert_terminal(
        &mut self,
        entry_id: TranscriptItemId,
        entry: TranscriptEntryView,
    ) -> Result<bool, ReduceError> {
        if self.entries.contains(&entry_id) {
            return Err(ReduceError::DuplicateTranscriptItem(entry_id));
        }
        Ok(self.entries.upsert(entry_id, entry))
    }

    fn item_mut(
        &mut self,
        item_id: &TranscriptItemId,
    ) -> Result<&mut TranscriptItemView, ReduceError> {
        let entry = self
            .entries
            .get_mut(item_id)
            .ok_or_else(|| ReduceError::UnknownTranscriptItem(item_id.clone()))?;
        match entry {
            TranscriptEntryView::Text(item) => Ok(item),
            TranscriptEntryView::Tool(_)
            | TranscriptEntryView::Artifact(_)
            | TranscriptEntryView::Mail(_) => Err(ReduceError::EntryKindChanged(item_id.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
        TranscriptRole,
    };

    use super::{AgentView, ReduceError};

    fn agent() -> AgentView {
        let id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        AgentView::new(id, "Agent A".to_owned(), AgentStatus::Running)
    }

    fn item(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn tool(value: &str) -> ToolCallId {
        ToolCallId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn start_tool(agent: &mut AgentView, entry: &str, call: &str, label: &str) {
        agent
            .set_tool_activity(
                item(entry),
                0,
                tool(call),
                label.to_owned(),
                ToolCallStatus::Queued,
                ToolPresentation::default(),
            )
            .unwrap_or_else(|error| panic!("start tool: {error}"));
    }

    fn move_tool(
        agent: &mut AgentView,
        entry: &str,
        revision: u64,
        call: &str,
        label: &str,
        status: ToolCallStatus,
    ) {
        agent
            .set_tool_activity(
                item(entry),
                revision,
                tool(call),
                label.to_owned(),
                status,
                ToolPresentation::default(),
            )
            .unwrap_or_else(|error| panic!("move tool: {error}"));
    }

    #[test]
    fn starting_the_same_item_twice_is_rejected() {
        let mut agent = agent();
        agent
            .start_item(item("i1"), TranscriptRole::Assistant)
            .unwrap_or_else(|error| panic!("first start: {error}"));

        assert!(matches!(
            agent.start_item(item("i1"), TranscriptRole::Assistant),
            Err(ReduceError::DuplicateTranscriptItem(_))
        ));
    }

    #[test]
    fn deltas_must_carry_the_next_item_revision() {
        let mut agent = agent();
        agent
            .start_item(item("i1"), TranscriptRole::Assistant)
            .unwrap_or_else(|error| panic!("start: {error}"));

        assert!(agent.append_delta(&item("i1"), 1, "one ").is_ok());
        assert!(matches!(
            agent.append_delta(&item("i1"), 3, "skipped"),
            Err(ReduceError::ItemRevisionGap { expected: 2, .. })
        ));
        // The rejected delta must not have appended its text.
        let sources: Vec<_> = agent.transcript().map(|i| i.source.as_str()).collect();
        assert_eq!(sources, ["one "]);
    }

    /// The event contract says a finalized item receives no further deltas. Now the projection does.
    #[test]
    fn a_delta_after_finalization_is_refused_and_the_text_does_not_land() {
        let mut agent = agent();
        agent
            .start_item(item("i1"), TranscriptRole::Assistant)
            .unwrap_or_else(|error| panic!("start: {error}"));
        agent
            .append_delta(&item("i1"), 1, "hello")
            .unwrap_or_else(|error| panic!("delta: {error}"));
        agent
            .finalize_item(&item("i1"), 2)
            .unwrap_or_else(|error| panic!("finalize: {error}"));

        assert!(matches!(
            agent.append_delta(&item("i1"), 3, " and more"),
            Err(ReduceError::ItemAlreadyFinalized(_))
        ));
        assert!(
            matches!(
                agent.finalize_item(&item("i1"), 3),
                Err(ReduceError::ItemAlreadyFinalized(_))
            ),
            "and a second finalization is refused too, rather than consuming a revision"
        );

        let sources: Vec<_> = agent.transcript().map(|i| i.source.as_str()).collect();
        assert_eq!(sources, ["hello"], "the refused text must not have landed");
    }

    #[test]
    fn a_delta_for_an_unknown_item_is_rejected() {
        let mut agent = agent();

        assert!(matches!(
            agent.append_delta(&item("missing"), 1, "text"),
            Err(ReduceError::UnknownTranscriptItem(_))
        ));
    }

    #[test]
    fn tool_activity_iterates_in_arrival_order_not_identifier_order() {
        let mut agent = agent();
        start_tool(&mut agent, "entry-z", "tool-z", "z");
        start_tool(&mut agent, "entry-a", "tool-a", "a");

        let labels: Vec<_> = agent.tool_activity().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["z", "a"]);
    }

    #[test]
    fn updating_a_tool_keeps_its_position_and_replaces_its_status() {
        let mut agent = agent();
        start_tool(&mut agent, "entry-z", "tool-z", "z");
        start_tool(&mut agent, "entry-a", "tool-a", "a");
        move_tool(
            &mut agent,
            "entry-z",
            1,
            "tool-z",
            "z",
            ToolCallStatus::Running,
        );
        move_tool(
            &mut agent,
            "entry-z",
            2,
            "tool-z",
            "z",
            ToolCallStatus::Succeeded,
        );

        let tools: Vec<_> = agent
            .tool_activity()
            .map(|t| (t.label.as_str(), t.status))
            .collect();
        assert_eq!(
            tools,
            [
                ("z", ToolCallStatus::Succeeded),
                ("a", ToolCallStatus::Queued),
            ]
        );
    }
}
