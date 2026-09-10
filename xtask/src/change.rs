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
    EffectKind, EquivalenceDomain, EquivalenceState, Idempotency, ImpactClass, LifecycleEvent,
    PlanId, PlanState, PreconditionKind, ProtectionLevel, ProtectionMode, RecoveryAssetType,
    RecoveryCapability, RecoveryCost, RecoveryObjective, RecoveryValidation, RestoreMethod,
    RiskClass, RiskDimension, StrategyKind, Verdict, VerificationClass, VerificationContract,
    VerificationResult, VerificationSet, VerificationStatus,
};
use ono_change_protection::policy::{
    CostLimits, FreeSpaceFloor, Profile, effective_mode, mode_rank,
};
use ono_change_protection::settings::{self, ChangeSettings};
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
    problems.extend(check_execution_methods(root, &registries));
    problems.extend(check_effect_domains(root, &registries));
    problems.extend(check_restore_methods(&registries));
    problems.extend(check_asset_types(&registries));
    problems.extend(check_consistency(&registries));
    problems.extend(check_consistency_ownership(root, &registries));
    problems.extend(check_quiesce(root, &registries));
    problems.extend(check_capabilities(&registries));
    problems.extend(check_command_inventory(root, &registries));
    problems.extend(check_schemas(root, &registries));
    problems.extend(check_errors(root, &registries));
    problems.extend(check_risk_gates(&registries));
    problems.extend(check_strategies(&registries));
    problems.extend(check_privacy(&registries));
    problems.extend(check_providers(root, &registries));
    problems.extend(check_provider_capabilities(root, &registries));
    problems.extend(check_shipped_providers(root, &registries));
    problems.extend(check_settings(&registries));
    problems.extend(check_verification(&registries));
    problems.extend(check_policies(&registries));
    problems.extend(check_assets(&registries));
    problems.extend(check_plannable_operations(root));
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

/// §2.17 and §12.3, at the registry: no execution method may admit a command line, and the rows
/// are exactly the variants of `ono_change_core::Execution`.
///
/// `Execution` carries data, so it has no `ALL`; its variants are read out of `ono-change-core`'s
/// source, in both directions. A method added to the shell without a row fails the gate, and a row
/// naming no variant fails it too.
fn check_execution_methods(root: &Path, registries: &Registries) -> Vec<Problem> {
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
    let source = root.join("crates").join("ono-change-core").join("src");
    if source.is_dir() {
        match rust_files(&source)
            .iter()
            .find_map(|(_, text)| enum_variants(text, "Execution"))
        {
            None => problems.push(Problem::new(
                "crates/ono-change-core/src",
                "declares no `pub enum Execution`, so `execution_methods` cannot be held against \
                 the ways the shell actually runs an action",
            )),
            Some(variants) => {
                let implemented: BTreeSet<String> = variants
                    .iter()
                    .map(String::as_str)
                    .map(kebab_name)
                    .collect();
                let declared = registries.ids("actions.yaml", "execution_methods");
                problems.extend(compare(
                    location,
                    "execution_methods",
                    &declared,
                    &implemented,
                    "execution method",
                ));
            }
        }
    }
    problems
}

