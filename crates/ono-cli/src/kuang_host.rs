//! The KUANG/11 host state one session holds, shared with the session provider (ADR-0107).
//!
//! Spec §31.8 separates package presence from code execution, and the two live in different
//! places: presence is the plugin home on disk, execution is the supervisor instances this
//! session started. `get plugin` overlays one on the other, and every other KUANG/11 table —
//! grants, the audit trail, assistants — derives from the same two sources. They are kept here,
//! behind the session's shared tables, so `ono.shell` answers from them exactly as it answers
//! `get job` (ADR-0090), and acts on them through the mutation road of ADR-0068.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_kuang_protocol::{
    AuditEvent, Capability, CapabilityRequest, DeclarationClass, HOST_API, Manifest, PluginState,
    Role, RuntimeKind, ShutdownReason, WireError,
};
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Policy, Supervisor};
use ono_provider_api::{Action, ActionOutcome, ObjectId};
use ono_value::{ErrorValue, MapValue, Provenance, RecordValue, Schema, SchemaId, Value};

/// One discovered package: its directory and its parsed manifest.
#[derive(Debug, Clone)]
pub struct Installed {
    /// Where the package lives.
    pub directory: PathBuf,
    /// The validated manifest.
    pub manifest: Manifest,
}

/// The management state of one package, on disk (spec §31.31): what the operator decided about
/// it, as opposed to what the package is doing now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Management {
    /// Whether policy makes the package eligible for loading (spec §31.3).
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// The source reference `install plugin` resolved, when it was installed rather than placed.
    #[serde(default)]
    pub installed_from: Option<String>,
    /// The content hash recorded at install, which `verify plugin` re-checks (spec §31.36).
    #[serde(default)]
    pub integrity: Option<String>,
    /// Whether the operator let a package with a background role run jobs no command created
    /// (spec §31.38). Recorded; nothing in this build starts such jobs.
    #[serde(default)]
    pub background: bool,
}

const fn enabled_by_default() -> bool {
    true
}

impl Default for Management {
    fn default() -> Self {
        Self {
            enabled: true,
            installed_from: None,
            integrity: None,
            background: false,
        }
    }
}

/// One runtime instance this session loaded (spec §31.10).
#[derive(Debug)]
pub struct Instance {
    /// The package id the instance runs.
    pub id: String,
    /// The supervisor's handle.
    pub plugin: Arc<LoadedPlugin>,
    /// When the instance was created.
    pub loaded_at: Value,
}

/// One capability grant this session made to a package (spec §31.18, `ono.capability-grant/1`).
#[derive(Debug, Clone)]
pub struct Grant {
    /// The grant's own identity.
    pub id: ono_value::Uuid,
    /// The package it was made to.
    pub plugin: String,
    /// The granted family.
    pub capability: Capability,
    /// The scope, when the manifest asked for one.
    pub scope: Option<serde_json::Map<String, serde_json::Value>>,
    /// How the package declared the capability, when it did.
    pub class: Option<DeclarationClass>,
    /// Where the decision came from: `session` for `--grant` at load, `prompt` for
    /// `grant capability` afterwards.
    pub source: &'static str,
    /// When it was made.
    pub granted_at: Value,
    /// When it was revoked; `None` while it stands.
    pub revoked_at: Option<Value>,
}

/// The host: where packages are, which of them run, and what they were granted.
#[derive(Debug, Default)]
pub struct Host {
    plugin_path: Vec<PathBuf>,
    state_dir: Option<PathBuf>,
    instances: Vec<Instance>,
    grants: Vec<Grant>,
    minted: u64,
    /// The trails of instances that are gone, and the host's own events: an unload does not
    /// erase what a package did (spec §31.37).
    retained_audit: Vec<AuditEvent>,
}

impl Host {
    /// Tells the host where the session's plugin home and state directory are. Called before
    /// every pipeline, since both come from the environment and the environment can change.
    pub fn configure(&mut self, plugin_path: Vec<PathBuf>, state_dir: Option<PathBuf>) {
        self.plugin_path = plugin_path;
        self.state_dir = state_dir;
    }

    /// The directories packages are installed under, in search order.
    #[must_use]
    pub fn plugin_path(&self) -> &[PathBuf] {
        &self.plugin_path
    }

    /// The first directory of the plugin path — where `install plugin` places a package.
    #[must_use]
    pub fn install_root(&self) -> Option<&Path> {
        self.plugin_path.first().map(PathBuf::as_path)
    }

    /// Every package installed under the plugin path, in directory order.
    ///
    /// A directory whose manifest does not validate is reported as a failure rather than
    /// silently skipped: an installed package that cannot load is a fact about this machine.
    #[must_use]
    pub fn installed(&self) -> (Vec<Installed>, Vec<ErrorValue>) {
        let mut found = Vec::new();
        let mut failures = Vec::new();
        for root in &self.plugin_path {
            let (packages, problems) = packages_under(root);
            found.extend(packages);
            failures.extend(problems);
        }
        (found, failures)
    }

    /// The installed package with this id, if any.
    #[must_use]
    pub fn installed_package(&self, id: &str) -> Option<Installed> {
        self.installed()
            .0
            .into_iter()
            .find(|package| package.manifest.package.id == id)
    }

