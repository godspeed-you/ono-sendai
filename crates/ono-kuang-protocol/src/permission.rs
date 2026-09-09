//! The permission layer: what a user grants, projected onto what the broker enforces.
//!
//! "A user grants intentions. KUANG/11 grants capabilities." (K11P §0.3, ADR-0600.) A
//! [`PermissionDescriptor`] is a human-facing statement — *Connect to Kubernetes clusters* —
//! that resolves to one or more exact capability requirements; an [`AccessProfile`] is a named
//! set of them offered as one installation choice; a [`PermissionSet`] is everything a package
//! declares, or everything the host derives for a package that declares nothing.
//!
//! Nothing here grants anything. The host classifies every capability family into a consent
//! class ([`ConsentClass`]) with a minimum risk a package may raise and never lower, checks a
//! declared descriptor against that floor before any package byte runs, and executes a decision
//! by minting ordinary capability grants through the same policy every manual grant goes
//! through. The broker stays authoritative; this module is the vocabulary above it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as Json};

use crate::{
    Capability, CapabilityRequest, DeclarationClass, Enforcement, KuangError, KuangErrorCode,
    Manifest,
};

/// The longest a permission title may be, in characters (K11P §8.3).
pub const TITLE_LIMIT: usize = 120;
/// The longest a permission purpose may be, in characters (K11P §8.3).
pub const PURPOSE_LIMIT: usize = 240;

macro_rules! closed_words {
    ($(#[$meta:meta])* $name:ident { $( $variant:ident => $id:literal, $doc:literal; )* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name {
            $( #[doc = $doc] $variant, )*
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )* ];

            /// The word as the manifest and the contracts spell it.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $( $name::$variant => $id, )* }
            }

            /// The value a word names, or `None` for a word outside the vocabulary.
            #[must_use]
            pub fn from_id(id: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.id() == id)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.id())
            }
        }
    };
}

closed_words! {
    /// The host-controlled kind of a permission (K11P §6.4). A package chooses one; the host
    /// renders it beside the package's own title, so friendly wording cannot relabel the
    /// authority underneath.
    PermissionKind {
        ExternalObserve => "external-observe", "Reaching an external system to read it.";
        ExternalChange => "external-change", "Changing state in an external system.";
        FilesystemRead => "filesystem-read", "Reading files on this machine.";
        FilesystemWrite => "filesystem-write", "Creating, changing or deleting files on this machine.";
        ExecuteHelper => "execute-helper", "Running a program on this machine.";
        SecretUse => "secret-use", "Using a secret without exposing its value.";
        LocalContribution => "local-contribution", "Contributing to Ono itself, within the package's own declarations.";
        ModelUse => "model-use", "Sending requests to a model provider.";
        HostObserve => "host-observe", "Reading what Ono already knows about this machine.";
        HostChange => "host-change", "Changing this machine through Ono's own providers.";
        OtherSensitive => "other-sensitive", "Sensitive authority of another shape; the host names the capability.";
    }
}

closed_words! {
    /// When a permission is decided (K11P §6.5).
    PermissionPhase {
        Automatic => "automatic", "Bounded extension-local authority; included, never asked.";
        Install => "install", "Decided at installation, as part of an access profile.";
        Jit => "jit", "Asked when the concrete need and scope are known.";
        Explicit => "explicit", "Never granted by the recommended flow; the user enables it deliberately.";
    }
}

closed_words! {
    /// The risk a permission carries (K11P §6.6), ordered from harmless to destructive. The host
    /// derives the floor from the capabilities; a package may raise it and never lower it.
    PermissionRisk {
        Local => "local", "Affects only the package's own contributions to Ono.";
        Observe => "observe", "Reads state that already exists.";
        SensitiveRead => "sensitive-read", "Reads files or secrets.";
        Execute => "execute", "Runs a program.";
        Mutate => "mutate", "Changes state reversibly.";
        Destructive => "destructive", "Changes state in a way that destroys something.";
    }
}

closed_words! {
    /// The consent class the host places every capability in (K11P §7, ADR-0600 §2).
    ConsentClass {
        ExtensionLocal => "extension-local", "Class A: authority the host constrains to the package's own declared contributions; included without a prompt.";
        Observation => "observation", "Class B: the package's core non-mutating purpose; may be part of the recommended profile.";
        Conditional => "conditional", "Class C: the concrete scope is known only at use; asked just in time.";
        Explicit => "explicit", "Class D: changes external or host state; never in the recommended profile.";
        Destructive => "destructive", "Class E: destructive or exceptional; explicit consent, never unattended.";
    }
}

impl ConsentClass {
    /// The letter K11P §7 gives the class.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            ConsentClass::ExtensionLocal => 'A',
            ConsentClass::Observation => 'B',
            ConsentClass::Conditional => 'C',
            ConsentClass::Explicit => 'D',
            ConsentClass::Destructive => 'E',
        }
    }

    /// The phases a descriptor over a capability of this class may declare (ADR-0600 §2).
    #[must_use]
    pub const fn allowed_phases(self) -> &'static [PermissionPhase] {
        match self {
            ConsentClass::ExtensionLocal => &[
                PermissionPhase::Automatic,
                PermissionPhase::Install,
                PermissionPhase::Jit,
                PermissionPhase::Explicit,
            ],
            ConsentClass::Observation => &[
                PermissionPhase::Install,
                PermissionPhase::Jit,
                PermissionPhase::Explicit,
            ],
            ConsentClass::Conditional => &[PermissionPhase::Jit, PermissionPhase::Explicit],
            ConsentClass::Explicit | ConsentClass::Destructive => &[PermissionPhase::Explicit],
        }
    }

    /// The phase a derived permission takes (K11P §8.1, ADR-0600 §4).
    #[must_use]
    pub const fn derived_phase(self) -> PermissionPhase {
        match self {
            ConsentClass::ExtensionLocal => PermissionPhase::Automatic,
            ConsentClass::Observation => PermissionPhase::Install,
            ConsentClass::Conditional => PermissionPhase::Jit,
            ConsentClass::Explicit | ConsentClass::Destructive => PermissionPhase::Explicit,
        }
    }
}

