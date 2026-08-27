//! The `kuang-package/1` manifest: parsing and fail-closed validation (spec §31.5, §31.7).
//!
//! The manifest is read and judged before any package byte executes — spec §31.89 rule 1,
//! "Manifest before code". Every section that carries authority is closed: an unknown key in it
//! invalidates the manifest rather than being ignored (`docs/spec/kuang/manifest.v1.yaml`,
//! ADR-0022 §10). Every identity rule of spec §31.5 is checked here, and a manifest that fails
//! one is `package.invalid`, never merely warned about.

use std::str::FromStr;

use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as Json};

use crate::{
    ApiVersion, Capability, DeclarationClass, KuangError, KuangErrorCode, OverflowPolicy,
    PACKAGE_FORMAT, VersionRange,
};

/// An extension role of spec §31.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Derives findings from objects and snapshots.
    Analysis,
    /// Exposes external or system resources as targets.
    Provider,
    /// Structures the output of an external protocol or tool.
    Adapter,
    /// A specialised renderer or TUI lens.
    View,
    /// Consumes live streams with bounded state.
    EventProcessor,
    /// Reasons over context and proposes tool calls.
    Assistant,
    /// Reacts to events or schedules.
    Automation,
    /// Executes on a linked host (spec §31.39).
    RemoteComponent,
}

/// The isolation tier a runtime artifact declares, named by what it is (spec §31.10,
/// ADR-0022 §15). T0 has no manifest spelling: in-process code is reserved for Ono itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    /// T2, the default tier for third-party code.
    WasmComponent,
    /// T1, a separate process speaking the native protocol. Weaker isolation, stronger trust.
    NativeProcess,
    /// T3, a protocol endpoint. No local code at all.
    RemoteService,
    /// Runs nothing; the package is contributions only.
    Declarative,
}

/// The scheduling class the supervisor applies (spec §31.15, §31.67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CpuBudget {
    /// Must never block terminal input.
    Interactive,
    /// Ordinary work.
    Batch,
    /// Yields to everything else.
    Background,
}

/// When the runtime instance is created (spec §31.68).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Startup {
    /// Metadata registers as placeholders; code loads on first invocation. The normal choice.
    Lazy,
    /// A candidate for loading at session start. Ono startup does not wait for it.
    Preload,
}

/// The state classes of spec §31.31.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Persistence {
    /// Dies with the invocation.
    Invocation,
    /// Dies with the view.
    View,
    /// Dies with the session.
    Session,
    /// Survives the session. Requires `state.persist`, a quota and a version.
    Persistent,
    /// A cache the host may clear at any time.
    SharedCache,
}

/// The outbound network posture (spec §31.21). There is no third answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outbound {
    /// No outbound connections at all; the supervisor refuses any attempt.
    None,
    /// Every connection goes through the host under `network.connect` with a host/port scope.
    Brokered,
}

/// One requested capability, in any of the three spellings spec §31.7 allows.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityRequest {
    /// The family being requested.
    pub capability: Capability,
    /// The requested scope. `None` asks unscoped, which policy may refuse.
    pub scope: Option<JsonMap<String, Json>>,
    /// Which of the package's roles needs it. `None` means all declared roles.
    pub roles: Option<Vec<String>>,
    /// Package-authored purpose text. Sanitised before display, never trusted as a claim.
    pub purpose: Option<String>,
}

/// The `package` section: who this is, immutably (spec §31.5).
#[derive(Debug, Clone, PartialEq)]
pub struct PackageInfo {
    /// The immutable reverse-DNS package id, e.g. `dev.example.packet-eye`.
    pub id: String,
    /// The display name. Mutable, need not be unique, never load-bearing.
    pub name: String,
    /// The release version, semantic-version syntax.
    pub version: String,
    /// One line, what the package is for.
    pub description: String,
    /// The publisher namespace the id must begin with.
    pub publisher: String,
    /// An SPDX identifier.
    pub license: String,
    /// A URL for humans. Never fetched by the shell.
    pub homepage: Option<String>,
}