    /// The management state recorded for `id`; the defaults when nothing was recorded.
    #[must_use]
    pub fn management(&self, id: &str) -> Management {
        let Some(path) = self.management_path(id) else {
            return Management::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Records the management state of `id` on disk (spec §31.31).
    ///
    /// # Errors
    ///
    /// The I/O failure, when the state directory cannot be written.
    pub fn write_management(&self, id: &str, management: &Management) -> Result<(), ErrorValue> {
        let path = self.management_path(id).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::IoNotFound,
                "no state directory is configured for plugin management state",
            )
            .with_help("set `XDG_STATE_HOME` or `HOME` (spec §31.31)")
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        }
        let text = serde_json::to_string_pretty(management).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the management state of `{id}` does not serialise: {error}"),
            )
        })?;
        std::fs::write(&path, text).map_err(|error| io_error(&path, &error))
    }

    /// Forgets the management state of `id`, when a package is removed.
    pub fn remove_management(&self, id: &str) {
        if let Some(path) = self.management_path(id) {
            let _ = std::fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    fn management_path(&self, id: &str) -> Option<PathBuf> {
        self.state_dir
            .as_ref()
            .map(|dir| dir.join("kuang").join(id).join("management.json"))
    }

    /// The loaded instance of `id`.
    #[must_use]
    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|instance| instance.id == id)
    }

    /// The supervisor handle of the loaded package `id`.
    #[must_use]
    pub fn plugin(&self, id: &str) -> Option<Arc<LoadedPlugin>> {
        self.instance(id)
            .map(|instance| Arc::clone(&instance.plugin))
    }

    /// The ids of every loaded package.
    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.instances.iter().map(|instance| instance.id.as_str())
    }

    /// Keeps a loaded instance, answering the one it replaces so the caller can shut it down.
    pub fn add_instance(&mut self, id: String, plugin: LoadedPlugin) -> Option<Instance> {
        let previous = self.remove_instance(&id);
        self.instances.push(Instance {
            id,
            plugin: Arc::new(plugin),
            loaded_at: Value::now(),
        });
        previous
    }

    /// Takes the instance of `id` out of the host, so the caller can shut it down.
    pub fn remove_instance(&mut self, id: &str) -> Option<Instance> {
        let index = self
            .instances
            .iter()
            .position(|instance| instance.id == id)?;
        Some(self.instances.remove(index))
    }

    /// A fresh identity for a grant or a host event, stable within the session.
    fn mint(&mut self) -> ono_value::Uuid {
        self.minted += 1;
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x40;
        bytes[8] = 0x80;
        bytes[10..].copy_from_slice(&self.minted.to_be_bytes()[2..]);
        ono_value::Uuid::from_bytes(bytes)
    }

    /// Records a grant of `capability` to `plugin`, answering it (spec §31.18).
    pub fn grant(
        &mut self,
        plugin: &str,
        capability: Capability,
        scope: Option<serde_json::Map<String, serde_json::Value>>,
        class: Option<DeclarationClass>,
        source: &'static str,
    ) -> Grant {
        let grant = Grant {
            id: self.mint(),
            plugin: plugin.to_owned(),
            capability,
            scope,
            class,
            source,
            granted_at: Value::now(),
            revoked_at: None,
        };
        self.grants.push(grant.clone());
        self.record_host_event(plugin, capability.id(), "capability.grant", true);
        grant
    }

    /// Revokes the grant with this identity, answering whether one stood.
    pub fn revoke(&mut self, id: ono_value::Uuid) -> Option<Grant> {
        let index = self
            .grants
            .iter()
            .position(|grant| grant.id == id && grant.revoked_at.is_none())?;
        self.grants[index].revoked_at = Some(Value::now());
        let grant = self.grants[index].clone();
        self.record_host_event(
            &grant.plugin,
            grant.capability.id(),
            "capability.revoke",
            true,
        );
        Some(grant)
    }

    /// The grants that stand for `plugin`, oldest first.
    pub fn standing_grants(&self, plugin: &str) -> impl Iterator<Item = &Grant> {
        self.grants
            .iter()
            .filter(move |grant| grant.plugin == plugin && grant.revoked_at.is_none())
    }

    /// Every grant ever made, revoked ones included: a revoked grant is retained rather than
    /// deleted (`ono.capability-grant/1`).
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The broker policy the standing grants of `plugin` amount to (spec §31.19).
    #[must_use]
    pub fn policy_for(&self, plugin: &str) -> Policy {
        self.standing_grants(plugin)
            .fold(Policy::deny_all(), |policy, grant| {
                policy.grant(grant.capability, grant.scope.clone())
            })
    }

    /// Keeps the trail of an instance that is going away.
    pub fn retain_audit(&mut self, events: Vec<AuditEvent>) {
        self.retained_audit.extend(events);
    }

    /// Records a host-side action about a package — a load, a grant, a revocation — in the
    /// same trail the packages' own actions go to (spec §31.37).
    pub fn record_host_event(&mut self, plugin: &str, capability: &str, action: &str, ok: bool) {
        let id = self.mint();
        self.retained_audit.push(AuditEvent {
            id: id.to_string(),
            plugin: plugin.to_owned(),
            invocation: "host".to_owned(),
            capability: capability.to_owned(),
            scope: None,
            enforcement: ono_kuang_protocol::Enforcement::Broker,
            action: action.to_owned(),
            target: None,
            at: jiff::Timestamp::now().to_string(),
            result: if ok {
                ono_kuang_protocol::AuditResult::Success
            } else {
                ono_kuang_protocol::AuditResult::Denied
            },
            user_confirmation: None,
            lease: None,
            link: None,
            error: None,
        });
    }

    /// Every audit event the host knows: the retained ones, then each running instance's.
    #[must_use]
    pub fn audit_events(&self) -> Vec<AuditEvent> {
        let mut events = self.retained_audit.clone();
        for instance in &self.instances {
            events.extend(instance.plugin.audit());
        }
        events
    }

    /// The `ono.plugin-audit-event/1` records (spec §31.37).
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when a record does not fit its contract.
    pub fn audit_records(&self) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = schema("ono.plugin-audit-event")?;
        self.audit_events()
            .iter()
            .map(|event| audit_record(&schema, event))
            .collect()
    }

    /// The `ono.capability-grant/1` records: with `plugin`, that package's declared requests
    /// merged with its grants; without, the capability definitions the broker knows followed by
    /// every package's rows (`kuang.yaml`, ADR-0111).
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when a record does not fit its contract.
    pub fn capability_records(&self, plugin: Option<&str>) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = schema("ono.capability-grant")?;
        let (installed, _) = self.installed();
        let mut records = Vec::new();
        if plugin.is_none() {
            for capability in Capability::ALL {
                records.push(definition_record(&schema, *capability)?);
            }
        }
        for package in installed
            .iter()
            .filter(|package| plugin.is_none_or(|id| package.manifest.package.id == id))
        {
            let id = &package.manifest.package.id;
            let instance = self.instance(id);
            let declared: Vec<(&CapabilityRequest, DeclarationClass)> = package
                .manifest
                .required_capabilities
                .iter()
                .map(|request| (request, DeclarationClass::Required))
                .chain(
                    package
                        .manifest
                        .optional_capabilities
                        .iter()
                        .map(|request| (request, DeclarationClass::Optional)),
                )
                .chain(
                    package
                        .manifest
                        .runtime_requested_capabilities
                        .iter()
                        .map(|request| (request, DeclarationClass::RuntimeRequested)),
                )
                .collect();
            for (request, class) in &declared {
                let grant = self
                    .standing_grants(id)
                    .find(|grant| grant.capability == request.capability);
                records.push(grant_record(
                    &schema,
                    id,
                    request.capability,
                    *class,
                    grant,
                    request.purpose.as_deref(),
                    instance,
                )?);
            }
            for grant in self
                .grants
                .iter()
                .filter(|grant| grant.plugin == *id)
                .filter(|grant| {
                    !declared
                        .iter()
                        .any(|(request, _)| request.capability == grant.capability)
                })
            {
                records.push(grant_record(
                    &schema,
                    id,
                    grant.capability,
                    grant.class.unwrap_or(DeclarationClass::RuntimeRequested),
                    Some(grant),
                    None,
                    instance,
                )?);
            }
        }
        Ok(records)
    }

    /// The `ono.plugin/1` records of the installed set, with this session's runtime states over
    /// them (spec §31.8), and the packages that could not be read.
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when the plugin contract is missing from the build.
    pub fn plugin_records(&self) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        let schema = schema("ono.plugin")?;
        let (packages, failures) = self.installed();
        let mut records = Vec::with_capacity(packages.len());
        for package in &packages {
            let management = self.management(&package.manifest.package.id);
            records.push(plugin_record(
                &schema,
                package,
                &management,
                self.instance(&package.manifest.package.id),
            )?);
        }
        Ok((records, failures))
    }
}