/// The scope a grant template names: a concrete record, or one of three words the host
/// resolves (K11P §8.2, §19.3).
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeTemplate {
    /// Scope keys the capability declares, with concrete values.
    Concrete(JsonMap<String, Json>),
    /// The host derives the concrete value from validated data at the time of use, and grants
    /// only that value. Not a wildcard (K11P §19.3).
    RuntimeDerived,
    /// The package's own declared contributions — only for `relation.write` (K11P §7.1).
    PackageContributions,
    /// The provider instance the package fronts — only for `provider.mutate`.
    ProviderInstance,
}

impl ScopeTemplate {
    /// The word a non-concrete template is written as.
    #[must_use]
    pub const fn word(&self) -> Option<&'static str> {
        match self {
            ScopeTemplate::Concrete(_) => None,
            ScopeTemplate::RuntimeDerived => Some("runtime-derived"),
            ScopeTemplate::PackageContributions => Some("package-contributions"),
            ScopeTemplate::ProviderInstance => Some("provider-instance"),
        }
    }

    /// The template as JSON: the record, or the word.
    #[must_use]
    pub fn as_json(&self) -> Json {
        match self {
            ScopeTemplate::Concrete(scope) => Json::Object(scope.clone()),
            other => Json::String(other.word().unwrap_or_default().to_owned()),
        }
    }

    /// The concrete record, when the template is one.
    #[must_use]
    pub fn concrete(&self) -> Option<&JsonMap<String, Json>> {
        match self {
            ScopeTemplate::Concrete(scope) => Some(scope),
            _ => None,
        }
    }
}

/// One capability a permission resolves to, with the scope it is granted in.
#[derive(Debug, Clone, PartialEq)]
pub struct GrantTemplate {
    /// The family.
    pub capability: Capability,
    /// The scope. `None` grants the family unscoped, which is what a bare manifest declaration
    /// asks for.
    pub scope: Option<ScopeTemplate>,
}

impl GrantTemplate {
    /// The consent class this grant falls in: the family's, narrowed for a bounded
    /// `relation.write` (ADR-0600 §2).
    #[must_use]
    pub fn consent_class(&self) -> ConsentClass {
        consent_class(self.capability, self.scope.as_ref())
    }

    /// The weakest enforcement among the scope keys this grant would carry, or `broker` for a
    /// grant with no scope to enforce.
    #[must_use]
    pub fn enforcement(&self) -> Enforcement {
        let keys = self.capability.scope_keys();
        let advisory = match &self.scope {
            Some(ScopeTemplate::Concrete(scope)) => scope.keys().any(|key| {
                keys.iter().any(|declared| {
                    declared.name == key && declared.enforcement == Enforcement::Advisory
                })
            }),
            Some(ScopeTemplate::ProviderInstance) => true,
            _ => false,
        };
        if advisory {
            Enforcement::Advisory
        } else {
            Enforcement::Broker
        }
    }
}

/// A user-facing permission (K11P §6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionDescriptor {
    /// The package-local stable id, a kebab slug (K11P §6.3).
    pub id: String,
    /// The host-controlled kind.
    pub kind: PermissionKind,
    /// The package's title, sanitised.
    pub title: String,
    /// The package's purpose text, sanitised.
    pub purpose: Option<String>,
    /// When it is decided.
    pub phase: PermissionPhase,
    /// Whether it belongs in the recommended profile.
    pub recommended: bool,
    /// The risk, never below the host's floor.
    pub risk: PermissionRisk,
    /// What it resolves to.
    pub grants: Vec<GrantTemplate>,
    /// A package-authored rendering of the scope, sanitised. The host renders the exact scope
    /// beside it whenever details are shown.
    pub scope_text: Option<String>,
    /// Whether the host derived it because the package declared nothing for the capability.
    pub derived: bool,
}

impl PermissionDescriptor {
    /// The families the permission resolves to, in declaration order, without repeats.
    pub fn capabilities(&self) -> impl Iterator<Item = Capability> + '_ {
        let mut seen = BTreeSet::new();
        self.grants
            .iter()
            .map(|grant| grant.capability)
            .filter(move |capability| seen.insert(*capability))
    }

    /// The strongest consent class among the permission's grants.
    #[must_use]
    pub fn consent_class(&self) -> ConsentClass {
        self.grants
            .iter()
            .map(GrantTemplate::consent_class)
            .max()
            .unwrap_or(ConsentClass::ExtensionLocal)
    }

    /// The weakest enforcement among the permission's grants.
    #[must_use]
    pub fn enforcement(&self) -> Enforcement {
        if self
            .grants
            .iter()
            .any(|grant| grant.enforcement() == Enforcement::Advisory)
        {
            Enforcement::Advisory
        } else {
            Enforcement::Broker
        }
    }

    /// Whether the permission is included without a question (K11P §7.1).
    #[must_use]
    pub fn is_automatic(&self) -> bool {
        self.phase == PermissionPhase::Automatic
    }

    /// Whether this permission may sit in a recommended or minimal profile (K11P §9.1): decided
    /// at install or automatic, and of class A or B — or class C with a concrete program scope.
    #[must_use]
    pub fn is_safe_default(&self) -> bool {
        if !matches!(
            self.phase,
            PermissionPhase::Install | PermissionPhase::Automatic
        ) {
            return false;
        }
        if self.risk >= PermissionRisk::Mutate {
            return false;
        }
        self.grants.iter().all(|grant| match grant.consent_class() {
            ConsentClass::ExtensionLocal | ConsentClass::Observation => true,
            ConsentClass::Conditional => grant
                .scope
                .as_ref()
                .and_then(ScopeTemplate::concrete)
                .is_some_and(|scope| {
                    scope.contains_key("programs") || scope.contains_key("executables")
                }),
            ConsentClass::Explicit | ConsentClass::Destructive => false,
        })
    }
}

/// A named set of permission decisions offered as one installation choice (K11P §4.6, §9).
#[derive(Debug, Clone, PartialEq)]
pub struct AccessProfile {
    /// The profile's name, e.g. `recommended`.
    pub name: String,
    /// The package's title for it, sanitised.
    pub title: String,
    /// The permission ids it decides `allow`.
    pub permissions: Vec<String>,
}

