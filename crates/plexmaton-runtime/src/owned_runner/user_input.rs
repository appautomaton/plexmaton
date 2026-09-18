//! Dedicated bounded lane for product-owned child input.

use super::*;

pub(super) const USER_INPUT_CAPACITY: usize = 1;

pub(super) enum UserInputCommand {
    Submit {
        input: Box<Input>,
        selected_skill: Option<String>,
        #[cfg(test)]
        fail: bool,
        reply: oneshot::Sender<Result<DispatchReport, RuntimeError>>,
    },
    Approval {
        approval_id: plexmaton_core::ApprovalId,
        decision: plexmaton_core::ApprovalDecision,
        reply: oneshot::Sender<Result<DispatchReport, RuntimeError>>,
    },
    #[cfg(test)]
    Hold {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    },
}

impl OwnedChildRunner {
    /// Admits one product-owned input into its dedicated bounded lane.
    pub(crate) fn begin_user_input(
        &mut self,
        input: Input,
        selected_skill: Option<String>,
    ) -> Result<(), OwnedRunnerError> {
        if self.user_input_reply.is_some() {
            return Err(OwnedRunnerError::UserInputBusy);
        }
        let (reply, result) = oneshot::channel();
        let sender = self.user_input.as_ref().ok_or(OwnedRunnerError::Closed)?;
        match sender.try_send(UserInputCommand::Submit {
            input: Box::new(input),
            selected_skill,
            #[cfg(test)]
            fail: std::mem::take(&mut self.fail_next_user_input),
            reply,
        }) {
            Ok(()) => {
                self.user_input_reply = Some(result);
                #[cfg(test)]
                if let Some(admitted) = self.next_user_input_admitted.take() {
                    admitted.notify_one();
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(OwnedRunnerError::UserInputBusy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(OwnedRunnerError::Closed);
            }
        }
        Ok(())
    }

    /// Admits one owner-authenticated approval on the same capacity-one lane as direct input.
    pub(crate) fn begin_attention_decision(
        &mut self,
        approval_id: plexmaton_core::ApprovalId,
        decision: plexmaton_core::ApprovalDecision,
    ) -> Result<(), OwnedRunnerError> {
        if self.user_input_reply.is_some() {
            return Err(OwnedRunnerError::UserInputBusy);
        }
        let (reply, result) = oneshot::channel();
        let sender = self.user_input.as_ref().ok_or(OwnedRunnerError::Closed)?;
        match sender.try_send(UserInputCommand::Approval {
            approval_id,
            decision,
            reply,
        }) {
            Ok(()) => self.user_input_reply = Some(result),
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(OwnedRunnerError::UserInputBusy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(OwnedRunnerError::Closed);
            }
        }
        Ok(())
    }

    /// Finishes one accepted user input; cancellation leaves its result available for retry.
    pub(crate) async fn finish_user_input(&mut self) -> Result<DispatchReport, OwnedRunnerError> {
        let result = self
            .user_input_reply
            .as_mut()
            .ok_or(OwnedRunnerError::WorkerFailed)?;
        let outcome = match (&mut *result).await {
            Ok(result) => result.map_err(OwnedRunnerError::from),
            Err(_) => Err(OwnedRunnerError::WorkerFailed),
        };
        self.user_input_reply.take();
        outcome
    }

    #[cfg(test)]
    pub(crate) fn fail_next_user_input_for_test(&mut self) {
        self.fail_next_user_input = true;
    }

    #[cfg(test)]
    pub(crate) fn notify_next_user_input_admission_for_test(&mut self) -> Arc<Notify> {
        let admitted = Arc::new(Notify::new());
        self.next_user_input_admitted = Some(Arc::clone(&admitted));
        admitted
    }

    #[cfg(test)]
    pub(crate) async fn hold_user_input_for_test(
        &self,
    ) -> Result<(Arc<Notify>, Arc<Notify>), OwnedRunnerError> {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.user_input
            .as_ref()
            .ok_or(OwnedRunnerError::Closed)?
            .send(UserInputCommand::Hold {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            })
            .await
            .map_err(|_| OwnedRunnerError::Closed)?;
        entered.notified().await;
        Ok((entered, release))
    }
}
