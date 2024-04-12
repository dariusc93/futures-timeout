use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::future::FusedFuture;
use futures::stream::FusedStream;
use futures::{Future, FutureExt, Stream};
use futures_timer::Delay;
use pin_project::pin_project;

pub trait TimeoutExt: Sized {
    /// Requires a [`Future`] or [`Stream`] to complete before the specific duration has elapsed.
    ///
    /// **Note: If a [`Stream`] returns an item, the timer will reset until `Poll::Ready(None)` is returned**
    fn timeout(self, duration: Duration) -> Timeout<Self> {
        Timeout {
            inner: self,
            timer: Some(Delay::new(duration)),
            duration,
        }
    }
}

impl<T: Sized> TimeoutExt for T {}

#[derive(Debug)]
#[pin_project]
pub struct Timeout<T> {
    #[pin]
    inner: T,
    timer: Option<Delay>,
    duration: Duration,
}

impl<T: Future> Future for Timeout<T> {
    type Output = io::Result<T::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if let Some(timer) = this.timer.as_mut() {
            if timer.poll_unpin(cx).is_ready() {
                return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
            }
        }

        let item = futures::ready!(this.inner.poll(cx).map(Ok));
        this.timer.take();
        Poll::Ready(item)
    }
}

impl<T: Future> FusedFuture for Timeout<T> {
    fn is_terminated(&self) -> bool {
        self.timer.is_none()
    }
}

impl<T: Stream> Stream for Timeout<T> {
    type Item = io::Result<T::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if let Some(timer) = this.timer.as_mut() {
            if timer.poll_unpin(cx).is_ready() {
                return Poll::Ready(Some(Err(io::ErrorKind::TimedOut.into())));
            }
        }

        match futures::ready!(this.inner.poll_next(cx)) {
            Some(item) => {
                if let Some(timer) = this.timer.as_mut() {
                    timer.reset(*this.duration);
                }
                Poll::Ready(Some(Ok(item)))
            }
            None => {
                this.timer.take();
                Poll::Ready(None)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T: Stream> FusedStream for Timeout<T> {
    fn is_terminated(&self) -> bool {
        self.timer.is_none()
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::TimeoutExt;

    #[tokio::test]
    async fn fut_timeout() {
        tokio::time::sleep(Duration::from_secs(15))
            .timeout(Duration::from_secs(10))
            .await
            .expect_err("error");
    }

    #[tokio::test]
    async fn stream_timeout() {
        tokio::time::sleep(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .await
            .expect("error")
    }
}
