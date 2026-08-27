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
use ono_kuang_protocol::{Manifest, PluginState, Role, RuntimeKind};
use ono_kuang_supervisor::LoadedPlugin;
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

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
}

const fn enabled_by_default() -> bool {
    true
}

impl Default for Management {
    fn default() -> Self {
        Self {
            enabled: true,
            installed_from: None,
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

/// The host: where packages are, and which of them run.
#[derive(Debug, Default)]
pub struct Host {
    plugin_path: Vec<PathBuf>,
    state_dir: Option<PathBuf>,
    instances: Vec<Instance>,
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
