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
    /// Which trail this is, mixed into every identity it mints.
    ///
    /// `ono.plugin-audit-event/1` identifies an event by `id`, and a host assembles one stream
    /// out of its own events and every instance's. Two counters that both start at one would
    /// mint two events claiming to be the same one, so the source is part of the identity.
    source: u32,
    /// Which *run* of that source this is, mixed into every identity as well.
    ///
    /// A trail persisted across sessions (spec §31.33) holds events from many processes, and a
    /// counter that restarts at one in every process would mint, in the second session, the
    /// same ids the first already wrote — and the persistence step, which keeps an event once,
    /// would drop the second session's events on the floor. The nonce is minted when the trail
    /// is, from the process and the clock, so two runs of one package never share an id.
    session: u32,
}

impl AuditTrail {
    /// An empty trail with no source of its own.
    #[must_use]
    pub fn new() -> Self {
        Self {
            session: session_nonce(),
            ..Self::default()
        }
    }

    /// An empty trail whose identities are distinguishable from every other source's, and from
    /// every earlier run of the same source.
    #[must_use]
    pub fn for_source(source: &str) -> Self {
        Self {
            source: fnv(source),
            session: session_nonce(),
            ..Self::default()
        }
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
        self.record_correlated(
            plugin,
            invocation,
            capability,
            scope,
            enforcement,
            action,
            target,
            at,
            result,
            error,
            None,
        );
    }

    /// [`Self::record`], stamped with the request id one user action minted (ADR-0604 §5).
    #[allow(
        clippy::too_many_arguments,
        reason = "the audit record simply has this many parts"
    )]
    pub fn record_correlated(
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
        correlation: Option<String>,
    ) {
        let id = {
            let mut counter = match self.counter.lock() {
                Ok(counter) => counter,
                Err(poisoned) => poisoned.into_inner(),
            };
            *counter += 1;
            // A v4-shaped identity derived from the source, the run and the sequence: unique
            // across sources, across the sessions one persisted trail gathers, and in order
            // within one run.
            format!(
                "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
                self.source,
                (self.session >> 16) & 0xffff,
                (self.session >> 4) & 0xfff,
                self.session & 0xfff,
                *counter
            )
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
            correlation,
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

/// A small stable hash of a trail's source name, for the identity namespace above.
///
/// FNV-1a: deterministic across runs and machines, which is what makes an audit id citable.
fn fnv(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A nonce for one run of a trail: the process and the clock, folded to the bits an id carries.
fn session_nonce() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    let mixed = (nanos as u64) ^ (u64::from(std::process::id()) << 32);
    // 28 bits are what the three nonce fields hold: 16 + 12 in the two middle groups; the top
    // 4 of the third field's 12 are folded in, so nothing of the clock is thrown away unmixed.
    ((mixed as u32) ^ ((mixed >> 32) as u32)) & 0x0fff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_mint_distinct_ids_for_two_runs_of_one_source() {
        // Spec §31.33: a trail persisted across sessions keeps an event once, by id. Two runs of
        // one package — two processes, or two loads — must therefore never mint the same id,
        // or the second run's events vanish at the persistence step.
        let first = AuditTrail::for_source("dev.example.echo");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = AuditTrail::for_source("dev.example.echo");
        for trail in [&first, &second] {
            trail.record(
                "dev.example.echo",
                "host",
                "clock.read",
                None,
                Enforcement::Broker,
                "clock.now",
                None,
                "2026-09-08T00:00:00Z".to_owned(),
                AuditResult::Success,
                None,
            );
        }
        let (a, b) = (
            first.snapshot()[0].id.clone(),
            second.snapshot()[0].id.clone(),
        );
        assert_ne!(a, b, "two runs of one source share no identity");
        assert!(
            a.starts_with(&format!("{:08x}-", fnv("dev.example.echo"))),
            "the source is still the first group: {a}"
        );
        assert_eq!(a.len(), 36, "and the identity stays v4-shaped: {a}");
    }
}