/// The `compatibility` section: the version dimensions of spec §31.62, declared independently.
#[derive(Debug, Clone, PartialEq)]
pub struct Compatibility {
    /// The host API range the package speaks, e.g. `>=11.1 <12`.
    pub kuang_api: VersionRange,
    /// The Ono language version range its contributed contracts assume.
    pub ono_language: String,
    /// The value protocol range. Absent means the one the host API implies.
    pub value_protocol: Option<String>,
    /// The schema language range. Absent means the host default.
    pub schema_language: Option<String>,
    /// Required only when the package contributes a view.
    pub view_protocol: Option<String>,
    /// Required only when the package requests `model.infer`.
    pub model_broker: Option<String>,
    /// Required only when the package declares a remote component.
    pub remote_extension: Option<String>,
    /// The platform tuples the runtime artifact supports, e.g. `linux-amd64`.
    pub platforms: Vec<String>,
}

/// The `runtime` section: how the code is isolated (spec §31.10, §31.15).
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    /// The isolation tier, named by what the artifact is.
    pub kind: RuntimeKind,
    /// The artifact inside the archive. Required unless `kind` runs nothing locally.
    pub entry: Option<String>,
    /// The protocol endpoint for `remote-service`. Declaring it grants nothing.
    pub endpoint: Option<String>,
    /// The instance's declared memory ceiling, in bytes. Host policy caps it.
    pub memory_max: u64,
    /// The scheduling class.
    pub cpu_budget: CpuBudget,
    /// Lazy or preload.
    pub startup: Startup,
    /// Preferred cap on in-flight host calls. `None` accepts the host default.
    pub max_concurrent_calls: Option<u32>,
    /// The package's preferred overflow policy. Host policy has final authority (spec §31.15).
    pub overflow: Option<OverflowPolicy>,
}

/// The `state` section (spec §31.31, §31.32).
#[derive(Debug, Clone, PartialEq)]
pub struct StateDeclaration {
    /// The state class.
    pub persistence: Persistence,
    /// The requested ceiling in bytes. Required for `persistent`.
    pub quota: Option<u64>,
    /// The state format version. Required for `persistent`.
    pub version: Option<u32>,
    /// The migration steps, one per version step (spec §31.32). Carried as data; migrations
    /// run in a later increment of Phase I.
    pub migrations: Option<Json>,
}

/// The `network` section (spec §31.21). Required even when the answer is `none`.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkDeclaration {
    /// The outbound posture.
    pub outbound: Outbound,
    /// The declared destinations, for `brokered`. `None` for `none`.
    pub destinations: Option<Vec<JsonMap<String, Json>>>,
}

/// The `contributions` section: paths inside the archive (spec §31.22–§31.27).
///
/// These are the *declarations*; the documents themselves cross the handshake as a
/// [`crate::ContributionSet`], which is what the host validates and registers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContributionPaths {
    /// Command contracts (spec §31.22).
    pub commands: Option<Vec<String>>,
    /// Object schemas (spec §31.23).
    pub schemas: Option<Vec<String>>,
    /// Target declarations (spec §31.23).
    pub targets: Option<Vec<String>>,
    /// View declarations (spec §31.27).
    pub views: Option<Vec<String>>,
    /// Relation shapes as `from->to` (spec §31.7).
    pub relations: Option<Vec<String>>,
    /// Namespaced annotation keys (spec §31.23).
    pub annotations: Option<Vec<String>>,
    /// Assistant tool descriptors (spec §31.46).
    pub tools: Option<Vec<String>>,
}

/// The `dependencies` section (spec §31.30). Composition stays protocol-mediated.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dependencies {
    /// Package dependencies as `{id, version}` records.
    pub packages: Option<Json>,
    /// Schema dependencies as `{id, version}` records.
    pub schemas: Option<Json>,
    /// Capabilities another package must hold for a composition to work. Informational.
    pub capabilities: Option<Vec<String>>,
    /// Whether an unresolved dependency degrades rather than blocks.
    pub optional: Option<bool>,
}

