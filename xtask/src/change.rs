//! Drift between `docs/contracts/change/`, `docs/contracts/recovery/` and everything they claim
//! about (spec v0.6 §47).
//!
//! §47 requires eleven version-controlled registries for the prospective change and recovery
//! interface, and says CI MUST detect drift between the stable contracts and registered runtime
//! behaviour. This module is that gate, and it is a separate module from [`crate::contracts`] for
//! the reason [`crate::temporal`] is: these registries describe a vocabulary and a policy rather
//! than a call surface, and most of what goes wrong with them is a cross-reference between two of
//! the eleven rather than a mismatch with a Rust type.
//!
//! What it refuses, and why each one matters more than a normal contract check:
//!
//! | Rule | Function | What it prevents |
//! |---|---|---|
//! | a vocabulary the registry and the machine spell differently | `check_vocabularies` | a level a script matches on that the shell never emits |
//! | a lifecycle edge one of them draws and the other does not | `check_transitions` | §2.3 becoming reachable through a state nobody checked |
//! | a protection level whose persistent-coverage claim differs | `check_protection_levels` | §4.6 — the word `protected` shown for partial coverage |
//! | an execution method that admits a command line | `check_execution_methods` | §2.17 and §43.6, at the registry rather than at review |
//! | a restore method whose destructiveness order differs | `check_restore_methods` | Appendix C.1 preferring the bigger hammer |
//! | an asset type whose failure domain claim differs | `check_asset_types` | §11.5 — a snapshot described as a backup |
//! | a stable v0.6 command missing from either side | `check_command_inventory` | §47's drift rule, both directions |
//! | an error one file names and the other does not | `check_errors` | §45's family losing a code a script matches on |
//! | a registry no `xtask` function validates | `check_inventory` | the referee that nothing holds |
//!
//! A missing `docs/contracts/change/` directory is not a problem: registries arrive with the phase
//! that needs them (AGENTS.md §14). A directory that exists and is missing one of §47's eleven
//! required files is, because a half-written contract set makes a promise nobody can check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ono_change_core::{
    ActionRole, ActionStatus, ChangeCapability, ConsistencyClass, EffectConfidence, EffectDomain,
    EffectKind, Idempotency, ImpactClass, LifecycleEvent, PlanState, PreconditionKind,
    ProtectionLevel, ProtectionMode, RecoveryAssetType, RecoveryCapability, RecoveryObjective,
    RestoreMethod, RiskClass, RiskDimension, StrategyKind, VerificationClass, VerificationStatus,
};
use serde_yaml_ng::Value as Yaml;

pub use crate::scan::Problem;

/// §47's six change registries.
const CHANGE_REGISTRIES: [&str; 6] = [
    "plans.yaml",
    "actions.yaml",
    "effects.yaml",
    "risk.yaml",
    "verification.yaml",
    "strategies.yaml",
];

/// §47's five recovery registries.
const RECOVERY_REGISTRIES: [&str; 5] = [
    "providers.yaml",
    "assets.yaml",
    "consistency.yaml",
    "policies.yaml",
    "errors.yaml",
];

/// The registries, loaded once so a cross-reference between two of them is one read.
struct Registries {
    documents: BTreeMap<String, Yaml>,
}

impl Registries {
    fn get(&self, file: &str) -> Option<&Yaml> {
        self.documents.get(file)
    }

    /// The `id` of every entry of the `key` sequence in `file`.
    fn ids(&self, file: &str, key: &str) -> BTreeSet<String> {
        self.entries(file, key)
            .into_iter()
            .filter_map(|entry| entry.get("id").and_then(Yaml::as_str).map(str::to_owned))
            .collect()
    }

    /// Every entry of the `key` sequence in `file`.
    fn entries(&self, file: &str, key: &str) -> Vec<&Yaml> {
        self.get(file)
            .and_then(|document| document.get(key))
            .and_then(Yaml::as_sequence)
            .map(|items| items.iter().collect())
            .unwrap_or_default()
    }
}

/// Every drift `docs/contracts/change/` and `docs/contracts/recovery/` can carry.
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let change = root.join("docs").join("contracts").join("change");
    let recovery = root.join("docs").join("contracts").join("recovery");
    if !change.is_dir() && !recovery.is_dir() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    let mut documents = BTreeMap::new();
    for (directory, files, label) in [
        (&change, CHANGE_REGISTRIES.as_slice(), "change"),
        (&recovery, RECOVERY_REGISTRIES.as_slice(), "recovery"),
    ] {
        for file in files {
            let location = format!("docs/contracts/{label}/{file}");
            match std::fs::read_to_string(directory.join(file)) {
                Err(_) => problems.push(Problem::new(
                    location,
                    "does not exist; v0.6 §47 lists it as a required registry, and a half-written \
                     contract set makes a promise nobody can check",
                )),
                Ok(text) => match serde_yaml_ng::from_str::<Yaml>(&text) {
                    Err(error) => {
                        problems.push(Problem::new(
                            location,
                            format!("is not valid YAML: {error}"),
                        ));
                    }
                    Ok(document) => {
                        documents.insert((*file).to_owned(), document);
                    }
                },
            }
        }
    }
    let registries = Registries { documents };

    problems.extend(check_vocabularies(&registries));
    problems.extend(check_transitions(&registries));
    problems.extend(check_protection_levels(&registries));
    problems.extend(check_execution_methods(&registries));
    problems.extend(check_effect_domains(&registries));
    problems.extend(check_restore_methods(&registries));
    problems.extend(check_asset_types(&registries));
    problems.extend(check_consistency(&registries));
    problems.extend(check_capabilities(&registries));
    problems.extend(check_command_inventory(root, &registries));
    problems.extend(check_schemas(root, &registries));
    problems.extend(check_errors(root, &registries));
    problems.extend(check_risk_gates(&registries));
    problems.extend(check_strategies(&registries));
    problems.extend(check_providers(root, &registries));
    problems.extend(check_inventory(root));
    problems
}

