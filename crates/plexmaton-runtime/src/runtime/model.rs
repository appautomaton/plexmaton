//! Provider-operation future retained across cancellation of an event poll.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{FutureExt as _, future::BoxFuture};
use plexmaton_agent::{ModelCall, ModelError, ModelEvent, ModelStepId};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) trait ModelDriver: Send + Sync + 'static {
    fn drive(
        &self,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ()>;
}

#[derive(Debug)]
pub(crate) enum ModelSignal {
    Event {
        step_id: ModelStepId,
        event: ModelEvent,
    },
    Terminal {
        step_id: ModelStepId,
        event: ModelEvent,
    },
    Failed {
        step_id: ModelStepId,
        error: ModelError,
    },
}

/// A completed future may be observed by a cancelled outer poll and awaited again during cleanup.
pub(super) struct RetainedModelFuture {
    future: Option<BoxFuture<'static, Result<(), ()>>>,
    result: Option<Result<(), ()>>,
}

impl RetainedModelFuture {
    pub(super) fn new(future: BoxFuture<'static, ()>) -> Self {
        let future = AssertUnwindSafe(future)
            .catch_unwind()
            .map(|result| result.map_err(|_| ()))
            .boxed();
        Self {
            future: Some(future),
            result: None,
        }
    }
}

impl Future for RetainedModelFuture {
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = this.result {
            return Poll::Ready(result);
        }
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(Err(()));
        };
        match future.as_mut().poll(context) {
            Poll::Ready(result) => {
                this.future = None;
                this.result = Some(result);
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
