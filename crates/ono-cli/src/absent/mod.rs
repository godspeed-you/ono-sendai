//! What a build leaves out, and how it says so (#127, ADR-0910, ADR-0911).
//!
//! `ono-cli` builds as `full`, the default and the whole product, or as `core`: the object shell
//! alone, for container images and embedded Linux targets. The core build compiles nine tiers
//! out, and each of them still has an answer, because a name the shell knows must never fall
//! through to a parse error, a panic or a search of `PATH`:
//!
//! - a **command** of a compiled-out tier — `map`, `trace process`, `link host`, `load plugin`,
//!   `at -1h`, `plan …`, `adapt ls` — refuses with `resolve.not_in_build`, naming the tier, before
//!   anything else resolves the stage. [`claims`] is that decision, and the command registry the
//!   shell advertises through `help`, completion and `get command` is [`registry`], which carries
//!   only what this build can run;
//! - a **target** whose provider is compiled out — `service`, `session`, `journal`, `log`,
//!   `container`, `image` — keeps its commands, and the provider that would answer it is replaced
//!   by an [`AbsentProvider`] that is unavailable. `get service` then refuses exactly as a full
//!   build does on a host without systemd: `provider.unavailable`, through the registry's own
//!   path, with the reason naming the build rather than the host.
//!
//! The modules of the compiled-out tiers that the rest of the shell calls into are replaced by the
//! small inert modules beside this file (`absent/spatial.rs`, `absent/temporal.rs`, …), mounted
//! at the same paths from `lib.rs`. They hold what the core path needs from a tier that is not
//! there — "no place is active", "no plan claims this stage" — and nothing else.

use std::sync::{Arc, OnceLock};

use ono_command::{CommandContract, CommandRegistry, Elevation};
use ono_core::ErrorCode;
use ono_parser::{Stage, StageHead};
use ono_pipeline::ValueStream;
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Selector};
use ono_value::{ErrorValue, Schema, Value};

/// A tier of the product that a build can leave out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The external command adapters of spec v0.3.
    Adapter,
    /// Prospective change, protection and recovery, spec v0.6.
    Change,
    /// The container engine provider (spec §9.1).
    Container,
    /// The relationship graph and `trace` of spec §22.
    Graph,
    /// The KUANG/11 plugin runtime of spec §31.
    Kuang,
    /// Remote links and the agent of spec §21.
    Remote,
    /// The spatial systems interface of spec v0.4.
    Spatial,
    /// The systemd, logind and journal providers.
    Systemd,
    /// The temporal and causal systems interface of spec v0.5.
    Temporal,
}

impl Tier {
    /// Every tier, in the order `ono --version` lists them.
    pub const ALL: [Self; 9] = [
        Self::Adapter,
        Self::Change,
        Self::Container,
        Self::Graph,
        Self::Kuang,
        Self::Remote,
        Self::Spatial,
        Self::Systemd,
        Self::Temporal,
    ];

    /// The name of the cargo feature that carries the tier, and the word a user reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Adapter => "adapter",
            Self::Change => "change",
            Self::Container => "container",
            Self::Graph => "graph",
            Self::Kuang => "kuang",
            Self::Remote => "remote",
            Self::Spatial => "spatial",
            Self::Systemd => "systemd",
            Self::Temporal => "temporal",
        }
    }

    /// What the tier is, in the words of an error message.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Adapter => "the external command adapters (spec v0.3)",
            Self::Change => "prospective change, protection and recovery (spec v0.6)",
            Self::Container => "the container engine provider",
            Self::Graph => "the relationship graph and `trace` (spec §22)",
            Self::Kuang => "the KUANG/11 plugin runtime (spec §31)",
            Self::Remote => "remote links and the agent (spec §21)",
            Self::Spatial => "the spatial systems interface (spec v0.4)",
            Self::Systemd => "the systemd, logind and journal providers",
            Self::Temporal => "the temporal and causal systems interface (spec v0.5)",
        }
    }

    /// Whether this binary was compiled with the tier.
    #[must_use]
    pub const fn is_built(self) -> bool {
        match self {
            Self::Adapter => cfg!(feature = "adapter"),
            Self::Change => cfg!(feature = "change"),
            Self::Container => cfg!(feature = "container"),
            Self::Graph => cfg!(feature = "graph"),
            Self::Kuang => cfg!(feature = "kuang"),
            Self::Remote => cfg!(feature = "remote"),
            Self::Spatial => cfg!(feature = "spatial"),
            Self::Systemd => cfg!(feature = "systemd"),
            Self::Temporal => cfg!(feature = "temporal"),
        }
    }
}

/// Whether this binary carries every tier. `lib.rs` refuses any build in between, so this is
/// the whole of the profile question.
#[must_use]
pub const fn is_full() -> bool {
    Tier::Adapter.is_built()
}