/// Every package directory under `root`, with the problems of those that do not validate.
#[must_use]
pub fn packages_under(root: &Path) -> (Vec<Installed>, Vec<ErrorValue>) {
    let mut found = Vec::new();
    let mut failures = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return (found, failures);
    };
    let mut directories: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    directories.sort();
    for directory in directories {
        match read_package(&directory) {
            Ok(Some(package)) => found.push(package),
            Ok(None) => {}
            Err(failure) => failures.push(failure),
        }
    }
    (found, failures)
}

/// The package in `directory`, `None` when the directory holds no manifest.
///
/// # Errors
///
/// `provider.schema_violation` naming the directory whose manifest does not validate.
pub fn read_package(directory: &Path) -> Result<Option<Installed>, ErrorValue> {
    let manifest_path = directory.join("manifest.yaml");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return Ok(None);
    };
    match Manifest::parse(&text) {
        Ok(manifest) => Ok(Some(Installed {
            directory: directory.to_path_buf(),
            manifest,
        })),
        Err(error) => {
            let mut failure = ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!(
                    "{} holds a package that does not validate: {}: {}",
                    directory.display(),
                    error.code().name(),
                    error.message()
                ),
            );
            if let Some(help) = error.help() {
                failure = failure.with_help(help);
            }
            Err(failure)
        }
    }
}

/// A core schema by name, as the build embeds it.
///
/// # Errors
///
/// `provider.schema_violation` when the contract is missing from the build.
pub fn schema(name: &str) -> Result<Arc<Schema>, ErrorValue> {
    let id = SchemaId::new(name, 1);
    ono_value::builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!(
                "{} advertises {id} but no contract defines it",
                provider_id()
            ),
        )
    })
}

/// The provider every record here is attributed to.
#[must_use]
pub fn provider_id() -> &'static str {
    crate::session_provider::PROVIDER_ID
}

/// The provenance of a record of `schema`, answered by the shell itself.
#[must_use]
pub fn provenance(schema: &Schema) -> Provenance {
    Provenance::local(provider_id(), schema.id().clone())
}

/// The isolation tier `runtime.kind` names (lifecycle.v1 `isolation_tiers`).
///
/// A declarative package has no runtime of its own: what runs is the core's interpreter of its
/// packs, in process, so the tier reported is `core-built-in` (ADR-0107).
#[must_use]
pub fn isolation(manifest: &Manifest) -> &'static str {
    match manifest.runtime.as_ref().map(|runtime| runtime.kind) {
        Some(RuntimeKind::NativeProcess) => "trusted-native",
        Some(RuntimeKind::WasmComponent) => "isolated-component",
        Some(RuntimeKind::RemoteService) => "remote-service",
        Some(RuntimeKind::Declarative) | None => "core-built-in",
    }
}

/// The role as the manifest spells it (spec §31.4).
#[must_use]
pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Analysis => "analysis",
        Role::Provider => "provider",
        Role::Adapter => "adapter",
        Role::View => "view",
        Role::EventProcessor => "event-processor",
        Role::Assistant => "assistant",
        Role::Automation => "automation",
        Role::RemoteComponent => "remote-component",
    }
}

/// The source reference a package in the plugin home came from: what `install plugin` recorded,
/// else the directory itself as a `path:` reference (spec §31.9).
#[must_use]
pub fn source_of(package: &Installed, management: &Management) -> String {
    management
        .installed_from
        .clone()
        .unwrap_or_else(|| format!("path:{}", package.directory.display()))
}

/// The content hash of the package's artifact: its manifest and its runtime entry, in that
/// order (spec §31.36's "are these the exact bytes referenced?").
#[must_use]
pub fn integrity_of(package: &Installed) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    for relative in artifact_files(&package.manifest) {
        if let Ok(bytes) = std::fs::read(package.directory.join(&relative)) {
            hasher.update(relative.as_bytes());
            hasher.update(bytes);
        }
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// The files that make up a package artifact, relative to its directory.
#[must_use]
pub fn artifact_files(manifest: &Manifest) -> Vec<String> {
    let mut files = vec!["manifest.yaml".to_owned()];
    if let Some(entry) = manifest
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.entry.clone())
    {
        files.push(entry);
    }
    if let Some(contributions) = &manifest.contributions {
        for paths in [
            &contributions.commands,
            &contributions.schemas,
            &contributions.targets,
            &contributions.views,
            &contributions.relations,
            &contributions.annotations,
            &contributions.tools,
            &contributions.adapters,
        ]
        .into_iter()
        .flatten()
        {
            files.extend(paths.iter().cloned());
        }
    }
    files
}

/// When the artifact was placed: the manifest's modification time, the closest fact on disk.
#[must_use]
pub fn installed_at(package: &Installed) -> Value {
    std::fs::metadata(package.directory.join("manifest.yaml"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map_or(Value::Null, Value::Timestamp)
}

/// Why a loaded instance is degraded: the optional capabilities it was denied (spec §31.17).
#[must_use]
pub fn degraded_reason(plugin: &LoadedPlugin) -> Option<String> {
    let denied: Vec<String> = plugin
        .contract()
        .denied
        .iter()
        .map(|denied| format!("{} not granted ({})", denied.capability, denied.reason))
        .collect();
    (!denied.is_empty()).then(|| denied.join("; "))
}

/// The `ono.plugin/1` record of one installed package (spec §31.8).
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn plugin_record(
    schema: &Arc<Schema>,
    package: &Installed,
    management: &Management,
    instance: Option<&Instance>,
) -> Result<RecordValue, ErrorValue> {
    let manifest = &package.manifest;
    let plugin = instance.map(|instance| &*instance.plugin);
    let state = plugin.map_or(PluginState::Installed, LoadedPlugin::state);
    let roles = Value::list(
        manifest
            .roles
            .iter()
            .map(|role| Value::string(role_name(*role))),
    );
    let text_or_null = |text: Option<String>| text.map_or(Value::Null, |text| Value::string(&text));
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::string(&manifest.package.id))?
        .set("name", Value::string(&manifest.package.name))?
        .set("version", Value::string(&manifest.package.version))?
        .set("publisher", Value::string(&manifest.package.publisher))?
        .set("state", Value::string(state.as_str()))?
        // An unsigned package placed on this machine is a local development package — visibly
        // untrusted, and capability-limited exactly like every other (spec §31.36).
        .set("trust", Value::string("local"))?
        .set("isolation", Value::string(isolation(manifest)))?
        .set("roles", roles)?
        .set("enabled", Value::Bool(management.enabled))?
        // One directory per package id in the plugin home (ADR-0051): the installed version is
        // the active one.
        .set("active_version", Value::Bool(true))?
        .set("source", Value::string(&source_of(package, management)))?
        .set("integrity", Value::string(&integrity_of(package)))?
        .set(
            "kuang_api",
            Value::string(manifest.compatibility.kuang_api.source()),
        )?
        // Invocations run in the foreground of this session and finish before the next
        // statement, so nothing is running when a table is read.
        .set("jobs", Value::Int(0))?
        .set("memory", Value::Null)?
        .set("state_usage", Value::Null)?
        .set(
            "degraded_reason",
            text_or_null(
                plugin
                    .filter(|_| state == PluginState::Degraded)
                    .and_then(degraded_reason),
            ),
        )?
        .set(
            "quarantine_reason",
            text_or_null(plugin.and_then(LoadedPlugin::quarantine_reason)),
        )?
        .set("installed_at", installed_at(package))?
        .set(
            "loaded_at",
            instance.map_or(Value::Null, |instance| instance.loaded_at.clone()),
        )?
        .set("restart_count", Value::Int(0))?
        .set(
            "last_error",
            plugin
                .and_then(LoadedPlugin::last_failure)
                .map_or(Value::Null, |error| {
                    crate::plugins::error_value(&error).into_value()
                }),
        )?
        .build())
}

