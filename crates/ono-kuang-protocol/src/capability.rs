//! The KUANG/11 capability model: the forty-seven families — spec §31.16's twenty-nine,
//! ADR-0594's `provider.mutate`, v0.5 §30.7's six temporal families and v0.6 §48.3's eleven
//! change and recovery families — their scope shapes, and the shapes of a grant, a lease and a
//! revocation (spec §31.16–§31.19, §31.49; v0.6 §48.3, §48.4).
//!
//! The authoritative family list is `kuang_capabilities` in `docs/contracts/capabilities.yaml`;
//! `docs/contracts/kuang/capabilities.v1.yaml` adds the scope shapes and enforcement levels this
//! module encodes. The rule the `enforcement` field exists to serve, spec §31.16 verbatim:
//! "A scope that cannot be enforced reliably MUST NOT be offered as if it were a security
//! boundary."

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{KuangError, KuangErrorCode};

/// The risk a capability's operations carry (`docs/contracts/capabilities.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// Reading state that already exists.
    Read,
    /// Keeping a live stream open over state as it changes.
    Observe,
    /// Changing state reversibly.
    Mutate,
    /// Changing state in a way that destroys something.
    Destructive,
}

/// Whether the capability may need privilege elevation (`docs/contracts/capabilities.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Elevation {
    /// Never needs elevation.
    None,
    /// Needs elevation depending on the object it touches.
    Conditional,
    /// Always needs elevation.
    Required,
}

/// Whether the broker can actually check a scope, or only record it (spec §31.16, ADR-0022 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Enforcement {
    /// Checked on every host call, against the value the operation will use. The only level
    /// that may be presented as a security boundary.
    Broker,
    /// Recorded, audited and shown — and labelled advisory on every surface that shows it.
    Advisory,
}

/// The value shape of one scope key (`docs/contracts/kuang/capabilities.v1.yaml` → `scope_types`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// Absolute path patterns, `**` matching any number of components.
    PathGlob,
    /// Host names or addresses, with a leading `*.` allowed for a domain.
    HostList,
    /// TCP/UDP port numbers.
    PortList,
    /// Exact names or `*`-globs of a named resource.
    NameList,
    /// Exact ids or `*`-globs of a namespaced id.
    IdList,
    /// Signal names without the `SIG` prefix.
    SignalList,
    /// An Ono `where` predicate, evaluated by the host.
    Predicate,
    /// How far back a history or context read may reach.
    Window,
    /// Data classes from the assistant contracts (spec §31.44).
    ClassList,
}

/// One scope key a capability declares: its name, its value shape and its enforcement level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeKey {
    /// The key as the manifest writes it, e.g. `paths`.
    pub name: &'static str,
    /// The value shape.
    pub kind: ScopeKind,
    /// Whether the broker checks it or only records it.
    pub enforcement: Enforcement,
}

const fn broker(name: &'static str, kind: ScopeKind) -> ScopeKey {
    ScopeKey {
        name,
        kind,
        enforcement: Enforcement::Broker,
    }
}

/// A scope key the host records and cannot check, because the value it names is resolved by the
/// package rather than by the broker (spec §31.16's "a scope that cannot be enforced reliably
/// MUST NOT be offered as if it were a security boundary").
const fn advisory(name: &'static str, kind: ScopeKind) -> ScopeKey {
    ScopeKey {
        name,
        kind,
        enforcement: Enforcement::Advisory,
    }
}

macro_rules! capabilities {
    ($( $variant:ident => $id:literal, $risk:ident, $elevation:ident, [$($key:expr),*], $doc:literal; )*) => {
        /// One of the forty-seven capability families: spec §31.16's twenty-nine,
        /// `provider.mutate` (ADR-0594), v0.5 §30.7's six temporal families (ADR-0626) and
        /// v0.6 §48.3's eleven change and recovery families.
        ///
        /// ```
        /// use ono_kuang_protocol::{Capability, Risk};
        /// assert_eq!(Capability::FilesystemWrite.id(), "filesystem.write");
        /// assert_eq!(Capability::FilesystemWrite.risk(), Risk::Destructive);
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Capability {
            $( #[doc = $doc] $variant, )*
        }

        impl Capability {
            /// Every family, in registry order.
            pub const ALL: &'static [Capability] = &[ $( Capability::$variant, )* ];

            /// The capability id as `docs/contracts/capabilities.yaml` spells it.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $( Capability::$variant => $id, )* }
            }

            /// The risk level of the family.
            #[must_use]
            pub const fn risk(self) -> Risk {
                match self { $( Capability::$variant => Risk::$risk, )* }
            }

            /// Whether operations under it may need privilege elevation.
            #[must_use]
            pub const fn elevation(self) -> Elevation {
                match self { $( Capability::$variant => Elevation::$elevation, )* }
            }

            /// The scope keys the family declares. Empty means the family is unscoped —
            /// deliberately, never decoratively (`docs/contracts/kuang/capabilities.v1.yaml`).
            #[must_use]
            pub fn scope_keys(self) -> &'static [ScopeKey] {
                match self { $( Capability::$variant => { const KEYS: &[ScopeKey] = &[$($key),*]; KEYS }, )* }
            }
        }
    };
}