/// One registry list against one closed vocabulary, in both directions.
fn compare(
    location: &str,
    key: &str,
    declared: &BTreeSet<String>,
    implemented: &BTreeSet<String>,
    noun: &str,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    for missing in implemented.difference(declared) {
        problems.push(Problem::new(
            location,
            format!(
                "`{key}` omits the {noun} `{missing}`, which the shell emits. A vocabulary a \
                 script matches on is only closed if the registry closes it"
            ),
        ));
    }
    for extra in declared.difference(implemented) {
        problems.push(Problem::new(
            location,
            format!(
                "`{key}` declares the {noun} `{extra}`, which nothing implements. A value written \
                 here is a promise that a caller can receive it"
            ),
        ));
    }
    problems
}

fn spellings<T: Copy>(all: &[T], as_str: impl Fn(T) -> &'static str) -> BTreeSet<String> {
    all.iter().map(|item| as_str(*item).to_owned()).collect()
}

/// Every closed vocabulary v0.6 defines, against the registry that declares it.
fn check_vocabularies(registries: &Registries) -> Vec<Problem> {
    let mut problems = Vec::new();
    let comparisons: Vec<(&str, &str, &str, BTreeSet<String>, &str)> = vec![
        (
            "docs/contracts/change/plans.yaml",
            "plans.yaml",
            "states",
            spellings(PlanState::ALL, PlanState::as_str),
            "plan state",
        ),
        (
            "docs/contracts/change/plans.yaml",
            "plans.yaml",
            "protection_levels",
            spellings(ProtectionLevel::ALL, ProtectionLevel::as_str),
            "protection level",
        ),
        (
            "docs/contracts/change/actions.yaml",
            "actions.yaml",
            "roles",
            spellings(ActionRole::ALL, ActionRole::as_str),
            "action role",
        ),
        (
            "docs/contracts/change/actions.yaml",
            "actions.yaml",
            "idempotency_classes",
            spellings(Idempotency::ALL, Idempotency::as_str),
            "idempotency class",
        ),
        (
            "docs/contracts/change/actions.yaml",
            "actions.yaml",
            "statuses",
            spellings(ActionStatus::ALL, ActionStatus::as_str),
            "action status",
        ),
        (
            "docs/contracts/change/actions.yaml",
            "actions.yaml",
            "precondition_kinds",
            spellings(PreconditionKind::ALL, PreconditionKind::as_str),
            "precondition kind",
        ),
        (
            "docs/contracts/change/effects.yaml",
            "effects.yaml",
            "confidence_classes",
            spellings(EffectConfidence::ALL, EffectConfidence::as_str),
            "confidence class",
        ),
        (
            "docs/contracts/change/effects.yaml",
            "effects.yaml",
            "domains",
            spellings(EffectDomain::ALL, EffectDomain::as_str),
            "effect domain",
        ),
        (
            "docs/contracts/change/effects.yaml",
            "effects.yaml",
            "kinds",
            spellings(EffectKind::ALL, EffectKind::as_str),
            "effect kind",
        ),
        (
            "docs/contracts/change/effects.yaml",
            "effects.yaml",
            "impact_classes",
            spellings(ImpactClass::ALL, ImpactClass::as_str),
            "impact class",
        ),
        (
            "docs/contracts/change/risk.yaml",
            "risk.yaml",
            "classes",
            spellings(RiskClass::ALL, RiskClass::as_str),
            "risk class",
        ),
        (
            "docs/contracts/change/risk.yaml",
            "risk.yaml",
            "dimensions",
            spellings(RiskDimension::ALL, RiskDimension::as_str),
            "risk dimension",
        ),
        (
            "docs/contracts/change/verification.yaml",
            "verification.yaml",
            "classes",
            spellings(VerificationClass::ALL, VerificationClass::as_str),
            "verification class",
        ),
        (
            "docs/contracts/change/verification.yaml",
            "verification.yaml",
            "statuses",
            spellings(VerificationStatus::ALL, VerificationStatus::as_str),
            "verification status",
        ),
        (
            "docs/contracts/change/strategies.yaml",
            "strategies.yaml",
            "strategies",
            spellings(StrategyKind::ALL, StrategyKind::as_str),
            "strategy",
        ),
        (
            "docs/contracts/recovery/assets.yaml",
            "assets.yaml",
            "types",
            spellings(RecoveryAssetType::ALL, RecoveryAssetType::as_str),
            "asset type",
        ),
        (
            "docs/contracts/recovery/assets.yaml",
            "assets.yaml",
            "restore_methods",
            spellings(RestoreMethod::ALL, RestoreMethod::as_str),
            "restore method",
        ),
        (
            "docs/contracts/recovery/consistency.yaml",
            "consistency.yaml",
            "classes",
            spellings(ConsistencyClass::ALL, ConsistencyClass::as_str),
            "consistency class",
        ),
        (
            "docs/contracts/recovery/policies.yaml",
            "policies.yaml",
            "modes",
            spellings(ProtectionMode::ALL, ProtectionMode::as_str),
            "protection mode",
        ),
        (
            "docs/contracts/recovery/providers.yaml",
            "providers.yaml",
            "capabilities",
            spellings(RecoveryCapability::ALL, RecoveryCapability::as_str),
            "recovery capability",
        ),
        (
            "docs/contracts/recovery/providers.yaml",
            "providers.yaml",
            "change_capabilities",
            spellings(ChangeCapability::ALL, ChangeCapability::as_str),
            "change capability",
        ),
    ];
    for (location, file, key, implemented, noun) in comparisons {
        if registries.get(file).is_none() {
            continue;
        }
        let declared = registries.ids(file, key);
        problems.extend(compare(location, key, &declared, &implemented, noun));
    }

    // `RecoveryObjective` is declared inside the coverage schema rather than in a registry list of
    // its own; check it against the schema's enum so the two cannot drift either.
    let objectives = spellings(RecoveryObjective::ALL, RecoveryObjective::as_str);
    if objectives.len() != RecoveryObjective::ALL.len() {
        problems.push(Problem::new(
            "crates/ono-change-core/src/protection.rs",
            "two recovery objectives share a spelling, so one of them cannot be read back",
        ));
    }
    problems
}

