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
        }
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
    for shape in &shapes {
        let Some((from, to)) = shape.split_once("->") else {
            report.problems.push(format!(
                "`{shape}` is not a `<from>-><to>` shape (spec §31.7)"
            ));
            continue;
        };
        match (spatial_kind(from), spatial_kind(to)) {
            (Some(source), Some(target)) => report.relations.push(format!(
                "{package}.{}_to_{}",
                source.to_ascii_lowercase(),
                target.to_ascii_lowercase()
            )),
            _ => report.problems.push(format!(
                "`{shape}` names a kind of place v0.4 section 3.3 does not define"
            )),
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

/// The §3.3 kind a shape names, however it spelled the case.
fn spatial_kind(name: &str) -> Option<&'static str> {
    ono_spatial_core::SpatialType::ALL
        .iter()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(name.trim()))
        .map(|kind| kind.as_str())
}
