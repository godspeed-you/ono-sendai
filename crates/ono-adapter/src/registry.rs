//! The adapter registry: which adapters exist, and what one of them answers for an invocation
//! (spec v0.3 §1.6, §1.14–§1.16, §1.24, §1.25, §1.46).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
            Self::StructuredSupported { .. } | Self::StructuredSupportedWithLimits { .. } => false,
            Self::UnsupportedInvocation { fallback, .. }
            | Self::IncompatibleVersion { fallback, .. } => {
                !matches!(demand, OutputDemand::Structured { .. }) && *fallback == Fallback::Raw
            }
            Self::ExecutableMismatch { .. } | Self::Conflict { .. } => {
                !matches!(demand, OutputDemand::Structured { .. })
            }
        }
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
        }
    }
}

/// The installed adapters and the probe cache (spec v0.3 §1.24).
pub struct Registry {
    packs: Vec<Arc<AdapterPack>>,
    prober: Prober,
    versions: Mutex<HashMap<Identity, Option<Version>>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.packs.iter().map(|pack| pack.id()).collect();
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
            packs: packs.into_iter().map(Arc::new).collect(),
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
    pub fn with_pack(mut self, pack: AdapterPack) -> Self {
        self.packs.push(Arc::new(pack));
        self
    }

    /// Adds several packs.
    #[must_use]
    pub fn with_packs(mut self, packs: Vec<AdapterPack>) -> Self {
        self.packs.extend(packs.into_iter().map(Arc::new));
        self
    }

    /// The packs, in load order.
    #[must_use]
    pub fn packs(&self) -> Vec<&AdapterPack> {
        self.packs.iter().map(Arc::as_ref).collect()
    }

    /// Every adapter, with the pack it belongs to.
    pub fn adapters(&self) -> impl Iterator<Item = (&AdapterPack, &Adapter)> {
        self.packs.iter().flat_map(|pack| {
            pack.adapters()
                .iter()
                .map(move |adapter| (pack.as_ref(), adapter))
        })
    }

    /// The adapters that name `program` (a name or a path) among their executables.
    #[must_use]
    pub fn adapters_for(&self, program: &str) -> Vec<&Adapter> {
        let name = basename(program);
        self.adapters()
            .filter(|(_, adapter)| {
                adapter
                    .executable()
                    .names()
                    .iter()
                    .any(|declared| basename(declared) == name)
            })
            .map(|(_, adapter)| adapter)
            .collect()
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
        for (index, pack) in self.packs.iter().enumerate() {
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
                let answer =
                    self.answer(pack, index, position, adapter, declared, executable, argv);
                let rank = Ranked {
                    exact_path: declared.contains('/'),
                    specificity: match &answer {
                        Negotiation::StructuredSupported { plan, .. }
                        | Negotiation::StructuredSupportedWithLimits { plan, .. } => plan
                            .invocation()
                            .matcher()
                            .words()
                            .iter()
                            .map(Vec::len)
                            .max()
                            .unwrap_or(0),
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
    let mut passthrough = Vec::new();
    let mut words: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument.starts_with('-') && argument.len() > 1 {
            let (flag, inline_value) = argument
                .split_once('=')
                .map_or((argument, None), |(f, v)| (f, Some(v)));
            if matcher
                .allowed_flags()
                .iter()
                .any(|allowed| allowed == flag)
                && inline_value.is_none()
            {
                passthrough.push(argument.to_owned());
            } else if matcher
                .allowed_flags_with_value()
                .iter()
                .any(|allowed| allowed == flag)
            {
                passthrough.push(argument.to_owned());
                if inline_value.is_none() {
                    index += 1;
                    match arguments.get(index) {
                        Some(value) => passthrough.push(value.clone()),
                        None => return Err(format!("`{flag}` without its value")),
                    }
                }
            } else {
                return Err(format!("`{argument}`"));
            }
        } else {
            words.push(argument);
        }
        index += 1;
    }

    // The positional words select the invocation; what follows them passes through when the
    // contract allows it.
    let selected = matcher
        .words()
        .iter()
        .filter(|alternative| {
            words.len() >= alternative.len() && alternative.iter().zip(&words).all(|(a, b)| a == b)
        })
        .max_by_key(|alternative| alternative.len());
    let Some(alternative) = selected else {
        return Err(format!(
            "`{}`",
            words.first().copied().unwrap_or("this form")
        ));
    };
    let rest = &words[alternative.len()..];
    if !rest.is_empty() {
        if matcher.positionals() == Positionals::Forbid {
            return Err(format!("`{}`", rest.join(" ")));
        }
        passthrough.extend(rest.iter().map(|word| (*word).to_owned()));
    }
    Ok(passthrough)
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
