//! The negotiated runtime contract of spec §31.63, and the limits of spec §31.15.
//!
//! Loading produces one record both sides hold: the operator sees what was granted and denied,
//! and the plugin receives the same answer through `lifecycle.init` and MUST adapt to it rather
//! than re-prompting. The object schema this mirrors is
//! `docs/spec/schemas/plugin-runtime.v1.yaml`.

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as Json};

use crate::{DeclarationClass, Enforcement};

/// The five overflow policies of spec §31.15, in the order that section gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverflowPolicy {
    /// The producer waits. Only where the provider safely supports being paused.
    BlockUpstream,
    /// For replaceable telemetry, where the newest observation supersedes the older ones.
    DropOldest,
    /// Explicit only, never a default and never inferred.
    DropNewest,
    /// Combine repeated updates by object identity. Requires the schema to declare one.
    Coalesce,
    /// Terminate the stream with `runtime.backpressure_failure` rather than lose data.
    FailStream,
}

/// One granted capability inside the contract, with its enforcement level visible —
/// an advisory scope is visible as advisory in the contract, not only in the prompt
/// (ADR-0022 §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrantedCapability {
    /// The capability id.
    pub capability: String,
    /// The declaration class the grant answers.
    pub class: DeclarationClass,
    /// The granted scope. `None` means the capability is unscoped or was granted unscoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<JsonMap<String, Json>>,
    /// Whether the scope is checked by the broker or only recorded.
    pub enforcement: Enforcement,
}

/// One requested capability that was not granted. Entries are always `optional` or
/// `runtime-requested`: a denied `required` capability fails the load and no contract exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeniedCapability {
    /// The capability id.
    pub capability: String,
    /// The declaration class of the request.
    pub class: DeclarationClass,
    /// Why it was denied, in policy terms.
    pub reason: String,
}

/// The effective quotas of spec §31.15: each the smaller of the package's declaration and host
/// policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveLimits {
    /// The instance's memory ceiling in bytes, where the tier can enforce one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_max: Option<u64>,
    /// The persistent-state ceiling in bytes. Exceeding it fails the write, never evicts.
    pub state_quota: u64,
    /// The bounded event queue depth per stream — the credit window of the pull protocol.
    pub queue_depth: u32,
    /// The cancellation deadline for one host call, in milliseconds.
    pub call_deadline_ms: u64,
    /// The largest frame either side may send, in bytes. Beyond it is a protocol violation.
    pub max_frame: u32,
}

/// The contract negotiated when a package is loaded (spec §31.63).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginContract {
    /// The negotiated host API version — one version, not the range the package asked for.
    pub host_api: String,
    /// The negotiated value protocol, e.g. `ono-value/1`.
    pub value_protocol: String,
    /// Every granted capability.
    pub granted: Vec<GrantedCapability>,
    /// Every requested capability that was not granted.
    pub denied: Vec<DeniedCapability>,
    /// The effective limits.
    pub limits: EffectiveLimits,
    /// The effective default overflow policy for this instance's streams.
    pub overflow: OverflowPolicy,
    /// Whether the instance runs with denied optional capabilities (spec §31.8).
    pub degraded: bool,
}

impl PluginContract {
    /// The granted entry for `capability`, when one exists.
    #[must_use]
    pub fn grant(&self, capability: &str) -> Option<&GrantedCapability> {
        self.granted
            .iter()
            .find(|grant| grant.capability == capability)
    }
}