capabilities! {
    ObjectRead => "object.read", Read, None,
        [broker("schemas", ScopeKind::IdList), broker("targets", ScopeKind::IdList)],
        "Snapshot and query calls in the `objects` domain.";
    SchemaRead => "schema.read", Read, None, [],
        "Reading schema definitions. Schemas are public contract; there is nothing to scope.";
    ProcessRead => "process.read", Read, None,
        [broker("selector", ScopeKind::Predicate)],
        "Reading process objects and their detail.";
    ProcessSignal => "process.signal", Mutate, Conditional,
        [broker("signals", ScopeKind::SignalList), broker("selector", ScopeKind::Predicate)],
        "Delivering the listed signals to processes matching the selector.";
    ProcessExec => "process.exec", Mutate, Conditional,
        [
            broker("programs", ScopeKind::PathGlob),
            broker("executables", ScopeKind::IdList),
            broker("argv_policy", ScopeKind::NameList)
        ],
        "Starting an external program through the host, with its stdio brokered.";
    FilesystemRead => "filesystem.read", Read, Conditional,
        [broker("paths", ScopeKind::PathGlob)],
        "Reading file content, metadata and directory entries under the scoped paths.";
    FilesystemWrite => "filesystem.write", Destructive, Conditional,
        [broker("paths", ScopeKind::PathGlob)],
        "Creating, modifying, renaming and deleting files under the scoped paths.";
    FilesystemWatch => "filesystem.watch", Observe, Conditional,
        [broker("paths", ScopeKind::PathGlob)],
        "Subscribing to filesystem change events under the scoped paths.";
    NetworkObserve => "network.observe", Read, Conditional, [],
        "Reading network state through the host's own providers.";
    NetworkConnect => "network.connect", Mutate, None,
        [broker("hosts", ScopeKind::HostList), broker("ports", ScopeKind::PortList)],
        "Opening outbound connections through the network broker (spec §31.21).";
    NetworkListen => "network.listen", Mutate, Conditional,
        [broker("ports", ScopeKind::PortList)],
        "Binding and accepting inbound connections on the scoped ports.";
    ServiceRead => "service.read", Read, None,
        [broker("units", ScopeKind::NameList)],
        "Reading service-manager units, their state and their properties.";
    ServiceMutate => "service.mutate", Mutate, Required,
        [broker("units", ScopeKind::NameList)],
        "Starting, stopping, restarting, enabling and disabling the scoped units.";
    ContainerRead => "container.read", Read, Conditional,
        [broker("containers", ScopeKind::NameList)],
        "Reading container objects and their metadata.";
    ContainerExec => "container.exec", Mutate, Conditional,
        [broker("containers", ScopeKind::NameList)],
        "Executing a program inside the scoped containers.";
    RemoteRead => "remote.read", Read, None,
        [broker("links", ScopeKind::NameList)],
        "Reading objects across a linked host, subject to remote policy (spec §31.40).";
    RemoteMutate => "remote.mutate", Mutate, Conditional,
        [broker("links", ScopeKind::NameList)],
        "Performing mutations across a linked host.";
    HistoryRead => "history.read", Read, None,
        [broker("window", ScopeKind::Window)],
        "Reading the session's semantic history within the scoped window.";
    HistoryWrite => "history.write", Mutate, None, [],
        "Adding history entries, attributed to the package by the host (ADR-0022 §4).";
    ContextRead => "context.read", Read, None, [],
        "Reading the current context stack. Bounded by construction (spec §31.12).";
    UiView => "ui.view", Read, None, [],
        "Contributing a view and submitting view trees. Never terminal ownership (spec §31.27).";
    UiNotify => "ui.notify", Mutate, None, [],
        "Raising a rate-limited notification to the operator.";
    RelationRead => "relation.read", Read, None, [],
        "Reading relationship edges from the graph.";
    RelationWrite => "relation.write", Mutate, None,
        [broker("relations", ScopeKind::IdList)],
        "Contributing edges along the scoped relation ids, attributed to the package by the host (spec §31.26, ADR-0600 §2).";
    SecretUse => "secret.use", Mutate, None,
        [broker("secrets", ScopeKind::NameList)],
        "Requesting opaque secret handles by name. Never enumeration (spec §31.20).";
    ModelInfer => "model.infer", Mutate, None,
        [broker("providers", ScopeKind::NameList), ScopeKey {
            name: "data_class",
            kind: ScopeKind::ClassList,
            enforcement: Enforcement::Advisory,
        }],
        "Submitting an inference request through the model broker (spec §31.43).";
    PluginInvoke => "plugin.invoke", Mutate, None,
        [broker("plugins", ScopeKind::NameList)],
        "Invoking a contribution of another loaded package through the host (spec §31.30).";
    StatePersist => "state.persist", Mutate, None, [],
        "Storing state that survives the session, in the package's own quota-bounded store.";
    ClockRead => "clock.read", Read, None, [],
        "Reading monotonic and wall-clock time.";
    ProviderMutate => "provider.mutate", Mutate, None,
        [ScopeKey {
            name: "instances",
            kind: ScopeKind::IdList,
            enforcement: Enforcement::Advisory,
        }, ScopeKey {
            name: "resources",
            kind: ScopeKind::IdList,
            enforcement: Enforcement::Advisory,
        }, ScopeKey {
            name: "actions",
            kind: ScopeKind::NameList,
            enforcement: Enforcement::Advisory,
        }],
        "Changing state in the external system a provider package fronts (ADR-0594).";
    TemporalReadCurrent => "temporal.read.current", Read, None, [],
        "Reading the present temporal context: whether the session is historical, and at which instant (v0.5 §30.7).";
    TemporalReadHistory => "temporal.read.history", Read, None,
        [broker("window", ScopeKind::Window)],
        "Reading recorded events and reconstructed past state within the scoped window (v0.5 §30.7).";
    TemporalReadEvidence => "temporal.read.evidence", Read, None, [],
        "Reading the evidence records and coverage behind a temporal claim (v0.5 §7.3, §30.7).";
    TemporalContributeEvents => "temporal.contribute.events", Mutate, None,
        [broker("kinds", ScopeKind::NameList)],
        "Contributing canonical temporal events of the scoped kinds, attributed to the package by the host (v0.5 §37.3).";
    TemporalContributeCausality => "temporal.contribute.causality", Mutate, None,
        [broker("rules", ScopeKind::IdList)],
        "Registering namespaced causal rules and contributing the causal links they produce (v0.5 §37.4).";
    TemporalRecorderManage => "temporal.recorder.manage", Mutate, None, [],
        "Starting and stopping the persistent history recorder (v0.5 §10.3, §30.7).";
    ChangePlanRead => "change.plan.read", Read, None,
        [broker("plans", ScopeKind::IdList), broker("schemas", ScopeKind::IdList)],
        "Reading change plans, their actions and their computed impact (v0.6 §48.3). §48.4: it is no route to execution.";
    ChangePlanContribute => "change.plan.contribute", Observe, None,
        [broker("schemas", ScopeKind::IdList)],
        "Contributing actions, effects, impact edges and risk findings to a plan being resolved (v0.6 §48.3). Still not permission to run anything.";
    ChangeActionExecute => "change.action.execute", Mutate, Conditional,
        [broker("schemas", ScopeKind::IdList), broker("plans", ScopeKind::IdList)],
        "Carrying out a mutating plan action (v0.6 §48.3). The only change family that authorises a change.";
    VerificationObserve => "verification.observe", Read, None,
        [broker("schemas", ScopeKind::IdList)],
        "Observing a verification contract and reporting the result (v0.6 §48.3, §25.1).";
    RecoveryDiscover => "recovery.discover", Read, None,
        [broker("domain_kinds", ScopeKind::NameList)],
        "Finding candidate protection for a target (v0.6 §12.2, §48.3). Read-only.";
    RecoveryPrepare => "recovery.prepare", Mutate, Conditional,
        [broker("domain_kinds", ScopeKind::NameList), broker("scopes", ScopeKind::IdList)],
        "Creating a recovery asset, which mutates the storage or control plane (v0.6 §5.5, §12.2).";
    RecoveryRestore => "recovery.restore", Destructive, Required,
        [broker("domain_kinds", ScopeKind::NameList), broker("scopes", ScopeKind::IdList)],
        "Using an asset to put state back (v0.6 §12.2). Destructive and elevation-required: §43.4 lets recovery need stronger privilege than the mutation it undoes, and §13.6 and §14.6 make restoring the operation that can lose the most.";
    RecoveryCleanup => "recovery.cleanup", Mutate, Conditional,
        [broker("domain_kinds", ScopeKind::NameList), broker("scopes", ScopeKind::IdList)],
        "Removing a recovery asset (v0.6 §12.2, §37).";
    RecoveryEstimateCost => "recovery.estimate-cost", Read, None,
        [broker("domain_kinds", ScopeKind::NameList)],
        "Reporting what an asset costs to create and to keep (v0.6 §12.2, §38).";
    RecoveryQuiesce => "recovery.quiesce", Mutate, Conditional,
        [advisory("applications", ScopeKind::NameList)],
        "Pausing an application so a snapshot is application-consistent (v0.6 §16.4, §39.3). The application name is the package's own word for something the host cannot resolve, so the scope is advisory.";
    RecoveryTransaction => "recovery.transaction", Mutate, Conditional,
        [advisory("applications", ScopeKind::NameList)],
        "Beginning, preparing, committing and rolling back inside the provider's own boundary (v0.6 §27.1). §27.2 forbids the word once a second boundary is involved, and §27.3 makes generic two-phase commit a non-goal.";
}

