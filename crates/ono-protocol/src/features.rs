//! The requests this protocol grew after version 1, and the ids an agent announces them by
//! (ADR-1056's one rule, ADR-1095).
//!
//! A request an older peer cannot ignore is a new frame kind, and its availability is a capability
//! id in the agent's offer. The agent puts the id into `Accept.capabilities` with
//! [`ServerConfig::with_capabilities`](crate::ServerConfig::with_capabilities); the client sends
//! the frame only when [`Negotiated::capabilities`](crate::Negotiated::capabilities) contains it,
//! and refuses locally otherwise. An older agent announces nothing, so it is never sent one; an
//! older client never sends one; an agent that receives a frame it did not announce fails the link
//! as a protocol mismatch.
//!
//! These ids name a part of the protocol, not a permission over the machine: what a request may
//! *read* is still decided per connection when it arrives (§10.2), so an id here never grants
//! anything by being negotiated.

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// The probe: one round trip the agent answers at once, touching no provider (v0.4 §19.1).
pub const PROBE_CAPABILITY: &str = "ono.link.probe/1";

/// Every protocol feature this build implements.
pub const FEATURES: &[&str] = &[PROBE_CAPABILITY];

/// Whether `id` names a protocol feature rather than a provider capability.
#[must_use]
pub fn is_feature(id: &str) -> bool {
    FEATURES.contains(&id)
}

/// The local refusal for a request the peer did not announce (ADR-1056, rule 2).
pub(crate) fn not_offered(feature: &str, what: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteProtocolMismatch,
        format!(
            "the far side of this link does not offer `{feature}`, so {what} was not sent; an \
             agent of an older version announces no such request"
        ),
    )
    .with_metadata("capability", Value::string(feature))
    .with_retryable(false)
    .with_help("upgrade `ono` on the far side to an agent that announces it")
}