fn io_error(path: &Path, error: &std::io::Error) -> ErrorValue {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
        std::io::ErrorKind::AlreadyExists => ErrorCode::IoAlreadyExists,
        _ => ErrorCode::IoNotFound,
    };
    ErrorValue::new(code, format!("{}: {error}", path.display()))
}

// --- verification, spec §31.36 -----------------------------------------------------------------

/// What `verify plugin` found: the record, and the errors of the checks that block.
#[derive(Debug)]
pub struct Verification {
    /// The `ono.verification-result/1` record.
    pub record: RecordValue,
    /// One structured error per blocking check that failed, in check order.
    pub blocking: Vec<ErrorValue>,
}

/// A package reference as `verify`, `find` and `install` resolve it: an installed id, or a
/// `path:` reference to a directory (lifecycle.v1 `sources`).
#[derive(Debug)]
pub struct Resolved {
    /// The reference as it will be recorded.
    pub source: String,
    /// The package, or why its manifest does not validate.
    pub package: Result<Installed, ErrorValue>,
}

impl Host {
    /// Resolves `reference`: `path:<dir>` reads that directory; anything else is an installed
    /// package id.
    ///
    /// # Errors
    ///
    /// `resolve.target_not_found` when nothing answers to the reference.
    pub fn resolve(&self, reference: &str) -> Result<Resolved, ErrorValue> {
        if let Some(path) = reference.strip_prefix("path:") {
            let directory = PathBuf::from(path);
            return match read_package(&directory) {
                Ok(Some(package)) => Ok(Resolved {
                    source: reference.to_owned(),
                    package: Ok(package),
                }),
                Ok(None) => Err(ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("{} holds no `manifest.yaml`", directory.display()),
                )
                .with_help("a `path:` reference names an unpacked package directory (spec §31.9)")),
                Err(error) => Ok(Resolved {
                    source: reference.to_owned(),
                    package: Err(error),
                }),
            };
        }
        if let Some(scheme) = reference.split_once(':').map(|(scheme, _)| scheme)
            && matches!(scheme, "file" | "registry" | "git" | "oci")
        {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("the `{scheme}:` source scheme is not available in this build"),
            )
            .with_help("`path:<directory>` is the source this build resolves (spec §31.9)"));
        }
        let package = self.installed_package(reference).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no installed package answers to `{reference}`"),
            )
            .with_help("`get plugin` lists the installed set (spec §31.8)")
        })?;
        Ok(Resolved {
            source: format!("path:{}", package.directory.display()),
            package: Ok(package),
        })
    }

    /// Verifies a resolved package (spec §31.36).
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when the record does not fit its contract.
    pub fn verify(&self, resolved: &Resolved) -> Result<Verification, ErrorValue> {
        let management = resolved
            .package
            .as_ref()
            .map(|package| self.management(&package.manifest.package.id))
            .unwrap_or_default();
        verification(resolved, &management)
    }

    /// The `ono.plugin-package/1` records of the packages matching `term` in the configured
    /// sources, or in the one `--source` names (spec §31.9). Nothing is executed.
    ///
    /// # Errors
    ///
    /// The unreadable source, or a record that does not fit its contract.
    pub fn package_records(
        &self,
        term: &str,
        source: Option<&str>,
    ) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        let schema = schema("ono.plugin-package")?;
        let (installed, _) = self.installed();
        let (candidates, failures) = match source {
            None => (
                installed
                    .iter()
                    .map(|package| {
                        (
                            format!("path:{}", package.directory.display()),
                            package.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
                Vec::new(),
            ),
            Some(reference) => {
                let Some(path) = reference.strip_prefix("path:") else {
                    return Err(ErrorValue::new(
                        ErrorCode::ProviderUnsupported,
                        format!("`{reference}` is not a source this build searches"),
                    )
                    .with_help("`--source path:<directory>` (spec §31.9)"));
                };
                let directory = PathBuf::from(path);
                match read_package(&directory)? {
                    Some(package) => (vec![(reference.to_owned(), package)], Vec::new()),
                    None => {
                        let (packages, failures) = packages_under(&directory);
                        (
                            packages
                                .into_iter()
                                .map(|package| {
                                    (format!("path:{}", package.directory.display()), package)
                                })
                                .collect(),
                            failures,
                        )
                    }
                }
            }
        };
        let needle = term.to_lowercase();
        let mut records = Vec::new();
        for (reference, package) in candidates {
            let info = &package.manifest.package;
            if !info.id.to_lowercase().contains(&needle)
                && !info.name.to_lowercase().contains(&needle)
            {
                continue;
            }
            let already = installed.iter().any(|held| {
                held.manifest.package.id == info.id && held.manifest.package.version == info.version
            });
            records.push(package_record(&schema, &package, &reference, already)?);
        }
        Ok((records, failures))
    }
}

/// Builds the verification of `resolved` against what `management` recorded.
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn verification(
    resolved: &Resolved,
    management: &Management,
) -> Result<Verification, ErrorValue> {
    let schema = schema("ono.verification-result")?;
    let mut blocking = Vec::new();
    let mut warnings = vec![
        "signature: absent".to_owned(),
        "transparency: unknown".to_owned(),
    ];
    let (package_name, integrity, compatibility, manifest, runtime) = match &resolved.package {
        Ok(package) => {
            let integrity = match &management.integrity {
                Some(recorded) if *recorded == integrity_of(package) => "valid",
                Some(_) => {
                    blocking.push(
                        ErrorValue::new(
                            ErrorCode::KuangPackageIntegrityFailed,
                            format!(
                                "the bytes of `{}` are not the ones recorded at install",
                                package.manifest.package.id
                            ),
                        )
                        .with_metadata("check", Value::string("integrity")),
                    );
                    "invalid"
                }
                None => {
                    warnings.push("integrity: unknown, no hash was recorded".to_owned());
                    "unknown"
                }
            };
            let compatibility = match package
                .manifest
                .check_host(HOST_API, &ono_kuang_supervisor::host_platform())
            {
                Ok(()) => "compatible",
                Err(error) => {
                    blocking.push(
                        crate::plugins::error_value(&error)
                            .with_metadata("check", Value::string("compatibility")),
                    );
                    "incompatible"
                }
            };
            (
                package.manifest.package.id.clone(),
                integrity,
                compatibility,
                "valid",
                isolation(&package.manifest),
            )
        }
        Err(error) => {
            blocking.push(
                error
                    .clone()
                    .with_metadata("check", Value::string("manifest")),
            );
            (
                resolved.source.clone(),
                "unknown",
                "unknown",
                "invalid",
                "isolated-component",
            )
        }
    };
    let names = |names: &[String]| Value::list(names.iter().map(|name| Value::string(name)));
    let record = RecordValue::builder(Arc::clone(&schema), provenance(&schema))
        .set("package", Value::string(&package_name))?
        .set("source", Value::string(&resolved.source))?
        .set("integrity", Value::string(integrity))?
        .set("signature", Value::string("absent"))?
        .set("publisher", Value::Null)?
        .set("key", Value::Null)?
        .set("trust", Value::string("unknown"))?
        .set("transparency", Value::string("unknown"))?
        .set("compatibility", Value::string(compatibility))?
        .set("manifest", Value::string(manifest))?
        .set("runtime", Value::string(runtime))?
        .set(
            "blocking_failures",
            Value::list(blocking.iter().map(|error| {
                error
                    .metadata()
                    .get("check")
                    .cloned()
                    .unwrap_or_else(|| Value::string(error.code().name()))
            })),
        )?
        .set("warnings", names(&warnings))?
        .set("verified_at", Value::now())?
        .build();
    Ok(Verification { record, blocking })
}

// --- packages as a source offers them, spec §31.9 ---------------------------------------------

/// A map value from string-keyed pairs.
#[must_use]
pub fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = MapValue::new();
    for (key, value) in entries {
        map.insert(key.into(), value);
    }
    Value::Map(Arc::new(map))
}

