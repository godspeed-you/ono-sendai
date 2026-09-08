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
    AuditEvent, Capability, CapabilityRequest, DeclarationClass, ExecutionTier, HOST_API, Manifest,
    PackageSignature, PluginState, Role, RuntimeKind, SIGNATURE_FILE, ShutdownReason, WireError,
    artifact_files,
};
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Policy, Supervisor};
use ono_provider_api::{Action, ActionOutcome, ObjectId};

use crate::kuang_trust::{Trust, TrustStore};
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
    /// The catalog that resolved the install, by name, so an upgrade resolves through the same
    /// lineage and a later catalog collision cannot redirect it (K11P §10.5, ADR-0601 §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// The signing key at install, `ed25519:…`, or `unsigned`. An upgrade signed by an unrelated
    /// key is refused (K11P §34.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_key: Option<String>,
    /// The access profile the install decided, `recommended` unless another was named
    /// (ADR-0602). What readiness and an upgrade's delta are measured against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Where the payload came from: the source kind, its identity, the distribution package
    /// when one supplied it, and the digest the copy is pinned to (K11A §18, ADR-0606 §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::kuang_acquire::Origin>,
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
            catalog: None,
            publisher_key: None,
            profile: None,
            origin: None,
        }
    }
}

/// The session's published context, as a source the loader hands a package (ADR-0567).
pub struct SharedContext(pub Arc<std::sync::Mutex<crate::session_provider::SessionTables>>);

impl std::fmt::Debug for SharedContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedContext")
    }
}

impl ono_kuang_supervisor::ContextSource for SharedContext {
    fn context(&self) -> serde_json::Value {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .context
            .clone()
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
    /// The §31.18 duration word the record carries.
    pub duration: &'static str,
    /// When it stops working. `None` for a grant with no expiry; a grant with one is a lease
    /// (spec §31.49), and the broker checks the window on every call.
    pub expires_at: Option<jiff::Timestamp>,
    /// When it was revoked; `None` while it stands.
    pub revoked_at: Option<Value>,
    /// The user-facing permission whose decision minted it (K11P §16.3, ADR-0604). `None` for
    /// a grant made by hand, which is what lets `get permission` show it as `custom`.
    pub permission: Option<String>,
    /// The access profile whose selection minted it, when one did.
    pub profile: Option<String>,
    /// The request the decision was part of, shared with its audit events (ADR-0604 §5).
    pub correlation: Option<String>,
}

/// What minted a grant beyond the operator's own hand: the permission, the profile and the
/// request it belongs to (ADR-0604).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantOrigin {
    /// The permission id, when a permission decision minted the grant.
    pub permission: Option<String>,
    /// The access profile, when a profile selected it.
    pub profile: Option<String>,
    /// The correlation id of the user action.
    pub correlation: Option<String>,
}

impl Grant {
    /// Whether the grant still answers as of `now`: not revoked, and inside its window.
    #[must_use]
    pub fn stands_at(&self, now: jiff::Timestamp) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|expiry| expiry > now)
    }
}

/// The host: where packages are, which of them run, and what they were granted.
#[derive(Debug, Default)]
pub struct Host {
    plugin_path: Vec<PathBuf>,
    state_dir: Option<PathBuf>,
    /// Where the operator's capability policy is kept — apart from the packages, so a package
    /// update cannot rewrite it (spec §31.19).
    config_dir: Option<PathBuf>,
    instances: Vec<Instance>,
    grants: Vec<Grant>,
    minted: u64,
    /// The trails of instances that are gone, and the host's own events: an unload does not
    /// erase what a package did (spec §31.37).
    retained_audit: Vec<AuditEvent>,
    /// The trail earlier sessions wrote, read back from disk (spec §31.33, §31.37).
    persisted_audit: Vec<AuditEvent>,
    /// The ids already on disk, so a flush appends what is new and nothing twice.
    written_audit: std::collections::BTreeSet<String>,
    /// What tells this session's minted identities from every other session's (see `mint`).
    session_nonce: u32,
    /// Which policy store the grants in memory were read from, so it is read once per session.
    policy_read: Option<PathBuf>,
    /// Whose keys this machine accepts, and the stores that exist and could not be read
    /// (spec §31.36, ADR-0312).
    trust: TrustContext,
    /// Which stores the trust in memory was read from, so they are read once per session.
    trust_read: Option<(PathBuf, Option<PathBuf>)>,
    /// The operator's model providers (spec §31.43's `get model` table, ADR-0566).
    models: ono_model_broker::Catalogue,
    /// Which catalogue the providers in memory were read from, so it is read once per session.
    models_read: Option<PathBuf>,
    /// Why the catalogue could not be read, when it could not: shown beside `get model`'s rows
    /// rather than swallowed, because a catalogue nobody can read is not an empty one.
    models_problem: Option<ErrorValue>,
    /// The permission decisions, read from `<config>/kuang/permissions.yaml` and made this
    /// session (ADR-0604).
    pub(crate) decisions: Vec<crate::kuang_permissions::Decision>,
    /// Which store the decisions were read from, so it is read once per session.
    pub(crate) decisions_read: Option<PathBuf>,
    /// Just-in-time denials remembered for this session, as `(plugin, permission, subject)`,
    /// so a pipeline is not asked twice (ADR-0603 §2).
    pub(crate) session_denials: std::collections::BTreeSet<(String, String, String)>,
    /// The catalogs this session resolves names against (ADR-0601).
    pub(crate) catalogs: crate::kuang_catalog::Catalogs,
    /// Which directories the catalogs were read from, so they are read once per session.
    pub(crate) catalogs_read: Option<(Option<PathBuf>, PathBuf)>,
    /// The local package sources a catalog release is looked for in (ADR-0601 §3).
    pub(crate) sources: Vec<PathBuf>,
    /// The package cache a catalog release is looked for in first.
    pub(crate) cache_dir: Option<PathBuf>,
    /// The system source roots an operating-system package places payloads under (K11A §8.2).
    pub(crate) system_roots: Vec<PathBuf>,
}