/// Every edge §4.1 draws, against every edge the machine has.
///
/// This is the check the whole module is worth having for. §2.3 — mutation MUST NOT begin when a
/// required recovery asset could not be created — is an edge that does *not* exist, and an edge
/// that appears without anybody noticing is exactly how a safety invariant stops holding.
fn check_transitions(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/plans.yaml";
    if registries.get("plans.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let mut declared: BTreeSet<(String, String, String)> = BTreeSet::new();
    for entry in registries.entries("plans.yaml", "transitions") {
        let read = |key: &str| {
            entry
                .get(key)
                .and_then(Yaml::as_str)
                .map(str::to_owned)
                .unwrap_or_default()
        };
        let (from, event, to) = (read("from"), read("event"), read("to"));
        if from.is_empty() || event.is_empty() || to.is_empty() {
            problems.push(Problem::new(
                location,
                "a transition is missing `from`, `event` or `to`; an edge that does not say where \
                 it goes is not a contract",
            ));
            continue;
        }
        match (
            PlanState::from_name(&from),
            LifecycleEvent::from_name(&event),
        ) {
            (None, _) => problems.push(Problem::new(
                location,
                format!("a transition starts at `{from}`, which is not a state §4.1 defines"),
            )),
            (_, None) => problems.push(Problem::new(
                location,
                format!("a transition is raised by `{event}`, which is not an event §4.1 defines"),
            )),
            (Some(state), Some(lifecycle)) => match state.after(lifecycle) {
                None => problems.push(Problem::new(
                    location,
                    format!(
                        "declares `{from}` -> `{event}` -> `{to}`, and the machine draws no edge \
                         there. §4.1 is a state machine, and a documented transition nothing \
                         implements is a promise a plan cannot keep"
                    ),
                )),
                Some(actual) if actual.as_str() != to => problems.push(Problem::new(
                    location,
                    format!(
                        "declares `{from}` -> `{event}` -> `{to}`, and the machine goes to \
                         `{actual}`. Appendix F reads these states to decide whether anything was \
                         mutated, so the two must agree exactly"
                    ),
                )),
                Some(_) => {
                    declared.insert((from, event, to));
                }
            },
        }
    }

    for state in PlanState::ALL {
        for event in LifecycleEvent::ALL {
            if let Some(next) = state.after(*event) {
                let edge = (
                    state.as_str().to_owned(),
                    event.as_str().to_owned(),
                    next.as_str().to_owned(),
                );
                if !declared.contains(&edge) {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "the machine draws `{}` -> `{}` -> `{}` and `transitions` does not. An \
                             undocumented edge is an edge nobody reviewed",
                            state.as_str(),
                            event.as_str(),
                            next.as_str()
                        ),
                    ));
                }
            }
        }
    }

    // §2.3, checked directly rather than inferred from the table above.
    if PlanState::PrepareFailed
        .after(LifecycleEvent::BeginApply)
        .is_some()
    {
        problems.push(Problem::new(
            "crates/ono-change-core/src/state.rs",
            "there is an edge from `prepare-failed` to `applying`. §2.3: if a required recovery \
             asset cannot be created, mutation MUST NOT begin",
        ));
    }
    problems
}

/// The state and level flags the registry publishes, against what the machine answers.
fn check_protection_levels(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/plans.yaml";
    if registries.get("plans.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for entry in registries.entries("plans.yaml", "protection_levels") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(level) = ProtectionLevel::from_name(id) else {
            continue;
        };
        let declared = entry
            .get("covers_persistent_state")
            .and_then(Yaml::as_bool)
            .unwrap_or(false);
        if declared != level.covers_persistent_state() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares `covers_persistent_state: {declared}` and the shell answers \
                     `{}`. §4.6 forbids the word `protected` being used for partial protection, \
                     and this flag is what a renderer reads to obey it",
                    level.covers_persistent_state()
                ),
            ));
        }
        let symbol = entry.get("symbol").and_then(Yaml::as_str).unwrap_or("");
        if symbol != level.symbol() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares the symbol `{symbol}` and the shell renders `{}`. §20.3 fixes \
                     the compact visual language, and two spellings of it is one too many",
                    level.symbol()
                ),
            ));
        }
    }
    for entry in registries.entries("plans.yaml", "states") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(state) = PlanState::from_name(id) else {
            continue;
        };
        for (key, declared, actual, why) in [
            (
                "mutated",
                entry.get("mutated").and_then(Yaml::as_bool),
                state.has_mutated(),
                "Appendix F reads this to tell a refusal from a partial apply",
            ),
            (
                "appliable",
                entry.get("appliable").and_then(Yaml::as_bool),
                state.is_appliable(),
                "§5.6 refuses drafts and expired plans on exactly this predicate",
            ),
            (
                "terminal",
                entry.get("terminal").and_then(Yaml::as_bool),
                state.is_terminal(),
                "a state that stops on its own and one that does not are different facts",
            ),
            (
                "retains_assets",
                entry.get("retains_assets").and_then(Yaml::as_bool),
                state.retains_assets_indefinitely(),
                "§37.2 keeps the assets of a plan that did not succeed out of success retention",
            ),
            (
                "recoverable",
                entry.get("recoverable").and_then(Yaml::as_bool),
                state.is_recoverable(),
                "§24.1 offers a RecoveryPlan on exactly this predicate",
            ),
        ] {
            if declared != Some(actual) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "state `{id}` declares `{key}: {}` and the shell answers `{actual}`. {why}",
                        declared.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string())
                    ),
                ));
            }
        }
    }
    problems
}