fn string_list(items: &[String]) -> Value {
    Value::list(items.iter().map(|item| Value::string(item)))
}

/// A serde-kebab enum as the string its contract spells.
fn kebab<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(Value::string))
        .unwrap_or(Value::Null)
}

/// A JSON object as a map value.
fn json_map(object: Option<&serde_json::Map<String, serde_json::Value>>) -> Value {
    object.map_or(Value::Null, |object| {
        ono_value::from_json(
            &serde_json::Value::Object(object.clone()),
            ono_value::builtin_schemas(),
        )
        .unwrap_or(Value::Null)
    })
}

fn request_row(request: &CapabilityRequest, class: DeclarationClass, state: &str) -> Value {
    map([
        ("capability", Value::string(request.capability.id())),
        ("class", kebab(&class)),
        ("scope", json_map(request.scope.as_ref())),
        (
            "roles",
            request.roles.as_deref().map_or(Value::Null, string_list),
        ),
        (
            "purpose",
            request
                .purpose
                .as_deref()
                .map_or(Value::Null, Value::string),
        ),
        ("state", Value::string(state)),
    ])
}

/// Every capability request of `manifest` with its class, as `{capability, class, scope, roles,
/// purpose, state}` rows (spec §31.17).
#[must_use]
pub fn capability_requests(manifest: &Manifest, plugin: Option<&LoadedPlugin>) -> Value {
    let state = |request: &CapabilityRequest| match plugin {
        Some(plugin) if plugin.contract().grant(request.capability.id()).is_some() => "granted",
        Some(_) => "denied",
        None => "not-requested-yet",
    };
    let rows = manifest
        .required_capabilities
        .iter()
        .map(|request| request_row(request, DeclarationClass::Required, state(request)))
        .chain(
            manifest
                .optional_capabilities
                .iter()
                .map(|request| request_row(request, DeclarationClass::Optional, state(request))),
        )
        .chain(
            manifest
                .runtime_requested_capabilities
                .iter()
                .map(|request| {
                    request_row(request, DeclarationClass::RuntimeRequested, state(request))
                }),
        );
    Value::list(rows)
}

/// The network declaration as a map: `outbound: none` is a stated answer (spec §31.21).
#[must_use]
pub fn network_of(manifest: &Manifest) -> Value {
    map([
        (
            "outbound",
            Value::string(match manifest.network.outbound {
                ono_kuang_protocol::Outbound::None => "none",
                ono_kuang_protocol::Outbound::Brokered => "brokered",
            }),
        ),
        (
            "destinations",
            manifest
                .network
                .destinations
                .as_ref()
                .map_or(Value::Null, |destinations| {
                    Value::list(destinations.iter().map(|entry| json_map(Some(entry))))
                }),
        ),
    ])
}

/// What the manifest says the package contributes: the contribution files by kind.
#[must_use]
pub fn declared_contributions(manifest: &Manifest) -> Value {
    let paths = manifest.contributions.as_ref();
    let group = |select: fn(&ono_kuang_protocol::ContributionPaths) -> &Option<Vec<String>>| {
        paths
            .and_then(|paths| select(paths).as_deref())
            .map_or_else(|| Value::list([]), string_list)
    };
    map([
        ("commands", group(|paths| &paths.commands)),
        ("schemas", group(|paths| &paths.schemas)),
        ("targets", group(|paths| &paths.targets)),
        ("views", group(|paths| &paths.views)),
        ("relations", group(|paths| &paths.relations)),
        ("annotations", group(|paths| &paths.annotations)),
        ("tools", group(|paths| &paths.tools)),
        ("adapters", group(|paths| &paths.adapters)),
    ])
}