/// The name of the profile every package with visible permissions must offer, and the default.
pub const RECOMMENDED: &str = "recommended";
/// The name of the smallest profile every such package must offer.
pub const MINIMAL: &str = "minimal";

/// Everything a package's permission layer consists of.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PermissionSet {
    /// The descriptors, declared first and derived after, in order.
    pub descriptors: Vec<PermissionDescriptor>,
    /// The profiles, `minimal` and `recommended` among them whenever a descriptor exists.
    pub profiles: Vec<AccessProfile>,
    /// Whether the package declared a `permissions` section at all.
    pub declared: bool,
}

impl PermissionSet {
    /// The descriptor with this id.
    #[must_use]
    pub fn descriptor(&self, id: &str) -> Option<&PermissionDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }

    /// The profile with this name.
    #[must_use]
    pub fn profile(&self, name: &str) -> Option<&AccessProfile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    /// The descriptors that resolve to `capability`, in order.
    pub fn for_capability(
        &self,
        capability: Capability,
    ) -> impl Iterator<Item = &PermissionDescriptor> + '_ {
        self.descriptors.iter().filter(move |descriptor| {
            descriptor
                .grants
                .iter()
                .any(|grant| grant.capability == capability)
        })
    }

    /// The just-in-time permission that governs `capability`, if the package has one
    /// (ADR-0603 §1).
    #[must_use]
    pub fn jit_permission(&self, capability: Capability) -> Option<&PermissionDescriptor> {
        self.for_capability(capability)
            .find(|descriptor| descriptor.phase == PermissionPhase::Jit)
    }

    /// The descriptors a profile decides `allow`, in the profile's order.
    pub fn profile_descriptors<'a>(
        &'a self,
        profile: &'a AccessProfile,
    ) -> impl Iterator<Item = &'a PermissionDescriptor> + 'a {
        profile
            .permissions
            .iter()
            .filter_map(move |id| self.descriptor(id))
    }

    /// The descriptors included without a question.
    pub fn automatic(&self) -> impl Iterator<Item = &PermissionDescriptor> + '_ {
        self.descriptors
            .iter()
            .filter(|descriptor| descriptor.is_automatic())
    }

    /// The permission layer of `manifest`: what it declared, plus a derived descriptor for every
    /// declared capability no descriptor maps, or everything derived when it declared nothing
    /// (K11P §8.1, ADR-0600 §4). A derived descriptor over an unmapped capability is never in a
    /// declared profile, so nothing a package left out is granted by leaving it out.
    #[must_use]
    pub fn of(manifest: &Manifest) -> Self {
        let mut set = manifest.permissions.clone().unwrap_or_default();
        let declared = set.declared;
        let mapped: BTreeSet<Capability> = set
            .descriptors
            .iter()
            .flat_map(|descriptor| descriptor.grants.iter().map(|grant| grant.capability))
            .collect();
        let mut minimal = Vec::new();
        let mut recommended = Vec::new();
        for (class, request) in manifest.capability_requests() {
            if mapped.contains(&request.capability) {
                continue;
            }
            let descriptor = derive_descriptor(manifest, class, request);
            let consent = descriptor.consent_class();
            if !declared {
                if consent == ConsentClass::ExtensionLocal {
                    minimal.push(descriptor.id.clone());
                }
                if descriptor.recommended {
                    recommended.push(descriptor.id.clone());
                }
            }
            set.descriptors.push(descriptor);
        }
        if !declared && !set.descriptors.is_empty() {
            set.profiles = vec![
                AccessProfile {
                    name: MINIMAL.to_owned(),
                    title: "Minimal".to_owned(),
                    permissions: minimal,
                },
                AccessProfile {
                    name: RECOMMENDED.to_owned(),
                    title: "Recommended".to_owned(),
                    permissions: recommended,
                },
            ];
        }
        set
    }
}

/// The derived descriptor for one declared capability (ADR-0600 §4).
fn derive_descriptor(
    manifest: &Manifest,
    class: DeclarationClass,
    request: &CapabilityRequest,
) -> PermissionDescriptor {
    let capability = request.capability;
    let scope = match (&request.scope, capability) {
        (Some(scope), _) => Some(ScopeTemplate::Concrete(scope.clone())),
        (None, Capability::RelationWrite) => Some(ScopeTemplate::PackageContributions),
        (None, _) => None,
    };
    let _ = manifest;
    let consent = consent_class(capability, scope.as_ref());
    let phase = match (class, consent) {
        // A prompt can never precede a load, so a required family is decided at install or
        // deliberately — never just in time (ADR-0600 §4).
        (DeclarationClass::Required, ConsentClass::Conditional) => PermissionPhase::Explicit,
        (_, consent) => consent.derived_phase(),
    };
    let recommended = matches!(
        consent,
        ConsentClass::ExtensionLocal | ConsentClass::Observation
    );
    PermissionDescriptor {
        id: derived_id(capability),
        kind: default_kind(capability, scope.as_ref()),
        title: family_title(capability).to_owned(),
        purpose: request
            .purpose
            .as_deref()
            .map(|purpose| sanitize(purpose, PURPOSE_LIMIT)),
        phase,
        recommended,
        risk: minimum_risk(capability, scope.as_ref()),
        grants: vec![GrantTemplate { capability, scope }],
        scope_text: None,
        derived: true,
    }
}

/// The id of a derived permission: the family with `.` as `-`, e.g. `network-connect`.
#[must_use]
pub fn derived_id(capability: Capability) -> String {
    capability.id().replace('.', "-")
}

