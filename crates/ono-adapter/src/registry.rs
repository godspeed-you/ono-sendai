//! The adapter registry: which adapters exist, and what one of them answers for an invocation
//! (spec v0.3 §1.6, §1.14–§1.16, §1.24, §1.25, §1.46).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use crate::contract::{Adapter, AdapterPack, DemandKind, Fallback, Positionals, StdinMode, Tier};
use crate::demand::OutputDemand;
use crate::version::{Version, VersionRange};

/// Runs a version probe: the executable, the probe's arguments, and back come the bytes it
/// wrote (stdout and stderr together, as text), or nothing when it could not be run.
///
/// Injected so the registry itself never spawns — the shell hands in its process subsystem,
/// tests hand in a closure — and so a probe is exactly as bounded as its caller makes it.
pub type Prober = Box<dyn Fn(&Path, &[String]) -> Option<String> + Send + Sync>;

/// The identity of an executable file: what the probe cache is keyed by (spec v0.3 §1.46).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    path: PathBuf,
    device: u64,
    inode: u64,
    modified: Option<std::time::SystemTime>,
    size: u64,
}

impl Identity {
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
            modified: metadata.modified().ok(),
            size: metadata.len(),
        })
    }
}

/// An adapter's compiled answer for one invocation: what to run and how to read it
/// (spec v0.3 §1.7).
#[derive(Clone)]
pub struct AdapterPlan {
    pack: Arc<AdapterPack>,
    adapter: usize,
    invocation: usize,
    executable: PathBuf,
    version: Option<Version>,
    argv: Vec<String>,
    user_invocation: Vec<String>,
}

impl fmt::Debug for AdapterPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdapterPlan")
            .field("adapter", &self.adapter().full_id())
            .field("executable", &self.executable)
            .field("argv", &self.argv)
            .finish_non_exhaustive()
    }
}

impl PartialEq for AdapterPlan {
    fn eq(&self, other: &Self) -> bool {
        self.adapter().full_id() == other.adapter().full_id()
            && self.executable == other.executable
            && self.argv == other.argv
    }
}

impl AdapterPlan {
    /// The adapter the plan came from.
    #[must_use]
    pub fn adapter(&self) -> &Adapter {
        &self.pack.adapters()[self.adapter]
    }

    /// The invocation contract that matched.
    #[must_use]
    pub fn invocation(&self) -> &crate::contract::Invocation {
        &self.adapter().invocations()[self.invocation]
    }

    /// The pinned executable.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The executable's version, when the probe found one.
    #[must_use]
    pub fn version(&self) -> Option<&Version> {
        self.version.as_ref()
    }

    /// The exact argv to run, program first.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The environment to set on top of the session's.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<String, String> {
        self.invocation().plan().env()
    }

    /// What the child sees on stdin.
    #[must_use]
    pub fn stdin(&self) -> StdinMode {
        self.invocation().plan().stdin()
    }

    /// The invocation as the user typed it.
    #[must_use]
    pub fn user_invocation(&self) -> &[String] {
        &self.user_invocation
    }
}