/// The `ono.plugin-package/1` record of a package as a source describes it.
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn package_record(
    schema: &Arc<Schema>,
    package: &Installed,
    source: &str,
    installed: bool,
) -> Result<RecordValue, ErrorValue> {
    let manifest = &package.manifest;
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::string(&manifest.package.id))?
        .set("name", Value::string(&manifest.package.name))?
        .set("version", Value::string(&manifest.package.version))?
        .set("publisher", Value::string(&manifest.package.publisher))?
        .set("summary", Value::string(&manifest.package.description))?
        .set("source", Value::string(source))?
        .set("license", Value::string(&manifest.package.license))?
        .set(
            "kuang_api",
            Value::string(manifest.compatibility.kuang_api.source()),
        )?
        .set("platforms", string_list(&manifest.compatibility.platforms))?
        .set(
            "roles",
            Value::list(
                manifest
                    .roles
                    .iter()
                    .map(|role| Value::string(role_name(*role))),
            ),
        )?
        .set("contributions", declared_contributions(manifest))?
        .set(
            "requested_capabilities",
            capability_requests(manifest, None),
        )?
        .set("network", network_of(manifest))?
        .set("integrity", Value::Null)?
        .set("signature", Value::string("absent"))?
        .set("trust", Value::string("local"))?
        .set("installed", Value::Bool(installed))?
        .set("size", Value::Null)?
        .set("published_at", Value::Null)?
        .build())
}

// --- inspection, spec §31.33 -------------------------------------------------------------------

/// What an instance contributes, by kind — the resolved ids.
#[derive(Debug, Clone, Default)]
pub struct Contributions {
    /// Command ids.
    pub commands: Vec<String>,
    /// Target names.
    pub targets: Vec<String>,
    /// Schema ids.
    pub schemas: Vec<String>,
}

impl Contributions {
    /// What a loaded instance registered.
    #[must_use]
    pub fn of(plugin: &LoadedPlugin) -> Self {
        Self {
            commands: plugin
                .commands()
                .iter()
                .map(|command| command.contribution.id.clone())
                .collect(),
            targets: plugin
                .targets()
                .iter()
                .map(|target| target.contribution.name.clone())
                .collect(),
            schemas: plugin
                .targets()
                .iter()
                .map(|target| target.contribution.schema.clone())
                .collect(),
        }
    }

    fn value(&self) -> Value {
        map([
            ("commands", string_list(&self.commands)),
            ("targets", string_list(&self.targets)),
            ("schemas", string_list(&self.schemas)),
        ])
    }
}

/// Learns what an unloaded package contributes by running its handshake once, under the
/// deny-all policy, and shutting the instance down (ADR-0108, spec deviation).
///
/// # Errors
///
/// The supervisor's refusal, when the package cannot be started.
pub async fn discover(package: &Installed) -> Result<Contributions, ErrorValue> {
    let entry = package
        .manifest
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.entry.as_ref())
        .map(|entry| package.directory.join(entry))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::KuangPackageInvalid,
                format!(
                    "`{}` declares no runtime to start",
                    package.manifest.package.id
                ),
            )
        })?;
    let loaded = Supervisor::load(LoadConfig::new(entry, package.manifest.clone()))
        .await
        .map_err(|error| crate::plugins::error_value(&error))?;
    let contributions = Contributions::of(&loaded);
    loaded.shutdown(ShutdownReason::Unload).await;
    Ok(contributions)
}

/// The `ono.plugin-runtime/1` record of a loaded instance — the negotiated contract (spec §31.63).
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn runtime_record(package: &Installed, instance: &Instance) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.plugin-runtime")?;
    let contract = instance.plugin.contract();
    let manifest = &package.manifest;
    let limits = &contract.limits;
    let int = |value: u64| Value::Int(i128::from(value));
    Ok(
        RecordValue::builder(Arc::clone(&schema), provenance(&schema))
            .set(
                "instance",
                Value::string(&format!(
                    "{}@{}",
                    instance.id,
                    ono_value::canonical_text(&instance.loaded_at).unwrap_or_default()
                )),
            )?
            .set("plugin", plugin_ref(manifest))?
            .set("host_api", Value::string(&contract.host_api))?
            .set("value_protocol", Value::string(&contract.value_protocol))?
            .set("isolation", Value::string(isolation(manifest)))?
            .set(
                "granted",
                Value::list(contract.granted.iter().map(|granted| {
                    map([
                        ("capability", Value::string(&granted.capability)),
                        ("class", kebab(&granted.class)),
                        ("scope", json_map(granted.scope.as_ref())),
                        ("enforcement", kebab(&granted.enforcement)),
                    ])
                })),
            )?
            .set(
                "denied",
                Value::list(contract.denied.iter().map(|denied| {
                    map([
                        ("capability", Value::string(&denied.capability)),
                        ("class", kebab(&denied.class)),
                        ("reason", Value::string(&denied.reason)),
                    ])
                })),
            )?
            .set(
                "disabled_features",
                string_list(instance.plugin.disabled_features()),
            )?
            .set(
                "limits",
                map([
                    (
                        "memory_max",
                        limits.memory_max.map_or(Value::Null, |bytes| {
                            Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(bytes)))
                        }),
                    ),
                    (
                        "state_quota",
                        Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(
                            limits.state_quota,
                        ))),
                    ),
                    ("queue_depth", int(u64::from(limits.queue_depth))),
                    ("call_deadline_ms", int(limits.call_deadline_ms)),
                    ("max_frame", int(u64::from(limits.max_frame))),
                ]),
            )?
            .set("overflow", kebab(&contract.overflow))?
            .set("network", network_of(manifest))?
            .set("degraded", Value::Bool(contract.degraded))?
            .set("started_at", instance.loaded_at.clone())?
            .set("development_mode", Value::Bool(false))?
            .build(),
    )
}

/// The reference to a package version, as every KUANG/11 record carries it.
#[must_use]
pub fn plugin_ref(manifest: &Manifest) -> Value {
    map([
        ("id", Value::string(&manifest.package.id)),
        ("version", Value::string(&manifest.package.version)),
    ])
}