/// Appendix A.5's persistence predicate, which decides what `PROTECTED` may cover, §2.13's
/// irreversibility, and §2.4's rule that the confidence lattice has no strengthening method.
///
/// The last is read out of `ono-change-core`'s source: `EffectConfidence::weakest_of` must exist,
/// and no other inherent method may take a second confidence or be named as a strengthening. The
/// type's derived `Ord` is not an inherent method, and this check does not see it.
fn check_effect_domains(root: &Path, registries: &Registries) -> Vec<Problem> {
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
    let source = root.join("crates").join("ono-change-core").join("src");
    if source.is_dir() {
        let mut methods = Vec::new();
        for (path, text) in rust_files(&source) {
            if let Some(declared) = inherent_methods(&text, "EffectConfidence") {
                let file = relative(root, &path);
                methods.extend(
                    declared
                        .into_iter()
                        .map(|(name, signature)| (file.clone(), name, signature)),
                );
            }
        }
        if !methods.iter().any(|(_, name, _)| name == "weakest_of") {
            problems.push(Problem::new(
                "crates/ono-change-core/src",
                "declares no `EffectConfidence::weakest_of`. §2.4 and §8.1 make it the lattice's \
                 one combining operation, and `effects.yaml`'s `lattice.operation` names it",
            ));
        }
        for (file, name, signature) in methods {
            if name == "weakest_of" {
                continue;
            }
            let parameters = signature
                .split_once('(')
                .map_or("", |(_, rest)| rest.split("->").next().unwrap_or(rest));
            let combines = parameters.contains("Self") || parameters.contains("EffectConfidence");
            let named = ["strong", "promot", "upgrad", "raise"]
                .iter()
                .any(|word| name.contains(word));
            if combines || named {
                problems.push(Problem::new(
                    file,
                    format!(
                        "`EffectConfidence::{name}` is an operation beside `weakest_of` that takes \
                         a second confidence or names a strengthening. §2.4 forbids unknown being \
                         promoted, and `effects.yaml`'s `strengthening_operation: null` promises \
                         the type offers no such method"
                    ),
                ));
            }
        }
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
                    "rule `{}` is registered against the dimension `{}` and the engine emits into \
                     `{}`. §40.2 shows the dimension a gate is objecting on, so the two must agree",
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
        // `RiskRuleSpec` exposes no class — each evaluation chooses one — so `emits` can only be
        // held to §19.2's vocabulary here, not to what the engine emits.
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

/// §44's six privacy rules each name the mechanism that answers them.
///
/// A privacy rule with no mechanism is a sentence somebody has to remember, and §44 is the part of
/// v0.6 where remembering is not enough: recovery assets may hold credentials, private keys,
/// database files and application secrets, and every one of the six rules is about something that
/// would leak silently.
fn check_privacy(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/policies.yaml";
    if registries.get("policies.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let rules = registries.entries("policies.yaml", "privacy");
    if rules.len() < 6 {
        problems.push(Problem::new(
            location,
            format!(
                "`privacy` lists {} rules; §44 states six, and a rule nobody wrote down is one \
                 nobody checks",
                rules.len()
            ),
        ));
    }
    for entry in rules {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        for key in ["rule", "mechanism"] {
            if entry
                .get(key)
                .and_then(Yaml::as_str)
                .is_none_or(str::is_empty)
            {
                problems.push(Problem::new(
                    location,
                    format!(
                        "privacy rule `{id}` states no `{key}`. §44's rules are about data that \
                         leaks silently, so each has to say what stops it"
                    ),
                ));
            }
        }
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

/// §6.1's plannable operations, against the command contracts they are the mutating half of.
///
/// §6.2's default is refusal, so the registry is the whole of what a shell may plan. A row whose
/// command id no contract declares is an operation nothing can resolve — it is dropped silently,
/// and the registry goes on describing it. A mutating command with no row is the other direction
/// and is not a defect: §6.2 refuses it deliberately, and the refusal names what is missing.
fn check_plannable_operations(root: &Path) -> Vec<Problem> {
    let location = "docs/contracts/change/actions.yaml";
    let path = root
        .join("docs")
        .join("contracts")
        .join("change")
        .join("actions.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let Ok(commands) = ono_command::CommandRegistry::load() else {
        return Vec::new();
    };
    let mutating = read_verbs(root);
    let mut problems = Vec::new();
    for entry in document
        .get("plannable_operations")
        .and_then(Yaml::as_sequence)
        .map(|items| items.as_slice())
        .unwrap_or_default()
    {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(contract) = commands.get(id) else {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` is declared plannable and no command contract declares it. §6.1 makes                      an operation plannable through its contract, and a row naming a command that                      does not exist is an operation nobody can reach"
                ),
            ));
            continue;
        };
        if mutating.get(contract.verb()) == Some(&false) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` is declared plannable and its verb does not mutate. §6.1's registry                      describes mutations; a query has no effects to declare and nothing to protect"
                ),
            ));
        }
        if entry
            .get("effects")
            .and_then(Yaml::as_sequence)
            .is_none_or(Vec::is_empty)
        {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares no effect. §6.1 requires the expected direct effects, and an                      operation with none is one a plan would show as changing nothing"
                ),
            ));
        }
        if entry
            .get("verification")
            .and_then(Yaml::as_sequence)
            .is_none_or(Vec::is_empty)
        {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares no verification. §23.1: every plan containing a MUTATE action                      MUST carry at least one contract, and a plan of this operation alone would                      have none"
                ),
            ));
        }
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

/// §53's reference configuration, against the settings the shell actually defaults to.
///
/// §53 ends with the sentence the whole file serves — configuration MUST NOT silently weaken
/// explicit plan requirements — and that is only checkable if the two lists of defaults agree.
/// So the registry's own defaults are fed through the reader the shell uses, and the rows
/// [`ChangeSettings::entries`] prints for the result must be [`ChangeSettings::defaults`]'s rows
/// exactly. Provenance — whether a value was written — is not part of the comparison. A registry that documents `require` where the shell
/// defaults to `prefer` would otherwise promise protection nobody gets.
fn check_settings(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/plans.yaml";
    if registries.get("plans.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let rows = registries.entries("plans.yaml", "settings");
    let declared: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get("key").and_then(Yaml::as_str))
        .collect();
    if declared != settings::KEYS {
        problems.push(Problem::new(
            location,
            format!(
                "`settings` lists {declared:?}; §53 defines {:?}, in that order. A key the shell \
                 reads and the registry omits is a setting nobody can discover",
                settings::KEYS
            ),
        ));
        return problems;
    }

    let mut values: BTreeMap<String, ono_value::Value> = BTreeMap::new();
    for row in &rows {
        let key = row.get("key").and_then(Yaml::as_str).unwrap_or_default();
        let kind = row.get("type").and_then(Yaml::as_str).unwrap_or_default();
        let Some(default) = row.get("default") else {
            problems.push(Problem::new(
                location,
                format!("`{key}` states no default, and §53 gives one for every key"),
            ));
            continue;
        };
        match setting_value(kind, default) {
            Some(value) => {
                values.insert(key.to_owned(), value);
            }
            None => problems.push(Problem::new(
                location,
                format!(
                    "`{key}` declares the type `{kind}` and the default `{default:?}`, which do \
                     not go together. A default the shell cannot read is a default it will not use"
                ),
            )),
        }
    }

    match ChangeSettings::from_settings(&|key| values.get(key).cloned()) {
        Err(error) => problems.push(Problem::new(
            location,
            format!(
                "the declared defaults are not readable by the shell: {}",
                error.message()
            ),
        )),
        Ok(read) => {
            // The key/value rows §53 prints, row by row. Where a value came from — whether the
            // operator wrote it — is provenance rather than configuration, and is not compared.
            let defaults = ChangeSettings::defaults().entries();
            let read = read.entries();
            if read != defaults {
                let differing: Vec<String> = read
                    .iter()
                    .filter(|row| !defaults.contains(row))
                    .map(|(key, value)| format!("`{key}` reads `{value}`"))
                    .collect();
                problems.push(Problem::new(
                    location,
                    format!(
                        "the declared defaults do not produce the shell's own defaults. §53 prints \
                         one reference configuration, and a registry that documents a different \
                         one makes every promise in it unverifiable. Differing: {}",
                        differing.join(", ")
                    ),
                ));
            }
        }
    }
    problems
}