impl Capability {
    /// Resolves a family from its id, or `None` for an id the registry does not carry.
    ///
    /// An unknown capability id in a manifest is `package.invalid`, never a silently ignored
    /// line (`docs/contracts/kuang/manifest.v1.yaml`).
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|family| family.id() == id)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Capability {
    type Err = KuangError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::from_id(text).ok_or_else(|| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("`{text}` is not a capability of `docs/contracts/capabilities.yaml`"),
            )
        })
    }
}

/// The declaration class of a requested capability (spec §31.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationClass {
    /// The package cannot load without the grant.
    Required,
    /// The package loads without the grant and MUST adapt; its state becomes `degraded`.
    Optional,
    /// The package may ask later, in response to an explicit user action, and only within the
    /// declared scope.
    RuntimeRequested,
}

/// How long a grant lasts (spec §31.18, `docs/contracts/kuang/capabilities.v1.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantDuration {
    /// One host call.
    Once,
    /// The current command.
    Command,
    /// The current view.
    View,
    /// The current Ono session. Never written to persistent policy.
    Session,
    /// The current session on one link.
    LinkSession,
    /// Persistent, for the exact scope granted. Inspectable and revocable.
    Always,
}

/// A policy decision for one request (spec §31.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    /// The call proceeds and is audited.
    Allow,
    /// The call fails with `capability.denied` and is audited.
    Deny,
    /// The operator is prompted once. In a non-interactive context `ask` resolves to `deny`.
    Ask,
}