/// What an adapter answers for an invocation (spec v0.3 §1.6).
#[derive(Debug, Clone, PartialEq)]
pub enum Negotiation {
    /// No adapter knows the executable; another adapter may try, or the program runs raw.
    NotApplicable,
    /// An adapter knows the executable but the context keeps raw semantics.
    RawPreferred {
        /// Why.
        reason: String,
    },
    /// The adapter provides its schema with high confidence.
    StructuredSupported {
        /// What to run and how to read it.
        plan: AdapterPlan,
        /// Every adapter that answered, by full id, in the order considered.
        candidates: Vec<String>,
        /// Why the winner won.
        selection: String,
    },
    /// Structure is valid but some fields or semantics are unavailable.
    StructuredSupportedWithLimits {
        /// What to run and how to read it.
        plan: AdapterPlan,
        /// What is missing, in words provenance and `explain` show.
        limits: Vec<String>,
        /// Every adapter that answered.
        candidates: Vec<String>,
        /// Why the winner won.
        selection: String,
    },
    /// The adapter knows the executable but not this option combination.
    UnsupportedInvocation {
        /// The adapter that refused.
        adapter: String,
        /// What it could not guarantee.
        reason: String,
        /// What the adapter does about it under an interactive demand.
        fallback: Fallback,
    },
    /// The executable's version is outside the tested range.
    IncompatibleVersion {
        /// The adapter that refused.
        adapter: String,
        /// The version found, or `None` when detection failed.
        found: Option<Version>,
        /// The range supported, as the contract writes it.
        supported: String,
        /// What the adapter does about it under an interactive demand.
        fallback: Fallback,
    },
    /// The resolved binary is not the one the contract pins.
    ExecutableMismatch {
        /// The adapter that refused.
        adapter: String,
        /// The path the contract names.
        expected: String,
        /// The path that resolved.
        found: PathBuf,
    },
    /// Two installations of one adapter id claim the invocation; nothing can separate them.
    Conflict {
        /// The claimants.
        candidates: Vec<String>,
    },
    /// A remote agent answered that it adapts the invocation on its side (spec v0.3 §1.54);
    /// the plan is the remote's and never reaches this side.
    RemoteAdapted {
        /// The remote's own description of what it will do.
        state: String,
    },
    /// The adapter exists but its pack may not influence structured output here.
    Disabled {
        /// The adapter that would have answered.
        adapter: String,
        /// Why the pack is held back.
        reason: String,
    },
}

impl Negotiation {
    /// The plan, when the invocation is adapted.
    #[must_use]
    pub fn plan(&self) -> Option<&AdapterPlan> {
        match self {
            Self::StructuredSupported { plan, .. }
            | Self::StructuredSupportedWithLimits { plan, .. } => Some(plan),
            _ => None,
        }
    }

    /// Whether the invocation runs raw under `demand`: nothing is adapted and nothing fails.
    #[must_use]
    pub fn runs_raw(&self, demand: &OutputDemand) -> bool {
        match self {
            Self::NotApplicable | Self::RawPreferred { .. } => true,
            Self::StructuredSupported { .. }
            | Self::StructuredSupportedWithLimits { .. }
            | Self::RemoteAdapted { .. } => false,
            Self::UnsupportedInvocation { fallback, .. }
            | Self::IncompatibleVersion { fallback, .. } => {
                !matches!(demand, OutputDemand::Structured { .. }) && *fallback == Fallback::Raw
            }
            Self::ExecutableMismatch { .. } | Self::Conflict { .. } | Self::Disabled { .. } => {
                !matches!(demand, OutputDemand::Structured { .. })
            }
        }
    }