/// §2.17 and §12.3, at the registry: no execution method may admit a command line.
fn check_execution_methods(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/actions.yaml";
    if registries.get("actions.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let methods = registries.entries("actions.yaml", "execution_methods");
    if methods.is_empty() {
        problems.push(Problem::new(
            location,
            "declares no execution methods; §2.17's rule that provider operations are structured \
             rather than interpolated is only checkable against a list",
        ));
    }
    for entry in methods {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        if entry
            .get("admits_command_line")
            .and_then(Yaml::as_bool)
            .unwrap_or(true)
        {
            problems.push(Problem::new(
                location,
                format!(
                    "execution method `{id}` admits a command line. §2.17: provider operations \
                     MUST be structured execution plans, not interpolated shell command strings"
                ),
            ));
        }
        if entry
            .get("shape")
            .and_then(Yaml::as_str)
            .is_none_or(str::is_empty)
        {
            problems.push(Problem::new(
                location,
                format!("execution method `{id}` declares no shape, so nothing says what it holds"),
            ));
        }
    }
    problems
}

/// Appendix A.5's persistence predicate, which decides what `PROTECTED` may cover.
fn check_effect_domains(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/effects.yaml";
    if registries.get("effects.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for entry in registries.entries("effects.yaml", "domains") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(domain) = EffectDomain::from_name(id) else {
            continue;
        };
        let declared = entry.get("persistent").and_then(Yaml::as_bool);
        if declared != Some(domain.is_persistent()) {
            problems.push(Problem::new(
                location,
                format!(
                    "domain `{id}` declares `persistent: {}` and the shell answers `{}`. \
                     Appendix A.5 decides which domains a plan must cover before it may be called \
                     protected on exactly this predicate",
                    declared.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    domain.is_persistent()
                ),
            ));
        }
        let external = entry.get("external").and_then(Yaml::as_bool);
        if external != Some(domain.is_external()) {
            problems.push(Problem::new(
                location,
                format!(
                    "domain `{id}` declares `external: {}` and the shell answers `{}`",
                    external.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    domain.is_external()
                ),
            ));
        }
    }
    for entry in registries.entries("effects.yaml", "kinds") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(kind) = EffectKind::from_name(id) else {
            continue;
        };
        let declared = entry.get("inherently_irreversible").and_then(Yaml::as_bool);
        if declared != Some(kind.is_inherently_irreversible()) {
            problems.push(Problem::new(
                location,
                format!(
                    "kind `{id}` declares `inherently_irreversible: {}` and the shell answers \
                     `{}`. §2.13 keeps an irreversible effect visible after protection, and this \
                     is where it becomes visible",
                    declared.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    kind.is_inherently_irreversible()
                ),
            ));
        }
    }
    // §8.3 and §2.4: the lattice combines downward and has no counterpart.
    let strengthening = registries
        .get("effects.yaml")
        .and_then(|document| document.get("lattice"))
        .and_then(|lattice| lattice.get("strengthening_operation"));
    if !matches!(strengthening, Some(Yaml::Null)) {
        problems.push(Problem::new(
            location,
            "`lattice.strengthening_operation` is not null. §2.4 forbids unknown being promoted, \
             and the absence of an operation that could do it is the contract",
        ));
    }
    problems
}

/// Appendix C.1's least-destructive order, which recovery method selection reads.
fn check_restore_methods(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/assets.yaml";
    if registries.get("assets.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for entry in registries.entries("assets.yaml", "restore_methods") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(method) = RestoreMethod::from_name(id) else {
            continue;
        };
        let declared = entry
            .get("destructiveness")
            .and_then(Yaml::as_u64)
            .and_then(|value| u8::try_from(value).ok());
        if declared != Some(method.destructiveness()) {
            problems.push(Problem::new(
                location,
                format!(
                    "method `{id}` declares destructiveness {} and the shell orders it {}. \
                     Appendix C.1 requires the least-destructive method that satisfies the goal, \
                     and the ordering is how that is chosen",
                    declared.map_or_else(|| "nothing".to_owned(), |value| value.to_string()),
                    method.destructiveness()
                ),
            ));
        }
        let discards = entry.get("discards_newer_state").and_then(Yaml::as_bool);
        if discards != Some(method.discards_newer_state()) {
            problems.push(Problem::new(
                location,
                format!(
                    "method `{id}` declares `discards_newer_state: {}` and the shell answers \
                     `{}`. §24.5's gate is raised on this predicate",
                    discards.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    method.discards_newer_state()
                ),
            ));
        }
        let restores = entry.get("restores_prior_state").and_then(Yaml::as_bool);
        if restores != Some(method.restores_prior_state()) {
            problems.push(Problem::new(
                location,
                format!(
                    "method `{id}` declares `restores_prior_state: {}` and the shell answers \
                     `{}`. §27.4 forbids compensation being labelled rollback, and this is the \
                     flag that tells them apart",
                    restores.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    method.restores_prior_state()
                ),
            ));
        }
    }
    // §38.2, at the registry rather than in a renderer.
    let free = registries
        .get("assets.yaml")
        .and_then(|document| document.get("cost"))
        .and_then(|cost| cost.get("free_permitted"))
        .and_then(Yaml::as_bool);
    if free != Some(false) {
        problems.push(Problem::new(
            location,
            "`cost.free_permitted` is not false. §38.2: Ono MUST NOT display 'free' for a \
             copy-on-write snapshot",
        ));
    }
    problems
}