/// One registry default read as the type the row declares.
fn setting_value(kind: &str, default: &Yaml) -> Option<ono_value::Value> {
    match (kind, default) {
        ("bool", Yaml::Bool(flag)) => Some(ono_value::Value::Bool(*flag)),
        ("int", Yaml::Number(number)) => number
            .as_i64()
            .map(|count| ono_value::Value::Int(i128::from(count))),
        ("string" | "duration", Yaml::String(text)) => Some(ono_value::Value::string(text)),
        _ => None,
    }
}

/// §23's verification model, against the code that decides what a check means.
///
/// §2.14 is the invariant: verification is separate from execution success. The registry says
/// which outcome each class produces and which outcomes count as a pass, and those are answers
/// [`VerificationSet::verdict`] gives — so they are asked of it here rather than trusted.
fn check_verification(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/change/verification.yaml";
    if registries.get("verification.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for (key, implemented, noun) in [
        (
            "verdicts",
            spellings(Verdict::ALL, Verdict::as_str),
            "verdict",
        ),
        (
            "equivalence_domains",
            spellings(EquivalenceDomain::ALL, EquivalenceDomain::as_str),
            "equivalence domain",
        ),
        (
            "equivalence_states",
            spellings(EquivalenceState::ALL, EquivalenceState::as_str),
            "equivalence state",
        ),
    ] {
        let declared = registries.ids("verification.yaml", key);
        problems.extend(compare(location, key, &declared, &implemented, noun));
    }

    // §23.2: what a failing or unanswered check of each class does to the plan.
    for entry in registries.entries("verification.yaml", "classes") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(class) = VerificationClass::from_name(id) else {
            continue;
        };
        for (field, status) in [
            ("on_failure", VerificationStatus::Failed),
            ("on_unknown", VerificationStatus::Unknown),
        ] {
            let Some(declared) = entry.get(field).and_then(Yaml::as_str) else {
                problems.push(Problem::new(
                    location,
                    format!("class `{id}` does not say what `{field}` means for the plan (§23.2)"),
                ));
                continue;
            };
            let produced = verdict_of(class, status);
            if declared != produced.as_str() {
                problems.push(Problem::new(
                    location,
                    format!(
                        "class `{id}` declares `{field}: {declared}` and the shell produces \
                         `{produced}`. §23.2 fixes this mapping, and a registry that disagrees \
                         with it describes a plan outcome nobody will see"
                    ),
                ));
            }
        }
    }

    // §23.3 and §2.4: a check that was not answered has not passed.
    for entry in registries.entries("verification.yaml", "statuses") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(status) = VerificationStatus::from_name(id) else {
            continue;
        };
        let declared = entry
            .get("counts_as_pass")
            .and_then(Yaml::as_bool)
            .unwrap_or(false);
        let passes = verdict_of(VerificationClass::Required, status) == Verdict::Verified;
        if declared != passes {
            problems.push(Problem::new(
                location,
                format!(
                    "status `{id}` declares `counts_as_pass: {declared}` and a required check in \
                     that status composes to `{}`. §23.5 forbids treating a timeout as success, \
                     and this is the row that would authorise it",
                    verdict_of(VerificationClass::Required, status)
                ),
            ));
        }
    }

    problems.extend(check_timeouts(registries, location));
    problems
}

/// The verdict one result of this class and status composes to (§23.2, §4.8).
fn verdict_of(class: VerificationClass, status: VerificationStatus) -> Verdict {
    let plan = PlanId::of("xtask", "verification", "0");
    let contract = VerificationContract::new(&plan, class, "subject", "expression");
    let result = VerificationResult::new(plan, &contract, status, jiff::Timestamp::UNIX_EPOCH);
    VerificationSet::verdict(std::slice::from_ref(&result))
}

/// §23.5's rule: verification contracts have explicit timeout semantics and never wait forever.
fn check_timeouts(registries: &Registries, location: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(timeouts) = registries
        .get("verification.yaml")
        .and_then(|document| document.get("timeouts"))
    else {
        problems.push(Problem::new(
            location,
            "declares no `timeouts`. §23.5 requires explicit timeout semantics, and a registry \
             silent about them cannot be held to it",
        ));
        return problems;
    };
    for (field, wanted) in [("required", true), ("unbounded_permitted", false)] {
        if timeouts.get(field).and_then(Yaml::as_bool) != Some(wanted) {
            problems.push(Problem::new(
                location,
                format!("`timeouts.{field}` is not `{wanted}`. §23.5 forbids infinite waiting"),
            ));
        }
    }

    // The contract type has no absent timeout: this binding fails to compile if it grows one, and
    // that is the checkable form of "no contract can be built without a timeout".
    let plan = PlanId::of("xtask", "verification", "0");
    let contract =
        VerificationContract::new(&plan, VerificationClass::Required, "subject", "expression");
    let timeout: std::time::Duration = contract.timeout();
    let declared = timeouts.get("default").and_then(Yaml::as_str).unwrap_or("");
    if declared != compact_seconds(timeout) {
        problems.push(Problem::new(
            location,
            format!(
                "`timeouts.default` is `{declared}` and a contract built without one gets {}. \
                 §23.5's default is the timeout an operator inherits by saying nothing",
                compact_seconds(timeout)
            ),
        ));
    }
    if timeout.is_zero() {
        problems.push(Problem::new(
            "crates/ono-change-core/src/verification.rs",
            "the default verification timeout is zero, so every check times out before it runs",
        ));
    }

    // §23.1: a plan that mutates and cannot be verified never seals.
    if let Some(minimum) = registries
        .get("verification.yaml")
        .and_then(|document| document.get("minimum"))
    {
        let declared = minimum.get("error").and_then(Yaml::as_str).unwrap_or("");
        if ono_core::ErrorCode::from_name(declared).is_none() {
            problems.push(Problem::new(
                location,
                format!(
                    "`minimum.error` is `{declared}`, which is not an error the shell can raise. \
                     §23.1's refusal has to exist for the rule to be enforceable"
                ),
            ));
        }
    }
    problems
}