    /// The structured error a refusal becomes under a demand it cannot satisfy (spec v0.3
    /// §1.16, §1.18, §1.65), or `None` when the invocation runs raw or is adapted.
    ///
    /// `executable` and `argv` are what was asked for; the payload carries them under the
    /// keys of ADR-0053 so every emitter agrees.
    #[must_use]
    pub fn refusal(
        &self,
        demand: &OutputDemand,
        executable: &Path,
        argv: &[String],
    ) -> Option<ErrorValue> {
        if self.runs_raw(demand) || self.plan().is_some() {
            return None;
        }
        let invocation = argv.join(" ");
        let payload = |error: ErrorValue, adapter: &str| {
            error
                .with_metadata("adapter", Value::string(adapter))
                .with_metadata(
                    "executable",
                    Value::string(&executable.display().to_string()),
                )
                .with_metadata("invocation", Value::string(&invocation))
                .with_metadata("raw_fallback_safe", Value::Bool(true))
                .with_metadata("recovery", Value::string(&format!("raw {invocation}")))
        };
        let help = format!(
            "`raw {invocation}` runs the program as typed; `{invocation} | from <format>` decodes \
             its output yourself (spec v0.3 §1.16)"
        );
        Some(match self {
            Self::UnsupportedInvocation {
                adapter, reason, ..
            } => payload(
                ErrorValue::new(
                    ErrorCode::AdapterUnsupportedInvocation,
                    format!(
                        "adapter {adapter} recognizes `{}` but cannot guarantee structured \
                         semantics for {reason}",
                        argv.first().map_or("", String::as_str)
                    ),
                )
                .with_help(help),
                adapter,
            ),
            Self::IncompatibleVersion {
                adapter,
                found,
                supported,
                ..
            } => payload(
                ErrorValue::new(
                    ErrorCode::AdapterVersionIncompatible,
                    format!(
                        "adapter {adapter} supports {supported}, found {}",
                        found
                            .as_ref()
                            .map_or_else(|| "no detectable version".to_owned(), Version::to_string)
                    ),
                )
                .with_help(help)
                .with_metadata(
                    "executable_version",
                    found
                        .as_ref()
                        .map_or(Value::Null, |version| Value::string(&version.to_string())),
                )
                .with_metadata("supported", Value::string(supported)),
                adapter,
            ),
            Self::ExecutableMismatch {
                adapter,
                expected,
                found,
            } => payload(
                ErrorValue::new(
                    ErrorCode::AdapterExecutableMismatch,
                    format!(
                        "adapter {adapter} is written for {expected}, and {} resolved instead",
                        found.display()
                    ),
                )
                .with_help(help),
                adapter,
            ),
            Self::Conflict { candidates } => payload(
                ErrorValue::new(
                    ErrorCode::AdapterConflict,
                    format!(
                        "{} claim this invocation and cannot be separated",
                        candidates.join(" and ")
                    ),
                )
                .with_help("disable one of the installed copies; until then the program runs raw")
                .with_metadata(
                    "candidates",
                    Value::list(candidates.iter().map(|candidate| Value::string(candidate))),
                ),
                candidates.first().map_or("", String::as_str),
            ),
            Self::NotApplicable
            | Self::RawPreferred { .. }
            | Self::StructuredSupported { .. }
            | Self::StructuredSupportedWithLimits { .. }
            | Self::RemoteAdapted { .. } => return None,
            Self::Disabled { adapter, reason } => payload(
                ErrorValue::new(
                    ErrorCode::AdapterDisabled,
                    format!("adapter {adapter} is disabled here: {reason}"),
                )
                .with_help(format!(
                    "`load plugin <package> --grant process.exec` enables a package's adapters \
                     (spec v0.3 §1.22); `raw {invocation}` runs the program as typed"
                )),
                adapter,
            ),
        })
    }

    /// The state in the words of spec v0.3 §1.57, for `explain`, history and diagnostics.
    #[must_use]
    pub fn describe(&self, demand: &OutputDemand) -> String {
        let structured = matches!(demand, OutputDemand::Structured { .. });
        let consequence = |fallback: Fallback| -> &'static str {
            if structured {
                "fails: a structured consumer never receives text (spec v0.3 §1.18)"
            } else if fallback == Fallback::Raw {
                "runs raw"
            } else {
                "fails"
            }
        };
        match self {
            Self::NotApplicable => "raw (no adapter)".to_owned(),
            Self::RawPreferred { reason } => format!("raw ({reason})"),
            Self::StructuredSupported { plan, .. } => {
                format!("adapted by {}", plan.adapter().full_id())
            }
            Self::StructuredSupportedWithLimits { plan, limits, .. } => format!(
                "adapted by {} with limits: {}",
                plan.adapter().full_id(),
                limits.join("; ")
            ),
            Self::UnsupportedInvocation {
                adapter,
                reason,
                fallback,
            } => {
                let state = format!("unsupported invocation: {adapter} cannot guarantee {reason}");
                if structured {
                    format!("{state}; {}", consequence(*fallback))
                } else {
                    format!("raw ({state})")
                }
            }
            Self::IncompatibleVersion {
                adapter,
                found,
                supported,
                fallback,
            } => {
                let found = found
                    .as_ref()
                    .map_or_else(|| "undetected".to_owned(), Version::to_string);
                let state =
                    format!("version incompatible: {adapter} supports {supported}, found {found}");
                if structured {
                    format!("{state}; {}", consequence(*fallback))
                } else {
                    format!("raw ({state})")
                }
            }
            Self::ExecutableMismatch {
                adapter,
                expected,
                found,
            } => {
                let state = format!(
                    "executable mismatch: {adapter} is written for {expected}, found {}",
                    found.display()
                );
                if structured {
                    format!("{state}; fails")
                } else {
                    format!("raw ({state})")
                }
            }
            Self::Conflict { candidates } => {
                format!("conflict: {} cannot be separated", candidates.join(" and "))
            }
            Self::RemoteAdapted { state } => state.clone(),
            Self::Disabled { adapter, reason } => {
                let state = format!("adapter disabled: {adapter}, {reason}");
                if structured {
                    format!("{state}; fails")
                } else {
                    format!("raw ({state})")
                }
            }
        }
    }
}

