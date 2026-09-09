//! Remote recovery, planned per host (spec §29.3, §29.4).
//!
//! §29.4 is two sentences and both are load-bearing: *"Remote recovery is planned per host. Ono
//! MUST show whether recovery can proceed for disconnected hosts."* The first forbids one plan
//! that speaks for a fleet; the second forbids the plan being silent about the hosts it could not
//! reach.
//!
//! §29.3 supplies the rule that makes the second one sharp. A host whose link failed has an
//! *unknown* state, and Ono "MUST NOT mark unknown remote actions as failed or successful without
//! evidence". So a disconnected host does not get a fragment that says recovery will work there,
//! and it does not get one that says the host is broken either. It gets a fragment that says
//! recovery cannot proceed there yet, carrying `change.remote_state_unknown` — an error §45 marks
//! retryable, because querying again when the link returns is what resolves it.

use std::sync::Arc;

use ono_change_core::{RestoreMethod, error};
use ono_value::ErrorValue;

/// What one host could be established to hold when the recovery was planned (§29.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostState {
    /// The host answered, and its provider offers `method` from `asset`.
    Established {
        /// How the state there would be put back.
        method: RestoreMethod,
        /// The provider-native reference of the asset that holds it.
        asset: Arc<str>,
    },
    /// The host answered and holds nothing this recovery could restore from (§11.4).
    Unprotected {
        /// Why there is nothing to restore from there.
        reason: Arc<str>,
    },
    /// The link was down, so what the host holds could not be established (§29.3).
    Unestablished {
        /// The action whose outcome on that host is unknown.
        action: Arc<str>,
        /// What the link did.
        reason: Arc<str>,
    },
}

/// One host, as it was observed when the recovery was planned (§29.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObservation {
    host: Arc<str>,
    state: HostState,
}

impl HostObservation {
    /// Records that `host` is in `state`.
    #[must_use]
    pub fn new(host: impl Into<Arc<str>>, state: HostState) -> Self {
        Self {
            host: host.into(),
            state,
        }
    }

    /// `host` answered and would recover by `method` from `asset`.
    #[must_use]
    pub fn established(
        host: impl Into<Arc<str>>,
        method: RestoreMethod,
        asset: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(
            host,
            HostState::Established {
                method,
                asset: asset.into(),
            },
        )
    }

    /// The link to `host` was down while `action`'s outcome was being established (§29.3).
    #[must_use]
    pub fn disconnected(
        host: impl Into<Arc<str>>,
        action: impl Into<Arc<str>>,
        reason: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(
            host,
            HostState::Unestablished {
                action: action.into(),
                reason: reason.into(),
            },
        )
    }

    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// What was established about it.
    #[must_use]
    pub const fn state(&self) -> &HostState {
        &self.state
    }
}

/// The recovery planned for one host, and whether it can proceed there (§29.4).
#[derive(Debug, Clone, PartialEq)]
pub struct HostRecovery {
    host: Arc<str>,
    method: Option<RestoreMethod>,
    asset: Option<Arc<str>>,
    reason: Arc<str>,
    refusal: Option<ErrorValue>,
}

impl HostRecovery {
    /// The host this fragment is about.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether recovery can proceed here (§29.4).
    #[must_use]
    pub const fn can_proceed(&self) -> bool {
        self.refusal.is_none()
    }

    /// How the state there would be put back, where that was established.
    #[must_use]
    pub const fn method(&self) -> Option<RestoreMethod> {
        self.method
    }

    /// The asset it would restore from, where there is one.
    #[must_use]
    pub fn asset(&self) -> Option<&str> {
        self.asset.as_deref()
    }

    /// The sentence a person reads beside the host.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The refusal that says why recovery cannot proceed here.
    #[must_use]
    pub const fn refusal(&self) -> Option<&ErrorValue> {
        self.refusal.as_ref()
    }
}

/// A remote recovery, one fragment per host (§29.4).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteRecovery {
    hosts: Vec<HostRecovery>,
}

impl RemoteRecovery {
    /// Every host, in the order they were observed.
    #[must_use]
    pub fn hosts(&self) -> &[HostRecovery] {
        &self.hosts
    }

    /// The hosts recovery can proceed on.
    #[must_use]
    pub fn proceedable(&self) -> Vec<&HostRecovery> {
        self.hosts
            .iter()
            .filter(|host| host.can_proceed())
            .collect()
    }

    /// The hosts recovery cannot proceed on, with the refusal each carries (§29.4).
    #[must_use]
    pub fn blocked(&self) -> Vec<&HostRecovery> {
        self.hosts
            .iter()
            .filter(|host| !host.can_proceed())
            .collect()
    }

    /// Whether every host in the set could be planned for.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.hosts.iter().all(HostRecovery::can_proceed)
    }
}

/// Plans recovery per host, and says which hosts it cannot proceed on (§29.4).
///
/// A disconnected host produces a fragment carrying `change.remote_state_unknown` rather than one
/// that assumes recovery will work there. §29.3 forbids the assumption in either direction, and a
/// per-host plan that quietly omitted the host it could not reach would read as a plan for the
/// whole fleet.
#[must_use]
pub fn plan_hosts(observations: &[HostObservation]) -> RemoteRecovery {
    RemoteRecovery {
        hosts: observations
            .iter()
            .map(|observation| match observation.state() {
                HostState::Established { method, asset } => HostRecovery {
                    host: Arc::from(observation.host()),
                    method: Some(*method),
                    asset: Some(Arc::clone(asset)),
                    reason: Arc::from(format!(
                        "recovery can proceed here by {method} from {asset}"
                    )),
                    refusal: None,
                },
                HostState::Unprotected { reason } => HostRecovery {
                    host: Arc::from(observation.host()),
                    method: None,
                    asset: None,
                    reason: Arc::from(format!("recovery cannot proceed here: {reason}")),
                    refusal: Some(error::recovery_plan_incomplete(
                        &format!("a recovery asset on {}", observation.host()),
                        reason,
                    )),
                },
                HostState::Unestablished { action, reason } => HostRecovery {
                    host: Arc::from(observation.host()),
                    method: None,
                    asset: None,
                    reason: Arc::from(format!(
                        "recovery cannot proceed here: what this host holds could not be \
                         established ({reason})"
                    )),
                    refusal: Some(error::remote_state_unknown(observation.host(), action)),
                },
            })
            .collect(),
    }
}
