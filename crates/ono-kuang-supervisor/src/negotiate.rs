//! Capability negotiation at load (spec §31.63).
//!
//! Negotiation happens before any package byte executes: a denied *required* capability fails
//! the load and no instance is ever spawned — manifest before code, spec §31.89. A denied
//! *optional* capability produces a degraded contract the plugin must adapt to, once, in
//! `lifecycle.init`.

use ono_kuang_protocol::{
    DeclarationClass, DeniedCapability, EffectiveLimits, Enforcement, GrantedCapability, HOST_API,
    KuangError, KuangErrorCode, Manifest, OverflowPolicy, PluginContract, VALUE_PROTOCOL,
};

use crate::policy::{Evaluation, Policy};

/// The host's own ceilings. Each effective limit is the smaller of the package's declaration
/// and these (spec §31.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLimits {
    /// The persistent-state ceiling, in bytes.
    pub state_quota: u64,
    /// The bounded event queue depth per stream — the credit window.
    pub queue_depth: u32,
    /// The cancellation deadline for one host call, in milliseconds.
    pub call_deadline_ms: u64,
    /// The largest frame either side may send.
    pub max_frame: u32,
    /// The default overflow policy. Host policy has final authority (spec §31.15).
    pub overflow: OverflowPolicy,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            state_quota: 1024 * 1024,
            queue_depth: 16,
            call_deadline_ms: 5_000,
            max_frame: 1024 * 1024,
            overflow: OverflowPolicy::BlockUpstream,
        }
    }
}

/// Computes the negotiated contract for `manifest` under `policy` (spec §31.63).
///
/// # Errors
///
/// Returns `load.capability_denied` when a `required` capability is not granted in the declared
/// scope — the package stays enabled and unloaded, and nothing of it has run.
pub fn negotiate(
    manifest: &Manifest,
    policy: &Policy,
    limits: &HostLimits,
) -> Result<PluginContract, KuangError> {
    let mut granted = Vec::new();
    let mut denied = Vec::new();
    for (class, request) in manifest.capability_requests() {
        // At load the declared scope is checked for *coverage*: the grant must be at least as
        // wide as the declaration, textually. The per-call check against concrete values
        // happens at every host call regardless (spec §31.19: evaluated per call).
        let covered = policy.grants_capability(request.capability);
        if covered {
            granted.push(GrantedCapability {
                capability: request.capability.id().to_owned(),
                class,
                scope: request.scope.clone(),
                enforcement: request
                    .capability
                    .scope_keys()
                    .iter()
                    .find(|key| {
                        request
                            .scope
                            .as_ref()
                            .is_some_and(|scope| scope.contains_key(key.name))
                    })
                    .map_or(Enforcement::Broker, |key| key.enforcement),
            });
            continue;
        }
        match class {
            DeclarationClass::Required => {
                return Err(KuangError::new(
                    KuangErrorCode::LoadCapabilityDenied,
                    format!(
                        "required capability `{}` was not granted; the package stays enabled and unloaded",
                        request.capability
                    ),
                )
                .with_help(
                    "`inspect plugin <id>` lists every requested capability with its grant state",
                ));
            }
            DeclarationClass::Optional | DeclarationClass::RuntimeRequested => {
                let evaluation = policy.evaluate(request.capability, &[]);
                let reason = match evaluation {
                    Evaluation::Denied(source) => format!("{source:?}").to_lowercase(),
                    _ => "not granted".to_owned(),
                };
                denied.push(DeniedCapability {
                    capability: request.capability.id().to_owned(),
                    class,
                    reason,
                });
            }
        }
    }
    let declared_quota = manifest.state.as_ref().and_then(|state| state.quota);
    let state_quota = declared_quota.map_or(limits.state_quota, |declared| {
        declared.min(limits.state_quota)
    });
    let degraded = denied
        .iter()
        .any(|denial| denial.class == DeclarationClass::Optional);
    Ok(PluginContract {
        host_api: HOST_API.protocol_id(),
        value_protocol: VALUE_PROTOCOL.to_owned(),
        granted,
        denied,
        limits: EffectiveLimits {
            memory_max: manifest.runtime.as_ref().map(|runtime| runtime.memory_max),
            state_quota,
            queue_depth: limits.queue_depth,
            call_deadline_ms: limits.call_deadline_ms,
            max_frame: limits.max_frame,
        },
        overflow: manifest
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.overflow)
            .map_or(limits.overflow, |preferred| {
                // The manifest MAY declare a preference; host policy has final authority.
                // This host accepts any preference except `drop-newest`, which spec §31.15
                // says must be explicit and never a default — accepting it silently as the
                // instance-wide default would make it one.
                if preferred == OverflowPolicy::DropNewest {
                    limits.overflow
                } else {
                    preferred
                }
            }),
        degraded,
    })
}
