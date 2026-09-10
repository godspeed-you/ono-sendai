//! The deterministic test host of spec §31.73.
//!
//! "The SDK MUST include a deterministic test host." This is it: a thin, deliberate wrapper
//! around the *real* supervisor — the same handshake, the same capability broker, the same
//! framing, the same quarantine paths — with the sources of nondeterminism pinned:
//!
//! - **virtual time**: the host clock is fixed, so every audit record and every `clock.now`
//!   answer is reproducible;
//! - **an explicit capability policy**: grants are what the test declares, nothing more, and
//!   `deny by default` is the floor exactly as in production (spec §31.19) — the "fake
//!   capability broker" of §31.73 is the real broker under a test-authored policy;
//! - **recorded outcomes**: the audit trail is the test's assertion surface for the
//!   denial-path cases of spec §31.74.
//!
//! The conformance suite in `ono-kuang-sdk/tests/` runs the example plugin binary under this
//! host; a plugin publisher runs their own binary the same way.

use std::path::PathBuf;

use ono_kuang_protocol::{Capability, KuangError, Manifest};
use ono_kuang_supervisor::{
    ConfinementPlatform, HostClock, HostLimits, LoadConfig, LoadedPlugin, NativePlatform, Policy,
    Supervisor,
};
use serde_json::{Map as JsonMap, Value as Json};

/// The fixed instant every test-host clock reads (spec §31.73's virtual time).
pub const VIRTUAL_NOW: &str = "2026-08-26T12:00:00Z";

/// A deterministic host for one plugin binary.
pub struct TestHost {
    program: PathBuf,
    args: Vec<String>,
    manifest: String,
    policy: Policy,
    limits: HostLimits,
    platform: Option<String>,
    confinement: std::sync::Arc<dyn ConfinementPlatform>,
    models: Option<std::sync::Arc<dyn ono_model_broker::ModelBroker>>,
    context: Option<std::sync::Arc<dyn ono_kuang_supervisor::ContextSource>>,
    host: Option<std::sync::Arc<dyn ono_kuang_supervisor::HostServices>>,
    views: Option<std::sync::Arc<dyn ono_kuang_supervisor::ViewHost>>,
    consent: Option<std::sync::Arc<dyn ono_kuang_supervisor::ConsentSource>>,
}

