use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, MailId, ToolActivityId, ToolActivityStatus, TranscriptItemId,
    TranscriptRole,
};

use super::{ReduceError, ordered::OrderedById};

/// Projected transcript content. Semantic source is retained separately from terminal cells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptItemView {
    pub id: TranscriptItemId,
    pub role: TranscriptRole,
    pub source: String,
    pub revision: u64,
    pub finalized: bool,
}

impl TranscriptItemView {
    /// Accepts the next per-item revision, rejecting a lost or duplicated update.
    fn advance_revision(&mut self, received: u64) -> Result<(), ReduceError> {
        let expected = self.revision + 1;
        if received != expected {
            return Err(ReduceError::ItemRevisionGap {
                item_id: self.id.clone(),
                expected,
                received,
            });
        }
        self.revision = received;
        Ok(())
    }
}

/// One visible tool activity and its current lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolActivityView {
    pub id: ToolActivityId,
    pub label: String,
    pub status: ToolActivityStatus,
}

/// Durable work product announced by an agent, referenced by pointer rather than copied inline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactView {
    pub id: ArtifactId,
    pub label: String,
    pub pointer: String,
}

/// Typed mail delivered between sessions.
///
/// Sender identity is part of the product contract, so it is retained rather than reduced away.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailView {
    pub id: MailId,
    pub from: AgentId,
    pub summary: String,
}

/// Per-agent projection consumed by the renderer.
///
/// This type owns the invariants that are local to one agent — transcript item identity and
/// per-item revision continuity — so the workspace reducer is left owning only what is genuinely
/// cross-agent: stream ordering, selection, and the notice log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentView {
    pub id: AgentId,
    pub label: String,
    pub status: AgentStatus,
    items: OrderedById<TranscriptItemId, TranscriptItemView>,
    tools: OrderedById<ToolActivityId, ToolActivityView>,
    artifacts: OrderedById<ArtifactId, ArtifactView>,
    inbox: OrderedById<MailId, MailView>,
}

impl AgentView {
    pub(super) fn new(id: AgentId, label: String, status: AgentStatus) -> Self {
        Self {
            id,
            label,
            status,
            items: OrderedById::default(),
            tools: OrderedById::default(),
            artifacts: OrderedById::default(),
            inbox: OrderedById::default(),
        }
    }

    /// Iterates transcript items in arrival order.
    pub fn transcript(&self) -> impl Iterator<Item = &TranscriptItemView> {
        self.items.iter()
    }

    /// Iterates tool activity in arrival order.
    pub fn tool_activity(&self) -> impl Iterator<Item = &ToolActivityView> {
        self.tools.iter()
    }

    /// Iterates announced artifacts in arrival order.
    pub fn artifacts(&self) -> impl Iterator<Item = &ArtifactView> {
        self.artifacts.iter()
    }

    /// Iterates delivered mail in arrival order.
    pub fn inbox(&self) -> impl Iterator<Item = &MailView> {
        self.inbox.iter()
    }

    /// Opens a transcript item that later deltas will append to.
    pub(super) fn start_item(
        &mut self,
        item_id: TranscriptItemId,
        role: TranscriptRole,
    ) -> Result<(), ReduceError> {
        if self.items.contains(&item_id) {
            return Err(ReduceError::DuplicateTranscriptItem(item_id));
        }
        let _added = self.items.upsert(
            item_id.clone(),
            TranscriptItemView {
                id: item_id,
                role,
                source: String::new(),
                revision: 0,
                finalized: false,
            },
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

    /// Creates or updates one tool activity, keeping its arrival position.
    ///
    /// These three all report whether the collection now says anything different, because a
    /// producer polling a tool's state re-sends the state it last sent and FR-1 says that costs no
    /// frame.
    pub(super) fn set_tool_activity(
        &mut self,
        id: ToolActivityId,
        label: String,
        status: ToolActivityStatus,
    ) -> bool {
        self.tools
            .upsert(id.clone(), ToolActivityView { id, label, status })
    }

    /// Records a published artifact.
    pub(super) fn announce_artifact(
        &mut self,
        id: ArtifactId,
        label: String,
        pointer: String,
    ) -> bool {
        self.artifacts
            .upsert(id.clone(), ArtifactView { id, label, pointer })
    }

    /// Adds one mail item to this agent's inbox.
    pub(super) fn deliver_mail(&mut self, id: MailId, from: AgentId, summary: String) -> bool {
        self.inbox
            .upsert(id.clone(), MailView { id, from, summary })
    }

    fn item_mut(
        &mut self,
        item_id: &TranscriptItemId,
    ) -> Result<&mut TranscriptItemView, ReduceError> {
        self.items
            .get_mut(item_id)
            .ok_or_else(|| ReduceError::UnknownTranscriptItem(item_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ToolActivityId, ToolActivityStatus, TranscriptItemId, TranscriptRole,
    };

    use super::{AgentView, ReduceError};

    fn agent() -> AgentView {
        let id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        AgentView::new(id, "Agent A".to_owned(), AgentStatus::Running)
    }

    fn item(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn tool(value: &str) -> ToolActivityId {
        ToolActivityId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
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
        agent.set_tool_activity(tool("tool-z"), "z".into(), ToolActivityStatus::Running);
        agent.set_tool_activity(tool("tool-a"), "a".into(), ToolActivityStatus::Running);

        let labels: Vec<_> = agent.tool_activity().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["z", "a"]);
    }

    #[test]
    fn updating_a_tool_keeps_its_position_and_replaces_its_status() {
        let mut agent = agent();
        agent.set_tool_activity(tool("tool-z"), "z".into(), ToolActivityStatus::Running);
        agent.set_tool_activity(tool("tool-a"), "a".into(), ToolActivityStatus::Running);
        agent.set_tool_activity(tool("tool-z"), "z".into(), ToolActivityStatus::Succeeded);

        let tools: Vec<_> = agent
            .tool_activity()
            .map(|t| (t.label.as_str(), t.status))
            .collect();
        assert_eq!(
            tools,
            [
                ("z", ToolActivityStatus::Succeeded),
                ("a", ToolActivityStatus::Running),
            ]
        );
    }
}