/// The consent class of a family in a scope (ADR-0600 §2). `relation.write` is class A only
/// when bounded to the package's contributions; every other family's class is fixed.
#[must_use]
pub fn consent_class(capability: Capability, scope: Option<&ScopeTemplate>) -> ConsentClass {
    use Capability as C;
    match capability {
        C::RelationWrite => match scope {
            Some(ScopeTemplate::PackageContributions) => ConsentClass::ExtensionLocal,
            Some(ScopeTemplate::Concrete(scope)) if scope.contains_key("relations") => {
                ConsentClass::ExtensionLocal
            }
            _ => ConsentClass::Explicit,
        },
        C::ClockRead
        | C::StatePersist
        | C::HistoryWrite
        | C::UiView
        | C::UiNotify
        | C::SchemaRead => ConsentClass::ExtensionLocal,
        C::ObjectRead
        | C::ProcessRead
        | C::NetworkObserve
        | C::ServiceRead
        | C::ContainerRead
        | C::RemoteRead
        | C::FilesystemWatch
        | C::NetworkConnect
        | C::ContextRead
        | C::RelationRead
        | C::HistoryRead
        | C::FilesystemRead
        | C::TemporalReadCurrent
        | C::TemporalReadHistory
        | C::TemporalReadEvidence
        | C::ChangePlanRead
        | C::ChangePlanContribute
        | C::VerificationObserve
        | C::RecoveryDiscover
        | C::RecoveryEstimateCost
        | C::SecretUse => ConsentClass::Observation,
        C::ProcessExec | C::ContainerExec => ConsentClass::Conditional,
        C::ProviderMutate
        | C::ProcessSignal
        | C::ServiceMutate
        | C::NetworkListen
        | C::RemoteMutate
        | C::PluginInvoke
        | C::TemporalContributeEvents
        | C::TemporalContributeCausality
        | C::TemporalRecorderManage
        | C::ChangeActionExecute
        | C::RecoveryPrepare
        | C::RecoveryCleanup
        | C::RecoveryQuiesce
        | C::RecoveryTransaction
        | C::ModelInfer => ConsentClass::Explicit,
        // §43.4 lets recovery need stronger privilege than the mutation it undoes, and §13.6
        // and §14.6 make restoring the operation that can lose the most: newer snapshots,
        // clones and everything written since. Class E is the class K11P §7.5 reserves for
        // authority that no unattended acceptance may enable, which is where §48.4 puts it.
        C::RecoveryRestore | C::FilesystemWrite => ConsentClass::Destructive,
    }
}

/// The floor a package may not lower (K11P §6.6).
#[must_use]
pub fn minimum_risk(capability: Capability, scope: Option<&ScopeTemplate>) -> PermissionRisk {
    use Capability as C;
    match capability {
        C::FilesystemRead | C::SecretUse | C::TemporalReadHistory => PermissionRisk::SensitiveRead,
        C::ProcessExec | C::ContainerExec => PermissionRisk::Execute,
        C::FilesystemWrite | C::RecoveryRestore => PermissionRisk::Destructive,
        _ => match consent_class(capability, scope) {
            ConsentClass::ExtensionLocal => PermissionRisk::Local,
            ConsentClass::Observation => PermissionRisk::Observe,
            ConsentClass::Conditional => PermissionRisk::Execute,
            ConsentClass::Explicit => PermissionRisk::Mutate,
            ConsentClass::Destructive => PermissionRisk::Destructive,
        },
    }
}

/// The kind a derived permission takes.
#[must_use]
pub fn default_kind(capability: Capability, scope: Option<&ScopeTemplate>) -> PermissionKind {
    use Capability as C;
    match capability {
        C::NetworkConnect => PermissionKind::ExternalObserve,
        // §16.4 and §39.3: quiescing and transacting reach into an application the package
        // fronts, which is the same authority `provider.mutate` names and not Ono's own state.
        C::ProviderMutate | C::RecoveryQuiesce | C::RecoveryTransaction => {
            PermissionKind::ExternalChange
        }
        C::FilesystemRead | C::FilesystemWatch => PermissionKind::FilesystemRead,
        C::FilesystemWrite => PermissionKind::FilesystemWrite,
        C::ProcessExec | C::ContainerExec => PermissionKind::ExecuteHelper,
        C::SecretUse => PermissionKind::SecretUse,
        C::ModelInfer => PermissionKind::ModelUse,
        C::RelationWrite => match consent_class(capability, scope) {
            ConsentClass::ExtensionLocal => PermissionKind::LocalContribution,
            _ => PermissionKind::HostChange,
        },
        C::ClockRead
        | C::StatePersist
        | C::HistoryWrite
        | C::UiView
        | C::UiNotify
        | C::SchemaRead => PermissionKind::LocalContribution,
        C::ObjectRead
        | C::ProcessRead
        | C::NetworkObserve
        | C::ServiceRead
        | C::ContainerRead
        | C::RemoteRead
        | C::ContextRead
        | C::RelationRead
        | C::HistoryRead
        | C::TemporalReadCurrent
        | C::TemporalReadHistory
        | C::TemporalReadEvidence
        | C::ChangePlanRead
        | C::VerificationObserve
        | C::RecoveryDiscover
        | C::RecoveryEstimateCost => PermissionKind::HostObserve,
        // §48.3: contributing to a plan adds to Ono's own registries within the package's
        // declarations, which is what `local-contribution` names.
        C::ChangePlanContribute => PermissionKind::LocalContribution,
        C::ProcessSignal
        | C::ServiceMutate
        | C::NetworkListen
        | C::RemoteMutate
        | C::TemporalContributeEvents
        | C::TemporalContributeCausality
        | C::TemporalRecorderManage
        | C::ChangeActionExecute
        | C::RecoveryPrepare
        | C::RecoveryRestore
        | C::RecoveryCleanup
        | C::PluginInvoke => PermissionKind::HostChange,
    }
}