/// The `ono.plugin-inspection/1` record of one package (spec §31.33).
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn inspection_record(
    package: &Installed,
    management: &Management,
    instance: Option<&Instance>,
    contributions: &Contributions,
    last_error: Option<ErrorValue>,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.plugin-inspection")?;
    let manifest = &package.manifest;
    let plugin = instance.map(|instance| &*instance.plugin);
    let manifest_value = std::fs::read_to_string(package.directory.join("manifest.yaml"))
        .ok()
        .and_then(|text| ono_value::from_yaml(&text, ono_value::builtin_schemas()).ok())
        .unwrap_or(Value::Null);
    let resolved = Resolved {
        source: source_of(package, management),
        package: Ok(package.clone()),
    };
    let verification = verification(&resolved, management)?;
    let runtime = instance
        .map(|instance| runtime_record(package, instance))
        .transpose()?
        .map_or(Value::Null, RecordValue::into_value);
    let bytes = |bytes: u64| Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(bytes)));
    let last_error = last_error.or_else(|| {
        plugin
            .and_then(LoadedPlugin::last_failure)
            .map(|error| crate::plugins::error_value(&error))
    });
    Ok(
        RecordValue::builder(Arc::clone(&schema), provenance(&schema))
            .set("plugin", plugin_ref(manifest))?
            .set("manifest", manifest_value)?
            .set("origin", Value::string("plugin"))?
            .set("contributions", contributions.value())?
            .set("capability_grants", Value::list([]))?
            .set("capability_requests", capability_requests(manifest, plugin))?
            .set("verification", verification.record.into_value())?
            .set("runtime", runtime)?
            .set("memory_current", Value::Null)?
            .set(
                "memory_limit",
                manifest
                    .runtime
                    .as_ref()
                    .map_or(Value::Null, |runtime| bytes(runtime.memory_max)),
            )?
            .set("cpu_time", Value::Null)?
            .set("host_calls", Value::Int(0))?
            .set("open_streams", Value::Int(0))?
            .set("queued_events", Value::Int(0))?
            .set("dropped_events", Value::Int(0))?
            .set(
                "last_error",
                last_error.map_or(Value::Null, ErrorValue::into_value),
            )?
            .set("restart_count", Value::Int(0))?
            .set("network_destinations", Value::list([]))?
            .set("state_usage", Value::Null)?
            .set(
                "state_quota",
                manifest
                    .state
                    .as_ref()
                    .and_then(|state| state.quota)
                    .map_or(Value::Null, bytes),
            )?
            .set("jobs", Value::list([]))?
            .build(),
    )
}

// --- install and remove, spec §31.9 and §31.81 ------------------------------------------------

/// The plan `install plugin` shows before it mutates anything (spec §31.9, lifecycle.v1
/// `install_plan`): what will be added, what was requested, and what will be written.
#[must_use]
pub fn install_plan(package: &Installed, source: &str, destination: &Path) -> Value {
    let manifest = &package.manifest;
    map([
        (
            "package",
            Value::string(&format!(
                "{}@{}",
                manifest.package.id, manifest.package.version
            )),
        ),
        ("source", Value::string(source)),
        ("integrity", Value::string(&integrity_of(package))),
        ("signature", Value::string("unsigned")),
        ("contributions", declared_contributions(manifest)),
        ("capabilities", capability_requests(manifest, None)),
        (
            "filesystem",
            Value::list([Value::Path(Arc::from(destination))]),
        ),
        (
            "state",
            manifest.state.as_ref().map_or(Value::Null, |state| {
                map([
                    (
                        "persistence",
                        Value::string(&format!("{:?}", state.persistence).to_lowercase()),
                    ),
                    (
                        "quota",
                        state.quota.map_or(Value::Null, |quota| {
                            Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(quota)))
                        }),
                    ),
                ])
            }),
        ),
        ("network", network_of(manifest)),
    ])
}

/// The identity of a package version as an action's object.
#[must_use]
pub fn object_id(id: &str, version: &str) -> ObjectId {
    ObjectId::new(
        SchemaId::new("ono.plugin", 1),
        [Value::string(id), Value::string(version)],
    )
}

/// The `ono.action-result/1` value of one management action (spec §11.5, ADR-0068).
#[must_use]
pub fn action_result(
    outcome: ActionOutcome,
    operation: &str,
    started: std::time::Instant,
) -> Value {
    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    outcome
        .into_record(elapsed)
        .with_operation(operation)
        .into_value()
}

impl Host {
    /// Where `install plugin` would place `package`: under the first directory of the plugin
    /// path, by package id (ADR-0051).
    ///
    /// # Errors
    ///
    /// `io.not_found` when no plugin home is configured.
    pub fn install_destination(&self, package: &Installed) -> Result<PathBuf, ErrorValue> {
        let root = self.install_root().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::IoNotFound,
                "no plugin home is configured to install into",
            )
            .with_help("set `ONO_PLUGIN_PATH`, or `HOME` for `~/.config/ono/plugins` (ADR-0051)")
        })?;
        Ok(root.join(&package.manifest.package.id))
    }

    /// Whether this exact id and version is already in the plugin home.
    #[must_use]
    pub fn is_installed(&self, id: &str, version: &str) -> bool {
        self.installed_package(id)
            .is_some_and(|held| held.manifest.package.version == version)
    }

    /// Places a verified package in the plugin home and records where it came from. No
    /// package code runs and nothing is granted (spec §31.9).
    ///
    /// # Errors
    ///
    /// `io.already_exists` when the same version is installed; the I/O failure otherwise.
    pub fn install(&self, package: &Installed, source: &str) -> Result<Installed, ErrorValue> {
        let manifest = &package.manifest;
        let destination = self.install_destination(package)?;
        if self.is_installed(&manifest.package.id, &manifest.package.version) {
            return Err(ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!(
                    "`{}` {} is already installed",
                    manifest.package.id, manifest.package.version
                ),
            )
            .with_help("`remove plugin` it first; a package version is never silently replaced (spec §31.35)"));
        }
        if destination.exists() {
            // Another version of the same package: one directory per id (ADR-0051), so the
            // old version leaves. Its state stays (spec §31.81).
            self.remove_directory(&destination)?;
        }
        copy_tree(&package.directory, &destination)?;
        let installed = Installed {
            directory: destination,
            manifest: manifest.clone(),
        };
        let management = Management {
            enabled: true,
            installed_from: Some(source.to_owned()),
            integrity: Some(integrity_of(&installed)),
            background: false,
        };
        self.write_management(&manifest.package.id, &management)?;
        Ok(installed)
    }

    /// Removes an installed package's directory, and its management state unless `keep_state`
    /// (spec §31.81). The caller unloads a running instance first.
    ///
    /// # Errors
    ///
    /// The I/O failure.
    pub fn remove_package(&self, package: &Installed, keep_state: bool) -> Result<(), ErrorValue> {
        self.remove_directory(&package.directory)?;
        if !keep_state {
            self.remove_management(&package.manifest.package.id);
        }
        Ok(())
    }

    fn remove_directory(&self, directory: &Path) -> Result<(), ErrorValue> {
        // Only a directory under the plugin path is ever removed: a manifest elsewhere is a
        // source, never an installation.
        if !self
            .plugin_path
            .iter()
            .any(|root| directory.starts_with(root))
        {
            return Err(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!(
                    "{} is not under the plugin home and is not removed",
                    directory.display()
                ),
            ));
        }
        std::fs::remove_dir_all(directory).map_err(|error| io_error(directory, &error))
    }
}

/// Copies a package directory, file by file, keeping permissions so the runtime entry stays
/// executable.
fn copy_tree(from: &Path, to: &Path) -> Result<(), ErrorValue> {
    std::fs::create_dir_all(to).map_err(|error| io_error(to, &error))?;
    for entry in std::fs::read_dir(from).map_err(|error| io_error(from, &error))? {
        let entry = entry.map_err(|error| io_error(from, &error))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|error| io_error(&source, &error))?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target).map_err(|error| io_error(&source, &error))?;
        }
    }
    Ok(())
}