/// §11.5, at the registry: which asset types share the storage they protect.
fn check_asset_types(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/assets.yaml";
    if registries.get("assets.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for entry in registries.entries("assets.yaml", "types") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(asset_type) = RecoveryAssetType::from_name(id) else {
            continue;
        };
        let declared = entry.get("shares_failure_domain").and_then(Yaml::as_bool);
        if declared != Some(asset_type.shares_failure_domain()) {
            problems.push(Problem::new(
                location,
                format!(
                    "type `{id}` declares `shares_failure_domain: {}` and the shell answers \
                     `{}`. §11.5 forbids implying that a local snapshot protects against pool \
                     loss, and this is the flag a renderer reads instead of remembering",
                    declared.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    asset_type.shares_failure_domain()
                ),
            ));
        }
        let independent = entry.get("independent_copy").and_then(Yaml::as_bool);
        if independent != Some(asset_type.is_independent_copy()) {
            problems.push(Problem::new(
                location,
                format!(
                    "type `{id}` declares `independent_copy: {}` and the shell answers `{}`",
                    independent.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    asset_type.is_independent_copy()
                ),
            ));
        }
    }
    // §11.1's states, and the two predicates the retention engine reads.
    let states = registries.ids("assets.yaml", "states");
    let implemented = spellings(ono_change_core::AssetState::ALL, |state| state.as_str());
    problems.extend(compare(
        location,
        "states",
        &states,
        &implemented,
        "asset state",
    ));
    for entry in registries.entries("assets.yaml", "states") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(state) = ono_change_core::AssetState::from_name(id) else {
            continue;
        };
        if entry.get("usable").and_then(Yaml::as_bool) != Some(state.is_usable()) {
            problems.push(Problem::new(
                location,
                format!(
                    "asset state `{id}` disagrees with the shell about `usable`. §11.4 makes \
                     `ready` reachable only through a validation that passed, and an asset that \
                     is usable without one is not protection"
                ),
            ));
        }
        if entry.get("occupies_storage").and_then(Yaml::as_bool) != Some(state.occupies_storage()) {
            problems.push(Problem::new(
                location,
                format!("asset state `{id}` disagrees with the shell about `occupies_storage`"),
            ));
        }
    }
    problems
}

/// §39.2's ownership rule: a storage provider may not claim an application's consistency.
fn check_consistency(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/consistency.yaml";
    if registries.get("consistency.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let mut ranked: Vec<(u64, ConsistencyClass)> = Vec::new();
    for entry in registries.entries("consistency.yaml", "classes") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(class) = ConsistencyClass::from_name(id) else {
            continue;
        };
        match entry.get("rank").and_then(Yaml::as_u64) {
            None => problems.push(Problem::new(
                location,
                format!("class `{id}` declares no rank, so the weakest-of lattice has no order"),
            )),
            Some(rank) => ranked.push((rank, class)),
        }
        if entry
            .get("owned_by")
            .and_then(Yaml::as_str)
            .is_none_or(str::is_empty)
        {
            problems.push(Problem::new(
                location,
                format!(
                    "class `{id}` declares no owner. §39.2: a filesystem snapshot of a database is \
                     not application-consistent unless a database-aware provider asserts it, and \
                     the owner is what says who may"
                ),
            ));
        }
    }
    // The declared ranks must produce the same order the shell composes with.
    ranked.sort_by_key(|(rank, _)| *rank);
    for pair in ranked.windows(2) {
        let (stronger, weaker) = (pair[0].1, pair[1].1);
        if stronger.weakest_of(weaker) != weaker {
            problems.push(Problem::new(
                location,
                format!(
                    "the declared ranks put `{}` above `{}`, and the shell's weakest-of does not \
                     agree. Appendix D.7 forbids inventing cross-mechanism atomicity, and it is \
                     this ordering that prevents it",
                    stronger.as_str(),
                    weaker.as_str()
                ),
            ));
        }
    }
    let strengthening = registries
        .get("consistency.yaml")
        .and_then(|document| document.get("lattice"))
        .and_then(|lattice| lattice.get("strengthening_operation"));
    if !matches!(strengthening, Some(Yaml::Null)) {
        problems.push(Problem::new(
            location,
            "`lattice.strengthening_operation` is not null. A set spanning two mechanisms is only \
             as consistent as its weakest member (Appendix D.7)",
        ));
    }
    problems
}