/// The host-owned title of a derived permission (`docs/contracts/kuang/permissions.v1.yaml`).
#[must_use]
pub const fn family_title(capability: Capability) -> &'static str {
    use Capability as C;
    match capability {
        C::ObjectRead => "Read objects Ono already knows",
        C::SchemaRead => "Read object schemas",
        C::ProcessRead => "Read processes",
        C::ProcessSignal => "Send signals to processes",
        C::ProcessExec => "Run external programs",
        C::FilesystemRead => "Read files",
        C::FilesystemWrite => "Create, change or delete files",
        C::FilesystemWatch => "Watch files for changes",
        C::NetworkObserve => "Observe network state",
        C::NetworkConnect => "Connect to network hosts",
        C::NetworkListen => "Accept incoming network connections",
        C::ServiceRead => "Read services",
        C::ServiceMutate => "Start, stop or change services",
        C::ContainerRead => "Read containers",
        C::ContainerExec => "Run programs inside containers",
        C::RemoteRead => "Read objects on linked hosts",
        C::RemoteMutate => "Change objects on linked hosts",
        C::HistoryRead => "Read the session history",
        C::HistoryWrite => "Add entries to the session history",
        C::ContextRead => "Read the current context",
        C::UiView => "Show a view in the terminal",
        C::UiNotify => "Show notifications",
        C::RelationRead => "Read relationships",
        C::RelationWrite => "Add relationships to Ono",
        C::SecretUse => "Use secrets without exposing their values",
        C::ModelInfer => "Send requests to a model provider",
        C::PluginInvoke => "Invoke other plugins",
        C::StatePersist => "Keep its own state between sessions",
        C::ClockRead => "Read the clock",
        C::ProviderMutate => "Change resources in the external system",
        C::TemporalReadCurrent => "Read the current time context",
        C::TemporalReadHistory => "Read this machine's recorded history",
        C::TemporalReadEvidence => "Read the evidence behind temporal claims",
        C::TemporalContributeEvents => "Add events to Ono's temporal record",
        C::TemporalContributeCausality => "Add causal explanations to Ono",
        C::TemporalRecorderManage => "Start and stop the history recorder",
        C::ChangePlanRead => "Read change plans and their impact",
        C::ChangePlanContribute => "Contribute to change plans",
        C::ChangeActionExecute => "Carry out changes a plan describes",
        C::VerificationObserve => "Observe whether a change achieved what it intended",
        C::RecoveryDiscover => "Find what could protect a change",
        C::RecoveryPrepare => "Create recovery points",
        C::RecoveryRestore => "Restore state from a recovery point",
        C::RecoveryCleanup => "Remove recovery points",
        C::RecoveryEstimateCost => "Report what recovery points cost",
        C::RecoveryQuiesce => "Pause an application to capture consistent state",
        C::RecoveryTransaction => "Run transactions inside its own provider boundary",
    }
}

/// Package text as the host shows it (K11P §8.3): control characters and escape sequences
/// removed, whitespace collapsed to single spaces, and the length bounded.
#[must_use]
pub fn sanitize(text: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    let mut last_space = true;
    for c in text.chars() {
        if in_escape {
            // A CSI sequence ends at its final byte; anything else after ESC is one character.
            if c.is_ascii_alphabetic() || c == '~' {
                in_escape = false;
            }
            continue;
        }
        if c == '\u{1b}' {
            in_escape = true;
            continue;
        }
        if c.is_control() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        out.push(c);
        last_space = false;
    }
    let mut out = out.trim().to_owned();
    if out.chars().count() > limit {
        out = out
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>()
            + "…";
    }
    out
}

/// Whether `id` is a permission or profile id: a kebab-case ASCII slug (K11P §6.3).
#[must_use]
pub fn is_permission_id(id: &str) -> bool {
    crate::is_role_word(id)
}

/// The human rendering of a scope (K11P §19.1), beside which every detailed view also shows
/// the exact record.
#[must_use]
pub fn describe_scope(capability: Capability, scope: Option<&ScopeTemplate>) -> String {
    match scope {
        None => match capability.scope_keys().is_empty() {
            true => String::new(),
            false => "unscoped".to_owned(),
        },
        Some(ScopeTemplate::RuntimeDerived) => {
            "the exact value in use, derived at the time".to_owned()
        }
        Some(ScopeTemplate::PackageContributions) => {
            "the package's own declared relationships".to_owned()
        }
        Some(ScopeTemplate::ProviderInstance) => "the provider instance it fronts".to_owned(),
        Some(ScopeTemplate::Concrete(scope)) => {
            let mut parts = Vec::new();
            for (key, value) in scope {
                let values: Vec<String> = match value {
                    Json::Array(items) => items
                        .iter()
                        .map(|item| match item {
                            Json::String(text) => text.clone(),
                            other => other.to_string(),
                        })
                        .collect(),
                    Json::String(text) => vec![text.clone()],
                    other => vec![other.to_string()],
                };
                parts.push(format!("{key} {}", values.join(", ")));
            }
            parts.join("; ")
        }
    }
}

// --- validation of a declared section (ADR-0600 §3) --------------------------------------------

/// The raw `permissions` section, as the manifest parser hands it over.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPermissions {
    /// `profiles: {<name>: {title, permissions}}`.
    #[serde(default)]
    pub profiles: BTreeMap<String, RawProfile>,
    /// `requests: [{id, kind, title, purpose, phase, recommended, risk, grants, scope_text}]`.
    #[serde(default)]
    pub requests: Vec<RawPermissionRequest>,
}

/// One raw profile.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProfile {
    /// The profile's title.
    #[serde(default)]
    pub title: Option<String>,
    /// The permission ids.
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// One raw permission request.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPermissionRequest {
    /// The permission id.
    pub id: String,
    /// The kind word.
    pub kind: String,
    /// The title.
    pub title: String,
    /// The purpose.
    #[serde(default)]
    pub purpose: Option<String>,
    /// The phase word.
    pub phase: String,
    /// Whether it is recommended.
    #[serde(default)]
    pub recommended: bool,
    /// The risk word, when the package raises it.
    #[serde(default)]
    pub risk: Option<String>,
    /// The grants.
    #[serde(default)]
    pub grants: Vec<RawGrantTemplate>,
    /// The package's rendering of the scope.
    #[serde(default)]
    pub scope_text: Option<String>,
}

/// One raw grant template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawGrantTemplate {
    /// The capability id.
    pub capability: String,
    /// The scope: a record, or one of the three words.
    #[serde(default)]
    pub scope: Option<Json>,
}

fn invalid(detail: impl Into<String>) -> KuangError {
    KuangError::new(KuangErrorCode::PermissionInvalidMapping, detail.into()).with_help(
        "a permission maps to capabilities the package declares, in a phase and at a risk the \
         host allows for them (ADR-0600, K11P §6, §8, §9)",
    )
}