/// What the operator's trust stores say, as a verification needs it.
///
/// Carried rather than looked up, because a verification runs inside a stream the host does not
/// outlive: `inspect plugin` builds its record after the pipeline has been handed off.
#[derive(Debug, Clone, Default)]
pub struct TrustContext {
    /// The enrolled keys, system store and user store together.
    pub store: TrustStore,
    /// A store that exists and does not read as one. Fail closed: an unreadable store is not an
    /// empty store, because it may be the one holding the revocation.
    pub problems: Vec<ErrorValue>,
}

/// One package's stored capability decisions (spec §31.19's `policy.yaml`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct StoredPolicy {
    #[serde(default)]
    plugins: std::collections::BTreeMap<String, std::collections::BTreeMap<String, StoredDecision>>,
}

/// One stored decision: `allow` on its own, or the scope it is bounded by.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct StoredDecision {
    /// `allow` or `deny`. §31.19's example writes the word alone; this build writes a mapping so
    /// the scope has somewhere to live, and reads both.
    #[serde(default = "allow_word")]
    decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<serde_json::Map<String, serde_json::Value>>,
    /// The permission whose decision minted it (ADR-0604 §1). Absent for a manual grant, and
    /// for every grant an earlier release wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    permission: Option<String>,
    /// The profile whose selection minted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
}

fn allow_word() -> String {
    "allow".to_owned()
}

impl Host {
    /// Tells the host where the session's plugin home and state directory are. Called before
    /// every pipeline, since both come from the environment and the environment can change.
    pub fn configure(
        &mut self,
        plugin_path: Vec<PathBuf>,
        state_dir: Option<PathBuf>,
        config_dir: Option<PathBuf>,
        system_trust: PathBuf,
    ) {
        self.plugin_path = plugin_path;
        self.state_dir = state_dir;
        self.config_dir = config_dir;
        self.read_policy();
        self.read_decisions();
        self.read_models();
        self.read_trust(system_trust);
        self.read_audit();
    }

    /// Tells the host where names resolve from and where a catalog release is looked for:
    /// the operator's catalogs beside the machine's, the local package sources and the package
    /// cache (ADR-0601). Called beside [`Self::configure`].
    pub fn configure_sources(
        &mut self,
        system_config_dir: PathBuf,
        sources: Vec<PathBuf>,
        cache_dir: Option<PathBuf>,
        system_roots: Vec<PathBuf>,
    ) {
        self.sources = sources;
        self.cache_dir = cache_dir;
        self.system_roots = system_roots;
        let read_from = (self.config_dir.clone(), system_config_dir.clone());
        if self.catalogs_read.as_ref() != Some(&read_from) {
            self.catalogs = crate::kuang_catalog::Catalogs::read(
                self.config_dir.as_deref(),
                &system_config_dir,
            );
            self.catalogs_read = Some(read_from);
        }
    }

    /// Where the operator's model providers are declared: beside `policy.yaml` (ADR-0566).
    fn models_path(&self) -> Option<PathBuf> {
        self.config_dir
            .as_ref()
            .map(|dir| dir.join("kuang").join("models.yaml"))
    }

    /// Reads the model catalogue, once per path.
    fn read_models(&mut self) {
        let Some(path) = self.models_path() else {
            return;
        };
        if self.models_read.as_ref() == Some(&path) {
            return;
        }
        self.models_read = Some(path.clone());
        match ono_model_broker::Catalogue::read(&path) {
            Ok(catalogue) => {
                self.models = catalogue;
                self.models_problem = None;
            }
            Err(error) => {
                self.models = ono_model_broker::Catalogue::default();
                self.models_problem = Some(
                    ErrorValue::new(
                        ErrorCode::ProviderSchemaViolation,
                        format!("`{}`: {error}", path.display()),
                    )
                    .with_help(
                        "the file declares `providers`, one entry per model provider, as \
                         `docs/contracts/kuang/model-broker.v1.yaml` describes",
                    ),
                );
            }
        }
    }

