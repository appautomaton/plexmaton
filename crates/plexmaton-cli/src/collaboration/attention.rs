//! Canonical child Attention publication and process-local live routes.

use anyhow::Context as _;
use plexmaton_agent::collaboration::{
    AttentionReference, CollaborationEvent as SharedEvent, MailEndpoint,
};
use plexmaton_core::{
    AgentId, ApprovalId, AttentionId, AttentionRequest, CollaborationItemId, ConversationEvent,
    ConversationId,
};
use plexmaton_runtime::{OwnedRunnerUpdate, RunnerGeneration, RuntimeUpdate};
use plexmaton_session_store::collaboration::CollaborationAttempt;
use sha2::{Digest as _, Sha256};

use super::{Collaboration, LiveApprovalRoute};

pub(super) enum LiveAttentionMutation {
    Requested {
        agent: AgentId,
        approval_id: Option<ApprovalId>,
        route: LiveApprovalRoute,
    },
    Resolved {
        agent: AgentId,
        attention_id: AttentionId,
    },
}

impl Collaboration {
    pub(super) async fn admit_shutdown_attention_update(
        &mut self,
        update: &OwnedRunnerUpdate,
    ) -> anyhow::Result<()> {
        let OwnedRunnerUpdate::Runtime { identity, update } = update else {
            return Ok(());
        };
        let RuntimeUpdate::Event(envelope) = update.as_ref() else {
            return Ok(());
        };
        let event = &envelope.event;
        let producer = match event {
            ConversationEvent::AttentionRequested { agent_id, .. }
            | ConversationEvent::AttentionResolved { agent_id, .. } => agent_id,
            _ => return Ok(()),
        };
        anyhow::ensure!(
            producer == &identity.endpoint().agent,
            "shutdown Attention update names another producer"
        );
        let _mutation = self
            .admit_attention(
                &identity.endpoint().conversation,
                identity.generation(),
                event,
            )
            .await?;
        Ok(())
    }

    pub(super) async fn admit_pending_attention_for_shutdown(&mut self) -> anyhow::Result<()> {
        let pending = match self.pending_projection.as_ref() {
            Some(super::PendingRootProjection::RunnerEvent {
                child,
                generation,
                event,
                ..
            }) if matches!(
                event.as_ref(),
                ConversationEvent::AttentionRequested { .. }
                    | ConversationEvent::AttentionResolved { .. }
            ) =>
            {
                Some((child.clone(), *generation, event.as_ref().clone()))
            }
            _ => None,
        };
        if let Some((child, generation, event)) = pending {
            let _route = self.admit_attention(&child, generation, &event).await?;
        }
        Ok(())
    }

    pub(super) async fn admit_attention(
        &mut self,
        child: &ConversationId,
        generation: RunnerGeneration,
        event: &ConversationEvent,
    ) -> anyhow::Result<Option<LiveAttentionMutation>> {
        let records = self
            .owner
            .records()
            .await
            .context("read canonical child endpoint for Attention")?;
        let Some(producer) = records.iter().find_map(|record| match &record.event {
            SharedEvent::DelegationCreated { worker, .. } if &worker.conversation == child => {
                Some(worker.clone())
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        let Some((attempt, mutation)) = attention_attempt(producer, generation, event) else {
            return Ok(None);
        };
        self.owner
            .admit(attempt)
            .await
            .context("admit child Attention reference")?;
        Ok(Some(mutation))
    }

    pub(super) fn apply_live_attention(&mut self, mutation: LiveAttentionMutation) {
        match mutation {
            LiveAttentionMutation::Requested {
                agent,
                approval_id: Some(approval_id),
                route,
            } => {
                self.live_approvals.insert((agent, approval_id), route);
            }
            LiveAttentionMutation::Requested {
                approval_id: None, ..
            } => {}
            LiveAttentionMutation::Resolved {
                agent,
                attention_id,
            } => self
                .live_approvals
                .retain(|(owner, _), route| owner != &agent || route.attention_id != attention_id),
        }
    }
}

fn attention_attempt(
    producer: MailEndpoint,
    generation: RunnerGeneration,
    event: &ConversationEvent,
) -> Option<(CollaborationAttempt, LiveAttentionMutation)> {
    let (kind, attention_id, approval_id, agent) = match event {
        ConversationEvent::AttentionRequested {
            agent_id,
            attention_id,
            request,
        } => (
            "request",
            attention_id.clone(),
            match request {
                AttentionRequest::Approval { approval_id, .. } => Some(approval_id.clone()),
                AttentionRequest::Clarification { .. } => None,
            },
            agent_id.clone(),
        ),
        ConversationEvent::AttentionResolved {
            agent_id,
            attention_id,
        } => ("resolution", attention_id.clone(), None, agent_id.clone()),
        _ => return None,
    };
    let reference = AttentionReference {
        producer,
        attention_id: attention_id.clone(),
    };
    let shared = match kind {
        "request" => SharedEvent::AttentionRequested {
            attention: reference,
        },
        "resolution" => SharedEvent::AttentionResolved {
            attention: reference,
        },
        _ => unreachable!("Attention event kind is fixed above"),
    };
    let id = deterministic_item(kind, &shared);
    let mutation = if kind == "request" {
        LiveAttentionMutation::Requested {
            agent,
            approval_id,
            route: LiveApprovalRoute {
                attention_id,
                generation,
            },
        }
    } else {
        LiveAttentionMutation::Resolved {
            agent,
            attention_id,
        }
    };
    Some((CollaborationAttempt { id, event: shared }, mutation))
}

fn deterministic_item(kind: &str, event: &SharedEvent) -> CollaborationItemId {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    digest.update(serde_json::to_vec(event).expect("validated Attention reference serializes"));
    let hash = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    CollaborationItemId::new(format!("attention-{kind}-{hash}"))
        .expect("fixed prefix and SHA-256 are a bounded identity")
}