/// The action an `ono.plugin/1` object is the target of, for the outcome records.
#[must_use]
pub fn action(operation: &str, id: &str, version: &str) -> Action {
    Action::new("plugin", operation, object_id(id, version))
}

// --- capabilities and audit, spec §31.16–§31.19 and §31.37 -------------------------------------

/// A definition's identity: the same capability is the same row in every session.
fn definition_id(capability: Capability) -> ono_value::Uuid {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(capability.id().as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = 0x40 | (bytes[6] & 0x0f);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    ono_value::Uuid::from_bytes(bytes)
}

/// What the broker knows about a capability family before any package asks for it: denied by
/// default (spec §31.80), with the enforcement its scope keys declare.
fn definition_record(
    schema: &Arc<Schema>,
    capability: Capability,
) -> Result<RecordValue, ErrorValue> {
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::Uuid(definition_id(capability)))?
        .set("plugin", Value::Null)?
        .set("capability", Value::string(capability.id()))?
        .set("class", Value::Null)?
        .set("decision", Value::string("deny"))?
        .set("scope", Value::Null)?
        .set("enforcement", Value::string("broker"))?
        .set("duration", Value::string("always"))?
        .set("granted_at", Value::Null)?
        .set("expires_at", Value::Null)?
        .set("max_uses", Value::Null)?
        .set("uses", Value::Int(0))?
        .set("actions", Value::Null)?
        .set("selector", Value::Null)?
        .set("condition", Value::Null)?
        .set("source", Value::string("default"))?
        .set("link", Value::Null)?
        .set("purpose", Value::string(capability_summary(capability)))?
        .set("revoked_at", Value::Null)?
        .build())
}

fn capability_summary(capability: Capability) -> &'static str {
    match capability.risk() {
        ono_kuang_protocol::Risk::Read => "read",
        ono_kuang_protocol::Risk::Observe => "observe",
        ono_kuang_protocol::Risk::Mutate => "mutate",
        ono_kuang_protocol::Risk::Destructive => "destructive",
    }
}

/// One package's standing with one capability: what it declared, and what it holds.
fn grant_record(
    schema: &Arc<Schema>,
    plugin: &str,
    capability: Capability,
    class: DeclarationClass,
    grant: Option<&Grant>,
    purpose: Option<&str>,
    instance: Option<&Instance>,
) -> Result<RecordValue, ErrorValue> {
    let standing = grant.filter(|grant| grant.revoked_at.is_none());
    let enforcement = instance
        .and_then(|instance| {
            instance
                .plugin
                .contract()
                .grant(capability.id())
                .map(|granted| kebab(&granted.enforcement))
        })
        .unwrap_or_else(|| Value::string("broker"));
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set(
            "id",
            grant.map_or_else(
                || Value::Uuid(definition_id(capability)),
                |grant| Value::Uuid(grant.id),
            ),
        )?
        .set("plugin", Value::string(plugin))?
        .set("capability", Value::string(capability.id()))?
        .set("class", kebab(&class))?
        .set(
            "decision",
            Value::string(if standing.is_some() { "allow" } else { "deny" }),
        )?
        .set(
            "scope",
            grant.map_or(Value::Null, |grant| json_map(grant.scope.as_ref())),
        )?
        .set("enforcement", enforcement)?
        .set("duration", Value::string("session"))?
        .set(
            "granted_at",
            grant.map_or(Value::Null, |grant| grant.granted_at.clone()),
        )?
        .set("expires_at", Value::Null)?
        .set("max_uses", Value::Null)?
        .set("uses", Value::Int(0))?
        .set("actions", Value::Null)?
        .set("selector", Value::Null)?
        .set("condition", Value::Null)?
        .set(
            "source",
            Value::string(grant.map_or("default", |grant| grant.source)),
        )?
        .set("link", Value::Null)?
        .set("purpose", purpose.map_or(Value::Null, Value::string))?
        .set(
            "revoked_at",
            grant
                .and_then(|grant| grant.revoked_at.clone())
                .unwrap_or(Value::Null),
        )?
        .build())
}

/// The `ono.capability-grant/1` record of one grant as it was just made.
///
/// # Errors
///
/// `provider.schema_violation` when the record does not fit its contract.
pub fn grant_value(
    grant: &Grant,
    purpose: Option<&str>,
    instance: Option<&Instance>,
) -> Result<Value, ErrorValue> {
    let schema = schema("ono.capability-grant")?;
    Ok(grant_record(
        &schema,
        &grant.plugin,
        grant.capability,
        grant.class.unwrap_or(DeclarationClass::RuntimeRequested),
        Some(grant),
        purpose,
        instance,
    )?
    .into_value())
}

/// A wire error as an error value: the code itself where this build knows it.
#[must_use]
pub fn wire_error_value(error: &WireError) -> ErrorValue {
    let mut value = match ErrorCode::from_code(&error.code) {
        Some(code) => ErrorValue::new(code, error.message.as_str()),
        None => ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            format!("{}: {}", error.name, error.message),
        ),
    };
    if let Some(help) = &error.help {
        value = value.with_help(help.as_str());
    }
    value
}

fn json_value(json: Option<&serde_json::Value>) -> Value {
    json.and_then(|json| ono_value::from_json(json, ono_value::builtin_schemas()).ok())
        .unwrap_or(Value::Null)
}

/// One audit event as its record (spec §31.37).
fn audit_record(schema: &Arc<Schema>, event: &AuditEvent) -> Result<RecordValue, ErrorValue> {
    let text_or_null = |text: Option<&String>| text.map_or(Value::Null, |text| Value::string(text));
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set(
            "id",
            ono_value::Uuid::parse(&event.id)
                .map_or_else(|_| Value::string(&event.id), Value::Uuid),
        )?
        .set("plugin", Value::string(&event.plugin))?
        .set("invocation", Value::string(&event.invocation))?
        .set("capability", Value::string(&event.capability))?
        .set("scope", json_value(event.scope.as_ref()))?
        .set("enforcement", kebab(&event.enforcement))?
        .set("action", Value::string(&event.action))?
        .set("target", json_value(event.target.as_ref()))?
        .set(
            "at",
            Value::parse_timestamp(&event.at).unwrap_or_else(|_| Value::string(&event.at)),
        )?
        .set("result", kebab(&event.result))?
        .set(
            "user_confirmation",
            text_or_null(event.user_confirmation.as_ref()),
        )?
        .set("lease", text_or_null(event.lease.as_ref()))?
        .set("link", text_or_null(event.link.as_ref()))?
        .set(
            "error",
            event
                .error
                .as_ref()
                .map_or(Value::Null, |error| wire_error_value(error).into_value()),
        )?
        .build())
}