    /// The `ono.model-provider/1` records of `get model` (spec §31.43), and the catalogue's
    /// problem beside them when it has one.
    pub fn model_records(&self) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        let schema = schema("ono.model-provider")?;
        let path = std::env::var_os("PATH");
        let mut records = Vec::new();
        for provider in self.models.providers() {
            records.push(model_provider_record(&schema, provider, path.as_ref())?);
        }
        Ok((records, self.models_problem.iter().cloned().collect()))
    }

    /// The broker a loaded package's `models.*` calls reach: the catalogue, over the
    /// `ono-model/1` command transport.
    #[must_use]
    pub fn model_broker(&self) -> Arc<dyn ono_model_broker::ModelBroker> {
        Arc::new(ono_model_broker::CommandBroker::new(
            self.models.clone(),
            std::env::var_os("PATH"),
        ))
    }

    /// Reads both trust stores into this session, once per pair of paths (spec §31.36).
    fn read_trust(&mut self, system: PathBuf) {
        let user = crate::kuang_trust::user_path(self.config_dir.as_deref());
        let paths = (system, user);
        if self.trust_read.as_ref() == Some(&paths) {
            return;
        }
        let (store, problems) = TrustStore::read(Some(&paths.0), paths.1.as_deref());
        self.trust = TrustContext { store, problems };
        self.trust_read = Some(paths);
    }

    /// Whose keys this machine accepts.
    #[must_use]
    pub fn trust(&self) -> &TrustContext {
        &self.trust
    }

    /// The operator's configuration directory, where policy, decisions and catalogs live.
    #[must_use]
    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }

    /// Where the operator's capability policy lives (spec §31.19's suggested location).
    fn policy_path(&self) -> Option<PathBuf> {
        self.config_dir
            .as_ref()
            .map(|dir| dir.join("kuang").join("policy.yaml"))
    }

    /// Where the audit trail is appended (spec §31.33, §31.37).
    fn audit_path(&self) -> Option<PathBuf> {
        self.state_dir
            .as_ref()
            .map(|dir| dir.join("kuang").join("audit.jsonl"))
    }

    /// Reads the operator's stored grants into this session, once per store (spec §31.19).
    ///
    /// A stored grant enters the session as `always`, sourced `user-policy`: §31.19's precedence
    /// distinguishes the layer a decision came from, and an operator asking why a package they
    /// never granted anything to holds a capability is asking exactly that question.
    fn read_policy(&mut self) {
        let Some(path) = self.policy_path() else {
            return;
        };
        if self.policy_read.as_ref() == Some(&path) {
            return;
        }
        self.policy_read = Some(path.clone());
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if ono_value::yaml_depth(&text) > ono_value::MAX_YAML_DEPTH {
            return;
        }
        let Ok(stored) = serde_yaml_ng::from_str::<StoredPolicy>(&text) else {
            return;
        };
        for (plugin, decisions) in stored.plugins {
            for (capability, decision) in decisions {
                let Some(capability) = Capability::from_id(&capability) else {
                    continue;
                };
                if decision.decision != "allow" {
                    continue;
                }
                let id = self.mint();
                self.grants.push(Grant {
                    id,
                    plugin: plugin.clone(),
                    capability,
                    scope: decision.scope,
                    class: None,
                    source: "user-policy",
                    granted_at: Value::now(),
                    duration: "always",
                    expires_at: None,
                    revoked_at: None,
                    permission: decision.permission,
                    profile: decision.profile,
                    correlation: None,
                });
            }
        }
    }

    /// Writes the `always` grants that stand back to the policy store (spec §31.18, §31.19).
    ///
    /// # Errors
    ///
    /// The I/O failure, which an install transaction rolls back on (ADR-0602 §1).
    pub(crate) fn write_policy(&self) -> Result<(), ErrorValue> {
        let Some(path) = self.policy_path() else {
            return Ok(());
        };
        let mut stored = StoredPolicy::default();
        for grant in self
            .grants
            .iter()
            .filter(|grant| grant.duration == "always" && grant.revoked_at.is_none())
        {
            stored
                .plugins
                .entry(grant.plugin.clone())
                .or_default()
                .insert(
                    grant.capability.id().to_owned(),
                    StoredDecision {
                        decision: "allow".to_owned(),
                        scope: grant.scope.clone(),
                        permission: grant.permission.clone(),
                        profile: grant.profile.clone(),
                    },
                );
        }
        let text = serde_yaml_ng::to_string(&stored).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the policy store does not serialise: {error}"),
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        }
        std::fs::write(&path, text).map_err(|error| io_error(&path, &error))
    }

    /// Reads the trail earlier sessions wrote (spec §31.37).
    fn read_audit(&mut self) {
        let Some(path) = self.audit_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        self.persisted_audit = text
            .lines()
            .filter_map(|line| serde_json::from_str::<AuditEvent>(line).ok())
            .collect();
        self.written_audit = self
            .persisted_audit
            .iter()
            .map(|event| event.id.clone())
            .collect();
    }

    /// Appends everything this session has recorded and not yet written (spec §31.33, §31.37).
    ///
    /// An audit trail that dies with the process answers "what did that package do?" only for as
    /// long as nobody has left the shell, so it is appended at the start of every pipeline and
    /// once more when the session ends.
    pub fn persist_audit(&mut self) {
        let Some(path) = self.audit_path() else {
            return;
        };
        let fresh: Vec<AuditEvent> = self
            .live_audit()
            .into_iter()
            .filter(|event| !self.written_audit.contains(&event.id))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let mut text = String::new();
        for event in &fresh {
            let Ok(line) = serde_json::to_string(event) else {
                continue;
            };
            text.push_str(&line);
            text.push('\n');
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
        if appended.is_ok() {
            self.written_audit
                .extend(fresh.iter().map(|event| event.id.clone()));
            // What is on disk is what `get audit` reads, so an event moves from the live list to
            // the persisted one in the same step — never into neither.
            self.persisted_audit.extend(fresh);
        }
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

    /// The installed package a reference names: its canonical id, or its short name when
    /// exactly one installed package carries it (K11P §10.4, ADR-0601 §1).
    ///
    /// # Errors
    ///
    /// `plugin.not_found` when nothing answers, `plugin.reference_ambiguous` listing every
    /// candidate when two installed packages share the name.
    pub fn resolve_installed(&self, reference: &str) -> Result<Installed, ErrorValue> {
        let (installed, _) = self.installed();
        if let Some(package) = installed
            .iter()
            .find(|package| package.manifest.package.id == reference)
        {
            return Ok(package.clone());
        }
        let named: Vec<&Installed> = installed
            .iter()
            .filter(|package| package.manifest.package.name == reference)
            .collect();
        match named.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(ErrorValue::new(
                ErrorCode::PluginNotFound,
                format!("no installed package answers to `{reference}`"),
            )
            .with_help(
                "`get plugin` lists the installed set by id and name; `find plugin <word>` \
                 searches the catalogs (K11P §10)",
            )),
            several => {
                let ids: Vec<String> = several
                    .iter()
                    .map(|package| package.manifest.package.id.clone())
                    .collect();
                Err(ErrorValue::new(
                    ErrorCode::PluginReferenceAmbiguous,
                    format!(
                        "`{reference}` names {} installed packages: {}",
                        ids.len(),
                        ids.join(", ")
                    ),
                )
                .with_help("name one of them by its canonical id (K11P §10.3)")
                .with_metadata(
                    "candidates",
                    Value::list(ids.iter().map(|id| Value::string(id))),
                ))
            }
        }
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

    /// The package's own directory under the host's state root (spec §31.31).
    ///
    /// `~/.local/state/ono/kuang/<package-id>/` — the one place on the machine that belongs to
    /// this package: its private working directory is made inside it, and its persistent state
    /// lives beside that. `None` when the session has no state root at all.
    #[must_use]
    pub fn private_dir(&self, id: &str) -> Option<PathBuf> {
        self.state_dir
            .as_ref()
            .map(|dir| dir.join("kuang").join(id))
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

    /// A fresh identity for a grant or a host event: in order within the session, and unlike
    /// any earlier session's.
    ///
    /// The trail on disk keeps an event once, by id (spec §31.33), so a counter that restarted
    /// at one in every process minted, in the second session, the ids the first had already
    /// written — and the second session's grants and decisions never reached the file. The
    /// process and the clock are folded into the bytes the v4 shape leaves free.
    fn mint(&mut self) -> ono_value::Uuid {
        if self.session_nonce == 0 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default();
            let mixed = (nanos as u64) ^ (u64::from(std::process::id()) << 32);
            self.session_nonce = ((mixed as u32) ^ ((mixed >> 32) as u32)).max(1);
        }
        self.minted += 1;
        let mut bytes = [0_u8; 16];
        let nonce = self.session_nonce.to_be_bytes();
        bytes[2..6].copy_from_slice(&nonce);
        bytes[6] = 0x40;
        bytes[7] = nonce[3] ^ nonce[0];
        bytes[8] = 0x80;
        bytes[9] = nonce[2] ^ nonce[1];
        bytes[10..].copy_from_slice(&self.minted.to_be_bytes()[2..]);
        ono_value::Uuid::from_bytes(bytes)
    }

    /// Records a grant of `capability` to `plugin`, answering it (spec §31.18). `origin` says
    /// which permission decision minted it, when one did (ADR-0604).
    #[allow(
        clippy::too_many_arguments,
        reason = "a grant simply has this many parts, and every caller states each one"
    )]
    pub fn grant(
        &mut self,
        plugin: &str,
        capability: Capability,
        scope: Option<serde_json::Map<String, serde_json::Value>>,
        class: Option<DeclarationClass>,
        source: &'static str,
        duration: &'static str,
        expires_at: Option<jiff::Timestamp>,
        origin: GrantOrigin,
    ) -> Grant {
        let grant = Grant {
            id: self.mint(),
            plugin: plugin.to_owned(),
            capability,
            scope,
            class,
            source,
            granted_at: Value::now(),
            duration,
            expires_at,
            revoked_at: None,
            permission: origin.permission,
            profile: origin.profile,
            correlation: origin.correlation.clone(),
        };
        self.grants.push(grant.clone());
        self.record_host_event_correlated(
            plugin,
            capability.id(),
            "capability.grant",
            true,
            origin.correlation,
        );
        if duration == "always" {
            let _ = self.write_policy();
        }
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
        if grant.duration == "always" {
            let _ = self.write_policy();
        }
        self.record_host_event_correlated(
            &grant.plugin,
            grant.capability.id(),
            "capability.revoke",
            true,
            grant.correlation.clone(),
        );
        Some(grant)
    }

    /// [`Self::revoke`], stamped with the request the revocation is part of (ADR-0604 §5).
    pub fn revoke_correlated(&mut self, id: ono_value::Uuid, correlation: &str) -> Option<Grant> {
        let index = self
            .grants
            .iter()
            .position(|grant| grant.id == id && grant.revoked_at.is_none())?;
        self.grants[index].correlation = Some(correlation.to_owned());
        self.revoke(id)
    }

    /// Revokes every grant that still stands for `plugin`, answering how many ended.
    ///
    /// A grant is made to one package (spec §31.18), so a package that is removed takes the
    /// permissions it held with it unless `remove plugin --keep-grants` says otherwise
    /// (spec §31.81, ADR-0233). The grants themselves are retained as revoked rather than
    /// deleted, so the audit trail still shows what the package was once allowed to do.
    pub fn revoke_grants_of(&mut self, plugin: &str) -> usize {
        let standing: Vec<ono_value::Uuid> =
            self.standing_grants(plugin).map(|grant| grant.id).collect();
        standing
            .into_iter()
            .filter(|id| self.revoke(*id).is_some())
            .count()
    }

    /// The grants that stand for `plugin`, oldest first.
    ///
    /// A lease whose window has closed does not stand: §31.49 makes the expiry part of the
    /// grant, so `get capability` must stop calling it an allow the moment it stops being one.
    pub fn standing_grants(&self, plugin: &str) -> impl Iterator<Item = &Grant> {
        let now = jiff::Timestamp::now();
        self.grants
            .iter()
            .filter(move |grant| grant.plugin == plugin && grant.stands_at(now))
    }

    /// Every grant ever made, revoked ones included: a revoked grant is retained rather than
    /// deleted (`ono.capability-grant/1`).
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The broker policy the standing grants of `plugin` amount to (spec §31.19), with a
    /// permission the user denied standing ahead of any grant (ADR-0604 §2).
    #[must_use]
    pub fn policy_for(&self, plugin: &str) -> Policy {
        let mut policy = self
            .standing_grants(plugin)
            .fold(Policy::deny_all(), |policy, grant| {
                policy.grant_for(
                    grant.capability,
                    grant.scope.clone(),
                    grant.expires_at,
                    grant.permission.clone(),
                )
            });
        for decision in self
            .decisions
            .iter()
            .filter(|decision| decision.plugin == plugin && decision.record.decision == "deny")
        {
            for capability in &decision.record.capabilities {
                if let Some(capability) = Capability::from_id(capability) {
                    policy = policy.deny(capability);
                }
            }
        }
        policy
    }

    /// Keeps the trail of an instance that is going away.
    pub fn retain_audit(&mut self, events: Vec<AuditEvent>) {
        self.retained_audit.extend(events);
    }

    /// Records a host-side action about a package — a load, a grant, a revocation — in the
    /// same trail the packages' own actions go to (spec §31.37).
    pub fn record_host_event(&mut self, plugin: &str, capability: &str, action: &str, ok: bool) {
        self.record_host_event_correlated(plugin, capability, action, ok, None);
    }

    /// [`Self::record_host_event`], stamped with the request it is part of (ADR-0604 §5).
    pub fn record_host_event_correlated(
        &mut self,
        plugin: &str,
        capability: &str,
        action: &str,
        ok: bool,
        correlation: Option<String>,
    ) {
        let id = self.mint();
        self.retained_audit.push(AuditEvent {
            // The host is one source among several (`AuditTrail::for_source`); its events carry
            // the same v4-shaped identity with the host's own namespace, so nothing it records
            // can collide with a package's trail.
            id: format!("ffffffff-{}", &id.to_string()[9..]),
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
            correlation,
        });
    }

    /// Every audit event the host knows: the retained ones, then each running instance's.
    #[must_use]
    pub fn audit_events(&self) -> Vec<AuditEvent> {
        let mut events = self.persisted_audit.clone();
        events.extend(
            self.live_audit()
                .into_iter()
                .filter(|event| !self.written_audit.contains(&event.id)),
        );
        events
    }

    /// What this session has recorded: the retained trails and every running instance's.
    fn live_audit(&self) -> Vec<AuditEvent> {
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

    /// The derived readiness of a package (K11P §12.2, §17.2, ADR-0602 §2): a projection over
    /// the internal state, never a replacement for it.
    #[must_use]
    pub fn readiness(
        &self,
        package: &Installed,
        management: &Management,
        instance: Option<&Instance>,
    ) -> &'static str {
        if let Some(instance) = instance {
            match instance.plugin.state() {
                PluginState::Quarantined => return "blocked",
                PluginState::Installed | PluginState::Enabled => {}
                _ => return "running",
            }
        }
        if !management.enabled {
            return "blocked";
        }
        let signature = signature_of(package);
        if signature.failure.is_some() {
            return "blocked";
        }
        if standing_of(&signature, &self.trust).blocks() {
            return "blocked";
        }
        if crate::kuang_permissions::undecided(self, package, management).is_empty() {
            "ready"
        } else {
            "needs-permission"
        }
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
            let instance = self.instance(&package.manifest.package.id);
            let readiness = self.readiness(package, &management, instance);
            records.push(plugin_record(
                &schema,
                package,
                &management,
                instance,
                &self.trust,
                readiness,
            )?);
        }
        Ok((records, failures))
    }
}