/// The installed adapters and the probe cache (spec v0.3 §1.24).
pub struct Registry {
    /// The packs, each with why it may not influence structured output, if it may not
    /// (ADR-0065). Behind a lock so a registry can be shared and still gain packs.
    packs: Mutex<Vec<(Arc<AdapterPack>, Option<String>)>>,
    prober: Prober,
    versions: Mutex<HashMap<Identity, Option<Version>>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<String> = self
            .snapshot()
            .iter()
            .map(|(pack, _)| pack.id().to_owned())
            .collect();
        f.debug_struct("Registry")
            .field("packs", &ids)
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// A registry over `packs`, probing versions through `prober`.
    #[must_use]
    pub fn new(packs: Vec<AdapterPack>, prober: Prober) -> Self {
        Self {
            packs: Mutex::new(
                packs
                    .into_iter()
                    .map(|pack| (Arc::new(pack), None))
                    .collect(),
            ),
            prober,
            versions: Mutex::new(HashMap::new()),
        }
    }

    /// A registry over the first-party packs bundled with the shell.
    #[must_use]
    pub fn bundled(prober: Prober) -> Self {
        Self::new(crate::contract::first_party().to_vec(), prober)
    }

    /// Adds one pack. Load order never decides anything (spec v0.3 §1.25).
    #[must_use]
    pub fn with_pack(self, pack: AdapterPack) -> Self {
        self.add_pack(pack);
        self
    }

    /// Adds a pack that is known but may not influence structured output — its process.exec
    /// grant was denied, or its tier is not trusted yet (spec v0.3 §1.22, §1.56, ADR-0065).
    #[must_use]
    pub fn with_disabled_pack(self, pack: AdapterPack, reason: &str) -> Self {
        self.add_disabled_pack(pack, reason);
        self
    }

    /// Adds several packs.
    #[must_use]
    pub fn with_packs(self, packs: Vec<AdapterPack>) -> Self {
        for pack in packs {
            self.add_pack(pack);
        }
        self
    }

    /// Adds a pack to a registry the session already holds; a pack of the same id replaces
    /// the earlier one, so reloading a package never leaves two copies to conflict.
    pub fn add_pack(&self, pack: AdapterPack) {
        self.insert(pack, None);
    }

    /// Adds a disabled pack, replacing an earlier pack of the same id.
    pub fn add_disabled_pack(&self, pack: AdapterPack, reason: &str) {
        self.insert(pack, Some(reason.to_owned()));
    }

    fn insert(&self, pack: AdapterPack, disabled: Option<String>) {
        if let Ok(mut packs) = self.packs.lock() {
            packs.retain(|(held, _)| held.id() != pack.id());
            packs.push((Arc::new(pack), disabled));
        }
    }

    /// The packs as they are now, with their disabled reasons.
    fn snapshot(&self) -> Vec<(Arc<AdapterPack>, Option<String>)> {
        self.packs
            .lock()
            .map(|packs| packs.clone())
            .unwrap_or_default()
    }

    /// The packs, in load order.
    #[must_use]
    pub fn packs(&self) -> Vec<Arc<AdapterPack>> {
        self.snapshot().into_iter().map(|(pack, _)| pack).collect()
    }

