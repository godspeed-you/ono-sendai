//! The audit trail (spec §31.37) and the host clock it stamps records with.
//!
//! Timestamps are the host's, never the plugin's — a package cannot backdate its own trail.
//! Under the test host the clock is fixed, which is what makes plugin tests deterministic
//! (spec §31.73's virtual time).

use std::sync::{Arc, Mutex};

use ono_kuang_protocol::{AuditEvent, AuditResult, Enforcement, WireError};
use serde_json::Value as Json;

/// The host's clock. Fixed under the test host, system time otherwise.
#[derive(Debug, Clone)]
pub enum HostClock {
    /// Real wall-clock time.
    System,
    /// A fixed instant, for deterministic tests (spec §31.73).
    Fixed(String),
}

impl HostClock {
    /// The current instant as an RFC 3339 timestamp.
    #[must_use]
    pub fn now(&self) -> String {
        match self {
            HostClock::System => jiff::Timestamp::now().to_string(),
            HostClock::Fixed(instant) => instant.clone(),
        }
    }
}

/// The append-only audit trail one plugin instance accumulates.
///
/// Denials are recorded as loudly as successes: a package probing for capabilities it does not
/// hold is exactly what the trail is for (spec §31.37).
#[derive(Debug, Clone, Default)]
pub struct AuditTrail {
    events: Arc<Mutex<Vec<AuditEvent>>>,
    counter: Arc<Mutex<u64>>,
}

impl AuditTrail {
    /// An empty trail.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one record, minting its identity. Attribution and timestamp are the caller's —
    /// the supervisor's, never the plugin's.
    #[allow(
        clippy::too_many_arguments,
        reason = "the audit record simply has this many parts"
    )]
    pub fn record(
        &self,
        plugin: &str,
        invocation: &str,
        capability: &str,
        scope: Option<Json>,
        enforcement: Enforcement,
        action: &str,
        target: Option<Json>,
        at: String,
        result: AuditResult,
        error: Option<WireError>,
    ) {
        let id = {
            let mut counter = match self.counter.lock() {
                Ok(counter) => counter,
                Err(poisoned) => poisoned.into_inner(),
            };
            *counter += 1;
            // A stable v4-shaped identity derived from the sequence, so the trail is
            // deterministic under the test host.
            format!("00000000-0000-4000-8000-{:012x}", *counter)
        };
        let event = AuditEvent {
            id,
            plugin: plugin.to_owned(),
            invocation: invocation.to_owned(),
            capability: capability.to_owned(),
            scope,
            enforcement,
            action: action.to_owned(),
            target,
            at,
            result,
            user_confirmation: None,
            lease: None,
            link: None,
            error,
        };
        match self.events.lock() {
            Ok(mut events) => events.push(event),
            Err(poisoned) => poisoned.into_inner().push(event),
        }
    }

    /// A snapshot of every record so far, in order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<AuditEvent> {
        match self.events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}
