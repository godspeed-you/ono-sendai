//! A bounded stream of object events.
//!
//! Spec §31.15 requires bounded event queues, because a subscription to every socket event can
//! otherwise stall or exhaust the shell. The channel is bounded for the same reason the value
//! channel is (ADR-0013): a producer that outruns its consumer must be made to wait, not to
//! allocate.

use ono_pipeline::{CancelToken, PipelineConfig};
use tokio::sync::mpsc;

use crate::ObjectEvent;

/// Where a provider puts the events it observes.
#[derive(Debug, Clone)]
pub struct EventSink {
    sender: mpsc::Sender<ObjectEvent>,
    cancel: CancelToken,
}

impl EventSink {
    /// Sends one event, waiting while the consumer is behind.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the consumer has gone away or the stream was cancelled, which is the
    /// signal for the provider to stop observing.
    pub async fn send(&self, event: ObjectEvent) -> Result<(), Closed> {
        if self.cancel.is_cancelled() {
            return Err(Closed);
        }
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(Closed),
            outcome = self.sender.send(event) => outcome.map_err(|_| Closed),
        }
    }

    /// Whether the stream has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// The consumer's end of a subscription.
#[derive(Debug)]
pub struct EventStream {
    receiver: mpsc::Receiver<ObjectEvent>,
    cancel: CancelToken,
}

impl EventStream {
    /// Runs `producer`, feeding the events it observes into a bounded channel.
    pub fn spawn<F, Fut>(config: PipelineConfig, producer: F) -> Self
    where
        F: FnOnce(EventSink) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel(config.capacity());
        let cancel = config.cancel_token().clone();
        let sink = EventSink {
            sender,
            cancel: cancel.clone(),
        };
        tokio::spawn(producer(sink));
        Self { receiver, cancel }
    }

    /// The next event, or `None` when the provider has finished or the stream was cancelled.
    pub async fn recv(&mut self) -> Option<ObjectEvent> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => None,
            event = self.receiver.recv() => event,
        }
    }

    /// Stops the subscription. The provider observes this at its next send.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// The token this stream is cancelled through, so a caller can join it to a pipeline's.
    #[must_use]
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }
}

/// The stream is no longer being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the event stream is no longer being read")
    }
}

impl std::error::Error for Closed {}
