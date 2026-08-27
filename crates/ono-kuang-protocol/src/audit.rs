//! The audit record of spec §31.37, in the shape of `docs/spec/schemas/plugin-audit-event.v1.yaml`.
//!
//! Every capability-sensitive action a package takes — or is refused — is one record. Denials
//! are recorded as loudly as successes: a package probing for capabilities it does not hold is
//! exactly what the trail is for. Attribution, timestamps and identity are the host's;
//! a package cannot backdate or reassign its own trail.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::{Enforcement, WireError};

/// Spec §31.37's three outcomes. Conflating them would hide exactly the pattern the trail
/// exists to reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditResult {
    /// Policy permitted it and it worked.
    Success,
    /// Policy refused it.
    Denied,
    /// Policy permitted it and it did not work.
    Failed,
}

/// One capability-sensitive action a KUANG/11 package took, or was refused
/// (`ono.plugin-audit-event/1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// The record's own identity, so an event can be cited from a finding.
    pub id: String,
    /// The package that acted. Set by the host.
    pub plugin: String,
    /// The invocation the action belonged to — what lets a trail read as a story.
    pub invocation: String,
    /// The capability id the action was taken under.
    pub capability: String,
    /// The scope in force. `None` when the capability declares no scope — different from an
    /// empty scope, and the difference matters when reading a trail back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Json>,
    /// Whether the scope was checked or merely recorded. An advisory scope in an audit record
    /// must never read as though it had been enforced (spec §31.16).
    pub enforcement: Enforcement,
    /// What was attempted — a host call id such as `filesystem.read`.
    pub action: String,
    /// A reference to the object acted on. `None` for an action about no object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Json>,
    /// When it happened, by the host's clock.
    pub at: String,
    /// The outcome.
    pub result: AuditResult,
    /// The confirmation that authorised the action, where one was required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_confirmation: Option<String>,
    /// The capability lease the action was taken under (spec §31.49).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<String>,
    /// The link the action crossed, for a remote action (spec §31.40).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The structured error, for `denied` and `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}