/// One `ono.model-provider/1` record, field for field from the catalogue entry (ADR-0566).
fn model_provider_record(
    schema: &Arc<Schema>,
    provider: &ono_model_broker::ModelProvider,
    path: Option<&std::ffi::OsString>,
) -> Result<RecordValue, ErrorValue> {
    let unavailable = provider.unavailable_reason(path);
    let strings =
        |items: Vec<String>| Value::list(items.into_iter().map(|item| Value::string(&item)));
    let transformed: ono_value::MapValue = provider
        .transformed_classes()
        .into_iter()
        .map(|(class, how)| (Arc::<str>::from(class.as_str()), Value::string(&how)))
        .collect();
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::string(&provider.id))?
        .set("name", Value::string(&provider.name))?
        .set("kind", Value::string(provider.kind.as_str()))?
        .set("location", Value::string(&provider.location))?
        .set(
            "endpoint",
            provider
                .shown_endpoint()
                .map_or(Value::Null, |endpoint| Value::string(&endpoint)),
        )?
        .set(
            "context_window",
            provider
                .context_window
                .map_or(Value::Null, |tokens| Value::Int(i128::from(tokens))),
        )?
        .set("tools", Value::Bool(provider.tools))?
        .set("structured_output", Value::Bool(provider.structured_output))?
        .set("streaming", Value::Bool(provider.streaming))?
        .set("data_policy", Value::string(provider.data_policy.as_str()))?
        .set("allowed_classes", strings(provider.allowed_classes()))?
        .set("transformed_classes", Value::Map(Arc::new(transformed)))?
        .set("denied_classes", strings(provider.denied_classes()))?
        .set("available", Value::Bool(unavailable.is_none()))?
        .set(
            "unavailable_reason",
            unavailable.map_or(Value::Null, |why| Value::string(&why)),
        )?
        .build())
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
            // A permission mapping that lies is refused under its own code (ADR-0605), so a
            // script can tell it from a manifest that is merely malformed.
            let code =
                if error.code() == ono_kuang_protocol::KuangErrorCode::PermissionInvalidMapping {
                    ErrorCode::KuangPermissionInvalidMapping
                } else {
                    ErrorCode::ProviderSchemaViolation
                };
            let mut failure = ErrorValue::new(
                code,
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

/// The named execution tier a loaded instance of this package runs in (v0.4.1 §17.2).
///
/// A name rather than a boolean. `isolation` above answers "what kind of thing is the artifact",
/// out of spec §31.10's manifest vocabulary; this answers "what is installed around it", which is
/// the question §17.3 forbids anyone answering with the bare word "sandboxed". The two are
/// deliberately separate fields, because a `wasm-component` package on a build with no component
/// runtime declares one and runs in neither.
#[must_use]
pub fn execution_tier(manifest: &Manifest) -> &'static str {
    match manifest.runtime.as_ref().map(|runtime| runtime.kind) {
        Some(RuntimeKind::NativeProcess) => ExecutionTier::NativeConfined.id(),
        Some(RuntimeKind::WasmComponent) => ExecutionTier::Wasm.id(),
        Some(RuntimeKind::RemoteService) => "remote-service",
        // A declarative package has no runtime of its own: what runs is the core's interpreter of
        // its packs, in process (ADR-0107).
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

/// The content hash of the package's artifact (spec §31.36's "are these the exact bytes
/// referenced?").
///
/// It covers every file of [`artifact_files`], each under its own path, so moving a byte from
/// one file to another changes the answer.
#[must_use]
pub fn integrity_of(package: &Installed) -> String {
    ono_kuang_protocol::content_digest(&package.directory)
}

/// What the package's signature says (spec §31.36's "did a key sign these bytes?").
#[derive(Debug, Clone)]
pub struct SignatureCheck {
    /// `valid`, `invalid` or `absent`, as `ono.verification-result/1` spells them.
    pub state: &'static str,
    /// The document, when the package carries one that reads as a signature. Present even when
    /// the signature does not verify: what it claims is still what it claims.
    pub document: Option<PackageSignature>,
    /// Why the signature does not belong to this artifact. `None` when it does, and when there
    /// is none — `absent` is not a failure (spec §31.36).
    pub failure: Option<ErrorValue>,
    /// Who a keyless signature says signed, once it verified (ADR-0609). `None` when the package
    /// carries none, or when the one it carries did not verify.
    pub identity: Option<ono_kuang_protocol::KeylessIdentity>,
    /// The publisher the package claims, which is what an enrolled identity is judged against.
    pub publisher: String,
}

/// What the operator's stores say about whoever signed, whichever way they signed (ADR-0609 §3).
///
/// A revocation anywhere wins, as it does for a key alone. Otherwise the strongest standing any
/// signature on the package earns is the one reported: a package signed both ways is as trusted
/// as the better of its two answers, and a package signed neither way is `Unknown`.
#[must_use]
pub fn standing_of(check: &SignatureCheck, trust: &TrustContext) -> Trust {
    let mut standings = Vec::new();
    if check.failure.is_none() {
        if let Some(document) = &check.document {
            standings.push(trust.store.judge(document.publisher(), document.key()));
        }
        if let Some(identity) = &check.identity {
            standings.push(trust.store.judge_identity(&check.publisher, identity));
        }
    }
    if standings.contains(&Trust::Untrusted) {
        return Trust::Untrusted;
    }
    for wanted in [Trust::SystemTrusted, Trust::UserTrusted] {
        if standings.contains(&wanted) {
            return wanted;
        }
    }
    Trust::Unknown
}

/// Checks the signature the package carries, if it carries one.
///
/// A document that is there and cannot be read is `invalid`, never `absent`: a signature the
/// host cannot check is a claim it must not pass over.
#[must_use]
pub fn signature_of(package: &Installed) -> SignatureCheck {
    let publisher = package.manifest.package.publisher.clone();
    let keyed = std::fs::read_to_string(package.directory.join(SIGNATURE_FILE)).ok();
    let bundle =
        std::fs::read_to_string(package.directory.join(ono_kuang_protocol::BUNDLE_FILE)).ok();
    if keyed.is_none() && bundle.is_none() {
        return SignatureCheck {
            state: "absent",
            document: None,
            failure: None,
            identity: None,
            publisher,
        };
    }
    let refused = |document, identity, error: &ono_kuang_protocol::KuangError| SignatureCheck {
        state: "invalid",
        document,
        failure: Some(crate::plugins::error_value(error)),
        identity,
        publisher: package.manifest.package.publisher.clone(),
    };

    // A package may carry either form or both, and where both are present both must verify: one
    // that is honest under one signature and not the other is not honest (ADR-0609 §1).
    let mut document = None;
    if let Some(text) = &keyed {
        match PackageSignature::parse(text) {
            Err(error) => return refused(None, None, &error),
            Ok(parsed) => {
                if let Err(error) =
                    parsed.check(&package.manifest, &artifact_files(&package.directory))
                {
                    return refused(Some(parsed), None, &error);
                }
                document = Some(parsed);
            }
        }
    }
    let mut identity = None;
    if let Some(text) = &bundle {
        match ono_kuang_protocol::check_keyless(
            text,
            &package.manifest,
            artifact_files(&package.directory),
        ) {
            Err(error) => return refused(document, None, &error),
            Ok(signer) => identity = Some(signer),
        }
    }
    SignatureCheck {
        state: "valid",
        document,
        failure: None,
        identity,
        publisher,
    }
}

/// The word `ono.plugin/1.trust` and `ono.plugin-package/1.trust` carry for one package
/// (ADR-0312's table).
#[must_use]
pub fn trust_word(check: &SignatureCheck, standing: Trust) -> &'static str {
    match (check.state, standing) {
        ("absent", _) => "local",
        ("invalid", _) | (_, Trust::Untrusted) => "untrusted",
        (_, Trust::SystemTrusted | Trust::UserTrusted) => "verified",
        _ => "signed",
    }
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
    trust: &TrustContext,
    readiness: &str,
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
    let signature = signature_of(package);
    let standing = standing_of(&signature, trust);
    let package_trust = trust_word(&signature, standing);
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::string(&manifest.package.id))?
        .set("name", Value::string(&manifest.package.name))?
        .set("version", Value::string(&manifest.package.version))?
        .set("publisher", Value::string(&manifest.package.publisher))?
        .set("state", Value::string(state.as_str()))?
        // Spec §31.36's four questions stay four answers: this one is about who produced the
        // artifact, and `local` is what an unsigned development package says.
        .set("trust", Value::string(package_trust))?
        .set("isolation", Value::string(isolation(manifest)))?
        // v0.4.1 §17.2: the tier is a name, and it is the name that reaches audit, diagnostics
        // and documentation. `inspect plugin` shows the controls it stands for (ADR-0448).
        .set("execution_tier", Value::string(execution_tier(manifest)))?
        .set("roles", roles)?
        .set("enabled", Value::Bool(management.enabled))?
        .set("readiness", Value::string(readiness))?
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

pub(crate) fn io_error(path: &Path, error: &std::io::Error) -> ErrorValue {
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
        // An installed package by its canonical id or its short name (K11P §10.4).
        let package = self.resolve_installed(reference)?;
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
        // The hash recorded at install is what the *installed* bytes are held to. A candidate
        // read from elsewhere — an upgrade waiting in a source directory — is judged on its own
        // signature and trust, and recorded once it is placed (K11P §21.1).
        let management = resolved
            .package
            .as_ref()
            .ok()
            .filter(|package| {
                self.installed_package(&package.manifest.package.id)
                    .is_some_and(|installed| installed.directory == package.directory)
            })
            .map(|package| self.management(&package.manifest.package.id))
            .unwrap_or_default();
        verification(resolved, &management, &self.trust)
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
        // `--source system|catalog|local` narrows to one kind (K11A §10.4); `path:<dir>` reads
        // one directory; nothing narrows the installed set out of the answer.
        let kinds = match source {
            Some("system") => Some(ono_kuang_protocol::SourceKind::SystemPackage),
            Some("catalog") => Some(ono_kuang_protocol::SourceKind::CatalogNetwork),
            Some("local") => Some(ono_kuang_protocol::SourceKind::LocalPath),
            _ => None,
        };
        let source = if kinds.is_some() { None } else { source };
        let (candidates, mut failures) = match source {
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
                    .with_help(
                        "`--source system`, `--source catalog`, `--source local` or \
                         `--source path:<directory>` (spec §31.9, K11A §10.4)",
                    ));
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
            // An installed package answers with the lineage it was installed from (K11A §18).
            let origin = already.then(|| self.management(&info.id).origin).flatten();
            let kind = origin
                .as_ref()
                .and_then(crate::kuang_acquire::Origin::source_kind)
                .unwrap_or(ono_kuang_protocol::SourceKind::LocalPath);
            if kinds.is_some_and(|wanted| wanted != kind) {
                continue;
            }
            records.push(package_record(
                &schema,
                &package,
                &reference,
                already,
                &self.trust,
                None,
                kind,
                origin
                    .as_ref()
                    .and_then(|origin| origin.system_package.as_deref()),
            )?);
        }
        // The catalogs and the system sources, after the installed set (K11P §11.6, K11A
        // §10.1): nothing is executed, and a catalog's answer is marked as a catalog's
        // (ADR-0601 §2).
        let (from_catalogs, problems) =
            crate::kuang_catalog::search(self, term, &installed, kinds)?;
        records.extend(from_catalogs);
        failures.extend(problems);
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
    trust: &TrustContext,
) -> Result<Verification, ErrorValue> {
    let schema = schema("ono.verification-result")?;
    let mut blocking = Vec::new();
    let mut warnings = vec!["transparency: unknown".to_owned()];
    let mut signature = "unknown";
    let mut publisher = Value::Null;
    let mut key = Value::Null;
    let mut standing = Trust::Unknown;
    // A store that exists and cannot be read is blocking, because it may be the one holding the
    // revocation: an unreadable trust store is not an empty one (ADR-0312).
    for problem in &trust.problems {
        blocking.push(
            problem
                .clone()
                .with_metadata("check", Value::string("publisher")),
        );
    }
    let (package_name, integrity, compatibility, manifest, runtime) = match &resolved.package {
        Ok(package) => {
            let check = signature_of(package);
            signature = check.state;
            if let Some(document) = &check.document {
                // The key is reported whenever a document names one, valid or not: an operator
                // asking why a signature failed is asking whose key it claimed to be. The
                // publisher is reported only when the signature holds, because a publisher is
                // what a valid signature *attests to* and an invalid one attests to nothing
                // (`ono.verification-result/1`).
                key = Value::string(&document.key().to_string());
                if check.failure.is_none() {
                    publisher = Value::string(document.publisher());
                }
            }
            if let Some(failure) = &check.failure {
                blocking.push(
                    failure
                        .clone()
                        .with_metadata("check", Value::string("signature")),
                );
            }
            match (
                check.document.is_some() || check.identity.is_some(),
                check.failure.is_none(),
            ) {
                (true, true) => {
                    standing = standing_of(&check, trust);
                    if standing == Trust::Unknown {
                        warnings.push(
                            "trust: unknown, no trust store enrols this key for this publisher"
                                .to_owned(),
                        );
                    }
                }
                _ => warnings.push(format!("signature: {signature}")),
            }
            if standing.blocks() {
                blocking.push(
                    ErrorValue::new(
                        ErrorCode::KuangPublisherUntrusted,
                        format!(
                            "the key that signed `{}` is revoked in a trust store",
                            package.manifest.package.id
                        ),
                    )
                    .with_help("`verify plugin` names the key; remove its `revoked` entry to accept it again (spec §31.36)")
                    .with_metadata("check", Value::string("publisher")),
                );
            }
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
        .set("signature", Value::string(signature))?
        .set("publisher", publisher)?
        .set("key", key)?
        .set("trust", Value::string(standing.name()))?
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
#[allow(
    clippy::too_many_arguments,
    reason = "a package record has this many independent facts, and every caller states each"
)]
pub fn package_record(
    schema: &Arc<Schema>,
    package: &Installed,
    source: &str,
    installed: bool,
    trust: &TrustContext,
    catalog: Option<(&str, &str)>,
    kind: ono_kuang_protocol::SourceKind,
    system_package: Option<&str>,
) -> Result<RecordValue, ErrorValue> {
    let manifest = &package.manifest;
    let signature = signature_of(package);
    let standing = signature
        .document
        .as_ref()
        .map_or(Trust::Unknown, |document| {
            if signature.failure.is_none() {
                trust.store.judge(document.publisher(), document.key())
            } else {
                Trust::Unknown
            }
        });
    Ok(RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("id", Value::string(&manifest.package.id))?
        .set("name", Value::string(&manifest.package.name))?
        .set("version", Value::string(&manifest.package.version))?
        .set("publisher", Value::string(&manifest.package.publisher))?
        .set("summary", Value::string(&manifest.package.description))?
        .set("source", Value::string(source))?
        .set("source_kind", Value::string(kind.id()))?
        .set(
            "system_package",
            system_package.map_or(Value::Null, Value::string),
        )?
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
        .set("signature", Value::string(signature.state))?
        .set("trust", Value::string(trust_word(&signature, standing)))?
        .set("installed", Value::Bool(installed))?
        .set(
            "catalog",
            catalog.map_or(Value::Null, |(name, _)| Value::string(name)),
        )?
        .set(
            "catalog_verification",
            catalog.map_or(Value::Null, |(_, verification)| Value::string(verification)),
        )?
        .set("size", Value::Null)?
        .set("published_at", Value::Null)?
        .build())
}

// --- inspection, spec §31.33 -------------------------------------------------------------------

/// The human layer of an inspection (K11P §25.3): the permission records, the profiles, the
/// derived readiness and the standing grants, computed by the host while it holds its tables.
#[derive(Debug, Clone, Default)]
pub struct InspectionHuman {
    /// `ono.permission/1` values, every descriptor with its standing decision.
    pub permissions: Vec<Value>,
    /// The profiles the package offers, as records.
    pub profiles: Value,
    /// The derived readiness label.
    pub readiness: String,
    /// `ono.capability-grant/1` values of every grant that stands.
    pub grants: Vec<Value>,
}

/// The host's own sentence about a package's execution tier (v0.4.1 §15.2, K11P §18.2, §27.1):
/// never package-authored, and never the word "sandboxed" without the boundary it means.
#[must_use]
pub fn isolation_statement(manifest: &Manifest) -> &'static str {
    match manifest.runtime.as_ref().map(|runtime| runtime.kind) {
        Some(RuntimeKind::NativeProcess) => {
            "This plugin runs as the Ono user. KUANG/11 mediates brokered host capabilities and \
             applies process confinement, but this execution tier is not complete filesystem or \
             network isolation."
        }
        Some(RuntimeKind::WasmComponent) => {
            "This plugin runs as a component inside the runtime Ono embeds, with nothing but \
             its standard streams; every file, connection and program goes through the broker."
        }
        Some(RuntimeKind::RemoteService) => {
            "This plugin runs no local code; it is a protocol endpoint the broker connects to."
        }
        Some(RuntimeKind::Declarative) | None => {
            "This package runs no code; it contributes declarations only."
        }
    }
}

