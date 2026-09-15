//! Passive child-journal projection and explicit unavailable-history states.

use anyhow::Context as _;
use plexmaton_core::{AgentId, ConversationEvent, ConversationId, TranscriptItemId};
use plexmaton_runtime::LiveRuntime;
use plexmaton_session_store::StoreError;

use super::{Collaboration, forwarded, item_of};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryUnavailable {
    Missing,
    Locked,
    Invalid,
}

impl HistoryUnavailable {
    fn from_store(error: &StoreError) -> Self {
        match error {
            StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                Self::Missing
            }
            StoreError::WriterLocked => Self::Locked,
            _ => Self::Invalid,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::Missing => "History unavailable: this delegated conversation journal is missing.",
            Self::Locked => {
                "History unavailable: this delegated conversation journal is open in another session."
            }
            Self::Invalid => {
                "History unavailable: this delegated conversation journal could not be validated."
            }
        }
    }
}

impl Collaboration {
    /// Reads back what each child did in an earlier process, from the child's own journal.
    ///
    /// A live child streams its work through its runner and the pending projection forwards it.
    /// A resumed one has no runner until something explicitly addresses it, so its history is only
    /// in its journal. Reading is not waking (CHB-3): the journal is opened, projected and closed
    /// without constructing a runtime or dispatching anything. An unavailable journal retains its
    /// roster row and gains one process-local warning instead of presenting an empty conversation.
    pub(super) fn replay_children(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let children: Vec<(ConversationId, AgentId)> = self
            .announced
            .iter()
            .map(|(conversation, agent)| (conversation.clone(), agent.clone()))
            .collect();
        for (conversation, agent_id) in children {
            let file = match self.children.resume(&conversation) {
                Ok(file) => file,
                Err(error) => {
                    self.project_unavailable_history(
                        runtime,
                        &agent_id,
                        HistoryUnavailable::from_store(&error),
                    )?;
                    continue;
                }
            };
            let journal = file.journal();
            let projection = match journal.project(journal.selected_head()) {
                Ok(projection) => projection,
                Err(_) => {
                    self.project_unavailable_history(
                        runtime,
                        &agent_id,
                        HistoryUnavailable::Invalid,
                    )?;
                    continue;
                }
            };
            for envelope in projection.events() {
                let mut event = envelope.event.clone();
                if !forwarded(&event) {
                    continue;
                }
                let item = item_of(&event);
                *event.agent_mut() = agent_id.clone();
                runtime
                    .project_delegated(&event)
                    .context("project restored delegated work")?;
                if let Some(item) = item {
                    self.replayed.insert(item);
                }
            }
        }
        Ok(())
    }

    fn project_unavailable_history(
        &mut self,
        runtime: &mut LiveRuntime,
        agent_id: &AgentId,
        unavailable: HistoryUnavailable,
    ) -> anyhow::Result<()> {
        let item_id =
            TranscriptItemId::new(format!("child-history-unavailable-{}", agent_id.as_str()))
                .context("build child history warning identity")?;
        runtime
            .project_delegated(&ConversationEvent::RuntimeWarning {
                agent_id: agent_id.clone(),
                item_id,
                message: unavailable.message().to_owned(),
            })
            .context("project unavailable delegated history")
    }
}