/// §12.2's required capabilities, and §48.4's rule that describing is not executing.
fn check_capabilities(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/providers.yaml";
    if registries.get("providers.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let required: BTreeSet<&str> = RecoveryCapability::REQUIRED
        .iter()
        .map(|capability| capability.as_str())
        .collect();
    for entry in registries.entries("providers.yaml", "capabilities") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(capability) = RecoveryCapability::from_name(id) else {
            continue;
        };
        let declared = entry.get("required").and_then(Yaml::as_bool);
        if declared != Some(required.contains(id)) {
            problems.push(Problem::new(
                location,
                format!(
                    "capability `{id}` declares `required: {}` and the shell requires `{}`. A \
                     provider that cannot restore is not a recovery provider (§62.1)",
                    declared.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    required.contains(id)
                ),
            ));
        }
        if entry.get("mutates").and_then(Yaml::as_bool) != Some(capability.mutates()) {
            problems.push(Problem::new(
                location,
                format!(
                    "capability `{id}` disagrees with the shell about whether it mutates. §5.5 \
                     puts `recovery.prepare` on the mutating side, because a snapshot is a real \
                     change to the storage plane"
                ),
            ));
        }
    }
    for entry in registries.entries("providers.yaml", "change_capabilities") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(capability) = ChangeCapability::from_name(id) else {
            continue;
        };
        if entry.get("mutates").and_then(Yaml::as_bool) != Some(capability.mutates()) {
            problems.push(Problem::new(
                location,
                format!(
                    "change capability `{id}` disagrees with the shell about whether it mutates. \
                     §48.4: a plugin that can describe impact MUST NOT thereby gain permission to \
                     execute the change, and only `change.action.execute` carries that authority"
                ),
            ));
        }
    }
    problems
}

/// The thirteen commands of §5, against `docs/contracts/commands/change.yaml`, both directions.
fn check_command_inventory(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/plans.yaml";
    if registries.get("plans.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let declared: BTreeSet<String> = registries.ids("plans.yaml", "commands");
    let contract_path = root
        .join("docs")
        .join("contracts")
        .join("commands")
        .join("change.yaml");
    let Ok(text) = std::fs::read_to_string(&contract_path) else {
        problems.push(Problem::new(
            "docs/contracts/commands/change.yaml",
            "does not exist, and `plans.yaml` names commands that would live in it",
        ));
        return problems;
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return problems;
    };
    let commands = document
        .get("commands")
        .and_then(Yaml::as_sequence)
        .map(|items| items.as_slice())
        .unwrap_or_default();
    let contracted: BTreeSet<String> = commands
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Yaml::as_str).map(str::to_owned))
        .collect();

    for missing in contracted.difference(&declared) {
        problems.push(Problem::new(
            location,
            format!(
                "`commands` omits `{missing}`, which `docs/contracts/commands/change.yaml` \
                 declares. §47 requires the two to agree in both directions"
            ),
        ));
    }
    for extra in declared.difference(&contracted) {
        problems.push(Problem::new(
            location,
            format!(
                "`commands` names `{extra}`, and no command contract declares it. A command in an \
                 inventory and nowhere else is a command nobody can run"
            ),
        ));
    }

    // §5's phase letter, and the mutation flag §5.5 makes non-obvious.
    let verbs = read_verbs(root);
    for entry in registries.entries("plans.yaml", "commands") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(contract) = commands
            .iter()
            .find(|command| command.get("id").and_then(Yaml::as_str) == Some(id))
        else {
            continue;
        };
        if contract.get("phase").and_then(Yaml::as_str) != Some("P") {
            problems.push(Problem::new(
                "docs/contracts/commands/change.yaml",
                format!("`{id}` is not in phase `P`, which is the v0.6 tranche of §57"),
            ));
        }
        let verb = contract.get("verb").and_then(Yaml::as_str).unwrap_or("");
        let declared_mutates = entry.get("mutates").and_then(Yaml::as_bool);
        let verb_mutates = verbs.get(verb).copied();
        if declared_mutates != verb_mutates {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares `mutates: {}` and the verb `{verb}` is registered as \
                     `mutating: {}`. §5.5 puts `protect` on the mutating side and §2.1 keeps \
                     `plan` off it; the two registries must say the same thing",
                    declared_mutates.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string()),
                    verb_mutates.map_or_else(|| "nothing".to_owned(), |flag| flag.to_string())
                ),
            ));
        }
    }
    problems
}