/// A parsed, validated `kuang-package/1` manifest.
///
/// Constructed only through [`Manifest::parse`], so a value of this type is a manifest every
/// identity rule of spec §31.5 has already accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// The `package` section.
    pub package: PackageInfo,
    /// The `compatibility` section.
    pub compatibility: Compatibility,
    /// The `runtime` section. `None` for a declarative-only package.
    pub runtime: Option<Runtime>,
    /// The declared roles, at least one.
    pub roles: Vec<Role>,
    /// Capabilities the package cannot load without.
    pub required_capabilities: Vec<CapabilityRequest>,
    /// Capabilities the package can adapt to losing.
    pub optional_capabilities: Vec<CapabilityRequest>,
    /// Capabilities the package may request later, against an explicit user action.
    pub runtime_requested_capabilities: Vec<CapabilityRequest>,
    /// The `state` section. `None` means nothing outlives one invocation.
    pub state: Option<StateDeclaration>,
    /// The `network` section.
    pub network: NetworkDeclaration,
    /// The `contributions` section. `None` means the package contributes nothing.
    pub contributions: Option<ContributionPaths>,
    /// The `dependencies` section.
    pub dependencies: Option<Dependencies>,
    /// The `remote` section, carried as data (spec §31.39). Remote components are a later
    /// increment of Phase I; a local-only supervisor ignores but preserves the declaration.
    pub remote: Option<Json>,
}

impl Manifest {
    /// Parses and validates a manifest document.
    ///
    /// # Errors
    ///
    /// Returns `package.invalid` for a document that is not YAML, misses a mandatory field,
    /// carries an unknown key in a closed section, or breaks an identity rule of spec §31.5 —
    /// including a third-party claim on the `ono.*` namespace.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        let invalid = |detail: String| {
            KuangError::new(KuangErrorCode::PackageInvalid, detail)
                .with_help("`verify plugin <reference>` reports which rule failed (spec §31.7)")
        };
        let raw: RawManifest = serde_yaml_ng::from_str(text)
            .map_err(|error| invalid(format!("not a valid kuang-package/1 manifest: {error}")))?;
        if raw.format != PACKAGE_FORMAT {
            return Err(invalid(format!(
                "format is `{}`, this host reads `{PACKAGE_FORMAT}`",
                raw.format
            )));
        }
        validate_package(&raw.package)?;
        let compatibility = validate_compatibility(raw.compatibility)?;
        let runtime = raw.runtime.map(validate_runtime).transpose()?;
        if raw.roles.is_empty() {
            return Err(invalid(
                "`roles` must declare at least one role (spec §31.4)".into(),
            ));
        }
        let roles = raw
            .roles
            .iter()
            .map(|role| {
                serde_json::from_value::<Role>(Json::String(role.clone()))
                    .map_err(|_| invalid(format!("`{role}` is not a role of spec §31.4")))
            })
            .collect::<Result<Vec<Role>, KuangError>>()?;
        if roles.contains(&Role::Assistant) != raw.assistant.is_some() {
            return Err(invalid(
                "the `assistant` section is required for the assistant role and forbidden otherwise"
                    .into(),
            ));
        }
        let capabilities = raw.capabilities.unwrap_or_default();
        let required = validate_capability_entries(capabilities.required)?;
        let optional = validate_capability_entries(capabilities.optional)?;
        let runtime_requested = validate_capability_entries(capabilities.runtime_requested)?;
        let state = raw.state.map(validate_state).transpose()?;
        if let Some(state) = &state
            && state.persistence == Persistence::Persistent
        {
            let requests_persist = [&required, &optional, &runtime_requested]
                .iter()
                .any(|list| {
                    list.iter()
                        .any(|request| request.capability == Capability::StatePersist)
                });
            if !requests_persist {
                return Err(invalid(
                    "persistent state requires the `state.persist` capability (spec §31.31)".into(),
                ));
            }
        }
        let network = validate_network(raw.network)?;
        Ok(Self {
            package: PackageInfo {
                id: raw.package.id,
                name: raw.package.name,
                version: raw.package.version,
                description: raw.package.description,
                publisher: raw.package.publisher,
                license: raw.package.license,
                homepage: raw.package.homepage,
            },
            compatibility,
            runtime,
            roles,
            required_capabilities: required,
            optional_capabilities: optional,
            runtime_requested_capabilities: runtime_requested,
            state,
            network,
            contributions: raw.contributions.map(|raw| ContributionPaths {
                commands: raw.commands,
                schemas: raw.schemas,
                targets: raw.targets,
                views: raw.views,
                relations: raw.relations,
                annotations: raw.annotations,
                tools: raw.tools,
            }),
            dependencies: raw.dependencies.map(|raw| Dependencies {
                packages: raw.packages,
                schemas: raw.schemas,
                capabilities: raw.capabilities,
                optional: raw.optional,
            }),
            remote: raw.remote,
        })
    }

    /// Checks the manifest against the running host (spec §31.7, §31.62).
    ///
    /// # Errors
    ///
    /// Returns `package.incompatible` naming exactly which dimension is unmet: the host API
    /// version outside `compatibility.kuang_api`, or a platform not in `platforms`.
    pub fn check_host(&self, host_api: ApiVersion, platform: &str) -> Result<(), KuangError> {
        if !self.compatibility.kuang_api.contains(host_api) {
            return Err(KuangError::new(
                KuangErrorCode::PackageIncompatible,
                format!(
                    "the package requires kuang-host `{}`, this host is `{host_api}`",
                    self.compatibility.kuang_api
                ),
            )
            .with_metadata("dimension", Json::String("kuang_api".into()))
            .with_metadata(
                "required",
                Json::String(self.compatibility.kuang_api.source().to_owned()),
            ));
        }
        if !self
            .compatibility
            .platforms
            .iter()
            .any(|candidate| candidate == platform)
        {
            return Err(KuangError::new(
                KuangErrorCode::PackageIncompatible,
                format!("the package does not support platform `{platform}`"),
            )
            .with_metadata("dimension", Json::String("platforms".into())));
        }
        Ok(())
    }

    /// Every declared capability request with its class, in declaration order.
    pub fn capability_requests(
        &self,
    ) -> impl Iterator<Item = (DeclarationClass, &CapabilityRequest)> {
        let required = self
            .required_capabilities
            .iter()
            .map(|request| (DeclarationClass::Required, request));
        let optional = self
            .optional_capabilities
            .iter()
            .map(|request| (DeclarationClass::Optional, request));
        let runtime = self
            .runtime_requested_capabilities
            .iter()
            .map(|request| (DeclarationClass::RuntimeRequested, request));
        required.chain(optional).chain(runtime)
    }
}

