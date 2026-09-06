//! The primary agent's current work, derived from the semantic projection.

use plexmaton_core::{AgentStatus, AttentionKind, ToolCallStatus, TranscriptRole};

use super::{AgentView, AttentionView, TranscriptEntryView, ViewState};

/// One compact fact for the primary composer's boundary.
///
/// This is derived rather than stored: the tool, transcript and attention projections remain the
/// authorities, so replay cannot leave a status label to reconcile with them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrentWork<'a> {
    Thinking,
    Responding,
    RunningTool(&'a str),
    ApprovalRequired,
}

impl<'a> CurrentWork<'a> {
    pub(super) fn derive(
        agent: &'a AgentView,
        attention: impl Iterator<Item = &'a AttentionView>,
    ) -> Option<Self> {
        let mut action_required = false;
        let mut approval_requested = false;
        for request in attention.filter(|request| request.agent_id == agent.id) {
            action_required = true;
            approval_requested |= request.kind() == AttentionKind::Approval;
        }
        if approval_requested {
            return Some(Self::ApprovalRequired);
        }

        let mut running_tool = None;
        let mut responding = false;
        for entry in agent.entries() {
            match entry {
                TranscriptEntryView::Tool(tool)
                    if tool.status == ToolCallStatus::AwaitingApproval =>
                {
                    return Some(Self::ApprovalRequired);
                }
                TranscriptEntryView::Tool(tool)
                    if tool.status == ToolCallStatus::Running && running_tool.is_none() =>
                {
                    running_tool = Some(tool.label.as_str());
                }
                TranscriptEntryView::Text(item)
                    if item.role == TranscriptRole::Assistant && !item.finalized =>
                {
                    responding = true;
                }
                TranscriptEntryView::Text(_)
                | TranscriptEntryView::Tool(_)
                | TranscriptEntryView::Artifact(_)
                | TranscriptEntryView::Mail(_) => {}
            }
        }
        // The Attention band names other outstanding requests. They still suppress ambient work
        // here, because action required must not compete with a background label; the Attention
        // band already carries their exact request vocabulary.
        if action_required {
            return None;
        }

        if let Some(tool) = running_tool {
            return Some(Self::RunningTool(tool));
        }

        if agent.status != AgentStatus::Running {
            return None;
        }
        if responding {
            return Some(Self::Responding);
        }
        Some(Self::Thinking)
    }
}

impl ViewState {
    /// Current work for the primary composer's boundary, derived from its semantic facts.
    #[must_use]
    pub(crate) fn current_work(&self) -> Option<CurrentWork<'_>> {
        let primary = self.agents.primary()?;
        CurrentWork::derive(primary, self.attention.iter())
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, ToolCallId,
        ToolCallStatus, ToolPresentation, TranscriptItemId, TranscriptRole,
    };

    use super::CurrentWork;
    use crate::state::{AgentView, AttentionView};

    fn id(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn item(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn agent(status: AgentStatus) -> AgentView {
        AgentView::new(id("agent-a"), "Agent A".to_owned(), status)
    }

    fn open_text(agent: &mut AgentView, name: &str, role: TranscriptRole) {
        agent
            .start_item(item(name), role)
            .unwrap_or_else(|error| panic!("fixture: {error}"));
    }

    fn set_tool(agent: &mut AgentView, name: &str, status: ToolCallStatus) {
        let entry_id = item(&format!("entry-{name}"));
        let call_id = ToolCallId::new(format!("call-{name}"))
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        agent
            .set_tool_entry(
                entry_id.clone(),
                0,
                call_id.clone(),
                name.to_owned(),
                ToolCallStatus::Queued,
                ToolPresentation::default(),
            )
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        if status != ToolCallStatus::Queued {
            agent
                .set_tool_entry(
                    entry_id,
                    1,
                    call_id,
                    name.to_owned(),
                    status,
                    ToolPresentation::default(),
                )
                .unwrap_or_else(|error| panic!("fixture: {error}"));
        }
    }

    fn approval(agent_id: &str) -> AttentionView {
        AttentionView {
            id: AttentionId::new(format!("attention-{agent_id}"))
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: id(agent_id),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new(format!("approval-{agent_id}"))
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new(format!("call-{agent_id}"))
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit_file".to_owned(),
                capabilities: Vec::new(),
                detail: "Change one file".to_owned(),
            },
            acknowledged: false,
        }
    }

    fn clarification(agent_id: &str) -> AttentionView {
        AttentionView {
            id: AttentionId::new(format!("attention-{agent_id}"))
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: id(agent_id),
            request: AttentionRequest::Clarification {
                summary: "Choose a scope".to_owned(),
            },
            acknowledged: false,
        }
    }

    /// The current-work label is a priority projection, not a second lifecycle.
    #[test]
    fn current_work_priority_is_derived_from_semantic_facts() {
        let idle = agent(AgentStatus::Idle);
        let thinking = agent(AgentStatus::Running);

        let mut reasoning = agent(AgentStatus::Running);
        open_text(&mut reasoning, "reasoning", TranscriptRole::Reasoning);

        let mut responding = agent(AgentStatus::Running);
        open_text(&mut responding, "answer", TranscriptRole::Assistant);

        let mut running_tool = responding.clone();
        set_tool(&mut running_tool, "read_file", ToolCallStatus::Running);

        let mut awaiting = running_tool.clone();
        set_tool(&mut awaiting, "edit_file", ToolCallStatus::AwaitingApproval);

        let background_approval = [approval("agent-b")];
        let primary_approval = [approval("agent-a")];
        let primary_clarification = [clarification("agent-a")];
        let cases = [
            ("idle", &idle, &[][..], None),
            (
                "running before output",
                &thinking,
                &[][..],
                Some(CurrentWork::Thinking),
            ),
            (
                "plaintext reasoning",
                &reasoning,
                &[][..],
                Some(CurrentWork::Thinking),
            ),
            (
                "open assistant output",
                &responding,
                &[][..],
                Some(CurrentWork::Responding),
            ),
            (
                "background approval",
                &responding,
                &background_approval,
                Some(CurrentWork::Responding),
            ),
            (
                "running tool over response",
                &running_tool,
                &[][..],
                Some(CurrentWork::RunningTool("read_file")),
            ),
            (
                "primary approval over work",
                &running_tool,
                &primary_approval,
                Some(CurrentWork::ApprovalRequired),
            ),
            (
                "other primary action over ambient work",
                &running_tool,
                &primary_clarification,
                None,
            ),
            (
                "awaiting tool over running tool",
                &awaiting,
                &[][..],
                Some(CurrentWork::ApprovalRequired),
            ),
        ];

        for (name, agent, attention, expected) in cases {
            assert_eq!(
                CurrentWork::derive(agent, attention.iter()),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn parallel_running_tools_use_stable_first_appearance_order() {
        let mut agent = agent(AgentStatus::Waiting);
        set_tool(&mut agent, "first", ToolCallStatus::Running);
        set_tool(&mut agent, "second", ToolCallStatus::Running);

        assert_eq!(
            CurrentWork::derive(&agent, std::iter::empty()),
            Some(CurrentWork::RunningTool("first"))
        );
    }
}