/// The build profile, as `ono --version` names it: `full` or `core`.
#[must_use]
pub const fn profile() -> &'static str {
    if is_full() { "full" } else { "core" }
}

/// The tiers this binary was compiled without.
#[must_use]
pub fn missing() -> Vec<Tier> {
    Tier::ALL
        .into_iter()
        .filter(|tier| !tier.is_built())
        .collect()
}

/// What `ono --version` prints.
///
/// The full build prints the one line it always has. The core build adds a second line naming
/// the profile and the tiers it leaves out, so a user looking at a refusal can find out why from
/// the binary itself, and a script that reads the first line reads what it always read.
#[must_use]
pub fn version_text() -> String {
    let first = format!("{} {}", ono_core::SHORT_NAME, ono_core::VERSION);
    if is_full() {
        return first;
    }
    let names: Vec<&str> = missing().into_iter().map(Tier::name).collect();
    format!(
        "{first}\nbuild: {} (without {})",
        profile(),
        names.join(", ")
    )
}

/// `resolve.not_in_build` for `what`, which belongs to `tier` (ADR-0911).
#[must_use]
pub fn not_in_build(what: &str, tier: Tier) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ResolveNotInBuild,
        format!(
            "`{what}` belongs to {}, which this {} build of {} does not include",
            tier.description(),
            profile(),
            ono_core::SHORT_NAME
        ),
    )
    .with_metadata("tier", Value::string(tier.name()))
    .with_metadata("profile", Value::string(profile()))
    .with_help(format!(
        "`{name} --version` names this build and the tiers it leaves out; the full build of \
         {name} runs `{what}`. A program of the same name on PATH is reached with `exec:<name>`",
        name = ono_core::SHORT_NAME
    ))
}

/// The tier a command contract belongs to, when it is one a build can leave out.
///
/// The families of `docs/contracts/commands/` are the tiers, with one exception: `trace` is the
/// graph's verb and appears in the families of the targets it walks. The `service`, `container`
/// and `storage` families are not here — their commands stay in every build, and a provider that
/// is not there answers them as unavailable.
#[must_use]
pub fn tier_of(contract: &CommandContract) -> Option<Tier> {
    if contract.verb() == "trace" {
        return Some(Tier::Graph);
    }
    match contract.family() {
        "spatial" => Some(Tier::Spatial),
        "temporal" => Some(Tier::Temporal),
        "change" => Some(Tier::Change),
        "kuang" => Some(Tier::Kuang),
        "remote" => Some(Tier::Remote),
        _ => None,
    }
}

/// The tier a configuration key belongs to, when it is one a build can leave out.
///
/// A setting nothing in this binary reads is not offered by `get config` and is refused by `set
/// config`, for the same reason a compiled-out command is: a key that is accepted and then does
/// nothing is a promise the build cannot keep.
#[must_use]
pub fn tier_of_setting(key: &str) -> Option<Tier> {
    let prefixed = |prefix: &str| key.starts_with(prefix);
    if prefixed("spatial.") || prefixed("limits.orientation_") {
        Some(Tier::Spatial)
    } else if prefixed("temporal.") {
        Some(Tier::Temporal)
    } else if prefixed("change.") || prefixed("recovery.") {
        Some(Tier::Change)
    } else if prefixed("limits.remote_") || prefixed("safety.confirm.remote_") {
        Some(Tier::Remote)
    } else {
        None
    }
}

/// Whether this build carries the configuration key `key`.
#[must_use]
pub fn carries_setting(key: &str) -> bool {
    tier_of_setting(key).is_none_or(Tier::is_built)
}

/// The command registry this build advertises and runs: the embedded contracts, without the
/// commands of any tier this build leaves out.
///
/// # Errors
///
/// The structured error of an embedded contract that cannot be read, which is a build defect.
pub fn registry() -> Result<&'static CommandRegistry, ErrorValue> {
    static NARROWED: OnceLock<CommandRegistry> = OnceLock::new();
    let embedded = CommandRegistry::embedded()?;
    if is_full() {
        return Ok(embedded);
    }
    Ok(NARROWED.get_or_init(|| {
        embedded.retaining(|contract| tier_of(contract).is_none_or(Tier::is_built))
    }))
}

