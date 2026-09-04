//! Durable commit ordering between pure agent transitions and their outside effects.

use plexmaton_agent::{Effect, Input, Reaction, UndeliveredInput, UndeliveredReason};

use super::{
    LiveRuntime,
    journal::{CommitError, CommitReply, JournalWriterError, finish_commit},
    rejected_user_input,
};
use crate::{PersistenceFailure, RuntimeError};

pub(super) struct PendingCommit {
    reaction: Reaction,
    reply: CommitReply,
    rejected_inputs: Vec<UndeliveredInput>,
    after: AfterCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AfterCommit {
    None,
    Interrupt,
    Shutdown,
}

enum CommitFailure {
    Store(CommitError),
    Writer,
}

pub(super) fn merge_reaction(target: &mut Reaction, mut next: Reaction) {
    target.records.append(&mut next.records);
    target.released_inputs.append(&mut next.released_inputs);
    target.events.append(&mut next.events);
    target.effects.append(&mut next.effects);
    target.undelivered.append(&mut next.undelivered);
    target
        .unresolved_approvals
        .append(&mut next.unresolved_approvals);
    target.undelivered_model.append(&mut next.undelivered_model);
}

fn failure_inputs(
    reaction: &Reaction,
    rejected_input: Option<UndeliveredInput>,
) -> Vec<UndeliveredInput> {
    let mut released: Vec<_> = reaction.released_inputs.iter().collect();
    released.sort_by_key(|input| input.order());
    let direct = rejected_input.into_iter();
    let claimed = released.into_iter().map(|input| UndeliveredInput {
        text: input.text().to_owned(),
        reason: UndeliveredReason::PersistenceFailed,
    });
    direct.chain(claimed).collect()
}

impl LiveRuntime {
    pub(super) async fn apply_agent_input(
        &mut self,
        input: Input,
        rejected_input: Option<UndeliveredInput>,
        after: AfterCommit,
    ) -> Result<(), RuntimeError> {
        let reaction = self.agent.handle_at(input, self.clock.now());
        let rejected_inputs = failure_inputs(&reaction, rejected_input);
        self.begin_transition(reaction, rejected_inputs, after)?;
        self.finish_transition().await
    }

    pub(super) async fn apply_agent_input_during_join(
        &mut self,
        input: Input,
    ) -> Result<(), RuntimeError> {
        let reaction = self.agent.handle_at(input, self.clock.now());
        let rejected_inputs = failure_inputs(&reaction, None);
        self.begin_transition(reaction, rejected_inputs, AfterCommit::None)?;
        self.finish_pending_transition().await
    }

    pub(super) async fn apply_agent_input_after_usage(
        &mut self,
        input: Input,
        rejected_input: Option<UndeliveredInput>,
        after: AfterCommit,
    ) -> Result<(), RuntimeError> {
        let mut reaction = Reaction::default();
        let observed_at = self.clock.now();
        self.stage_missing_usage_at(&mut reaction, observed_at);
        merge_reaction(&mut reaction, self.agent.handle_at(input, observed_at));
        let rejected_inputs = failure_inputs(&reaction, rejected_input);
        self.begin_transition(reaction, rejected_inputs, after)?;
        self.finish_transition().await
    }

    pub(super) async fn finish_pending_inputs(&mut self) -> Result<(), RuntimeError> {
        self.finish_transition().await?;
        while !self.journal_failed {
            let Some(pending) = self.pending_inputs.pop_front() else {
                break;
            };
            let reaction = if matches!(&pending.input, Input::Interrupted) {
                let mut reaction = Reaction::default();
                self.stage_missing_usage_at(&mut reaction, pending.observed_at);
                merge_reaction(
                    &mut reaction,
                    self.agent.handle_at(pending.input, pending.observed_at),
                );
                reaction
            } else {
                self.agent.handle_at(pending.input, pending.observed_at)
            };
            let rejected_inputs = failure_inputs(&reaction, pending.rejected_input);
            self.begin_transition(reaction, rejected_inputs, pending.after)?;
            self.finish_transition().await?;
        }
        if self.journal_failed {
            while let Some(pending) = self.pending_inputs.pop_front() {
                if let Some(input) =
                    rejected_user_input(&pending.input, UndeliveredReason::PersistenceFailed)
                {
                    self.report.undelivered.push(input);
                    self.report.persistence_failure = Some(PersistenceFailure::NotWritten);
                }
            }
        }
        Ok(())
    }

