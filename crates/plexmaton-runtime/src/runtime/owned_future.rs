//! One cancellation-safe future whose completed result survives a cancelled outer poll.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{FutureExt as _, future::BoxFuture};

pub(super) struct RetainedFuture<T: Clone + Send + Unpin + 'static> {
    future: Option<BoxFuture<'static, Result<T, ()>>>,
    result: Option<Result<T, ()>>,
}

impl<T: Clone + Send + Unpin + 'static> RetainedFuture<T> {
    pub(super) fn new(future: BoxFuture<'static, T>) -> Self {
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

impl<T: Clone + Send + Unpin + 'static> Future for RetainedFuture<T> {
    type Output = Result<T, ()>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = &this.result {
            return Poll::Ready(result.clone());
        }
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(Err(()));
        };
        match future.as_mut().poll(context) {
            Poll::Ready(result) => {
                this.future = None;
                this.result = Some(result.clone());
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