/// A duration in the `30s` form §53 and §23.5 write.
fn compact_seconds(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    if seconds > 0 && seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds > 0 && seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

/// Appendix H's profiles, against the presets the shell expands them to.
///
/// Appendix H.5 is the rule that keeps a profile from being a back door: a plan may impose
/// stricter requirements than a profile, and a profile MUST NOT weaken a provider-declared safety
/// constraint. So every profile is compared against §53's defaults in the direction that matters —
/// a profile may tighten and may not loosen — and the expansion the shell shows an operator must
/// be the one the registry documents.
fn check_policies(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/policies.yaml";
    if registries.get("policies.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let declared: BTreeSet<String> = registries.ids("policies.yaml", "profiles");
    let implemented: BTreeSet<String> = spellings(Profile::ALL, Profile::as_str);
    problems.extend(compare(
        location,
        "profiles",
        &declared,
        &implemented,
        "protection profile",
    ));

    let defaults = ChangeSettings::defaults();
    for entry in registries.entries("policies.yaml", "profiles") {
        let Some(id) = entry.get("id").and_then(Yaml::as_str) else {
            continue;
        };
        let Some(profile) = Profile::from_name(id) else {
            continue;
        };
        let expansion: BTreeMap<&str, String> = profile.settings().into_iter().collect();

        // Appendix H: a profile expands to inspectable settings, and these are the settings.
        for (field, shown) in [
            ("protection", "protection"),
            ("risk_gate", "risk gate"),
            ("strategy", "strategy"),
        ] {
            let Some(declared) = entry.get(field).and_then(Yaml::as_str) else {
                continue;
            };
            if expansion.get(shown).map(String::as_str) != Some(declared) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "profile `{id}` declares `{field}: {declared}` and expands to `{}`. \
                         Appendix H forbids a profile hiding semantics, which is what a documented \
                         expansion nobody applies amounts to",
                        expansion.get(shown).map_or("nothing", String::as_str)
                    ),
                ));
            }
        }
        if entry.get("prompts").and_then(Yaml::as_bool) == Some(false) && profile.prompts() {
            problems.push(Problem::new(
                location,
                format!(
                    "profile `{id}` declares `prompts: false` and the shell still prompts under \
                     it. §17.4 and §40.3: a non-interactive run MUST NOT stop for a question"
                ),
            ));
        }

        // Appendix H.5, in the only direction that is safe: tighter, never looser.
        if mode_rank(profile.mode()) < mode_rank(defaults.default_protection()) {
            problems.push(Problem::new(
                location,
                format!(
                    "profile `{id}` protects at `{}` and §53 defaults to `{}`. Appendix H.5 lets a \
                     profile tighten and never loosen",
                    profile.mode(),
                    defaults.default_protection()
                ),
            ));
        }
        if profile.retention() < defaults.retention() {
            problems.push(Problem::new(
                location,
                format!(
                    "profile `{id}` retains assets for less time than §53's default. §37.1's \
                     retention is what makes a plan recoverable, and shortening it is a loosening"
                ),
            ));
        }
        if !floor_is_at_least(
            profile.limits().min_filesystem_free(),
            defaults.min_filesystem_free(),
        ) {
            problems.push(Problem::new(
                location,
                format!(
                    "profile `{id}` sets a lower free-space floor than §53's default. \
                     Appendix D.3 fails closed below the floor, so lowering it is a loosening"
                ),
            ));
        }
        if entry.get("opaque_actions").and_then(Yaml::as_bool) != Some(false) {
            problems.push(Problem::new(
                location,
                format!(
                    "profile `{id}` does not declare `opaque_actions: false`. §6.2 keeps an \
                     arbitrary external command unplannable unless an operator says otherwise, and \
                     no profile may say it for them"
                ),
            ));
        }
    }

    problems.extend(check_authority(registries, location, &defaults));
    problems.extend(check_auto_recovery(registries, location));
    problems
}

/// Whether `floor` is at least as strict as `least`.
fn floor_is_at_least(floor: FreeSpaceFloor, least: FreeSpaceFloor) -> bool {
    floor.stricter_of(least) == floor
}

/// Appendix H.5 and §53's closing line: the strictest requirement in force is the one that applies.
fn check_authority(
    registries: &Registries,
    location: &str,
    defaults: &ChangeSettings,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(authority) = registries
        .get("policies.yaml")
        .and_then(|document| document.get("authority"))
    else {
        problems.push(Problem::new(
            location,
            "declares no `authority`. Appendix H.5 and §53 both say a stricter requirement wins, \
             and a registry silent about it cannot be held to either",
        ));
        return problems;
    };
    for (field, wanted) in [
        ("profile_may_weaken_provider_constraint", false),
        ("plan_may_be_stricter_than_profile", true),
        ("configuration_may_weaken_plan", false),
    ] {
        if authority.get(field).and_then(Yaml::as_bool) != Some(wanted) {
            problems.push(Problem::new(
                location,
                format!("`authority.{field}` is not `{wanted}` (Appendix H.5, §53)"),
            ));
        }
    }

    // §53's closing sentence, asked of the function that answers it: a plan that required more
    // than the configuration allows keeps its requirement.
    for configured in ProtectionMode::ALL {
        for requested in ProtectionMode::ALL {
            let effective = effective_mode(*configured, Some(*requested));
            if mode_rank(effective) < mode_rank(*requested) {
                problems.push(Problem::new(
                    "crates/ono-change-protection/src/policy.rs",
                    format!(
                        "a plan requiring `{requested}` under a `{configured}` configuration runs \
                         at `{effective}`. §53: configuration MUST NOT silently weaken explicit \
                         plan requirements"
                    ),
                ));
            }
        }
    }
    if effective_mode(defaults.default_protection(), None) != defaults.default_protection() {
        problems.push(Problem::new(
            "crates/ono-change-protection/src/policy.rs",
            "a plan that requires nothing does not run at the configured default (§17.1)",
        ));
    }
    problems
}