    pub(super) fn begin_transition(
        &mut self,
        mut reaction: Reaction,
        rejected_inputs: Vec<UndeliveredInput>,
        after: AfterCommit,
    ) -> Result<(), RuntimeError> {
        if self.pending_commit.is_some()
            || (after != AfterCommit::None && self.after_commit.is_some())
        {
            return self.fail_before_queue(rejected_inputs);
        }
        if self.journal_failed {
            return Err(RuntimeError::JournalRequiresReopen);
        }
        if let Some(writer) = &self.journal
            && !reaction.records.is_empty()
        {
            let records = std::mem::take(&mut reaction.records);
            let reply = match writer.begin_append(records) {
                Ok(reply) => reply,
                Err(_) => return self.fail_before_queue(rejected_inputs),
            };
            self.pending_commit = Some(PendingCommit {
                reaction,
                reply,
                rejected_inputs,
                after,
            });
            return Ok(());
        }
        self.apply_ready_reaction(reaction)?;
        if after != AfterCommit::None {
            self.after_commit = Some(after);
        }
        Ok(())
    }

    fn fail_before_queue(
        &mut self,
        rejected_inputs: Vec<UndeliveredInput>,
    ) -> Result<(), RuntimeError> {
        self.journal_failed = true;
        if rejected_inputs.is_empty() {
            return Err(RuntimeError::JournalWriterUnavailable);
        }
        self.report.undelivered.extend(rejected_inputs);
        self.report.persistence_failure = Some(PersistenceFailure::NotWritten);
        Ok(())
    }

    pub(super) async fn finish_transition(&mut self) -> Result<(), RuntimeError> {
        self.finish_pending_transition().await?;
        self.finish_after_commit().await
    }

    async fn finish_pending_transition(&mut self) -> Result<(), RuntimeError> {
        if let Some(pending) = &mut self.pending_commit {
            let result = match finish_commit(&mut pending.reply).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(CommitFailure::Store(error)),
                Err(JournalWriterError::QueueFull)
                | Err(JournalWriterError::Stopped)
                | Err(JournalWriterError::TaskFailed) => Err(CommitFailure::Writer),
            };
            let pending = self
                .pending_commit
                .take()
                .unwrap_or_else(|| unreachable!("the awaited commit remains owned"));
            match result {
                Ok(()) => {
                    self.apply_ready_reaction(pending.reaction)?;
                    if pending.after != AfterCommit::None {
                        self.after_commit = Some(pending.after);
                    }
                }
                Err(failure) => {
                    self.journal_failed = true;
                    if !pending.rejected_inputs.is_empty() {
                        self.report.undelivered.extend(pending.rejected_inputs);
                        self.report.persistence_failure = Some(match &failure {
                            CommitFailure::Store(error) if !error.outcome_unknown => {
                                PersistenceFailure::NotWritten
                            }
                            CommitFailure::Store(_) | CommitFailure::Writer => {
                                PersistenceFailure::OutcomeUnknown
                            }
                        });
                        return Ok(());
                    }
                    return Err(match failure {
                        CommitFailure::Store(error) => RuntimeError::JournalAppendFailed {
                            source: error.source,
                        },
                        CommitFailure::Writer => RuntimeError::JournalWriterUnavailable,
                    });
                }
            }
        }
        Ok(())
    }

    async fn finish_after_commit(&mut self) -> Result<(), RuntimeError> {
        let Some(after) = self.after_commit else {
            return Ok(());
        };
        match after {
            AfterCommit::None => {}
            AfterCommit::Interrupt | AfterCommit::Shutdown => {
                self.cancel_active().await?;
                self.tools.cancel_and_join().await?;
            }
        }
        self.after_commit = None;
        Ok(())
    }

    pub(super) fn apply_ready_reaction(
        &mut self,
        mut reaction: Reaction,
    ) -> Result<(), RuntimeError> {
        reaction.records.clear();
        reaction.released_inputs.clear();
        self.pending.extend(reaction.events);
        self.report.undelivered.append(&mut reaction.undelivered);
        self.report
            .unresolved_approvals
            .append(&mut reaction.unresolved_approvals);
        self.report
            .undelivered_model
            .append(&mut reaction.undelivered_model);
        for effect in reaction.effects {
            match effect {
                Effect::CallModel(call) => self.spawn_model(call)?,
                Effect::AdmitTool(request) => self.tools.start_admission(request)?,
                Effect::RunTool(call) => self.tools.start_execution(call)?,
            }
        }
        Ok(())
    }
}