fn text_field(field: &str, id: &str, text: &str, limit: usize) -> Result<String, KuangError> {
    if text.trim().is_empty() {
        return Err(invalid(format!("permission `{id}` has an empty `{field}`")));
    }
    if text.contains(['\n', '\r']) {
        return Err(invalid(format!(
            "permission `{id}`'s `{field}` contains a line break, which could spoof the text \
             beside it (K11P §8.3)"
        )));
    }
    if text.chars().count() > limit {
        return Err(invalid(format!(
            "permission `{id}`'s `{field}` is longer than {limit} characters"
        )));
    }
    Ok(sanitize(text, limit))
}

/// Validates a declared `permissions` section against the manifest's own capability
/// declarations, answering the set every rule accepted (ADR-0600 §3).
///
/// # Errors
///
/// `permission.invalid_mapping` naming the first rule that failed.
pub fn validate_declared(
    raw: RawPermissions,
    requests: &[(DeclarationClass, &CapabilityRequest)],
) -> Result<PermissionSet, KuangError> {
    let mut descriptors: Vec<PermissionDescriptor> = Vec::new();
    for request in raw.requests {
        let id = request.id.clone();
        if !is_permission_id(&id) {
            return Err(invalid(format!(
                "`{id}` is not a permission id: a kebab-case slug of `[a-z][a-z0-9-]*`"
            )));
        }
        if descriptors.iter().any(|existing| existing.id == id) {
            return Err(invalid(format!("permission `{id}` is declared twice")));
        }
        let kind = PermissionKind::from_id(&request.kind).ok_or_else(|| {
            invalid(format!(
                "permission `{id}` names the kind `{}`, which the host does not define; the kinds \
                 are {}",
                request.kind,
                words(PermissionKind::ALL.iter().map(|kind| kind.id()))
            ))
        })?;
        let phase = PermissionPhase::from_id(&request.phase).ok_or_else(|| {
            invalid(format!(
                "permission `{id}` names the phase `{}`; the phases are {}",
                request.phase,
                words(PermissionPhase::ALL.iter().map(|phase| phase.id()))
            ))
        })?;
        let title = text_field("title", &id, &request.title, TITLE_LIMIT)?;
        let purpose = request
            .purpose
            .as_deref()
            .map(|purpose| text_field("purpose", &id, purpose, PURPOSE_LIMIT))
            .transpose()?;
        let scope_text = request
            .scope_text
            .as_deref()
            .map(|text| text_field("scope_text", &id, text, PURPOSE_LIMIT))
            .transpose()?;
        if request.grants.is_empty() {
            return Err(invalid(format!(
                "permission `{id}` grants nothing; a permission is a projection of at least one \
                 capability (K11P §3 invariant 5)"
            )));
        }
        let mut grants = Vec::new();
        for grant in request.grants {
            let capability = Capability::from_id(&grant.capability).ok_or_else(|| {
                invalid(format!(
                    "permission `{id}` grants `{}`, which is not a capability",
                    grant.capability
                ))
            })?;
            let declared = requests
                .iter()
                .find(|(_, request)| request.capability == capability);
            let Some((class, _)) = declared else {
                return Err(invalid(format!(
                    "permission `{id}` grants `{capability}`, which the manifest's `capabilities` \
                     section does not declare"
                )));
            };
            let scope = grant
                .scope
                .as_ref()
                .map(|scope| parse_scope(&id, capability, scope))
                .transpose()?;
            let consent = consent_class(capability, scope.as_ref());
            if !consent.allowed_phases().contains(&phase) {
                return Err(invalid(format!(
                    "permission `{id}` decides `{capability}` in phase `{phase}`, and a class \
                     {} ({consent}) capability may only be decided {}",
                    consent.letter(),
                    words(consent.allowed_phases().iter().map(|phase| phase.id()))
                )));
            }
            if *class == DeclarationClass::Required && phase == PermissionPhase::Jit {
                return Err(invalid(format!(
                    "permission `{id}` decides the required capability `{capability}` just in \
                     time, and a prompt can never precede a load"
                )));
            }
            grants.push(GrantTemplate { capability, scope });
        }
        let floor = grants
            .iter()
            .map(|grant| minimum_risk(grant.capability, grant.scope.as_ref()))
            .max()
            .unwrap_or(PermissionRisk::Local);
        let risk = match request.risk.as_deref() {
            None => floor,
            Some(word) => {
                let declared = PermissionRisk::from_id(word).ok_or_else(|| {
                    invalid(format!(
                        "permission `{id}` names the risk `{word}`; the risks are {}",
                        words(PermissionRisk::ALL.iter().map(|risk| risk.id()))
                    ))
                })?;
                if declared < floor {
                    return Err(invalid(format!(
                        "permission `{id}` declares the risk `{declared}`, and the host derives \
                         at least `{floor}` from what it grants; a package may raise the risk \
                         and never lower it (K11P §6.6)"
                    )));
                }
                declared
            }
        };
        let descriptor = PermissionDescriptor {
            id: id.clone(),
            kind,
            title,
            purpose,
            phase,
            recommended: request.recommended,
            risk,
            grants,
            scope_text,
            derived: false,
        };
        if request.recommended && !descriptor.is_safe_default() {
            return Err(invalid(format!(
                "permission `{id}` is marked recommended and is not a safe default: it must be \
                 decided at install or automatic, below the `mutate` risk, over class A or B \
                 capabilities — or class C with a concrete program (K11P §9.1)"
            )));
        }
        descriptors.push(descriptor);
    }

    let mut profiles = Vec::new();
    for (name, profile) in raw.profiles {
        if !is_permission_id(&name) {
            return Err(invalid(format!(
                "`{name}` is not a profile name: a kebab-case slug"
            )));
        }
        let mut seen = BTreeSet::new();
        for id in &profile.permissions {
            if descriptors.iter().all(|descriptor| &descriptor.id != id) {
                return Err(invalid(format!(
                    "profile `{name}` names the permission `{id}`, which the package does not \
                     declare"
                )));
            }
            if !seen.insert(id.clone()) {
                return Err(invalid(format!("profile `{name}` names `{id}` twice")));
            }
        }
        let title = match profile.title.as_deref() {
            Some(title) => text_field("title", &name, title, TITLE_LIMIT)?,
            None => name.clone(),
        };
        profiles.push(AccessProfile {
            name,
            title,
            permissions: profile.permissions,
        });
    }
    if !descriptors.is_empty() {
        for required in [MINIMAL, RECOMMENDED] {
            if profiles.iter().all(|profile| profile.name != required) {
                return Err(invalid(format!(
                    "a package with permission requests must offer a `{required}` profile \
                     (K11P §9.1)"
                )));
            }
        }
        for name in [MINIMAL, RECOMMENDED] {
            let Some(profile) = profiles.iter().find(|profile| profile.name == name) else {
                continue;
            };
            for id in &profile.permissions {
                let descriptor = descriptors
                    .iter()
                    .find(|descriptor| &descriptor.id == id)
                    .ok_or_else(|| invalid(format!("profile `{name}` names `{id}`")))?;
                if !descriptor.is_safe_default() {
                    return Err(invalid(format!(
                        "profile `{name}` includes `{id}`, which is not a safe default: a \
                         `{name}` profile never carries an explicit, just-in-time, mutating or \
                         destructive permission (K11P §9.1, §21.4)"
                    )));
                }
            }
        }
    }

    Ok(PermissionSet {
        descriptors,
        profiles,
        declared: true,
    })
}