/// §26's auto-recovery policy: off by default, and rejected at seal when its conditions fail.
fn check_auto_recovery(registries: &Registries, location: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(auto) = registries
        .get("policies.yaml")
        .and_then(|document| document.get("auto_recovery"))
    else {
        problems.push(Problem::new(
            location,
            "declares no `auto_recovery`. §26.1 makes it off by default, which is a default \
             somebody has to be able to read",
        ));
        return problems;
    };
    // YAML reads a bare `off` as false, and either spelling means the same thing here.
    let default_is_off = matches!(auto.get("default"), Some(Yaml::Bool(false)))
        || auto.get("default").and_then(Yaml::as_str) == Some("off");
    if !default_is_off {
        problems.push(Problem::new(
            location,
            "`auto_recovery.default` is not `off`. §26.1: automatic recovery after a failed \
             verification is off by default, and §26.2 says why",
        ));
    }
    let conditions = auto
        .get("conditions")
        .and_then(Yaml::as_sequence)
        .map_or(0, Vec::len);
    if conditions != 6 {
        problems.push(Problem::new(
            location,
            format!(
                "`auto_recovery.conditions` lists {conditions} entries and §26.3 names six. They \
                 are conjunctive, so a missing one is a declaration that would be accepted"
            ),
        ));
    }
    if auto.get("rejected_at").and_then(Yaml::as_str) != Some("seal") {
        problems.push(Problem::new(
            location,
            "`auto_recovery.rejected_at` is not `seal`. §26.3: a declaration whose conditions do \
             not hold MUST be rejected at seal time, not at the moment it would have run",
        ));
    }
    let error = auto.get("error").and_then(Yaml::as_str).unwrap_or("");
    if ono_core::ErrorCode::from_name(error).is_none() {
        problems.push(Problem::new(
            location,
            format!("`auto_recovery.error` is `{error}`, which is not an error the shell raises"),
        ));
    }
    problems
}