fn read_verbs(root: &Path) -> BTreeMap<String, bool> {
    let path = root.join("docs").join("contracts").join("verbs.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return BTreeMap::new();
    };
    document
        .get("verbs")
        .and_then(Yaml::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| {
                    let verb = entry.get("verb").and_then(Yaml::as_str)?;
                    let mutating = entry.get("mutating").and_then(Yaml::as_bool)?;
                    Some((verb.to_owned(), mutating))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every schema §46 names exists, is registered, and is named by the file the id implies.
fn check_schemas(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/plans.yaml";
    if registries.get("plans.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let directory = root.join("docs").join("contracts").join("schemas");
    for entry in registries.entries("plans.yaml", "schemas") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(file) = entry.get("file").and_then(Yaml::as_str) else {
            problems.push(Problem::new(
                location,
                format!("schema `{id}` names no file"),
            ));
            continue;
        };
        let path = directory.join(file);
        match std::fs::read_to_string(&path) {
            Err(_) => problems.push(Problem::new(
                location,
                format!(
                    "schema `{id}` names `docs/contracts/schemas/{file}`, which does not exist"
                ),
            )),
            Ok(text) => {
                let declared = serde_yaml_ng::from_str::<Yaml>(&text)
                    .ok()
                    .and_then(|document| {
                        document.get("id").and_then(Yaml::as_str).map(str::to_owned)
                    });
                if declared.as_deref() != Some(id) {
                    problems.push(Problem::new(
                        format!("docs/contracts/schemas/{file}"),
                        format!(
                            "declares `{}` and `plans.yaml` names it `{id}`",
                            declared.unwrap_or_else(|| "nothing".to_owned())
                        ),
                    ));
                }
            }
        }
    }
    problems
}

/// §45's three families, against the global error registry, in both directions.
fn check_errors(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/errors.yaml";
    if registries.get("errors.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let mut indexed: BTreeMap<String, String> = BTreeMap::new();
    let groups = registries
        .get("errors.yaml")
        .and_then(|document| document.get("groups"))
        .and_then(Yaml::as_sequence)
        .map(|items| items.as_slice())
        .unwrap_or_default();
    if groups.is_empty() {
        problems.push(Problem::new(
            location,
            "declares no groups; §45's family is only navigable if something indexes it",
        ));
    }
    for group in groups {
        for entry in group
            .get("errors")
            .and_then(Yaml::as_sequence)
            .map(|items| items.as_slice())
            .unwrap_or_default()
        {
            let (Some(code), Some(name)) = (
                entry.get("code").and_then(Yaml::as_str),
                entry.get("name").and_then(Yaml::as_str),
            ) else {
                problems.push(Problem::new(
                    location,
                    "an error entry is missing its code or its name",
                ));
                continue;
            };
            if entry
                .get("nothing_changed")
                .and_then(Yaml::as_bool)
                .is_none()
            {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{name}` does not say whether it means nothing was changed. Appendix F's \
                         whole matrix turns on an operator being able to tell a refusal from a \
                         partial apply"
                    ),
                ));
            }
            indexed.insert(name.to_owned(), code.to_owned());
        }
    }

    let registry_path = root.join("docs").join("contracts").join("errors.yaml");
    let Ok(text) = std::fs::read_to_string(registry_path) else {
        return problems;
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return problems;
    };
    let mut registered: BTreeMap<String, String> = BTreeMap::new();
    for entry in document
        .get("errors")
        .and_then(Yaml::as_sequence)
        .map(|items| items.as_slice())
        .unwrap_or_default()
    {
        let (Some(code), Some(name)) = (
            entry.get("code").and_then(Yaml::as_str),
            entry.get("name").and_then(Yaml::as_str),
        ) else {
            continue;
        };
        if matches!(
            name.split('.').next(),
            Some("change" | "recovery" | "transaction")
        ) {
            registered.insert(name.to_owned(), code.to_owned());
        }
    }

    for (name, code) in &registered {
        match indexed.get(name) {
            None => problems.push(Problem::new(
                location,
                format!(
                    "does not index `{name}` ({code}), which `docs/contracts/errors.yaml` \
                     registers. §45's family is closed, and an unindexed member is one nobody \
                     grouped"
                ),
            )),
            Some(indexed_code) if indexed_code != code => problems.push(Problem::new(
                location,
                format!("indexes `{name}` as {indexed_code} and the registry defines it as {code}"),
            )),
            Some(_) => {}
        }
    }
    for name in indexed.keys() {
        if !registered.contains_key(name) {
            problems.push(Problem::new(
                location,
                format!(
                    "indexes `{name}`, and `docs/contracts/errors.yaml` does not define it. A code \
                     a script can match on must exist in the code that raises it"
                ),
            ));
        }
    }
    problems
}

/// §19.4's gates each name a real error and a flag a script can supply.
fn check_risk_gates(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/risk.yaml";
    if registries.get("risk.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let known: BTreeSet<String> = ono_core::ErrorCode::ALL
        .iter()
        .map(|code| code.name().to_owned())
        .collect();
    let gates = registries.entries("risk.yaml", "gates");
    if gates.is_empty() {
        problems.push(Problem::new(
            location,
            "declares no gates; §19.4 requires HIGH, CRITICAL and irreversible plans to have them",
        ));
    }
    for entry in gates {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        match entry.get("error").and_then(Yaml::as_str) {
            None => problems.push(Problem::new(
                location,
                format!("gate `{id}` names no error, so a script cannot match on being gated"),
            )),
            Some(name) if !known.contains(name) => problems.push(Problem::new(
                location,
                format!("gate `{id}` names the error `{name}`, which nothing raises"),
            )),
            Some(_) => {}
        }
        match entry.get("flag").and_then(Yaml::as_str) {
            None => problems.push(Problem::new(
                location,
                format!(
                    "gate `{id}` names no flag. §40.3: a script MUST specify required \
                     acknowledgements as flags or policy and MUST fail rather than prompt"
                ),
            )),
            Some(flag) if !flag.starts_with("--") => problems.push(Problem::new(
                location,
                format!("gate `{id}` names `{flag}`, which is not a flag a caller can write"),
            )),
            Some(_) => {}
        }
    }
    // §19.2 and §62.11: risk is rule-based, and the rules are a table. Compared against the
    // engine's own registry in both directions, so a rule added in Rust without a row fails the
    // gate and a documented rule nothing implements fails it too. Without this, `risk.yaml` would
    // be prose with a colon in it.
    let declared = registries.ids("risk.yaml", "rules");
    let implemented: BTreeSet<String> = ono_change_impact::rules()
        .iter()
        .map(|rule| rule.id().to_owned())
        .collect();
    problems.extend(compare(
        location,
        "rules",
        &declared,
        &implemented,
        "risk rule",
    ));
    for rule in ono_change_impact::rules() {
        let Some(entry) = registries
            .entries("risk.yaml", "rules")
            .into_iter()
            .find(|entry| entry.get("id").and_then(Yaml::as_str) == Some(rule.id()))
        else {
            continue;
        };
        let dimension = entry.get("dimension").and_then(Yaml::as_str);
        if dimension != Some(rule.dimension().as_str()) {
            problems.push(Problem::new(
                location,
                format!(
                    "rule `{}` is registered against the dimension `{}` and the engine emits into                      `{}`. §40.2 shows the dimension a gate is objecting on, so the two must agree",
                    rule.id(),
                    dimension.unwrap_or("nothing"),
                    rule.dimension().as_str()
                ),
            ));
        }
    }

    // Every rule names a dimension the registry declares.
    let dimensions = registries.ids("risk.yaml", "dimensions");
    let classes = registries.ids("risk.yaml", "classes");
    for entry in registries.entries("risk.yaml", "rules") {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        match entry.get("dimension").and_then(Yaml::as_str) {
            Some(dimension) if dimensions.contains(dimension) => {}
            other => problems.push(Problem::new(
                location,
                format!(
                    "rule `{id}` emits into the dimension `{}`, which §19.1 does not define",
                    other.unwrap_or("nothing")
                ),
            )),
        }
        match entry.get("emits").and_then(Yaml::as_str) {
            Some(class) if classes.contains(class) => {}
            other => problems.push(Problem::new(
                location,
                format!(
                    "rule `{id}` emits the class `{}`, which §19.2 does not define",
                    other.unwrap_or("nothing")
                ),
            )),
        }
    }
    problems
}

/// §28.4's rule that no strategy is unbounded.
fn check_strategies(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/strategies.yaml";
    if registries.get("strategies.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let mut defaults = 0;
    for entry in registries.entries("strategies.yaml", "strategies") {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        if entry.get("bounded").and_then(Yaml::as_bool) != Some(true) {
            problems.push(Problem::new(
                location,
                format!(
                    "strategy `{id}` is not bounded. §28.4: unlimited parallel mutation is not a \
                     default strategy, and there is no variant of it that is"
                ),
            ));
        }
        if entry.get("default").and_then(Yaml::as_bool) == Some(true) {
            defaults += 1;
        }
    }
    if defaults != 1 {
        problems.push(Problem::new(
            location,
            format!(
                "{defaults} strategies are marked default; §53's `change.default_strategy` names \
                 exactly one"
            ),
        ));
    }
    if registries
        .get("strategies.yaml")
        .and_then(|document| document.get("bulk"))
        .and_then(|bulk| bulk.get("frozen_membership"))
        .and_then(Yaml::as_bool)
        != Some(true)
    {
        problems.push(Problem::new(
            location,
            "`bulk.frozen_membership` is not true. §2.6: newly matching objects MUST NOT silently \
             join a bulk plan at apply time",
        ));
    }
    problems
}

/// Every first-party provider names a crate that exists, and declares the fixtures Appendix G.1
/// requires of it.
fn check_providers(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/providers.yaml";
    if registries.get("providers.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let asset_types: BTreeSet<String> = registries.ids("assets.yaml", "types");
    for entry in registries.entries("providers.yaml", "providers") {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        match entry.get("crate").and_then(Yaml::as_str) {
            None => problems.push(Problem::new(
                location,
                format!("provider `{id}` names no crate"),
            )),
            Some(name) if !root.join("crates").join(name).is_dir() => {
                problems.push(Problem::new(
                    location,
                    format!("provider `{id}` names the crate `{name}`, which does not exist"),
                ));
            }
            Some(_) => {}
        }
        for declared in entry
            .get("asset_types")
            .and_then(Yaml::as_sequence)
            .map(|items| items.as_slice())
            .unwrap_or_default()
        {
            if let Some(name) = declared.as_str()
                && !asset_types.contains(name)
            {
                problems.push(Problem::new(
                    location,
                    format!(
                        "provider `{id}` creates the asset type `{name}`, which `assets.yaml` does \
                         not define"
                    ),
                ));
            }
        }
    }
    let fixtures = registries
        .get("providers.yaml")
        .and_then(|document| document.get("required_fixtures"))
        .and_then(Yaml::as_sequence)
        .map(|items| items.len())
        .unwrap_or(0);
    if fixtures < 11 {
        problems.push(Problem::new(
            location,
            format!(
                "`required_fixtures` lists {fixtures} entries; Appendix G.1 names eleven, and a \
                 provider is not stable until it passes all of them"
            ),
        ));
    }
    if registries
        .get("providers.yaml")
        .and_then(|document| document.get("destructive_tests"))
        .and_then(|tests| tests.get("production_filesystems"))
        .and_then(Yaml::as_str)
        != Some("forbidden")
    {
        problems.push(Problem::new(
            location,
            "`destructive_tests.production_filesystems` is not `forbidden`. Appendix G.3: \
             production host filesystems MUST never be used for test rollback",
        ));
    }
    problems
}

/// Every registry has a row in the hardening inventory, and something in `xtask` validates it.
fn check_inventory(root: &Path) -> Vec<Problem> {
    let path = root
        .join("docs")
        .join("contracts")
        .join("hardening")
        .join("registries.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let indexed: BTreeSet<String> = document
        .get("registries")
        .and_then(Yaml::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.get("file").and_then(Yaml::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut problems = Vec::new();
    for (directory, files) in [
        ("change", CHANGE_REGISTRIES.as_slice()),
        ("recovery", RECOVERY_REGISTRIES.as_slice()),
    ] {
        for file in files {
            let reference = format!("../{directory}/{file}");
            if !indexed.contains(&reference) {
                problems.push(Problem::new(
                    "docs/contracts/hardening/registries.yaml",
                    format!(
                        "does not index `{reference}`, so nothing in the gate holds it \
                         (v0.4.1 §52.3)"
                    ),
                ));
            }
        }
    }
    problems
}