fn parse_scope(
    id: &str,
    capability: Capability,
    scope: &Json,
) -> Result<ScopeTemplate, KuangError> {
    match scope {
        Json::String(word) => match word.as_str() {
            "runtime-derived" => {
                if capability.scope_keys().is_empty() {
                    return Err(invalid(format!(
                        "permission `{id}` derives a scope for `{capability}` at runtime, and \
                         the family has no scope to derive"
                    )));
                }
                Ok(ScopeTemplate::RuntimeDerived)
            }
            "package-contributions" if capability == Capability::RelationWrite => {
                Ok(ScopeTemplate::PackageContributions)
            }
            "provider-instance" if capability == Capability::ProviderMutate => {
                Ok(ScopeTemplate::ProviderInstance)
            }
            other => Err(invalid(format!(
                "permission `{id}` scopes `{capability}` by `{other}`, which is neither a record \
                 of scope keys nor a scope word that family accepts (`runtime-derived`; \
                 `package-contributions` for relation.write; `provider-instance` for \
                 provider.mutate)"
            ))),
        },
        Json::Object(record) => {
            let declared = capability.scope_keys();
            for key in record.keys() {
                if !declared.iter().any(|candidate| candidate.name == key) {
                    return Err(invalid(format!(
                        "permission `{id}` scopes `{capability}` by `{key}`, which the family \
                         does not declare (capabilities.v1.yaml)"
                    )));
                }
            }
            Ok(ScopeTemplate::Concrete(record.clone()))
        }
        Json::Null => Err(invalid(format!(
            "permission `{id}` writes a null scope for `{capability}`; omit `scope` to grant \
             it unscoped"
        ))),
        _ => Err(invalid(format!(
            "permission `{id}`'s scope for `{capability}` must be a record or a scope word"
        ))),
    }
}

fn words<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

// --- the permission delta of an upgrade (K11P §21.3, ADR-0604 §3) ------------------------------

closed_words! {
    /// Why an upgrade needs renewed consent for one permission.
    DeltaKind {
        Added => "added", "A permission newly in the selected profile.";
        CapabilityAdded => "capability-added", "A permission that resolves to a capability it did not before.";
        ScopeWidened => "scope-widened", "A scope that became broader.";
        RiskRaised => "risk-raised", "A risk that rose.";
        PhaseLoosened => "phase-loosened", "A permission that moved from asked or explicit towards automatic or recommended.";
        EnforcementWeakened => "enforcement-weakened", "A boundary that became advisory.";
        Repurposed => "repurposed", "A permission id reused for materially different authority.";
    }
}

/// One line of an upgrade's permission delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaEntry {
    /// The permission id.
    pub permission: String,
    /// Why consent is needed.
    pub kind: DeltaKind,
    /// The human line, e.g. `+ Read ~/.config/cloud/**`.
    pub detail: String,
}

/// Whether `after` needs more than `before` had, for the permissions the profile named
/// `profile` selects. Empty means the upgrade may keep every decision (K11P §21.2).
#[must_use]
pub fn delta(before: &PermissionSet, after: &PermissionSet, profile: &str) -> Vec<DeltaEntry> {
    let mut entries = Vec::new();
    let selected: Vec<&PermissionDescriptor> = match after.profile(profile) {
        Some(profile) => after
            .profile_descriptors(profile)
            .chain(after.automatic())
            .collect(),
        None => after.automatic().collect(),
    };
    let previously: BTreeSet<&str> = match before.profile(profile) {
        Some(profile) => before
            .profile_descriptors(profile)
            .chain(before.automatic())
            .map(|descriptor| descriptor.id.as_str())
            .collect(),
        None => before
            .automatic()
            .map(|descriptor| descriptor.id.as_str())
            .collect(),
    };
    let mut seen = BTreeSet::new();
    for descriptor in selected {
        if !seen.insert(descriptor.id.clone()) {
            continue;
        }
        let Some(old) = before.descriptor(&descriptor.id) else {
            entries.push(DeltaEntry {
                permission: descriptor.id.clone(),
                kind: DeltaKind::Added,
                detail: format!("+ {}", descriptor.title),
            });
            continue;
        };
        if !previously.contains(descriptor.id.as_str()) {
            entries.push(DeltaEntry {
                permission: descriptor.id.clone(),
                kind: DeltaKind::Added,
                detail: format!("+ {}", descriptor.title),
            });
            continue;
        }
        entries.extend(compare(old, descriptor));
    }
    // A permission that changed while it stays outside every profile is still a repurposed id
    // if its capabilities changed shape: the id is what a stored decision names.
    for descriptor in &after.descriptors {
        if seen.contains(&descriptor.id) {
            continue;
        }
        if let Some(old) = before.descriptor(&descriptor.id) {
            entries.extend(
                compare(old, descriptor)
                    .into_iter()
                    .filter(|entry| entry.kind == DeltaKind::Repurposed),
            );
        }
    }
    entries
}

