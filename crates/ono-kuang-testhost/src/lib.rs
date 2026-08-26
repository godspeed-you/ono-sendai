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
use ono_kuang_supervisor::{HostClock, HostLimits, LoadConfig, LoadedPlugin, Policy, Supervisor};
use serde_json::{Map as JsonMap, Value as Json};

/// The fixed instant every test-host clock reads (spec §31.73's virtual time).
pub const VIRTUAL_NOW: &str = "2026-08-26T12:00:00Z";

/// A deterministic host for one plugin binary.
#[derive(Debug)]
pub struct TestHost {
    program: PathBuf,
    args: Vec<String>,
    manifest: String,
    policy: Policy,
    limits: HostLimits,
    platform: Option<String>,
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
        }
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
        if let Some(platform) = self.platform {
            config.platform = platform;
        }
        Supervisor::load(config).await
    }
}
