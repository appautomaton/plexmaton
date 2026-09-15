//! Durable session placement for canonical collaboration rows.

use std::collections::BTreeMap;

use anyhow::Context as _;
use plexmaton_agent::collaboration::{CollaborationEvent, CollaborationRecord, MailEndpoint};
use plexmaton_core::{AgentId, ConversationEvent, ConversationId, DelegationId};
use plexmaton_runtime::LiveRuntime;

use super::{Collaboration, item_of, pending::OrphanedIngressLink};

impl Collaboration {
    pub(super) async fn persist_orphaned_link(
        &mut self,
        runtime: &mut LiveRuntime,
        link: OrphanedIngressLink,
    ) -> anyhow::Result<Option<ConversationId>> {
        if self.root.as_ref() == Some(&link.caller) {
            runtime
                .link_collaboration_item(link.reference)
                .await
                .context("persist Main collaboration placement after its tool wait ended")?;
            return Ok(None);
        }
        anyhow::ensure!(
            self.announced.contains_key(&link.caller.conversation),
            "settled collaboration ingress names an unknown child conversation"
        );
        let source = self
            .owner
            .child_session_source(&link.caller.conversation)
            .await
            .context("inspect delegated collaboration placement caller")?
            .context("settled collaboration ingress child is not running")?;
        anyhow::ensure!(
            source.endpoint() == &link.caller,
            "settled collaboration ingress names another child endpoint"
        );
        self.owner
            .link_child_collaboration_item(&link.caller.conversation, link.reference)
            .await
            .context("persist delegated collaboration placement after its tool wait ended")?;
        Ok(Some(link.caller.conversation))
    }

    pub(super) async fn session_placements(
        &self,
        owner: &AgentId,
        journal: &plexmaton_agent::ConversationJournal,
        records: &[CollaborationRecord],
    ) -> anyhow::Result<super::projection::SessionPlacements> {
        let inclusions = journal
            .collaboration_inclusions(journal.selected_head())
            .map_err(|error| {
                anyhow::anyhow!("read selected collaboration inclusions: {error:?}")
            })?;
        let resolved = self
            .owner
            .resolve_session_references(
                inclusions
                    .iter()
                    .map(|origin| origin.reference().clone())
                    .collect(),
            )
            .await
            .context("resolve selected collaboration inclusions")?;
        super::projection::SessionPlacements::build(
            self.collaboration.clone(),
            owner,
            journal,
            &resolved,
            records,
        )
    }

