//! Cooperative cancellation for a native pipeline (spec §18.5, ADR-0013).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// A cancellation scope shared by every stage of one pipeline.
///
/// Ctrl-C, a `try` block unwinding, or a consumer that has seen enough all trip the same token,
/// and every stage selects its channel against it. That is what makes a native pipeline stop at
/// its next await rather than at the end of its current producer (spec §18.5).
///
/// Cloning shares the scope; the clones are the same token, not copies of it.
///
/// ```
/// use ono_pipeline::CancelToken;
/// let token = CancelToken::new();
/// let watcher = token.clone();
/// assert!(!watcher.is_cancelled());
/// token.cancel();
/// assert!(watcher.is_cancelled());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    /// Whether the one `stream.cancelled` event owed to the consumer has been handed over.
    /// It lives here rather than on the stream so that a pipeline of ten stages still reports
    /// one cancellation, not ten.
    reported: AtomicBool,
    wake: Notify,
}

impl CancelToken {
    /// A fresh, uncancelled scope.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels the scope. Idempotent: cancelling twice is cancelling once.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.wake.notify_waiters();
    }

    /// Whether the scope has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves as soon as the scope is cancelled, and never otherwise.
    ///
    /// Safe to `select!` on repeatedly: the waiter registers before it re-checks the flag, so a
    /// cancellation racing with the registration cannot be missed.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let wait = self.inner.wake.notified();
        tokio::pin!(wait);
        wait.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        wait.await;
    }

    /// Claims the single cancellation report owed to the pipeline's consumer.
    ///
    /// Returns `true` exactly once, and only for a scope that was actually cancelled.
    pub(crate) fn claim_report(&self) -> bool {
        self.is_cancelled()
            && self
                .inner
                .reported
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }
}