/// What an instance contributes, by kind — the resolved ids.
#[derive(Debug, Clone, Default)]
pub struct Contributions {
    /// Command ids.
    pub commands: Vec<String>,
    /// Target names.
    pub targets: Vec<String>,
    /// Schema ids.
    pub schemas: Vec<String>,
    /// View ids (spec §31.27).
    pub views: Vec<String>,
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
            views: plugin.views().iter().map(|view| view.id.clone()).collect(),
        }
    }

    fn value(&self) -> Value {
        map([
            ("commands", string_list(&self.commands)),
            ("targets", string_list(&self.targets)),
            ("schemas", string_list(&self.schemas)),
            ("views", string_list(&self.views)),
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

/// What an instance is actually confined by, as the contract record shows it (spec §31.10).
///
/// Every field is something the host applied, not something the manifest asked for, and the two
/// `confinement` fields say in spec §31.16's own vocabulary how far each one reaches: a scope the
/// host can only check when the package asks it is `broker`, never presented as a boundary.
fn sandbox_of(instance: &Instance) -> Value {
    let sandbox = instance.plugin.sandbox();
    let bytes = |value: u64| Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(value)));
    map([
        ("memory_max", bytes(sandbox.memory_max)),
        (
            "memory_peak",
            instance.plugin.peak_memory().map_or(Value::Null, bytes),
        ),
        ("cpu_class", kebab(&sandbox.cpu_class)),
        ("nice", Value::Int(i128::from(sandbox.nice))),
        ("open_files", Value::Int(i128::from(sandbox.open_files))),
        ("file_size", bytes(sandbox.file_size)),
        (
            "working_directory",
            Value::Path(sandbox.working_directory.clone().into()),
        ),
        (
            "environment",
            Value::list(sandbox.environment.iter().map(|name| Value::string(name))),
        ),
        ("filesystem", Value::string(sandbox.filesystem.as_str())),
        ("network", Value::string(sandbox.network.as_str())),
    ])
}

/// The confinement report of v0.4.1 §16.5, one row per control the tier claimed.
///
/// Available here rather than only behind `RUST_LOG=debug`, because §54.2 says a refusal a user
/// meets has to be explainable without turning debug logging on, and a control that is *not* in
/// force is exactly that kind of fact. §16.5 also forbids the report exposing secrets:
/// `platform_detail` is the operating system's own error text and nothing else.
fn confinement_of(instance: &Instance) -> Value {
    let report = instance.plugin.confinement();
    Value::list(report.entries().iter().map(|entry| {
        map([
            ("control", Value::string(entry.control().id())),
            ("required", Value::Bool(entry.required())),
            ("attempted", Value::Bool(entry.attempted())),
            ("result", Value::string(entry.result().as_str())),
            (
                "platform_detail",
                entry.platform_detail().map_or(Value::Null, Value::string),
            ),
        ])
    }))
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
                "execution_tier",
                Value::string(instance.plugin.confinement().tier().id()),
            )?
            // v0.4.1 §15.2, §19.2: the sentence that says what the tier is and what it is not,
            // taken from the tier rather than written beside it, so the record and the
            // documentation cannot drift.
            .set(
                "execution_boundary",
                Value::string(instance.plugin.confinement().tier().boundary()),
            )?
            .set("confinement", confinement_of(instance))?
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
            .set("sandbox", sandbox_of(instance))?
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
    trust: &TrustContext,
    human: &InspectionHuman,
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
    let verification = verification(&resolved, management, trust)?;
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
            .set("capability_grants", Value::list(human.grants.clone()))?
            .set("capability_requests", capability_requests(manifest, plugin))?
            .set("verification", verification.record.into_value())?
            .set("permissions", Value::list(human.permissions.clone()))?
            .set("profiles", human.profiles.clone())?
            .set("readiness", Value::string(&human.readiness))?
            .set(
                "acquisition",
                management.origin.as_ref().map_or(Value::Null, |origin| {
                    origin.value(&manifest.package.version)
                }),
            )?
            .set(
                "isolation_statement",
                Value::string(isolation_statement(manifest)),
            )?
            .set("runtime", runtime)?
            // Spec §31.33's health block. The figures come from the kernel's own accounting of
            // the instance, sampled while it runs; `null` until the host has taken a sample, and
            // `null` for an unloaded package, because an unmeasured figure is not a zero
            // (spec §35.3).
            .set(
                "memory_current",
                plugin
                    .and_then(LoadedPlugin::current_memory)
                    .map_or(Value::Null, bytes),
            )?
            .set(
                "memory_limit",
                plugin.map_or_else(
                    || {
                        manifest
                            .runtime
                            .as_ref()
                            .map_or(Value::Null, |runtime| bytes(runtime.memory_max))
                    },
                    |plugin| bytes(plugin.sandbox().memory_max),
                ),
            )?
            .set(
                "cpu_time",
                plugin
                    .and_then(LoadedPlugin::cpu_time)
                    .map_or(Value::Null, |nanoseconds| {
                        Value::Duration(ono_value::Duration::from_nanoseconds(nanoseconds))
                    }),
            )?
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

    pub(crate) fn remove_directory(&self, directory: &Path) -> Result<(), ErrorValue> {
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
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), ErrorValue> {
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
    let standing = grant.filter(|grant| grant.stands_at(jiff::Timestamp::now()));
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
        .set(
            "duration",
            Value::string(grant.map_or("session", |grant| grant.duration)),
        )?
        .set(
            "granted_at",
            grant.map_or(Value::Null, |grant| grant.granted_at.clone()),
        )?
        .set(
            "expires_at",
            grant
                .and_then(|grant| grant.expires_at)
                .map_or(Value::Null, |expiry| {
                    Value::parse_timestamp(&expiry.to_string())
                        .unwrap_or_else(|_| Value::string(&expiry.to_string()))
                }),
        )?
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
        .set(
            "permission",
            grant
                .and_then(|grant| grant.permission.as_deref())
                .map_or(Value::Null, Value::string),
        )?
        .set(
            "profile",
            grant
                .and_then(|grant| grant.profile.as_deref())
                .map_or(Value::Null, Value::string),
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
        .set("correlation", text_or_null(event.correlation.as_ref()))?
        .build())
}