    /// The flags every adapter of `program` declares, for completion that invents nothing
    /// (spec v0.3 §1.59): what the contracts let through, and nothing else.
    #[must_use]
    pub fn declared_flags(&self, program: &str) -> Vec<String> {
        let mut flags: Vec<String> = self
            .adapters_for(program)
            .iter()
            .flat_map(|(pack, index)| {
                pack.adapters()[*index]
                    .invocations()
                    .iter()
                    .flat_map(|invocation| {
                        let matcher = invocation.matcher();
                        matcher
                            .allowed_flags()
                            .iter()
                            .chain(matcher.allowed_flags_with_value())
                            .chain(matcher.required_flags())
                            .cloned()
                            .collect::<Vec<String>>()
                    })
                    .collect::<Vec<String>>()
            })
            .collect();
        flags.sort();
        flags.dedup();
        flags
    }

    /// The schema the adapters of `program` produce, when they agree on one — for completion
    /// after the pipe (spec v0.3 §1.59).
    #[must_use]
    pub fn schema_for(&self, program: &str) -> Option<String> {
        let mut schemas: Vec<String> = self
            .adapters_for(program)
            .iter()
            .map(|(pack, index)| pack.adapters()[*index].schema().to_owned())
            .collect();
        schemas.sort();
        schemas.dedup();
        match schemas.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }

    /// The adapters that name `program` (a name or a path) among their executables, as the
    /// pack and the adapter's index in it.
    #[must_use]
    pub fn adapters_for(&self, program: &str) -> Vec<(Arc<AdapterPack>, usize)> {
        let name = basename(program);
        let mut found = Vec::new();
        for (pack, _) in self.snapshot() {
            for (index, adapter) in pack.adapters().iter().enumerate() {
                if adapter
                    .executable()
                    .names()
                    .iter()
                    .any(|declared| basename(declared) == name)
                {
                    found.push((Arc::clone(&pack), index));
                }
            }
        }
        found
    }

    /// What the registry answers for running `executable` with `argv` (program first, as the
    /// user typed it) when its stdout must satisfy `demand`.
    #[must_use]
    pub fn negotiate(
        &self,
        executable: &Path,
        argv: &[String],
        demand: &OutputDemand,
    ) -> Negotiation {
        let name = executable
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let wanted = match demand {
            OutputDemand::Structured { .. } => DemandKind::Structured,
            OutputDemand::Interactive => DemandKind::Interactive,
            OutputDemand::RawBytes | OutputDemand::Text | OutputDemand::Discard => {
                return if self.adapters_for(&name).is_empty() {
                    Negotiation::NotApplicable
                } else {
                    Negotiation::RawPreferred {
                        reason: "downstream bytes".to_owned(),
                    }
                };
            }
        };

        // Every adapter that names the executable answers; the answers are then ranked.
        let mut answers: Vec<(Ranked, Negotiation)> = Vec::new();
        let packs = self.snapshot();
        for (index, (pack, disabled)) in packs.iter().enumerate() {
            for (position, adapter) in pack.adapters().iter().enumerate() {
                let Some(declared) = adapter
                    .executable()
                    .names()
                    .iter()
                    .find(|declared| basename(declared) == name)
                else {
                    continue;
                };
                if !adapter.output_demand().contains(&wanted) {
                    continue;
                }
                let answer = match disabled {
                    Some(reason) => Negotiation::Disabled {
                        adapter: adapter.full_id(),
                        reason: reason.clone(),
                    },
                    None => self.answer(pack, index, position, adapter, declared, executable, argv),
                };
                let rank = Ranked {
                    exact_path: declared.contains('/'),
                    specificity: match &answer {
                        // More words matched and more flags required both mean a narrower
                        // invocation: `ss -t` is the TCP adapter's before the catch-all's.
                        Negotiation::StructuredSupported { plan, .. }
                        | Negotiation::StructuredSupportedWithLimits { plan, .. } => {
                            let matcher = plan.invocation().matcher();
                            matcher.words().iter().map(Vec::len).max().unwrap_or(0)
                                + matcher.required_flags().len()
                        }
                        _ => 0,
                    },
                    tier: pack.tier(),
                    id: adapter.full_id(),
                };
                answers.push((rank, answer));
            }
        }
        if answers.is_empty() {
            return Negotiation::NotApplicable;
        }

        let mut supported: Vec<(Ranked, Negotiation)> = answers
            .iter()
            .filter(|(_, answer)| answer.plan().is_some())
            .cloned()
            .collect();
        if supported.is_empty() {
            // Nothing adapts; report the most specific refusal, deterministically.
            answers.sort_by(|a, b| a.0.cmp(&b.0));
            return answers
                .into_iter()
                .next()
                .map(|(_, answer)| answer)
                .unwrap_or(Negotiation::NotApplicable);
        }
        supported.sort_by(|a, b| a.0.cmp(&b.0));
        let candidates: Vec<String> = {
            let mut ids: Vec<String> = supported.iter().map(|(rank, _)| rank.id.clone()).collect();
            ids.sort();
            ids
        };
        if supported.len() > 1 && supported[0].0.id == supported[1].0.id {
            return Negotiation::Conflict { candidates };
        }
        let selection = supported.get(1).map_or_else(
            || "the only candidate".to_owned(),
            |runner_up| supported[0].0.beats(&runner_up.0),
        );
        match supported.into_iter().next().map(|(_, answer)| answer) {
            Some(Negotiation::StructuredSupported { plan, .. }) => {
                Negotiation::StructuredSupported {
                    plan,
                    candidates,
                    selection,
                }
            }
            Some(Negotiation::StructuredSupportedWithLimits { plan, limits, .. }) => {
                Negotiation::StructuredSupportedWithLimits {
                    plan,
                    limits,
                    candidates,
                    selection,
                }
            }
            _ => Negotiation::NotApplicable,
        }
    }

