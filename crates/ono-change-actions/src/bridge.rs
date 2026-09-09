//! Running one provider call from a synchronous change provider (spec §51).
//!
//! §51's `ChangeProvider` is synchronous, and [`ono_provider_api::Provider::act`] is `async`
//! because a mutation talks to systemd, netlink or a container engine over a socket. Something
//! has to join the two, and `ono-cli` already does it the same way everywhere it crosses that
//! boundary: it holds a Tokio runtime and blocks on the future (`session.rs`, `complete.rs`).
//!
//! The join is a trait rather than a hard-wired `Handle::block_on` for one reason: a test must be
//! able to state what the outside world did without starting a runtime and without a clock. The
//! production implementation is [`RuntimeBridge`], and a test supplies its own.

use ono_pipeline::{StreamEvent, ValueStream};
use ono_provider_api::{Action, ActionOutcome, Provider, Query};
use ono_value::{ErrorValue, Value};

/// Carries one provider call to completion from a synchronous caller (§51).
pub trait Bridge: Send + Sync + std::fmt::Debug {
    /// Performs `action` and reports what the provider said (§4.7).
    ///
    /// # Errors
    ///
    /// Whatever the provider could not attempt at all. An action that was attempted and failed is
    /// an [`ActionOutcome`], not an error, because §16.5 forbids collapsing per-target results.
    fn act(&self, provider: &dyn Provider, action: &Action) -> Result<ActionOutcome, ErrorValue>;

    /// Reads the objects matching `query` back, for §23.1's provider-level acknowledgement.
    ///
    /// # Errors
    ///
    /// Whatever the provider could not answer. A per-object failure on the stream's error
    /// channel is returned as an error too: a verification that saw half the answer has not
    /// observed the object, and §23.5 forbids reading a partial answer as a pass.
    fn snapshot(&self, provider: &dyn Provider, query: &Query)
    -> Result<Vec<Value>, ErrorValue>;
}

/// The bridge `ono-cli` supplies: a Tokio runtime handle, blocked on (§51).
///
/// `block_on` requires that the caller is not already inside a runtime worker thread. That is
/// where `ono-cli` calls a change provider from — the interpreter thread, outside the runtime it
/// owns — and the same constraint the existing `runtime.block_on` call sites live under.
#[derive(Debug, Clone)]
pub struct RuntimeBridge {
    handle: tokio::runtime::Handle,
}

impl RuntimeBridge {
    /// Bridges through `handle`.
    #[must_use]
    pub const fn new(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }
}

impl Bridge for RuntimeBridge {
    fn act(&self, provider: &dyn Provider, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.handle.block_on(provider.act(action))
    }

    fn snapshot(
        &self,
        provider: &dyn Provider,
        query: &Query,
    ) -> Result<Vec<Value>, ErrorValue> {
        let stream = self.handle.block_on(async { provider.snapshot(query) })?;
        self.handle.block_on(drain(stream))
    }
}

/// Collects a value stream, and refuses on the first per-item failure.
///
/// # Errors
///
/// The first failure the stream carried. §23.3 distinguishes a check that could not answer from
/// one that failed, and a stream that reported an error has not answered.
pub async fn drain(mut stream: ValueStream) -> Result<Vec<Value>, ErrorValue> {
    let mut values = Vec::new();
    while let Some(event) = stream.recv().await {
        match event {
            StreamEvent::Value(value) => values.push(value),
            StreamEvent::Failure(error) => return Err(error),
        }
    }
    Ok(values)
}