// --- raw shapes: closed sections via deny_unknown_fields --------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    format: String,
    package: RawPackage,
    compatibility: RawCompatibility,
    #[serde(default)]
    runtime: Option<RawRuntime>,
    roles: Vec<String>,
    #[serde(default)]
    contributions: Option<RawContributions>,
    #[serde(default)]
    capabilities: Option<RawCapabilities>,
    #[serde(default)]
    state: Option<RawState>,
    network: RawNetwork,
    #[serde(default)]
    dependencies: Option<RawDependencies>,
    #[serde(default)]
    remote: Option<Json>,
    #[serde(default)]
    assistant: Option<Json>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackage {
    id: String,
    name: String,
    version: String,
    description: String,
    publisher: String,
    license: String,
    #[serde(default)]
    homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCompatibility {
    kuang_api: String,
    ono_language: String,
    #[serde(default)]
    value_protocol: Option<String>,
    #[serde(default)]
    schema_language: Option<String>,
    #[serde(default)]
    view_protocol: Option<String>,
    #[serde(default)]
    model_broker: Option<String>,
    #[serde(default)]
    remote_extension: Option<String>,
    platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuntime {
    kind: RuntimeKind,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    memory_max: Json,
    cpu_budget: CpuBudget,
    startup: Startup,
    #[serde(default)]
    max_concurrent_calls: Option<u32>,
    #[serde(default)]
    overflow: Option<OverflowPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContributions {
    #[serde(default)]
    commands: Option<Vec<String>>,
    #[serde(default)]
    schemas: Option<Vec<String>>,
    #[serde(default)]
    targets: Option<Vec<String>>,
    #[serde(default)]
    views: Option<Vec<String>>,
    #[serde(default)]
    relations: Option<Vec<String>>,
    #[serde(default)]
    annotations: Option<Vec<String>>,
    #[serde(default)]
    tools: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapabilities {
    #[serde(default)]
    required: Vec<RawCapabilityEntry>,
    #[serde(default)]
    optional: Vec<RawCapabilityEntry>,
    #[serde(default)]
    runtime_requested: Vec<RawCapabilityEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawCapabilityEntry {
    /// `- process.read`
    Bare(String),
    /// `- capability: filesystem.read` with optional scope, roles and purpose.
    Full {
        capability: String,
        #[serde(default)]
        scope: Option<JsonMap<String, Json>>,
        #[serde(default)]
        roles: Option<Vec<String>>,
        #[serde(default)]
        purpose: Option<String>,
    },
    /// `- filesystem.read: {paths: [...]}` — the single-key spelling of spec §31.7.
    Keyed(JsonMap<String, Json>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState {
    persistence: Persistence,
    #[serde(default)]
    quota: Option<Json>,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    migrations: Option<Json>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNetwork {
    outbound: Outbound,
    #[serde(default)]
    destinations: Option<Vec<JsonMap<String, Json>>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependencies {
    #[serde(default)]
    packages: Option<Json>,
    #[serde(default)]
    schemas: Option<Json>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    optional: Option<bool>,
}

// --- validation --------------------------------------------------------------------------------

fn invalid(detail: impl Into<String>) -> KuangError {
    KuangError::new(KuangErrorCode::PackageInvalid, detail.into())
}

/// Whether every dot-separated segment matches `[a-z][a-z0-9-]*` (spec §31.5).
fn valid_namespace(id: &str) -> bool {
    !id.is_empty()
        && id.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|first| first.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn validate_package(package: &RawPackage) -> Result<(), KuangError> {
    if !valid_namespace(&package.id) {
        return Err(invalid(format!(
            "`{}` is not a reverse-DNS package id of lowercase `[a-z][a-z0-9-]*` segments (spec §31.5)",
            package.id
        )));
    }
    if package.id == "ono" || package.id.starts_with("ono.") {
        return Err(invalid(format!(
            "`{}` claims the `ono.*` namespace, which only the Ono project may claim (spec §31.5)",
            package.id
        )));
    }
    if !valid_namespace(&package.publisher) {
        return Err(invalid(format!(
            "`{}` is not a valid publisher namespace (spec §31.5)",
            package.publisher
        )));
    }
    let Some(rest) = package
        .id
        .strip_prefix(&package.publisher)
        .and_then(|rest| rest.strip_prefix('.'))
    else {
        return Err(invalid(format!(
            "package id `{}` does not begin with its publisher namespace `{}` (spec §31.5)",
            package.id, package.publisher
        )));
    };
    if rest.is_empty() {
        return Err(invalid(
            "the package id must name a package inside the publisher namespace (spec §31.5)",
        ));
    }
    let mut version_parts = package
        .version
        .split('-')
        .next()
        .unwrap_or_default()
        .split('.');
    let numeric = version_parts.clone().count() == 3
        && version_parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    if !numeric {
        return Err(invalid(format!(
            "`{}` is not a semantic version (spec §31.7)",
            package.version
        )));
    }
    if package.description.trim().is_empty() {
        return Err(invalid(
            "`package.description` must not be empty (spec §31.7)",
        ));
    }
    if package.license.trim().is_empty() {
        return Err(invalid(
            "`package.license` must name an SPDX identifier (spec §31.7)",
        ));
    }
    Ok(())
}

fn validate_compatibility(raw: RawCompatibility) -> Result<Compatibility, KuangError> {
    let kuang_api: VersionRange = raw.kuang_api.parse()?;
    if raw.platforms.is_empty() {
        return Err(invalid(
            "`compatibility.platforms` must list at least one platform (spec §31.7)",
        ));
    }
    Ok(Compatibility {
        kuang_api,
        ono_language: raw.ono_language,
        value_protocol: raw.value_protocol,
        schema_language: raw.schema_language,
        view_protocol: raw.view_protocol,
        model_broker: raw.model_broker,
        remote_extension: raw.remote_extension,
        platforms: raw.platforms,
    })
}

fn validate_runtime(raw: RawRuntime) -> Result<Runtime, KuangError> {
    let needs_entry = matches!(
        raw.kind,
        RuntimeKind::WasmComponent | RuntimeKind::NativeProcess
    );
    if needs_entry && raw.entry.is_none() {
        return Err(invalid(
            "`runtime.entry` is required for a local runtime artifact (manifest.v1.yaml)",
        ));
    }
    if raw.endpoint.is_some() && raw.kind != RuntimeKind::RemoteService {
        return Err(invalid(
            "`runtime.endpoint` is only meaningful for `kind: remote-service`",
        ));
    }
    Ok(Runtime {
        kind: raw.kind,
        entry: raw.entry,
        endpoint: raw.endpoint,
        memory_max: parse_bytesize(&raw.memory_max, "runtime.memory_max")?,
        cpu_budget: raw.cpu_budget,
        startup: raw.startup,
        max_concurrent_calls: raw.max_concurrent_calls,
        overflow: raw.overflow,
    })
}

fn validate_state(raw: RawState) -> Result<StateDeclaration, KuangError> {
    let quota = raw
        .quota
        .as_ref()
        .map(|quota| parse_bytesize(quota, "state.quota"))
        .transpose()?;
    if raw.persistence == Persistence::Persistent && (quota.is_none() || raw.version.is_none()) {
        return Err(invalid(
            "persistent state requires `quota` and `version` (spec §31.31, §31.75)",
        ));
    }
    Ok(StateDeclaration {
        persistence: raw.persistence,
        quota,
        version: raw.version,
        migrations: raw.migrations,
    })
}

fn validate_network(raw: RawNetwork) -> Result<NetworkDeclaration, KuangError> {
    if raw.outbound == Outbound::None && raw.destinations.is_some() {
        return Err(invalid(
            "`network.destinations` contradicts `outbound: none` (spec §31.21)",
        ));
    }
    Ok(NetworkDeclaration {
        outbound: raw.outbound,
        destinations: raw.destinations,
    })
}

fn validate_capability_entries(
    entries: Vec<RawCapabilityEntry>,
) -> Result<Vec<CapabilityRequest>, KuangError> {
    entries
        .into_iter()
        .map(|entry| {
            let (id, scope, roles, purpose) = match entry {
                RawCapabilityEntry::Bare(id) => (id, None, None, None),
                RawCapabilityEntry::Full {
                    capability,
                    scope,
                    roles,
                    purpose,
                } => (capability, scope, roles, purpose),
                RawCapabilityEntry::Keyed(map) => {
                    if map.len() != 1 {
                        return Err(invalid(
                            "a capability entry maps exactly one capability id to its scope (spec §31.7)",
                        ));
                    }
                    let (id, scope) = map.into_iter().next().unwrap_or_default();
                    let scope = match scope {
                        Json::Object(scope) => Some(scope),
                        Json::Null => None,
                        _ => {
                            return Err(invalid(format!(
                                "the scope of `{id}` must be a record of scope keys (spec §31.16)"
                            )));
                        }
                    };
                    (id, scope, None, None)
                }
            };
            let capability: Capability = id.parse()?;
            if let Some(scope) = &scope {
                let declared = capability.scope_keys();
                for key in scope.keys() {
                    if !declared.iter().any(|candidate| candidate.name == key) {
                        return Err(invalid(format!(
                            "`{capability}` declares no scope key `{key}` (capabilities.v1.yaml)"
                        )));
                    }
                }
            }
            Ok(CapabilityRequest {
                capability,
                scope,
                roles,
                purpose,
            })
        })
        .collect()
}

/// Parses a byte size as the manifest writes it: a plain byte count, or `64MiB`-style units.
fn parse_bytesize(value: &Json, field: &str) -> Result<u64, KuangError> {
    let error = || {
        invalid(format!(
            "`{field}` must be a byte count or a size such as `64MiB`"
        ))
    };
    match value {
        Json::Number(number) => number.as_u64().ok_or_else(error),
        Json::String(text) => {
            let text = text.trim();
            let unit_start = text
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(text.len());
            let (digits, unit) = text.split_at(unit_start);
            let count: u64 = digits.parse().map_err(|_| error())?;
            let factor: u64 = match unit.trim() {
                "" | "B" => 1,
                "KiB" | "K" => 1024,
                "MiB" | "M" => 1024 * 1024,
                "GiB" | "G" => 1024 * 1024 * 1024,
                _ => return Err(error()),
            };
            count.checked_mul(factor).ok_or_else(error)
        }
        _ => Err(error()),
    }
}

/// Validates that a contributed id sits inside the package namespace and outside `ono.*`
/// (spec §31.5, §31.22).
pub fn validate_contributed_id(package_id: &str, kind: &str, id: &str) -> Result<(), KuangError> {
    if id == "ono" || id.starts_with("ono.") {
        return Err(invalid(format!(
            "{kind} id `{id}` claims the `ono.*` namespace, which only the Ono project may claim (spec §31.5)"
        )));
    }
    let expected = format!("{package_id}.");
    if !id.starts_with(&expected) {
        return Err(invalid(format!(
            "{kind} id `{id}` is not namespaced under the package id `{package_id}` (spec §31.5)"
        )));
    }
    Ok(())
}

impl FromStr for Manifest {
    type Err = KuangError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}