    /// One adapter's answer, before ranking.
    #[allow(
        clippy::too_many_arguments,
        reason = "the answer needs the whole context once"
    )]
    fn answer(
        &self,
        pack: &Arc<AdapterPack>,
        pack_index: usize,
        position: usize,
        adapter: &Adapter,
        declared: &str,
        executable: &Path,
        argv: &[String],
    ) -> Negotiation {
        let _ = pack_index;
        let full_id = adapter.full_id();
        // Identity pinning (spec v0.3 §1.22): a contract that names a path means that path.
        if declared.contains('/') && Path::new(declared) != executable {
            return Negotiation::ExecutableMismatch {
                adapter: full_id,
                expected: declared.to_owned(),
                found: executable.to_path_buf(),
            };
        }

        // Version (spec v0.3 §1.46).
        let range =
            VersionRange::parse(adapter.executable().versions()).unwrap_or(VersionRange::ANY);
        let version = if range.is_any() {
            None
        } else {
            let found = self.version_of(executable, adapter);
            if !found.as_ref().is_some_and(|found| range.contains(found)) {
                return Negotiation::IncompatibleVersion {
                    adapter: full_id,
                    found,
                    supported: adapter.executable().versions().to_owned(),
                    fallback: adapter.fallback(),
                };
            }
            found
        };

        // Invocation (spec v0.3 §1.14–§1.16).
        let user_arguments = argv.get(1..).unwrap_or_default();
        let mut refusal: Option<String> = None;
        for (index, invocation) in adapter.invocations().iter().enumerate() {
            match match_invocation(invocation, user_arguments) {
                Ok(passthrough) => {
                    let mut planned: Vec<String> = invocation.plan().argv().to_vec();
                    if let Some(first) = planned.first_mut() {
                        *first = executable.to_string_lossy().into_owned();
                    }
                    if invocation.plan().appends_user_flags() {
                        planned.extend(passthrough);
                    }
                    planned.extend(invocation.plan().trailing_argv().iter().cloned());
                    // The plan's argv names the program as the contract spells it; the first
                    // element is shown as typed so `explain` reads naturally, and the pinned
                    // path is what actually runs.
                    let mut shown = planned.clone();
                    if let Some(first) = shown.first_mut() {
                        first.clone_from(&invocation.plan().argv()[0]);
                    }
                    let plan = AdapterPlan {
                        pack: Arc::clone(pack),
                        adapter: position,
                        invocation: index,
                        executable: executable.to_path_buf(),
                        version,
                        argv: shown,
                        user_invocation: argv.to_vec(),
                    };
                    let limits = unreported_fields(adapter);
                    return if limits.is_empty() {
                        Negotiation::StructuredSupported {
                            plan,
                            candidates: Vec::new(),
                            selection: String::new(),
                        }
                    } else {
                        Negotiation::StructuredSupportedWithLimits {
                            plan,
                            limits,
                            candidates: Vec::new(),
                            selection: String::new(),
                        }
                    };
                }
                Err(reason) => {
                    refusal.get_or_insert(reason);
                }
            }
        }
        Negotiation::UnsupportedInvocation {
            adapter: full_id,
            reason: refusal.unwrap_or_else(|| "this invocation".to_owned()),
            fallback: adapter.fallback(),
        }
    }

    /// The executable's version, probed once per identity (spec v0.3 §1.46).
    fn version_of(&self, executable: &Path, adapter: &Adapter) -> Option<Version> {
        let probe = adapter.executable().version_probe()?;
        let identity = Identity::of(executable)?;
        if let Ok(cache) = self.versions.lock()
            && let Some(known) = cache.get(&identity)
        {
            return known.clone();
        }
        let output = (self.prober)(executable, probe.argv());
        let version = output.as_deref().and_then(|text| {
            let regex = regex::Regex::new(probe.pattern()).ok()?;
            let captured = regex.captures(text)?.get(1)?.as_str();
            Version::parse(captured)
        });
        if let Ok(mut cache) = self.versions.lock() {
            cache.insert(identity, version.clone());
        }
        version
    }
}