/// A grant with an expiry, a use count and an optional condition (spec §31.49).
///
/// Every field is checked at each use; failing any of them is `capability.lease_expired`, and a
/// lease is never extended in place — extending means issuing a new lease, so the audit trail
/// shows two decisions rather than one mutable one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    /// The capability id being leased.
    pub capability: String,
    /// The objects it reaches, as an Ono `where` predicate.
    pub selector: String,
    /// The operations permitted. `None` means every operation the capability allows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    /// When the lease stops working, as an RFC 3339 timestamp. A lease without an expiry is a
    /// grant, and is stored as one.
    pub expires_at: String,
    /// How many times it may be used. `None` means unlimited within the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
    /// A probe or policy reference that must hold at each use (spec §31.49).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_carry_all_47_families_of_the_registry_when_enumerated() {
        // §31.16's twenty-nine, `provider.mutate` (ADR-0594), v0.5 §30.7's six temporal
        // families (ADR-0626), and v0.6 §48.3's eleven change and recovery families.
        assert_eq!(Capability::ALL.len(), 47);
    }

    #[test]
    fn should_mark_only_the_declared_advisory_scope_keys_as_advisory_when_scopes_are_listed() {
        // ADR-0022 §3 put exactly one advisory scope key in the model; ADR-0594 added the three
        // of `provider.mutate`, whose wire the host does not parse; v0.6 §48.3 adds the two
        // application names a recovery plugin resolves inside its own system, which the host has
        // no way to compare a call against. Every other key is a boundary the broker enforces,
        // and a key that quietly became advisory would fail here.
        let advisory: Vec<(&str, &str)> = Capability::ALL
            .iter()
            .flat_map(|family| {
                family
                    .scope_keys()
                    .iter()
                    .filter(|key| key.enforcement == Enforcement::Advisory)
                    .map(|key| (family.id(), key.name))
            })
            .collect();
        assert_eq!(
            advisory,
            vec![
                ("model.infer", "data_class"),
                ("provider.mutate", "instances"),
                ("provider.mutate", "resources"),
                ("provider.mutate", "actions"),
                ("recovery.quiesce", "applications"),
                ("recovery.transaction", "applications"),
            ]
        );
    }

    #[test]
    fn should_refuse_an_unknown_capability_id_when_parsed() {
        let error = "process.launch".parse::<Capability>().unwrap_err();
        assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
    }

    #[test]
    fn should_declare_no_scope_for_history_write_when_looked_up() {
        // ADR-0022 §4: attribution is not a grant scope.
        assert!(Capability::HistoryWrite.scope_keys().is_empty());
    }
}