fn compare(old: &PermissionDescriptor, new: &PermissionDescriptor) -> Vec<DeltaEntry> {
    let mut entries = Vec::new();
    let entry = |kind: DeltaKind, detail: String| DeltaEntry {
        permission: new.id.clone(),
        kind,
        detail,
    };
    let old_capabilities: BTreeSet<Capability> = old.capabilities().collect();
    let new_capabilities: BTreeSet<Capability> = new.capabilities().collect();
    if old.kind != new.kind || !new_capabilities.is_superset(&old_capabilities) {
        entries.push(entry(
            DeltaKind::Repurposed,
            format!(
                "~ {} now means {} ({})",
                new.id,
                new.title,
                words(new_capabilities.iter().map(|capability| capability.id()))
            ),
        ));
        return entries;
    }
    for capability in new_capabilities.difference(&old_capabilities) {
        entries.push(entry(
            DeltaKind::CapabilityAdded,
            format!("+ {} ({capability})", new.title),
        ));
    }
    for grant in &new.grants {
        let Some(previous) = old
            .grants
            .iter()
            .find(|previous| previous.capability == grant.capability)
        else {
            continue;
        };
        if let Some(why) = widened(previous.scope.as_ref(), grant.scope.as_ref()) {
            entries.push(entry(
                DeltaKind::ScopeWidened,
                format!("+ {} — {why}", new.title),
            ));
        }
        if previous.enforcement() == Enforcement::Broker
            && grant.enforcement() == Enforcement::Advisory
        {
            entries.push(entry(
                DeltaKind::EnforcementWeakened,
                format!(
                    "~ {} — the {} scope is now advisory rather than enforced",
                    new.title, grant.capability
                ),
            ));
        }
    }
    if new.risk > old.risk {
        entries.push(entry(
            DeltaKind::RiskRaised,
            format!("~ {} — risk {} → {}", new.title, old.risk, new.risk),
        ));
    }
    if new.phase < old.phase || (new.recommended && !old.recommended) {
        entries.push(entry(
            DeltaKind::PhaseLoosened,
            format!(
                "~ {} — decided {} rather than {}",
                new.title, new.phase, old.phase
            ),
        ));
    }
    entries
}

/// Whether `after` covers more than `before`, and how.
fn widened(before: Option<&ScopeTemplate>, after: Option<&ScopeTemplate>) -> Option<String> {
    use ScopeTemplate as S;
    match (before, after) {
        (None, _) => None,
        (Some(_), None) => Some("no longer bounded by a scope".to_owned()),
        (Some(S::Concrete(old)), Some(S::Concrete(new))) => {
            for (key, old_value) in old {
                let Some(new_value) = new.get(key) else {
                    return Some(format!("`{key}` is no longer bounded"));
                };
                let old_items = list_of(old_value);
                let new_items = list_of(new_value);
                let extra: Vec<&String> = new_items
                    .iter()
                    .filter(|item| !old_items.contains(item))
                    .collect();
                if !extra.is_empty() {
                    return Some(format!(
                        "`{key}` now covers {}",
                        extra
                            .iter()
                            .map(|item| format!("`{item}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            None
        }
        (Some(S::Concrete(_)), Some(other)) => Some(format!(
            "the scope became `{}` rather than the exact values it named",
            other.word().unwrap_or_default()
        )),
        (Some(S::PackageContributions), Some(S::Concrete(scope)))
            if scope.contains_key("relations") =>
        {
            None
        }
        (Some(S::PackageContributions | S::ProviderInstance), Some(S::Concrete(_))) => {
            Some("the scope is now written out rather than bounded to the package".to_owned())
        }
        (Some(S::RuntimeDerived), Some(S::Concrete(_))) => None,
        (Some(S::RuntimeDerived), Some(S::RuntimeDerived))
        | (Some(S::PackageContributions), Some(S::PackageContributions))
        | (Some(S::ProviderInstance), Some(S::ProviderInstance)) => None,
        (Some(old), Some(new)) => Some(format!(
            "the scope changed from `{}` to `{}`",
            old.word().unwrap_or_default(),
            new.word().unwrap_or_default()
        )),
    }
}

fn list_of(value: &Json) -> Vec<String> {
    match value {
        Json::Array(items) => items
            .iter()
            .map(|item| match item {
                Json::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect(),
        Json::String(text) => vec![text.clone()],
        other => vec![other.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_place_every_family_in_exactly_one_class_when_classified() {
        for capability in Capability::ALL {
            let class = consent_class(*capability, None);
            let floor = minimum_risk(*capability, None);
            assert!(
                class.allowed_phases().contains(&class.derived_phase()),
                "{capability}: the derived phase must be one the class allows"
            );
            if class == ConsentClass::Explicit || class == ConsentClass::Destructive {
                assert!(floor >= PermissionRisk::Mutate, "{capability}");
            }
        }
    }

    #[test]
    fn should_bound_relation_write_to_class_a_only_when_scoped_to_the_package() {
        assert_eq!(
            consent_class(
                Capability::RelationWrite,
                Some(&ScopeTemplate::PackageContributions)
            ),
            ConsentClass::ExtensionLocal
        );
        assert_eq!(
            consent_class(Capability::RelationWrite, None),
            ConsentClass::Explicit
        );
    }

    #[test]
    fn should_strip_escape_sequences_and_bound_the_length_when_sanitising() {
        let text = "Harmless\u{1b}[31m theme\n access\u{7}";
        assert_eq!(sanitize(text, TITLE_LIMIT), "Harmless theme access");
        let long = "x".repeat(200);
        assert_eq!(sanitize(&long, 10).chars().count(), 10);
    }

    #[test]
    fn should_describe_a_concrete_scope_in_words() {
        let mut scope = JsonMap::new();
        scope.insert(
            "paths".to_owned(),
            Json::Array(vec![Json::String("~/.kube/config".into())]),
        );
        assert_eq!(
            describe_scope(
                Capability::FilesystemRead,
                Some(&ScopeTemplate::Concrete(scope))
            ),
            "paths ~/.kube/config"
        );
    }
}