/// Conflict resolution (spec v0.3 §1.25): exact path match, then invocation specificity, then
/// trust tier, then the adapter id — never load order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ranked {
    exact_path: bool,
    specificity: usize,
    tier: Tier,
    id: String,
}

impl Ranked {
    fn tier_rank(tier: Tier) -> u8 {
        match tier {
            Tier::FirstParty => 0,
            Tier::Recommended => 1,
            Tier::Community => 2,
            Tier::Experimental => 3,
        }
    }

    fn beats(&self, other: &Self) -> String {
        if self.exact_path != other.exact_path {
            "pins the resolved path exactly".to_owned()
        } else if self.specificity != other.specificity {
            "matches a more specific invocation".to_owned()
        } else if self.tier != other.tier {
            format!(
                "{} outranks {}",
                tier_name(self.tier),
                tier_name(other.tier)
            )
        } else {
            "earlier in lexical order, the final tie-break".to_owned()
        }
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .exact_path
            .cmp(&self.exact_path)
            .then(other.specificity.cmp(&self.specificity))
            .then(Self::tier_rank(self.tier).cmp(&Self::tier_rank(other.tier)))
            .then(self.id.cmp(&other.id))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::FirstParty => "first-party",
        Tier::Recommended => "recommended",
        Tier::Community => "community",
        Tier::Experimental => "experimental",
    }
}

