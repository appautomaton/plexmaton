//! One bounded blocking owner for a session journal file.

use std::thread::{self, JoinHandle};

use plexmaton_agent::JournalRecord;
use plexmaton_session_store::{JournalFile, StoreError};
use tokio::sync::{mpsc, oneshot};

const COMMAND_CAPACITY: usize = 1;

pub(super) trait JournalStore: Send + 'static {
    fn append(&mut self, record: JournalRecord) -> Result<(), StoreError>;
}

impl JournalStore for JournalFile {
    fn append(&mut self, record: JournalRecord) -> Result<(), StoreError> {
        JournalFile::append(self, record).map_err(|failure| failure.into_parts().0)
    }
}

pub(super) type CommitReply = oneshot::Receiver<Result<(), CommitError>>;

pub(super) struct CommitError {
    pub(super) source: StoreError,
    pub(super) outcome_unknown: bool,
}

enum Command {
    Append {
        records: Vec<JournalRecord>,
        reply: oneshot::Sender<Result<(), CommitError>>,
    },
}

pub(super) struct JournalWriter {
    sender: Option<mpsc::Sender<Command>>,
    worker: Option<JoinHandle<()>>,
    finished: Option<oneshot::Receiver<()>>,
}

impl JournalWriter {
    pub(super) fn spawn(store: Box<dyn JournalStore>) -> Result<Self, JournalWriterError> {
        let (sender, mut receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (finished_tx, finished) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("plexmaton-journal".to_owned())
            .stack_size(512 * 1024)
            .spawn(move || {
                let mut store = store;
                while let Some(Command::Append { records, reply }) = receiver.blocking_recv() {
                    let mut result = Ok(());
                    for (index, record) in records.into_iter().enumerate() {
                        if let Err(error) = store.append(record) {
                            result = Err(CommitError {
                                outcome_unknown: index != 0 || append_outcome_unknown(&error),
                                source: error,
                            });
                            break;
                        }
                    }
                    let _caller_cancelled = reply.send(result).is_err();
                }
                let _owner_dropped = finished_tx.send(()).is_err();
            })
            .map_err(|_| JournalWriterError::TaskFailed)?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            finished: Some(finished),
        })
    }

    pub(super) fn begin_append(
        &self,
        records: Vec<JournalRecord>,
    ) -> Result<CommitReply, JournalWriterError> {
        let (reply, receiver) = oneshot::channel();
        let command = Command::Append { records, reply };
        match self
            .sender
            .as_ref()
            .ok_or(JournalWriterError::Stopped)?
            .try_send(command)
        {
            Ok(()) => Ok(receiver),
            Err(mpsc::error::TrySendError::Full(_)) => Err(JournalWriterError::QueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(JournalWriterError::Stopped),
        }
    }

    pub(super) async fn shutdown(&mut self) -> Result<(), JournalWriterError> {
        self.sender.take();
        let finished = match &mut self.finished {
            Some(finished) => (&mut *finished).await.is_ok(),
            None => true,
        };
        self.finished = None;
        let joined = self
            .worker
            .take()
            .is_none_or(|worker| worker.join().is_ok());
        if finished && joined {
            Ok(())
        } else {
            Err(JournalWriterError::TaskFailed)
        }
    }
}

impl Drop for JournalWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _worker_failed = worker.join().is_err();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum JournalWriterError {
    QueueFull,
    Stopped,
    TaskFailed,
}

pub(super) async fn finish_commit(
    reply: &mut CommitReply,
) -> Result<Result<(), CommitError>, JournalWriterError> {
    reply.await.map_err(|_| JournalWriterError::TaskFailed)
}

fn append_outcome_unknown(error: &StoreError) -> bool {
    matches!(error, StoreError::Io { .. })
}