    /// Draws every fact in the log that is not on screen yet, on both sides it names.
    ///
    /// One pass over one log with one rule, because the alternative was three: a mail projection
    /// that filtered a direction, a task projection that compared text, and neither reachable from
    /// the settlement the other answered to. Every hole the user found was one of the three
    /// forgetting what another remembered — a letter Main sent, a task nobody read, a task change
    /// that waited for a restart.
    ///
    /// Returns whether anything arrived *for the root*, which is the one thing that earns it a turn.
    pub(super) async fn show(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<bool> {
        let records = self
            .owner
            .records()
            .await
            .context("read the collaboration log for projection")?;
        let arrived = self.observe_records(&records);
        self.refresh_root_records(runtime, &records).await?;
        Ok(arrived)
    }

    /// Retries root rows after one runtime update has made their session link observable.
    pub(crate) async fn refresh_root(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let records = self
            .owner
            .records()
            .await
            .context("read the collaboration log for root placement")?;
        self.refresh_root_records(runtime, &records).await
    }

    async fn refresh_root_records(
        &mut self,
        runtime: &mut LiveRuntime,
        records: &[CollaborationRecord],
    ) -> anyhow::Result<()> {
        let root = self
            .root
            .as_ref()
            .context("bound collaboration has no Main endpoint")?
            .agent
            .clone();
        if !self.pending_placement.contains(&root) {
            return Ok(());
        }
        if let Some(refusal) = runtime.delegated_projection_refusal() {
            return Err(refusal).context("project collaboration activity");
        }
        let source = runtime
            .collaboration_session_source()
            .context("capture Main's selected collaboration session")?;
        let placements = self
            .session_placements(&source.endpoint().agent, source.journal(), records)
            .await?;
        self.project_session_rows(runtime, records, &root, &placements, true, true)
    }

    pub(super) async fn refresh_child(
        &mut self,
        runtime: &mut LiveRuntime,
        child: &ConversationId,
    ) -> anyhow::Result<()> {
        let Some(agent) = self.announced.get(child).cloned() else {
            return Ok(());
        };
        if !self.pending_placement.contains(&agent) {
            return Ok(());
        }
        let Some(source) = self
            .owner
            .child_session_source(child)
            .await
            .map_err(|error| anyhow::anyhow!("inspect delegated session placement: {error}"))?
        else {
            return Ok(());
        };
        let records = self
            .owner
            .records()
            .await
            .context("read the collaboration log for delegated placement")?;
        let placements = self
            .session_placements(&source.endpoint().agent, source.journal(), &records)
            .await?;
        self.project_session_rows(runtime, &records, &agent, &placements, false, true)
    }

    pub(super) fn project_session_rows(
        &mut self,
        runtime: &mut LiveRuntime,
        records: &[CollaborationRecord],
        owner: &AgentId,
        placements: &super::projection::SessionPlacements,
        place_in_root: bool,
        require_anchor: bool,
    ) -> anyhow::Result<()> {
        // A task update names its delegation, so the session it was given to is read back from the
        // record that created it — the same log, one pass earlier.
        let workers: BTreeMap<DelegationId, MailEndpoint> = records
            .iter()
            .filter_map(|record| match &record.event {
                CollaborationEvent::DelegationCreated {
                    delegation, worker, ..
                } => Some((delegation.clone(), worker.clone())),
                _ => None,
            })
            .collect();
        let mut pending = false;
        for placed in super::projection::session_entries(
            records,
            owner,
            placements,
            &|endpoint| self.name(endpoint),
            &|delegation| workers.get(delegation).cloned(),
        ) {
            let Some(item) = item_of(&placed.event) else {
                continue;
            };
            if self.shown.contains(&item) {
                continue;
            }
            // Live rows wait for their own session's acknowledged link. Restore also reaches this
            // function, where old journals may supply an inclusion anchor instead.
            if require_anchor && placed.anchors.is_empty() {
                pending = true;
                break;
            }
            if place_in_root {
                runtime
                    .project_delegated_reference(&placed.event, placed.reference, placed.anchors)
                    .context("project placed collaboration log entry")?;
            } else {
                runtime
                    .project_delegated(&placed.event)
                    .context("project delegated collaboration log entry")?;
            }
            self.shown.insert(item);
        }
        if pending {
            self.pending_placement.insert(owner.clone());
        } else {
            self.pending_placement.remove(owner);
        }
        Ok(())
    }

    pub(super) fn observe_records(&mut self, records: &[CollaborationRecord]) -> bool {
        let Some(root) = self.root.clone() else {
            return false;
        };
        let workers: BTreeMap<DelegationId, MailEndpoint> = records
            .iter()
            .filter_map(|record| match &record.event {
                CollaborationEvent::DelegationCreated {
                    delegation, worker, ..
                } => Some((delegation.clone(), worker.clone())),
                _ => None,
            })
            .collect();
        let mut arrived = false;
        for record in records {
            if self.observed.contains(&record.id) {
                continue;
            }
            let drawn = super::projection::entries(
                record,
                &|endpoint| self.name(endpoint),
                &|delegation| workers.get(delegation).cloned(),
            );
            if drawn.is_empty() && !matches!(record.event, CollaborationEvent::TurnAdmitted { .. })
            {
                continue;
            }
            arrived |= drawn.iter().any(|(owner, event)| {
                owner == &root.agent && matches!(event, ConversationEvent::MailDelivered { .. })
            });
            for (owner, event) in &drawn {
                if item_of(event).is_some_and(|item| !self.shown.contains(&item)) {
                    self.pending_placement.insert(owner.clone());
                }
            }
            self.observed.insert(record.id.clone());
        }
        arrived
    }
}