/// Matches the user's arguments against one invocation contract.
///
/// Returns the arguments that pass through — allowed flags (with their values) and allowed
/// positionals, in the order typed — or the reason the invocation is not covered.
fn match_invocation(
    invocation: &crate::contract::Invocation,
    arguments: &[String],
) -> Result<Vec<String>, String> {
    let matcher = invocation.matcher();
    // `-tunap` is `-t -u -n -a -p` where the contract says so (spec v0.3 §1.32's own spelling);
    // a combined word the contract knows as a whole stays whole.
    let expanded: Vec<String>;
    let arguments: &[String] = if matcher.combines_flags() {
        expanded = arguments
            .iter()
            .flat_map(|argument| {
                let known = matcher.allowed_flags().iter().any(|f| f == argument)
                    || matcher
                        .allowed_flags_with_value()
                        .iter()
                        .any(|f| f == argument)
                    || matcher.required_flags().iter().any(|f| f == argument);
                if !known
                    && argument.len() > 2
                    && argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument[1..].chars().all(char::is_alphanumeric)
                {
                    argument[1..]
                        .chars()
                        .map(|letter| format!("-{letter}"))
                        .collect::<Vec<String>>()
                } else {
                    vec![argument.clone()]
                }
            })
            .collect();
        &expanded
    } else {
        arguments
    };
    for required in matcher.required_flags() {
        if !arguments.iter().any(|argument| argument == required) {
            return Err(format!("this form without `{required}`"));
        }
    }
    // Every token keeps its position, so what passes through is in the order typed: `find`'s
    // paths come before its tests, as the user wrote them (ADR-0056, ADR-0060).
    let mut passthrough: Vec<(usize, String)> = Vec::new();
    let mut words: Vec<(usize, &str)> = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument.starts_with('-') && argument.len() > 1 {
            let (flag, inline_value) = argument
                .split_once('=')
                .map_or((argument, None), |(f, v)| (f, Some(v)));
            if matcher
                .required_flags()
                .iter()
                .any(|required| required == flag)
                && inline_value.is_none()
            {
                // A required flag is what selected the invocation; the plan already spells it,
                // unless the contract also allows it through.
                if matcher
                    .allowed_flags()
                    .iter()
                    .any(|allowed| allowed == flag)
                {
                    passthrough.push((index, argument.to_owned()));
                }
            } else if matcher
                .allowed_flags()
                .iter()
                .any(|allowed| allowed == flag)
                && inline_value.is_none()
            {
                passthrough.push((index, argument.to_owned()));
            } else if matcher
                .allowed_flags_with_value()
                .iter()
                .any(|allowed| allowed == flag)
            {
                passthrough.push((index, argument.to_owned()));
                if inline_value.is_none() {
                    index += 1;
                    match arguments.get(index) {
                        Some(value) => passthrough.push((index, value.clone())),
                        None => return Err(format!("`{flag}` without its value")),
                    }
                }
            } else {
                return Err(format!("`{argument}`"));
            }
        } else if matcher
            .allowed_flags()
            .iter()
            .any(|allowed| allowed == argument)
        {
            // A bare word the contract lists among its flags — `!`, `(`, `)` for find.
            passthrough.push((index, argument.to_owned()));
        } else {
            words.push((index, argument));
        }
        index += 1;
    }

    // The positional words select the invocation; what follows them passes through when the
    // contract allows it.
    let selected = matcher
        .words()
        .iter()
        .filter(|alternative| {
            words.len() >= alternative.len()
                && alternative.iter().zip(&words).all(|(a, (_, b))| a == b)
        })
        .max_by_key(|alternative| alternative.len());
    let Some(alternative) = selected else {
        return Err(format!(
            "`{}`",
            words.first().map_or("this form", |(_, word)| word)
        ));
    };
    let rest = &words[alternative.len()..];
    if !rest.is_empty() {
        if matcher.positionals() == Positionals::Forbid {
            let spelled: Vec<&str> = rest.iter().map(|(_, word)| *word).collect();
            return Err(format!("`{}`", spelled.join(" ")));
        }
        passthrough.extend(rest.iter().map(|(at, word)| (*at, (*word).to_owned())));
    }
    passthrough.sort_by_key(|(at, _)| *at);
    Ok(passthrough.into_iter().map(|(_, word)| word).collect())
}

/// The schema fields the adapter's map never reports, which is what makes its support
/// "with limits" (spec v0.3 §1.6).
fn unreported_fields(adapter: &Adapter) -> Vec<String> {
    let Some(schema) = adapter
        .schema()
        .parse::<ono_value::SchemaId>()
        .ok()
        .and_then(|id| ono_value::builtin_schemas().get(&id))
    else {
        return Vec::new();
    };
    schema
        .fields()
        .iter()
        .filter(|field| !adapter.fields().contains_key(field.name()))
        .map(|field| format!("`{}` is not reported by {}", field.name(), adapter.id()))
        .collect()
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}
