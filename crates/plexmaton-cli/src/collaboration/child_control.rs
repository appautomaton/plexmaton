//! Authenticated child-controller projection and focused post-Handoff input routing.

use plexmaton_agent::{Input, UndeliveredInput, UndeliveredReason};
use plexmaton_core::AgentStatus;
use plexmaton_runtime::{
    DispatchReport, LiveRuntime, OwnedChildControl, OwnedChildControlSnapshot, UserInputRefusal,
    UserTargetInputRequest,
};

use super::Collaboration;
use crate::input::AddressedInput;

impl Collaboration {
    /// Rebuilds roster targets and their current durable controller state.
    pub(super) async fn sync_roster(
        &mut self,
        runtime: &mut LiveRuntime,
        status: AgentStatus,
    ) -> anyhow::Result<()> {
        let targets = self
            .owner
            .register_collaboration_targets()
            .await
            .map_err(|error| anyhow::anyhow!("register delegated targets: {error}"))?;
        let mut registered = Vec::with_capacity(targets.len());
        for target in targets {
            let user_target = target.user_input_target();
            let snapshot = self
                .owner
                .child_control_snapshot(&user_target)
                .await
                .map_err(|error| anyhow::anyhow!("read delegated controller: {error}"))?;
            registered.push((target.worker().conversation.clone(), user_target, snapshot));
        }
        for (child, target, snapshot) in registered {
            self.announce(runtime, child, status)?;
            let conversation = target.worker().conversation.clone();
            self.user_targets.insert(conversation, target);
            self.retain_control_snapshot(snapshot)?;
        }
        Ok(())
    }

    /// Refreshes controller revisions without changing roster lifecycle presentation.
    pub(super) async fn refresh_child_controls(&mut self) -> anyhow::Result<()> {
        let targets: Vec<_> = self.user_targets.values().cloned().collect();
        for target in targets {
            let snapshot = self
                .owner
                .child_control_snapshot(&target)
                .await
                .map_err(|error| anyhow::anyhow!("read delegated controller: {error}"))?;
            self.retain_control_snapshot(snapshot)?;
        }
        Ok(())
    }

    /// Applies authenticated display snapshots only after their roster agents exist (CCV-1).
    pub(crate) fn apply_child_controls(
        &self,
        workspace: &mut plexmaton_tui::Workspace,
    ) -> anyhow::Result<()> {
        for (conversation, snapshot) in &self.controls {
            let Some(agent) = self.announced.get(conversation) else {
                continue;
            };
            if workspace.state().agent(agent).is_none() {
                continue;
            }
            let control = match snapshot.control() {
                OwnedChildControl::Main => plexmaton_tui::ChildControl::Main,
                OwnedChildControl::HandoffPending => plexmaton_tui::ChildControl::HandoffPending,
                OwnedChildControl::User => plexmaton_tui::ChildControl::User,
            };
            workspace
                .set_child_control(
                    agent,
                    plexmaton_tui::ChildControlSnapshot {
                        revision: snapshot.revision(),
                        control,
                    },
                )
                .map_err(|error| anyhow::anyhow!("project delegated controller: {error}"))?;
        }
        Ok(())
    }

    pub(super) fn retain_control_snapshot(
        &mut self,
        snapshot: OwnedChildControlSnapshot,
    ) -> anyhow::Result<bool> {
        let conversation = snapshot.worker().conversation.clone();
        if let Some(previous) = self.controls.get(&conversation) {
            anyhow::ensure!(
                snapshot.revision() >= previous.revision(),
                "authenticated child control revision moved backward"
            );
            anyhow::ensure!(
                snapshot.revision() != previous.revision()
                    || snapshot.control() == previous.control(),
                "authenticated child control conflicted at one revision"
            );
            if snapshot == *previous {
                return Ok(false);
            }
        }
        self.controls.insert(conversation, snapshot);
        Ok(true)
    }

    /// Admits one visible child submission to the authenticated owner and leaves its settlement
    /// on the owner activity stream so provider/journal progress cannot hold the terminal loop.
    pub(crate) fn dispatch_child_input(&mut self, addressed: AddressedInput) -> DispatchReport {
        let target = self
            .announced
            .iter()
            .find_map(|(conversation, agent)| {
                (agent == &addressed.to).then(|| {
                    self.user_targets.get(conversation).cloned().zip(
                        self.controls
                            .get(conversation)
                            .map(|snapshot| snapshot.control()),
                    )
                })
            })
            .flatten();
        let Some((target, control)) = target else {
            return returned_child_input(addressed, UndeliveredReason::ControlUnavailable);
        };
        if control != OwnedChildControl::User {
            return returned_child_input(addressed, UndeliveredReason::ControlledByMain);
        }
        let request = UserTargetInputRequest::new(target, addressed.input, addressed.skill);
        match self.owner.begin_user_target_input(request) {
            Ok(()) => DispatchReport::default(),
            Err(failure) => {
                let reason = undelivered_reason(failure.reason());
                let mut report = DispatchReport::default();
                if let Some(input) = failure.into_undelivered(reason) {
                    report.undelivered.push(input);
                }
                report
            }
        }
    }
}

pub(crate) fn undelivered_reason(refusal: &UserInputRefusal) -> UndeliveredReason {
    match refusal {
        UserInputRefusal::ControlledByMain => UndeliveredReason::ControlledByMain,
        UserInputRefusal::ShuttingDown => UndeliveredReason::Shutdown,
        UserInputRefusal::InProgress => UndeliveredReason::QueueFull,
        UserInputRefusal::Interrupted => UndeliveredReason::Interrupted,
        UserInputRefusal::StaleTarget
        | UserInputRefusal::StaleTicket
        | UserInputRefusal::RequiresReopen
        | UserInputRefusal::UnsupportedInput
        | UserInputRefusal::Writer(_)
        | UserInputRefusal::Activation(_)
        | UserInputRefusal::Runner(_) => UndeliveredReason::ControlUnavailable,
    }
}

fn returned_child_input(addressed: AddressedInput, reason: UndeliveredReason) -> DispatchReport {
    let text = match addressed.input {
        Input::Submitted { text } | Input::Steered { text } => text,
        _ => return DispatchReport::default(),
    };
    let mut report = DispatchReport::default();
    report
        .undelivered
        .push(UndeliveredInput::with_skill(text, addressed.skill, reason));
    report
}