/// The refusal for `stage`, when what it names belongs to a tier this build leaves out.
///
/// Asked after functions and aliases, which are the user's and win over every name (ADR-0011),
/// and before everything else that claims a stage — the shell's own commands, the registry, and
/// `PATH`. A compiled-out name is Ono's vocabulary, so it is never quietly handed to a program
/// that happens to share it; `exec:<name>` still reaches that program.
#[must_use]
pub fn claims(stage: &Stage) -> Option<ErrorValue> {
    if is_full() {
        return None;
    }
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    match name.namespace.as_deref() {
        None | Some("ono") => {}
        // `exec:` and `fn:` are the shell's own namespaces (ADR-0011): a program and a user
        // function are never a compiled-out tier's (ADR-0911).
        Some("exec" | "fn") => return None,
        // A package's namespace: only KUANG/11 puts commands there (spec §31.5).
        Some(namespace) => {
            return absent(Tier::Kuang)
                .map(|tier| not_in_build(&format!("{namespace}:{}", name.name), tier));
        }
    }
    if name.name == ono_adapter::ADAPT {
        return absent(Tier::Adapter).map(|tier| not_in_build(ono_adapter::ADAPT, tier));
    }
    let embedded = CommandRegistry::embedded().ok()?;
    let resolved = embedded.resolve(&name.name, &stage.arguments).ok()?;
    let tier = absent(tier_of(resolved.contract)?)?;
    Some(not_in_build(&resolved.contract.spelling(), tier))
}

/// The refusal for a `help` or `explain` topic that names a command this build leaves out.
#[must_use]
pub fn topic(words: &[String]) -> Option<ErrorValue> {
    if is_full() {
        return None;
    }
    let (head, rest) = words.split_first()?;
    let embedded = CommandRegistry::embedded().ok()?;
    let contract = match rest.first() {
        Some(target) => embedded
            .find(head, Some(target))
            .or_else(|| embedded.find(head, None)),
        None => embedded.find(head, None).or_else(|| {
            // A verb every command of which is compiled out: `help map`, `help trace`.
            let commands = embedded.by_verb(head);
            let first = *commands.first()?;
            let tier = tier_of(first)?;
            commands
                .iter()
                .all(|command| tier_of(command) == Some(tier))
                .then_some(first)
        }),
    }?;
    let tier = absent(tier_of(contract)?)?;
    let what = if rest.is_empty() && embedded.find(head, None).is_none() {
        head.clone()
    } else {
        contract.spelling()
    };
    Some(not_in_build(&what, tier))
}

/// `tier`, when this build leaves it out.
fn absent(tier: Tier) -> Option<Tier> {
    (!tier.is_built()).then_some(tier)
}

/// A provider this build does not carry, standing where it would be registered.
///
/// It claims the targets the real provider claims and advertises the capabilities the command
/// contracts declare for them, so every command of those targets resolves and binds exactly as it
/// does in the full build — and then meets the registry's own `provider.unavailable`, which is
/// the answer a full build gives when the system behind the provider is absent (#127).
#[derive(Debug)]
pub struct AbsentProvider {
    id: &'static str,
    targets: &'static [&'static str],
    tier: Tier,
}

impl AbsentProvider {
    /// The providers of the tiers this build leaves out, one per provider the full build would
    /// register, under the full build's ids.
    #[must_use]
    pub fn missing() -> Vec<Self> {
        let all = [
            Self {
                id: "systemd",
                targets: &["service"],
                tier: Tier::Systemd,
            },
            Self {
                id: "systemd-logind",
                targets: &["session"],
                tier: Tier::Systemd,
            },
            Self {
                id: "systemd-journal",
                targets: &["journal", "log"],
                tier: Tier::Systemd,
            },
            Self {
                id: "container-engine",
                targets: &["container", "image"],
                tier: Tier::Container,
            },
        ];
        all.into_iter()
            .filter(|provider| !provider.tier.is_built())
            .collect()
    }

    /// Why this provider cannot answer, as the registry reports it after the provider's id.
    #[must_use]
    pub fn reason(&self) -> String {
        format!(
            "this {} build of {} does not include {}",
            profile(),
            ono_core::SHORT_NAME,
            self.tier.description()
        )
    }

    fn refusal(&self) -> ErrorValue {
        let target = self.targets.first().copied().unwrap_or_default();
        ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`{target}` cannot be answered here — {}: {}",
                self.id,
                self.reason()
            ),
        )
        .with_help(
            "the provider exists but the system it reads is not present. This is not the same as \
             there being none of the thing you asked for.",
        )
    }
}

#[async_trait::async_trait]
impl Provider for AbsentProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn targets(&self) -> &[&str] {
        self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Vec::new()
    }

    fn capabilities(&self) -> Vec<Capability> {
        let Ok(registry) = CommandRegistry::embedded() else {
            return Vec::new();
        };
        let mut capabilities: Vec<Capability> = Vec::new();
        for command in registry.commands() {
            let (Some(target), Some(id)) = (command.target(), command.provider_capability()) else {
                continue;
            };
            if !self.targets.contains(&target) || capabilities.iter().any(|known| known.id() == id)
            {
                continue;
            }
            let Some(spec) = registry.capability(id) else {
                continue;
            };
            let capability = Capability::new(id, spec.risk());
            capabilities.push(if spec.elevation() == Elevation::Required {
                capability.needing_elevation()
            } else {
                capability
            });
        }
        capabilities
    }

    fn availability(&self) -> Availability {
        Availability::unavailable(self.reason())
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        Err(self.refusal())
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Err(self.refusal())
    }
}