/// §11.4's validation, §37's retention, §38's cost model and §38.3's limits.
///
/// The blocks in `assets.yaml` are the ones a renderer and a provider read instead of remembering
/// a rule. Each is therefore held against the type that answers it, so a block nobody implements
/// cannot go on describing behaviour the shell does not have.
fn check_assets(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/assets.yaml";
    if registries.get("assets.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();

    // §11.4: five checks, and each one really gates readiness.
    let declared: BTreeSet<String> = registries.ids("assets.yaml", "validation_checks");
    let implemented: BTreeSet<String> = RecoveryValidation::CHECKS
        .iter()
        .map(|check| (*check).to_owned())
        .collect();
    problems.extend(compare(
        location,
        "validation_checks",
        &declared,
        &implemented,
        "validation check",
    ));
    let at = jiff::Timestamp::UNIX_EPOCH;
    for check in &declared {
        let passed = RecoveryValidation::complete(at, "xtask").passed(check);
        let failed = RecoveryValidation::none(at, "xtask").passed(check);
        if passed != Some(true) || failed != Some(false) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{check}` is declared a §11.4 check and the shell does not record it. An \
                     asset reaches `ready` through a validation, and a check nobody makes is a \
                     check that always passes"
                ),
            ));
        }
    }

    // §38.1: six dimensions, and the cost model can carry every one of them.
    let dimensions: BTreeSet<String> = registries
        .get("assets.yaml")
        .and_then(|document| document.get("cost"))
        .and_then(|cost| cost.get("dimensions"))
        .and_then(Yaml::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(Yaml::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let carried: BTreeSet<String> = RecoveryCost::DIMENSIONS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    problems.extend(compare(
        location,
        "cost.dimensions",
        &dimensions,
        &carried,
        "cost dimension",
    ));
    if registries
        .get("assets.yaml")
        .and_then(|document| document.get("cost"))
        .and_then(|cost| cost.get("free_permitted"))
        .and_then(Yaml::as_bool)
        != Some(false)
    {
        problems.push(Problem::new(
            location,
            "`cost.free_permitted` is not `false`. §38.2: Ono MUST NOT display \"free\" for a \
             copy-on-write snapshot",
        ));
    }

    // §38.3: five bounds, each of which the policy type can actually apply.
    let limits: BTreeSet<String> = registries
        .entries("assets.yaml", "limits")
        .into_iter()
        .filter_map(|entry| entry.get("key").and_then(Yaml::as_str).map(str::to_owned))
        .collect();
    let bounds: BTreeSet<String> = CostLimits::KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    problems.extend(compare(
        location,
        "limits",
        &limits,
        &bounds,
        "policy limit",
    ));

    problems.extend(check_retention(registries, location));
    problems
}

/// §37's retention rules, against the states that carry them.
fn check_retention(registries: &Registries, location: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(retention) = registries
        .get("assets.yaml")
        .and_then(|document| document.get("retention"))
    else {
        problems.push(Problem::new(
            location,
            "declares no `retention`. §37.1 fixes a default and §37.2 exempts the states that \
             need it most; neither is checkable without it",
        ));
        return problems;
    };
    let declared = retention
        .get("default")
        .and_then(Yaml::as_str)
        .unwrap_or("");
    if declared != compact_seconds(ono_change_core::DEFAULT_RETENTION) {
        problems.push(Problem::new(
            location,
            format!(
                "`retention.default` is `{declared}` and the shell retains for {}. §37.1's \
                 twenty-four hours is what an operator inherits by saying nothing",
                compact_seconds(ono_change_core::DEFAULT_RETENTION)
            ),
        ));
    }
    if retention.get("after").and_then(Yaml::as_str) != Some("successful-verification") {
        problems.push(Problem::new(
            location,
            "`retention.after` is not `successful-verification`. §37.1 starts the clock there, \
             and starting it earlier deletes an asset a plan may still need",
        ));
    }

    // §37.2: the states ordinary success retention may not touch.
    let exempt: BTreeSet<String> = retention
        .get("failure_states_exempt")
        .and_then(Yaml::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(Yaml::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let held: BTreeSet<String> = PlanState::ALL
        .iter()
        .filter(|state| state.retains_assets_indefinitely())
        .map(|state| state.as_str().to_owned())
        .collect();
    problems.extend(compare(
        location,
        "retention.failure_states_exempt",
        &exempt,
        &held,
        "plan state",
    ));

    if retention
        .get("cleanup_preview_required_when")
        .and_then(Yaml::as_str)
        .is_none_or(str::is_empty)
    {
        problems.push(Problem::new(
            location,
            "`retention.cleanup_preview_required_when` is empty. §37.3 requires the preview \
             before deleting an asset whose removal changes recovery capability, and §2.15 makes \
             it a refusal",
        ));
    }
    problems
}

/// §39.2: a provider may only claim a consistency class its role owns.
///
/// The claim sites are read out of each first-party provider's own source, because that is where
/// the claim is made. §39.2's example is the whole point — a filesystem snapshot containing
/// PostgreSQL files is crash-consistent, and a storage provider writing
/// `ConsistencyClass::ApplicationConsistent` would be labelling it a guarantee it cannot give.
fn check_consistency_ownership(root: &Path, registries: &Registries) -> Vec<Problem> {
    let mut problems = Vec::new();
    if registries.get("consistency.yaml").is_none() || registries.get("providers.yaml").is_none() {
        return problems;
    }
    let owners: BTreeMap<String, String> = registries
        .entries("consistency.yaml", "classes")
        .into_iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Yaml::as_str)?;
            let owner = entry.get("owned_by").and_then(Yaml::as_str)?;
            Some((id.to_owned(), owner.to_owned()))
        })
        .collect();

    for entry in registries.entries("providers.yaml", "providers") {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        let Some(role) = entry.get("role").and_then(Yaml::as_str) else {
            problems.push(Problem::new(
                "docs/contracts/recovery/providers.yaml",
                format!(
                    "provider `{id}` declares no `role`, so §39.2 cannot be applied to it: \
                     nothing says which consistency claims it is entitled to make"
                ),
            ));
            continue;
        };
        let Some(name) = entry.get("crate").and_then(Yaml::as_str) else {
            continue;
        };
        let source = root.join("crates").join(name).join("src");
        for (file, class) in consistency_claims(&source) {
            let owner = owners.get(&class).map(String::as_str).unwrap_or("nobody");
            if owner != role && owner != "nobody" {
                problems.push(Problem::new(
                    file,
                    format!(
                        "claims `{class}` consistency, which `consistency.yaml` gives to \
                         `{owner}`. Provider `{id}` is a `{role}`, and §39.2 forbids it asserting \
                         a guarantee on another layer's behalf"
                    ),
                ));
            }
        }
    }
    problems
}

/// Every `at_consistency(ConsistencyClass::…)` under `source`, with the file it is written in.
fn consistency_claims(source: &Path) -> Vec<(String, String)> {
    let mut claims = Vec::new();
    let mut stack = vec![source.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let location = path.to_string_lossy().into_owned();
            for line in text.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                let Some(rest) = line.split_once("at_consistency(ConsistencyClass::") else {
                    continue;
                };
                let variant: String = rest
                    .1
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                if let Some(class) = ConsistencyClass::ALL
                    .iter()
                    .find(|class| variant_name(class.as_str()) == variant)
                {
                    claims.push((location.clone(), class.as_str().to_owned()));
                }
            }
        }
    }
    claims
}

/// The Rust variant spelling of a kebab-case vocabulary word.
fn variant_name(spelling: &str) -> String {
    spelling
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}

/// §39.3's quiesce protocol, and the rule that no first-party provider claims it.
///
/// No first-party provider implements it: §39.3 is a MAY for a provider that can make an
/// application-consistent claim, and none of ZFS, Btrfs or the file store can. So besides the
/// protocol's shape, the check reads every `crates/ono-recovery-*/src` and refuses a declaration of
/// `recovery.quiesce` — a capability declared without the protocol behind it is exactly the
/// overstatement §39.1 forbids — and of `recovery.transaction`, which §27.1 reserves for a provider
/// that states its own atomicity guarantee.
fn check_quiesce(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/consistency.yaml";
    let mut problems = first_party_optional_capabilities(root);
    let Some(protocol) = registries
        .get("consistency.yaml")
        .and_then(|document| document.get("quiesce_protocol"))
    else {
        return problems;
    };
    let steps = protocol
        .get("steps")
        .and_then(Yaml::as_sequence)
        .map_or(0, Vec::len);
    if steps != 5 {
        problems.push(Problem::new(
            location,
            format!("`quiesce_protocol.steps` lists {steps} steps and §39.3 names five"),
        ));
    }
    for field in [
        "bounded_window_required",
        "resume_on_failure_required",
        "resume_failure_is_critical",
    ] {
        if protocol.get(field).and_then(Yaml::as_bool) != Some(true) {
            problems.push(Problem::new(
                location,
                format!(
                    "`quiesce_protocol.{field}` is not `true`. §18.4 bounds the window, resumes \
                     the application when creation fails, and makes a failure to resume its own \
                     critical error"
                ),
            ));
        }
    }
    // §18.4's two refusals are two because a still-paused application is a different fact from a
    // snapshot that did not happen.
    for code in ["recovery.quiesce_failed", "recovery.resume_failed"] {
        if ono_core::ErrorCode::from_name(code).is_none() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{code}` is named here and the shell cannot raise it. §18.4 needs both, and \
                     needs them separate"
                ),
            ));
        }
    }
    problems
}