impl std::fmt::Debug for TestHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn ConfinementPlatform` is not `Debug`, and a test host is read for its program and
        // its policy.
        f.debug_struct("TestHost")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("policy", &self.policy)
            .field("limits", &self.limits)
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

impl TestHost {
    /// A host for `program`, judged against `manifest` (a `kuang-package/1` document).
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, manifest: &str) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            manifest: manifest.to_owned(),
            policy: Policy::deny_all(),
            limits: HostLimits::default(),
            platform: None,
            confinement: NativePlatform::shared(),
            models: None,
            context: None,
            host: None,
            views: None,
            consent: None,
        }
    }

    /// Who answers a just-in-time permission request (K11P §14, ADR-0603). Without one, nobody
    /// can be asked and every such request is `permission.required`;
    /// [`ono_kuang_supervisor::ScriptedConsent`] answers what a test wrote down.
    #[must_use]
    pub fn consent(
        mut self,
        source: std::sync::Arc<dyn ono_kuang_supervisor::ConsentSource>,
    ) -> Self {
        self.consent = Some(source);
        self
    }

    /// Overrides what installs the process-level confinement controls of v0.4.1 §16.1.
    ///
    /// §59.7 requires an acceptance scenario in which `PR_SET_NO_NEW_PRIVS` fails and the plugin
    /// never runs, and no arrangement outside the process can make that call fail. This is the
    /// injectable platform layer that scenario asks for, at the boundary a host actually uses.
    #[must_use]
    pub fn confinement(mut self, platform: std::sync::Arc<dyn ConfinementPlatform>) -> Self {
        self.confinement = platform;
        self
    }

    /// What the object, relation, history, process and secret domains reach. Without one,
    /// every such call answers `provider.unavailable`.
    #[must_use]
    pub fn host(
        mut self,
        services: std::sync::Arc<dyn ono_kuang_supervisor::HostServices>,
    ) -> Self {
        self.host = Some(services);
        self
    }

    /// What takes a view (spec §31.27). Without one, nothing does and every view falls back;
    /// [`RecordingViews`] takes them all and records every tree.
    #[must_use]
    pub fn views(mut self, views: std::sync::Arc<dyn ono_kuang_supervisor::ViewHost>) -> Self {
        self.views = Some(views);
        self
    }

    /// What `context.get` answers with. Without one, the fixed context of spec §31.73.
    #[must_use]
    pub fn context(
        mut self,
        source: std::sync::Arc<dyn ono_kuang_supervisor::ContextSource>,
    ) -> Self {
        self.context = Some(source);
        self
    }

    /// The model broker `models.list` and `models.infer` reach. Without one, nothing is
    /// configured and `models.infer` answers `model.provider_unavailable`.
    #[must_use]
    pub fn models(mut self, broker: std::sync::Arc<dyn ono_model_broker::ModelBroker>) -> Self {
        self.models = Some(broker);
        self
    }

    /// Arguments for the plugin binary (a fixture binary may take a misbehaviour mode).
    #[must_use]
    pub fn args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self
    }

    /// Grants one capability, unscoped.
    #[must_use]
    pub fn grant(mut self, capability: Capability) -> Self {
        self.policy = self.policy.grant(capability, None);
        self
    }

    /// Grants one capability with a scope, e.g. `{"paths": ["/tmp/fixture/**"]}`.
    #[must_use]
    pub fn grant_scoped(mut self, capability: Capability, scope: JsonMap<String, Json>) -> Self {
        self.policy = self.policy.grant(capability, Some(scope));
        self
    }

    /// The operator's home directory `~` resolves against, in a path scope and in a path the
    /// package asks for (ADR-0593).
    ///
    /// A test names one deliberately rather than inheriting the process's `HOME`, so that what
    /// `~/.kube/config` reaches is a fixture the test wrote and not the developer's own file.
    #[must_use]
    pub fn home(mut self, home: impl Into<std::path::PathBuf>) -> Self {
        self.policy = self.policy.with_home(home);
        self
    }

    /// Adds an operator deny, which outranks any grant (spec §31.19).
    #[must_use]
    pub fn deny(mut self, capability: Capability) -> Self {
        self.policy = self.policy.deny(capability);
        self
    }

    /// Overrides the host limits — queue depth, state quota, frame ceiling — for
    /// backpressure and quota cases (spec §31.74).
    #[must_use]
    pub fn limits(mut self, limits: HostLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Overrides the platform tuple the manifest is checked against.
    #[must_use]
    pub fn platform(mut self, platform: &str) -> Self {
        self.platform = Some(platform.to_owned());
        self
    }

    /// Parses the manifest, negotiates, spawns and hands back the loaded instance.
    ///
    /// # Errors
    ///
    /// Exactly the errors `Supervisor::load` reports — the test host adds determinism, never
    /// leniency.
    pub async fn load(self) -> Result<LoadedPlugin, KuangError> {
        let manifest = Manifest::parse(&self.manifest)?;
        let mut config = LoadConfig::new(self.program, manifest);
        config.args = self.args;
        config.policy = self.policy;
        config.limits = self.limits;
        config.clock = HostClock::Fixed(VIRTUAL_NOW.to_owned());
        config.confinement = self.confinement;
        if let Some(models) = self.models {
            config.models = models;
        }
        if let Some(context) = self.context {
            config.context = context;
        }
        if let Some(host) = self.host {
            config.host = host;
        }
        if let Some(views) = self.views {
            config.views = views;
        }
        if let Some(consent) = self.consent {
            config.consent = consent;
        }
        if let Some(platform) = self.platform {
            config.platform = platform;
        }
        Supervisor::load(config).await
    }
}

/// What the test host found in a declarative adapter package (spec v0.3 §1.45, §2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterPackageReport {
    /// Everything wrong, in order; empty when the package may be loaded.
    pub problems: Vec<String>,
    /// The full ids of the adapters the package contributes.
    pub adapters: Vec<String>,
    /// Whether the default-deny policy lets the packs influence structured output: never.
    pub enabled_by_default: bool,
    /// Whether an explicit `process.exec` grant would.
    pub enabled_when_granted: bool,
}

/// Validates a declarative adapter package as the shell would before loading it: manifest,
/// packs against the contract and their fixtures, the executables scope, and the policy.
#[must_use]
pub fn check_adapter_package(directory: &std::path::Path) -> AdapterPackageReport {
    let mut report = AdapterPackageReport {
        problems: Vec::new(),
        adapters: Vec::new(),
        enabled_by_default: false,
        enabled_when_granted: false,
    };
    let manifest = match std::fs::read_to_string(directory.join("manifest.yaml"))
        .map_err(|error| error.to_string())
        .and_then(|text| Manifest::parse(&text).map_err(|error| error.to_string()))
    {
        Ok(manifest) => manifest,
        Err(error) => {
            report.problems.push(format!("manifest.yaml: {error}"));
            return report;
        }
    };
    match ono_kuang_supervisor::validate_package(directory, &manifest) {
        Ok(packs) => {
            for pack in &packs {
                report
                    .adapters
                    .extend(pack.adapters().iter().map(ono_adapter::Adapter::full_id));
            }
            let requested = manifest
                .required_capabilities
                .iter()
                .chain(&manifest.optional_capabilities)
                .any(|request| request.capability == Capability::ProcessExec);
            report.enabled_by_default =
                Policy::deny_all().grants_capability(Capability::ProcessExec);
            report.enabled_when_granted = requested
                && packs
                    .iter()
                    .all(|pack| pack.tier() == ono_adapter::Tier::Community);
            if !requested {
                report.problems.push(
                    "the package requests no process.exec, so nothing could ever run".to_owned(),
                );
            }
        }
        Err(error) => report.problems.push(error.message),
    }
    report
}

/// What the test host found in a package that contributes spatial relations (spec v0.4 §36.1,
/// §35.5, §31.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialPackageReport {
    /// Everything wrong, in order; empty when the package may be loaded.
    pub problems: Vec<String>,
    /// The relations the package would contribute, as the host registers them (ADR-0194).
    pub relations: Vec<String>,
    /// Whether the default-deny policy lets the contribution reach a map: never (§35.5).
    pub enabled_by_default: bool,
    /// Whether an explicit `relation.write` grant would.
    pub enabled_when_granted: bool,
}

/// Validates a package's spatial contributions as the shell would before loading it: the manifest,
/// the `<from>-><to>` shapes of v0.2 §31.7, and the capability that gates them.
///
/// v0.4 §35.5 puts the filter before the merge, so this answers the question a package author has
/// *before* the package ever runs: which relations would this contribute, and under what grant.
/// A shape naming something §3.3 does not place, or a contribution without `relation.write`, is a
/// package that would load and contribute nothing — reported here rather than discovered as an
/// empty map.
#[must_use]
pub fn check_spatial_package(directory: &std::path::Path) -> SpatialPackageReport {
    let mut report = SpatialPackageReport {
        problems: Vec::new(),
        relations: Vec::new(),
        enabled_by_default: Policy::deny_all().grants_capability(Capability::RelationWrite),
        enabled_when_granted: false,
    };
    let manifest = match std::fs::read_to_string(directory.join("manifest.yaml"))
        .map_err(|error| error.to_string())
        .and_then(|text| Manifest::parse(&text).map_err(|error| error.to_string()))
    {
        Ok(manifest) => manifest,
        Err(error) => {
            report.problems.push(format!("manifest.yaml: {error}"));
            return report;
        }
    };
    let shapes: Vec<String> = manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.relations.clone())
        .unwrap_or_default();
    if shapes.is_empty() {
        report
            .problems
            .push("the package declares no `contributions.relations`".to_owned());
        return report;
    }
    let package = manifest.package.id.clone();
    // The schema ids the package's own `contributions.targets` documents declare (§31.68). A
    // shape endpoint may name one of them, and reading them here is what lets this check answer
    // for a relation between two kinds of place the package contributes — without running it,
    // which is the whole point of asking before loading (ADR-0585).
    let schemas = declared_schemas(directory, &manifest);
    for shape in &shapes {
        let Some((from, to)) = ono_spatial_core::relation::parse_shape(shape) else {
            report.problems.push(format!(
                "`{shape}` is not a `<from>-><to>` shape (spec §31.7)"
            ));
            continue;
        };
        let unknown: Vec<&str> = [from, to]
            .into_iter()
            .filter(|endpoint| {
                !ono_spatial_core::relation::shape_endpoint_is_known(endpoint, &schemas)
            })
            .collect();
        if unknown.is_empty() {
            report
                .relations
                .push(ono_spatial_core::relation::contributed_id(
                    &package, from, to,
                ));
        } else {
            report.problems.push(format!(
                "`{shape}` names a kind of place nothing defines: {} is neither a type of v0.4 \
                 section 3.3 nor the id of a schema this package declares a target for",
                unknown.join(", ")
            ));
        }
    }
    let requested = manifest
        .required_capabilities
        .iter()
        .chain(&manifest.optional_capabilities)
        .any(|request| request.capability == Capability::RelationWrite);
    if !requested {
        report.problems.push(
            "the package requests no relation.write, so none of its edges could ever reach a map"
                .to_owned(),
        );
    }
    report.enabled_when_granted = requested && !report.relations.is_empty();
    report
}

/// What the test host found in a package that contributes history or causality (v0.5 §37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalPackageReport {
    /// Everything wrong, in order; empty when the package may be loaded.
    pub problems: Vec<String>,
    /// The ids of the temporal sources the package would contribute (§37.5).
    pub sources: Vec<String>,
    /// The ids of the causal rules the package would contribute (§37.4).
    pub rules: Vec<String>,
    /// The source ids whose answer ends by itself: a historical query provider (§37.5).
    pub bounded_sources: Vec<String>,
    /// The strongest claim any of the package's rules could carry once the host has capped it
    /// (§37.4). `None` when the package contributes no rules.
    pub strength_ceiling: Option<String>,
    /// Whether history reaches the package under the default policy: never (§30.7, §31.19).
    pub history_by_default: bool,
    /// Whether an explicit `temporal.contribute.events` grant would let it contribute.
    pub contributes_when_granted: bool,
}

/// Validates a package's temporal contributions as the shell would before loading it (§37).
///
/// The counterpart of [`check_spatial_package`], and it answers the questions §37 makes a package
/// author responsible for *before* the package runs: which sources would this contribute, whether
/// each one ends by itself, which causal rules it registers, and what the strongest thing it
/// could ever say is.
///
/// §37.5 requires a historical query provider to "map data into canonical Ono objects/events and
/// expose coverage/provenance", so a source that names no canonical schema, produces a kind Ono
/// does not have, or states no coverage is a problem here rather than a surprise later. §37.4
/// requires a third-party rule to be namespaced and to identify its source, and caps what it may
/// claim; a package that declares `authoritative` learns here that it will carry `asserted`.
#[must_use]
pub fn check_temporal_package(directory: &std::path::Path) -> TemporalPackageReport {
    let mut report = TemporalPackageReport {
        problems: Vec::new(),
        sources: Vec::new(),
        rules: Vec::new(),
        bounded_sources: Vec::new(),
        strength_ceiling: None,
        history_by_default: Policy::deny_all().grants_capability(Capability::TemporalReadHistory),
        contributes_when_granted: false,
    };
    let manifest = match std::fs::read_to_string(directory.join("manifest.yaml"))
        .map_err(|error| error.to_string())
        .and_then(|text| Manifest::parse(&text).map_err(|error| error.to_string()))
    {
        Ok(manifest) => manifest,
        Err(error) => {
            report.problems.push(format!("manifest.yaml: {error}"));
            return report;
        }
    };
    let contributions = manifest.contributions.clone().unwrap_or_default();
    let source_paths = contributions.temporal_sources.unwrap_or_default();
    let rule_paths = contributions.causal_rules.unwrap_or_default();
    if source_paths.is_empty() && rule_paths.is_empty() {
        report.problems.push(
            "the package declares neither `contributions.temporal_sources` nor \
             `contributions.causal_rules`, so it contributes nothing about the past"
                .to_owned(),
        );
        return report;
    }
    let package = manifest.package.id.clone();
    let own_schemas = declared_schemas(directory, &manifest);

    for path in &source_paths {
        let text = match std::fs::read_to_string(directory.join(path)) {
            Ok(text) => text,
            Err(error) => {
                report.problems.push(format!("{path}: {error}"));
                continue;
            }
        };
        let document = match ono_kuang_protocol::TemporalSourceDocument::parse(&text) {
            Ok(document) => document,
            Err(error) => {
                report.problems.push(format!("{path}: {}", error.message()));
                continue;
            }
        };
        for source in document.temporal_sources {
            if !source
                .id
                .starts_with(&format!("{package}.temporal-source."))
            {
                report.problems.push(format!(
                    "`{}` is not `<package.id>.temporal-source.<kebab-name>` (spec section 31.5)",
                    source.id
                ));
                continue;
            }
            for kind in &source.kinds {
                if ono_temporal_core::EventKind::from_name(kind).is_none() {
                    report.problems.push(format!(
                        "`{}` produces `{kind}`, which is not one of Ono's event kinds; a \
                         package refines a kind through `subtype` and the top-level kind stays \
                         Ono's (v0.5 section 6.1, section 37.1)",
                        source.id
                    ));
                }
            }
            if source.coverage.trim().is_empty() {
                report.problems.push(format!(
                    "`{}` states no coverage, and v0.5 section 37.5 requires a contributed \
                     source to expose coverage and provenance",
                    source.id
                ));
            }
            if !own_schemas.contains(&source.schema) && !source.schema.starts_with("ono.") {
                report.problems.push(format!(
                    "`{}` maps its data into `{}`, which is neither a core schema nor one this \
                     package declares a target for; v0.5 section 37.5 requires the mapping to \
                     reach canonical Ono objects",
                    source.id, source.schema
                ));
            }
            if source.answer.is_bounded() {
                report.bounded_sources.push(source.id.clone());
            }
            report.sources.push(source.id);
        }
    }

    let mut ceiling: Option<ono_temporal_core::EvidenceStrength> = None;
    for path in &rule_paths {
        let text = match std::fs::read_to_string(directory.join(path)) {
            Ok(text) => text,
            Err(error) => {
                report.problems.push(format!("{path}: {error}"));
                continue;
            }
        };
        let document = match ono_kuang_protocol::CausalRuleDocument::parse(&text) {
            Ok(document) => document,
            Err(error) => {
                report.problems.push(format!("{path}: {}", error.message()));
                continue;
            }
        };
        for rule in document.causal_rules {
            if !rule.rule_id.starts_with(&format!("{package}.")) {
                report.problems.push(format!(
                    "`{}` is not namespaced under `{package}`; v0.5 section 37.4 requires a \
                     third-party causal rule to be namespaced and to identify its source",
                    rule.rule_id
                ));
                continue;
            }
            if ono_temporal_core::CausalRelation::from_name(&rule.relation).is_none() {
                report.problems.push(format!(
                    "`{}` emits `{}`, which is not one of Ono's five relation classes (v0.5 \
                     section 15.1, section 37.1)",
                    rule.rule_id, rule.relation
                ));
                continue;
            }
            let Some(declared) = ono_temporal_core::EvidenceStrength::from_name(&rule.strength)
            else {
                report.problems.push(format!(
                    "`{}` claims strength `{}`, which is not one of the five of v0.5 section 7.2",
                    rule.rule_id, rule.strength
                ));
                continue;
            };
            // §37.4's ceiling, applied through the one operation §7.2 permits. A package that
            // declared something stronger learns here what it will actually carry.
            let effective = declared.weakest_of(ono_kuang_supervisor::CONTRIBUTED_STRENGTH_CEILING);
            if effective != declared {
                report.problems.push(format!(
                    "`{}` declares `{}` and will carry `{}`: v0.5 section 37.4 caps a plugin's \
                     causal strength unless the host contract trusts this package as \
                     authoritative for a domain",
                    rule.rule_id,
                    declared.as_str(),
                    effective.as_str()
                ));
            }
            ceiling = Some(match ceiling {
                None => effective,
                // The strongest of the rules, which is the weakest-of applied in reverse: the
                // report answers "what is the most this package could ever say".
                Some(current) if effective.weakest_of(current) == current => effective,
                Some(current) => current,
            });
            report.rules.push(rule.rule_id);
        }
    }
    report.strength_ceiling = ceiling.map(|strength| strength.as_str().to_owned());

    let requests = |capability: Capability| {
        manifest
            .required_capabilities
            .iter()
            .chain(&manifest.optional_capabilities)
            .any(|request| request.capability == capability)
    };
    if !report.sources.is_empty() && !requests(Capability::TemporalContributeEvents) {
        report.problems.push(
            "the package requests no temporal.contribute.events, so none of its events could \
             ever reach the ledger"
                .to_owned(),
        );
    }
    if !report.rules.is_empty() && !requests(Capability::TemporalContributeCausality) {
        report.problems.push(
            "the package requests no temporal.contribute.causality, so none of its links could \
             ever reach an explanation"
                .to_owned(),
        );
    }
    report.contributes_when_granted =
        report.problems.is_empty() && (!report.sources.is_empty() || !report.rules.is_empty());
    report
}

/// The schema ids the targets a package declares on disk answer with (spec §31.23, §31.68).
fn declared_schemas(directory: &std::path::Path, manifest: &Manifest) -> Vec<String> {
    let paths = manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.targets.clone())
        .unwrap_or_default();
    let mut schemas: Vec<String> = paths
        .iter()
        .filter_map(|path| std::fs::read_to_string(directory.join(path)).ok())
        .filter_map(|text| ono_kuang_protocol::TargetDocument::parse(&text).ok())
        .flat_map(|document| document.targets)
        .map(|target| target.schema)
        .collect();
    schemas.sort();
    schemas.dedup();
    schemas
}

/// A view host that takes every view and records every tree (spec §31.73): what a
/// conformance test looks at instead of a terminal, and where it injects the events.
#[derive(Debug, Default)]
pub struct RecordingViews {
    inner: std::sync::Arc<std::sync::Mutex<RecordedViews>>,
    /// When set, nothing takes a view: the redirected-output case of spec §31.28.
    redirected: bool,
}

#[derive(Debug, Default)]
struct RecordedViews {
    opened: Vec<String>,
    trees: Vec<Json>,
    closed: usize,
    events: Vec<tokio::sync::mpsc::Sender<ono_kuang_protocol::ViewEvent>>,
}

impl RecordingViews {
    /// A host with a terminal of 24 rows and 80 columns.
    #[must_use]
    pub fn terminal() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::default())
    }

    /// A host whose output is redirected: `views.open` answers `mounted: false`.
    #[must_use]
    pub fn redirected() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: std::sync::Arc::default(),
            redirected: true,
        })
    }

    /// The ids of the views opened so far.
    #[must_use]
    pub fn opened(&self) -> Vec<String> {
        lock(&self.inner).opened.clone()
    }

    /// Every tree submitted so far, in order.
    #[must_use]
    pub fn trees(&self) -> Vec<Json> {
        lock(&self.inner).trees.clone()
    }

    /// How many views were closed.
    #[must_use]
    pub fn closed(&self) -> usize {
        lock(&self.inner).closed
    }

    /// Sends a key to the view opened last; nothing when no view is open.
    pub fn press(&self, key: &str) {
        self.send(ono_kuang_protocol::ViewEvent {
            kind: "key".to_owned(),
            key: Some(key.to_owned()),
            size: None,
        });
    }

    /// Sends an event of `kind` — `cancel`, `focus`, `blur` — to the view opened last.
    pub fn signal(&self, kind: &str) {
        self.send(ono_kuang_protocol::ViewEvent {
            kind: kind.to_owned(),
            key: None,
            size: None,
        });
    }

    /// Resizes the view opened last.
    pub fn resize(&self, rows: u16, columns: u16) {
        self.send(ono_kuang_protocol::ViewEvent {
            kind: "resize".to_owned(),
            key: None,
            size: Some(ono_kuang_protocol::ViewSize { rows, columns }),
        });
    }

    fn send(&self, event: ono_kuang_protocol::ViewEvent) {
        // Nothing to send to when no view is open: the test asserts on what it recorded.
        if let Some(sender) = lock(&self.inner).events.last().cloned() {
            let _ = sender.try_send(event);
        }
    }
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct RecordedView {
    inner: std::sync::Arc<std::sync::Mutex<RecordedViews>>,
}

impl ono_kuang_supervisor::MountedView for RecordedView {
    fn size(&self) -> ono_kuang_protocol::ViewSize {
        ono_kuang_protocol::ViewSize {
            rows: 24,
            columns: 80,
        }
    }

    fn submit(&self, tree: &Json) -> Result<(), String> {
        lock(&self.inner).trees.push(tree.clone());
        Ok(())
    }

    fn close(&self) {
        lock(&self.inner).closed += 1;
    }
}

impl ono_kuang_supervisor::ViewHost for RecordingViews {
    fn open(
        &self,
        _package: &str,
        view: &ono_kuang_protocol::ViewContribution,
        events: tokio::sync::mpsc::Sender<ono_kuang_protocol::ViewEvent>,
    ) -> Result<Option<Box<dyn ono_kuang_supervisor::MountedView>>, String> {
        let mut inner = lock(&self.inner);
        inner.opened.push(view.id.clone());
        if self.redirected {
            return Ok(None);
        }
        inner.events.push(events);
        Ok(Some(Box::new(RecordedView {
            inner: std::sync::Arc::clone(&self.inner),
        })))
    }
}

/// What the test host found in a package that contributes change or recovery (v0.6 §48).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePackageReport {
    /// Everything wrong, in order; empty when the package may be loaded.
    pub problems: Vec<String>,
    /// The ids of the recovery providers the package would contribute (§48.2, §12.1).
    pub recovery_providers: Vec<String>,
    /// The ids of the impact providers (§48.2, §9.4).
    pub impact_providers: Vec<String>,
    /// The ids of the verification providers (§48.2, §25.1).
    pub verification_providers: Vec<String>,
    /// The ids of the risk rules (§48.2, §19.2).
    pub risk_rules: Vec<String>,
    /// The ids of the plan views (§48.2, §45).
    pub change_views: Vec<String>,
    /// Whether any contributed provider can put state back (§12.2, §62.1).
    ///
    /// The question §62.1 makes the important one: a package whose candidates nobody can use is
    /// snapshot theatre with a discovery step, and a publisher should learn that here.
    pub can_restore: bool,
    /// The strongest consistency any contributed provider may claim once §39.2 has been applied
    /// (§11.3). `None` when the package contributes no recovery provider.
    pub consistency_ceiling: Option<String>,
    /// Whether any contributed provider may execute a mutating plan action (§48.3, §48.4).
    ///
    /// False for a package that describes impact and contributes to plans, which is exactly the
    /// separation §48.4 exists to make visible.
    pub may_execute: bool,
    /// Whether recovery restoration reaches the package under the default policy: never
    /// (§31.19, Appendix H).
    pub restore_by_default: bool,
}

/// Validates a package's change and recovery contributions as the shell would before loading it.
///
/// The counterpart of [`check_temporal_package`] for v0.6 §48, and it answers what §48 makes a
/// package author responsible for **before** the package runs: which providers this would
/// contribute, whether any of them can actually restore, what the strongest consistency claim it
/// could ever make is, and whether it may execute anything at all.
///
/// The three refusals §48.4 turns on are checked here exactly as the supervisor checks them at
/// load, because a publisher should meet them before a user does: a provider that offers to
/// restore while holding no destructive authority, a provider claiming `application-consistent`
/// with no way to quiesce (§39.2), and a transaction reaching past the resources the provider
/// itself covers (§27.3).
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one report over five contribution types reads better whole than split by type"
)]
pub fn check_change_package(directory: &std::path::Path) -> ChangePackageReport {
    let mut report = ChangePackageReport {
        problems: Vec::new(),
        recovery_providers: Vec::new(),
        impact_providers: Vec::new(),
        verification_providers: Vec::new(),
        risk_rules: Vec::new(),
        change_views: Vec::new(),
        can_restore: false,
        consistency_ceiling: None,
        may_execute: false,
        restore_by_default: Policy::deny_all().grants_capability(Capability::RecoveryRestore),
    };
    let manifest = match std::fs::read_to_string(directory.join("manifest.yaml"))
        .map_err(|error| error.to_string())
        .and_then(|text| Manifest::parse(&text).map_err(|error| error.to_string()))
    {
        Ok(manifest) => manifest,
        Err(error) => {
            report.problems.push(format!("manifest.yaml: {error}"));
            return report;
        }
    };
    let contributions = manifest.contributions.clone().unwrap_or_default();
    let recovery_paths = contributions.recovery_providers.unwrap_or_default();
    let impact_paths = contributions.impact_providers.unwrap_or_default();
    let verification_paths = contributions.verification_providers.unwrap_or_default();
    let rule_paths = contributions.risk_rules.unwrap_or_default();
    let view_paths = contributions.change_views.unwrap_or_default();
    if recovery_paths.is_empty()
        && impact_paths.is_empty()
        && verification_paths.is_empty()
        && rule_paths.is_empty()
        && view_paths.is_empty()
    {
        report.problems.push(
            "the package declares none of `contributions.recovery_providers`, \
             `contributions.impact_providers`, `contributions.verification_providers`, \
             `contributions.risk_rules` or `contributions.change_views`, so it contributes \
             nothing to a change plan"
                .to_owned(),
        );
        return report;
    }
    let package = manifest.package.id.clone();
    let own_schemas = declared_schemas(directory, &manifest);

    let mut ceiling: Option<ono_change_core::ConsistencyClass> = None;
    for path in &recovery_paths {
        let document = match read_document(directory, path, &mut report.problems, |text| {
            ono_kuang_protocol::RecoveryProviderDocument::parse(text)
        }) {
            Some(document) => document,
            None => continue,
        };
        for provider in document.recovery_providers {
            let expected = format!("{package}.recovery-provider.");
            if !provider.id.starts_with(&expected) {
                report.problems.push(format!(
                    "`{}` is not `<package.id>.recovery-provider.<kebab-name>` (spec section \
                     31.5)",
                    provider.id
                ));
                continue;
            }
            if provider.domain_kinds.is_empty() {
                report.problems.push(format!(
                    "`{}` covers no persistence domain kind, so nothing it discovers could be \
                     mapped to a path (v0.6 section 11.2)",
                    provider.id
                ));
            }
            // The shell's own definition, so a package passing here is one the supervisor loads:
            // the asset type is from the registry, and §16.2's memory inclusion is stated for a
            // VM snapshot and for nothing else.
            if let Err(error) = ono_kuang_supervisor::validate_recovery_asset(&provider) {
                report.problems.push(error.message().to_owned());
            }
            let consistency = ono_change_core::ConsistencyClass::from_name(&provider.consistency);
            if consistency.is_none() {
                report.problems.push(format!(
                    "`{}` claims the consistency class `{}`, which is not one of the six of v0.6 \
                     section 11.3",
                    provider.id, provider.consistency
                ));
            }
            for method in &provider.restore_methods {
                if ono_change_core::RestoreMethod::from_name(method).is_none() {
                    report.problems.push(format!(
                        "`{}` offers the restore method `{method}`, which is not one of Appendix \
                         C.1's",
                        provider.id
                    ));
                }
            }
            let mut declared = Vec::new();
            for id in &provider.capabilities {
                match Capability::from_id(id) {
                    Some(capability)
                        if ono_change_core::RecoveryCapability::from_name(id).is_some() =>
                    {
                        declared.push(capability);
                    }
                    _ => report.problems.push(format!(
                        "`{}` declares `{id}`, which is not one of the seven recovery \
                         capabilities of v0.6 section 12.2",
                        provider.id
                    )),
                }
            }
            let offers_restore = !provider.restore_methods.is_empty()
                || provider
                    .capabilities
                    .iter()
                    .any(|id| id == ono_change_core::RecoveryCapability::Restore.as_str());
            if offers_restore {
                if declared
                    .iter()
                    .any(|capability| capability.risk() == ono_kuang_protocol::Risk::Destructive)
                {
                    report.can_restore = true;
                } else {
                    report.problems.push(format!(
                        "`{}` offers to restore and declares no capability of destructive risk; \
                         v0.6 section 48.4 refuses that at load, and section 43.4 is why \
                         restoring may need a stronger privilege than the mutation it undoes",
                        provider.id
                    ));
                }
            }
            let can_quiesce = provider
                .capabilities
                .iter()
                .any(|id| id == ono_change_core::RecoveryCapability::Quiesce.as_str());
            if consistency == Some(ono_change_core::ConsistencyClass::ApplicationConsistent)
                && !can_quiesce
            {
                report.problems.push(format!(
                    "`{}` claims `application-consistent` and declares no `recovery.quiesce`; \
                     v0.6 section 39.2 and section 16.4 put the claim with the provider that can \
                     quiesce the application, and `crash-consistent` is the claim this one can \
                     own",
                    provider.id
                ));
            }
            if let Some(transaction) = &provider.transaction {
                if !provider
                    .capabilities
                    .iter()
                    .any(|id| id == ono_change_core::RecoveryCapability::Transaction.as_str())
                {
                    report.problems.push(format!(
                        "`{}` states an atomicity guarantee and declares no \
                         `recovery.transaction` (v0.6 section 12.2, section 27.1)",
                        provider.id
                    ));
                }
                for resource in &transaction.resources {
                    if !provider.domain_kinds.iter().any(|kind| kind == resource) {
                        report.problems.push(format!(
                            "`{}` states atomicity over `{resource}`, which is not one of the \
                             domain kinds it covers; v0.6 section 27.1 scopes a provider \
                             transaction to its own resources and section 27.3 makes generic \
                             two-phase commit a non-goal",
                            provider.id
                        ));
                    }
                }
            }
            // The strongest claim the package could make, after section 39.2 has been applied: a
            // claim it cannot own is not part of the answer.
            if let Some(class) = consistency
                && (class != ono_change_core::ConsistencyClass::ApplicationConsistent
                    || can_quiesce)
            {
                ceiling = Some(match ceiling {
                    None => class,
                    Some(current) if class.weakest_of(current) == current => class,
                    Some(current) => current,
                });
            }
            report.recovery_providers.push(provider.id);
        }
    }
    report.consistency_ceiling = ceiling.map(|class| class.as_str().to_owned());

    for path in &impact_paths {
        let document = match read_document(directory, path, &mut report.problems, |text| {
            ono_kuang_protocol::ImpactProviderDocument::parse(text)
        }) {
            Some(document) => document,
            None => continue,
        };
        for provider in document.impact_providers {
            if !provider
                .id
                .starts_with(&format!("{package}.impact-provider."))
            {
                report.problems.push(format!(
                    "`{}` is not `<package.id>.impact-provider.<kebab-name>` (spec section 31.5)",
                    provider.id
                ));
                continue;
            }
            for schema in &provider.object_types {
                if !own_schemas.contains(schema) && !schema.starts_with("ono.") {
                    report.problems.push(format!(
                        "`{}` relates `{schema}`, which is neither a core schema nor one this \
                         package declares a target for",
                        provider.id
                    ));
                }
            }
            if let Some(ceiling) = &provider.confidence_ceiling
                && ono_change_core::EffectConfidence::from_name(ceiling).is_none()
            {
                report.problems.push(format!(
                    "`{}` declares the confidence ceiling `{ceiling}`, which is not one of the \
                     four of v0.6 section 8.1",
                    provider.id
                ));
            }
            report.impact_providers.push(provider.id);
        }
    }

    for path in &verification_paths {
        let document = match read_document(directory, path, &mut report.problems, |text| {
            ono_kuang_protocol::VerificationProviderDocument::parse(text)
        }) {
            Some(document) => document,
            None => continue,
        };
        for provider in document.verification_providers {
            if !provider
                .id
                .starts_with(&format!("{package}.verification-provider."))
            {
                report.problems.push(format!(
                    "`{}` is not `<package.id>.verification-provider.<kebab-name>` (spec section \
                     31.5)",
                    provider.id
                ));
                continue;
            }
            for check in &provider.checks {
                if ono_change_core::EquivalenceDomain::from_name(&check.equivalence).is_none() {
                    report.problems.push(format!(
                        "`{}` says its `{}` check is evidence about `{}`, which is not one of the \
                         three equivalence domains of v0.6 section 25.1; section 25.3 forbids a \
                         claim of recovery success without one of them",
                        provider.id, check.kind, check.equivalence
                    ));
                }
            }
            report.verification_providers.push(provider.id);
        }
    }

    for path in &rule_paths {
        let document = match read_document(directory, path, &mut report.problems, |text| {
            ono_kuang_protocol::RiskRuleDocument::parse(text)
        }) {
            Some(document) => document,
            None => continue,
        };
        for rule in document.risk_rules {
            if !rule.rule_id.starts_with(&format!("{package}.")) {
                report.problems.push(format!(
                    "`{}` is not namespaced under `{package}`; a contributed rule is a rule, and \
                     v0.6 section 19.2 makes it inspectable exactly as a built-in one is",
                    rule.rule_id
                ));
                continue;
            }
            if ono_change_core::RiskDimension::from_name(&rule.dimension).is_none() {
                report.problems.push(format!(
                    "`{}` emits into `{}`, which is not one of the ten risk dimensions of v0.6 \
                     section 19.1",
                    rule.rule_id, rule.dimension
                ));
            }
            if ono_change_core::RiskClass::from_name(&rule.emits).is_none() {
                report.problems.push(format!(
                    "`{}` declares that it emits `{}`, which is not one of the five risk classes \
                     of v0.6 section 19.2",
                    rule.rule_id, rule.emits
                ));
            }
            report.risk_rules.push(rule.rule_id);
        }
    }

    for path in &view_paths {
        let document = match read_document(directory, path, &mut report.problems, |text| {
            ono_kuang_protocol::ChangeViewDocument::parse(text)
        }) {
            Some(document) => document,
            None => continue,
        };
        for view in document.change_views {
            if !view.id.starts_with(&format!("{package}.change-view.")) {
                report.problems.push(format!(
                    "`{}` is not `<package.id>.change-view.<kebab-name>` (spec section 31.5)",
                    view.id
                ));
                continue;
            }
            for state in &view.plan_states {
                if ono_change_core::PlanState::from_name(state).is_none() {
                    report.problems.push(format!(
                        "`{}` renders the plan state `{state}`, which is not one a plan can be \
                         in (v0.6 section 4.1)",
                        view.id
                    ));
                }
            }
            report.change_views.push(view.id);
        }
    }

    let requests = |capability: Capability| {
        manifest
            .required_capabilities
            .iter()
            .chain(&manifest.optional_capabilities)
            .chain(&manifest.runtime_requested_capabilities)
            .any(|request| request.capability == capability)
    };
    report.may_execute = requests(Capability::ChangeActionExecute);
    if report.can_restore && !requests(Capability::RecoveryRestore) {
        report.problems.push(
            "the package requests no recovery.restore, so nothing it protected could ever be \
             put back (v0.6 section 48.4)"
                .to_owned(),
        );
        report.can_restore = false;
    }
    if !report.recovery_providers.is_empty() && !requests(Capability::RecoveryDiscover) {
        report.problems.push(
            "the package requests no recovery.discover, so none of its candidates could ever \
             reach a plan"
                .to_owned(),
        );
    }
    if (!report.risk_rules.is_empty() || !report.impact_providers.is_empty())
        && !requests(Capability::ChangePlanContribute)
    {
        report.problems.push(
            "the package requests no change.plan.contribute, so none of its rules or edges \
             could ever reach a plan"
                .to_owned(),
        );
    }
    report
}

/// Reads one on-disk contribution document, recording a read or parse failure as a problem.
fn read_document<T>(
    directory: &std::path::Path,
    path: &str,
    problems: &mut Vec<String>,
    parse: impl Fn(&str) -> Result<T, KuangError>,
) -> Option<T> {
    let text = match std::fs::read_to_string(directory.join(path)) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!("{path}: {error}"));
            return None;
        }
    };
    match parse(&text) {
        Ok(document) => Some(document),
        Err(error) => {
            problems.push(format!("{path}: {}", error.message()));
            None
        }
    }
}