/// §48's capabilities, against the capability registry the broker enforces.
///
/// §48.3's names are only a boundary if the broker knows them. A capability declared here and
/// absent from `docs/contracts/capabilities.yaml` is a permission nobody can grant or deny, and a
/// recovery provider running under it would be running outside the capability system entirely.
fn check_provider_capabilities(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/providers.yaml";
    if registries.get("providers.yaml").is_none() {
        return Vec::new();
    }
    let path = root
        .join("docs")
        .join("contracts")
        .join("capabilities.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let mut known: BTreeSet<String> = BTreeSet::new();
    for key in [
        "provider_capabilities",
        "kuang_capabilities",
        "capabilities",
    ] {
        if let Some(items) = document.get(key).and_then(Yaml::as_sequence) {
            known.extend(
                items
                    .iter()
                    .filter_map(|entry| entry.get("id").and_then(Yaml::as_str))
                    .map(str::to_owned),
            );
        }
    }
    if known.is_empty() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    for key in ["capabilities", "change_capabilities"] {
        for id in registries.ids("providers.yaml", key) {
            if !known.contains(&id) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{key}` declares `{id}`, which `docs/contracts/capabilities.yaml` does \
                         not define. §48.3's names are a boundary only where the broker knows \
                         them, and a provider running under an unknown one runs under none"
                    ),
                ));
            }
        }
    }
    problems
}

/// The providers this release ships, against the rows that claim them.
///
/// A row here naming a provider nothing registers is a mechanism an operator would look for and
/// not find, and a provider crate that ships without a row is a mechanism nothing holds to
/// Appendix G's fixture set. The identity compared is each crate's own `PROVIDER_ID`, because that
/// is the string the registry keys on at runtime.
///
/// Appendix G.4 is held here too: `version_variance.degrade_to` must be `unsupported`, and the
/// crate of every provider whose row names a `tool` must, read as text, declare a non-empty
/// `VALIDATED_VERSIONS`, test a version against it, and answer `ProviderAvailability::Unsupported`.
fn check_shipped_providers(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/recovery/providers.yaml";
    if registries.get("providers.yaml").is_none() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let declared: BTreeSet<String> = registries.ids("providers.yaml", "providers");
    let mut shipped: BTreeSet<String> = BTreeSet::new();
    let crates = root.join("crates");
    let crates = crates.as_path();
    let Ok(entries) = std::fs::read_dir(crates) else {
        return problems;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("ono-recovery-") {
            continue;
        }
        match provider_id_of(&entry.path().join("src")) {
            None => problems.push(Problem::new(
                location,
                format!(
                    "the crate `{name}` ships a recovery provider and declares no `PROVIDER_ID`, \
                     so nothing can tell which row of this registry describes it"
                ),
            )),
            Some(id) => {
                shipped.insert(id);
            }
        }
    }
    problems.extend(compare(
        location,
        "providers",
        &declared,
        &shipped,
        "recovery provider",
    ));

    // Appendix G.4: a provider degrades rather than executing semantics it has not validated.
    if registries
        .get("providers.yaml")
        .and_then(|document| document.get("version_variance"))
        .and_then(|variance| variance.get("degrade_to"))
        .and_then(Yaml::as_str)
        != Some("unsupported")
    {
        problems.push(Problem::new(
            location,
            "`version_variance.degrade_to` is not `unsupported`. Appendix G.4 and §56.3 both \
             choose blocking over guessing when a fact could not be established",
        ));
    }
    for entry in registries.entries("providers.yaml", "providers") {
        let id = entry.get("id").and_then(Yaml::as_str).unwrap_or("");
        let (Some(tool), Some(name)) = (
            entry.get("tool").and_then(Yaml::as_str),
            entry.get("crate").and_then(Yaml::as_str),
        ) else {
            continue;
        };
        let source = crates.join(name).join("src");
        if !source.is_dir() {
            continue;
        }
        let text = rust_files(&source)
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n");
        let versions = text
            .split_once("pub const VALIDATED_VERSIONS: &[&str] = &[")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map_or(0, |(list, _)| {
                list.split(',').filter(|item| item.contains('"')).count()
            });
        if versions == 0 {
            problems.push(Problem::new(
                location,
                format!(
                    "provider `{id}` drives `{tool}`, and its crate `{name}` declares no non-empty \
                     `pub const VALIDATED_VERSIONS: &[&str]`. Appendix G.4: a provider tests the \
                     tool versions it supports, and a list nobody wrote is a version nobody tested"
                ),
            ));
        }
        if !text.contains("VALIDATED_VERSIONS.contains(")
            || !text.contains("ProviderAvailability::Unsupported")
        {
            problems.push(Problem::new(
                location,
                format!(
                    "provider `{id}`'s crate `{name}` never tests a version against \
                     `VALIDATED_VERSIONS` and answers `ProviderAvailability::Unsupported`. \
                     Appendix G.4: a provider degrades rather than executing semantics it has not \
                     validated"
                ),
            ));
        }
    }

    // Appendix G.2's truth tests: the layouts a provider must refuse false coverage on. The list
    // is open — G.2 calls its eight "examples" — so what is checked is that every id declared here
    // names a test that exists, by the `Appendix G.2 truth test: <id>` marker the test carries.
    let truth = registries.ids("providers.yaml", "truth_tests");
    if truth.len() < 8 {
        problems.push(Problem::new(
            location,
            format!(
                "`truth_tests` lists {} entries; Appendix G.2 names eight misleading layouts, and \
                 they are the tests that decide whether a provider can be trusted at all",
                truth.len()
            ),
        ));
    }
    let markers = truth_test_markers(crates);
    for id in &truth {
        if !markers.contains(id) {
            problems.push(Problem::new(
                location,
                format!(
                    "`truth_tests` declares `{id}` and no test carries the marker \
                     `Appendix G.2 truth test: {id}`. G.2 requires the misleading layout to be \
                     presented and the false coverage refused, and a row nobody tests is a claim \
                     that a provider was trusted for nothing"
                ),
            ));
        }
    }
    problems
}

/// Every `Appendix G.2 truth test: <id>` marker under the workspace's test directories.
fn truth_test_markers(crates: &Path) -> BTreeSet<String> {
    const MARKER: &str = "Appendix G.2 truth test: ";
    let mut found = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(crates) else {
        return found;
    };
    let mut stack: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("tests"))
        .collect();
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                if let Some((_, rest)) = line.split_once(MARKER) {
                    found.insert(rest.trim_end_matches('.').trim().to_owned());
                }
            }
        }
    }
    found
}

/// The `PROVIDER_ID` a provider crate declares, if it declares one.
fn provider_id_of(source: &Path) -> Option<String> {
    let mut stack = vec![source.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                if let Some(rest) = line.split_once("pub const PROVIDER_ID: &str = \"")
                    && let Some((id, _)) = rest.1.split_once('"')
                {
                    return Some(id.to_owned());
                }
            }
        }
    }
    None
}

/// Every `.rs` file under `source`, with its text, in path order.
fn rust_files(source: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut files = Vec::new();
    let mut stack = vec![source.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                files.push((path, text));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

/// `path` relative to the repository root, for a refusal's location.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// The variants of `pub enum {name}` in `text`, or `None` where `text` declares no such enum.
///
/// A variant is an identifier starting with a capital at the enum's own brace depth; comment and
/// attribute lines are skipped, so a brace in a doc comment does not move the depth.
fn enum_variants(text: &str, name: &str) -> Option<BTreeSet<String>> {
    let header = format!("pub enum {name} {{");
    let start = text.find(&header)?.saturating_add(header.len());
    let mut depth = 1_usize;
    let mut variants = BTreeSet::new();
    for line in text.get(start..)?.lines() {
        let code = line.trim();
        if code.starts_with("//") || code.starts_with("#[") {
            continue;
        }
        if depth == 1 {
            let ident: String = code
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if ident.starts_with(|c: char| c.is_ascii_uppercase()) {
                variants.insert(ident);
            }
        }
        for c in code.chars() {
            match c {
                '{' => depth = depth.saturating_add(1),
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(variants);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Every method of the inherent `impl {type_name} { … }` blocks in `text`, as its name and its
/// signature up to the opening brace, or `None` where `text` has no such block.
fn inherent_methods(text: &str, type_name: &str) -> Option<Vec<(String, String)>> {
    let header = format!("impl {type_name} {{");
    let mut methods = Vec::new();
    let mut found = false;
    let mut rest = text;
    while let Some(at) = rest.find(&header) {
        found = true;
        let body = rest.get(at.saturating_add(header.len())..)?;
        let mut depth = 1_usize;
        let mut consumed = 0_usize;
        let mut signature: Option<String> = None;
        for line in body.split_inclusive('\n') {
            consumed = consumed.saturating_add(line.len());
            let code = line.trim();
            if code.starts_with("//") || code.starts_with("#[") {
                continue;
            }
            if depth == 1
                && signature.is_none()
                && (code.starts_with("fn ") || code.contains(" fn "))
            {
                signature = Some(String::new());
            }
            if let Some(written) = signature.as_mut() {
                written.push_str(code);
                written.push(' ');
                if code.contains('{') || code.ends_with(';') {
                    if let Some(name) = written
                        .split("fn ")
                        .nth(1)
                        .and_then(|after| after.split('(').next())
                    {
                        methods.push((name.trim().to_owned(), written.clone()));
                    }
                    signature = None;
                }
            }
            for c in code.chars() {
                match c {
                    '{' => depth = depth.saturating_add(1),
                    '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            if depth == 0 {
                break;
            }
        }
        rest = body.get(consumed..).unwrap_or("");
    }
    found.then_some(methods)
}

/// The kebab-case vocabulary spelling of a Rust variant name — the inverse of [`variant_name`].
fn kebab_name(variant: &str) -> String {
    let mut spelling = String::new();
    for (index, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                spelling.push('-');
            }
            spelling.push(c.to_ascii_lowercase());
        } else {
            spelling.push(c);
        }
    }
    spelling
}

/// Every declaration of §12.2's two optional capabilities in a first-party provider's source.
///
/// Every non-comment line under `crates/ono-recovery-*/src` is read. `recovery.quiesce` needs
/// §39.3's protocol behind it and `recovery.transaction` §27.1's own atomicity guarantee, and no
/// first-party provider has either.
fn first_party_optional_capabilities(root: &Path) -> Vec<Problem> {
    const DECLARATIONS: [(&str, &str); 4] = [
        ("RecoveryCapability::Quiesce", "recovery.quiesce"),
        ("\"recovery.quiesce\"", "recovery.quiesce"),
        ("RecoveryCapability::Transaction", "recovery.transaction"),
        ("\"recovery.transaction\"", "recovery.transaction"),
    ];
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return problems;
    };
    let mut crates: Vec<std::path::PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("ono-recovery-")
        })
        .map(|entry| entry.path())
        .collect();
    crates.sort();
    for directory in crates {
        for (path, text) in rust_files(&directory.join("src")) {
            let mut named = BTreeSet::new();
            for line in text.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for (needle, capability) in DECLARATIONS {
                    if line.contains(needle) {
                        named.insert(capability);
                    }
                }
            }
            for capability in named {
                problems.push(Problem::new(
                    relative(root, &path),
                    format!(
                        "names `{capability}`, and no first-party recovery provider may declare \
                         it: `recovery.quiesce` needs §39.3's protocol behind it and \
                         `recovery.transaction` §27.1's own atomicity guarantee, and ZFS, Btrfs \
                         and the file store have neither. A capability declared without its \
                         mechanism is the overstatement §39.1 forbids"
                    ),
                ));
            }
        }
    }
    problems
}
