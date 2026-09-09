//! Drift between `docs/contracts/temporal/` and everything it claims about (spec v0.5 §36.4).
//!
//! §36 requires six version-controlled registries for the temporal and causal interface, and
//! §36.4 says what the gate must refuse. This module is that gate. It is a separate module from
//! [`crate::contracts`] because the temporal registries have a different shape from the command,
//! schema and verb registries: they describe a vocabulary and a policy rather than a call
//! surface, and most of what can go wrong with them is a cross-reference between two of the six
//! rather than a mismatch with a Rust type.
//!
//! §36.4's six rules, and where each lives here:
//!
//! | §36.4 | Function |
//! |---|---|
//! | a stable temporal command is missing from registry | `check_command_inventory` |
//! | the implementation emits an undocumented canonical event kind | `check_event_kinds` |
//! | a built-in causal rule is not registered | `check_causal_rules` |
//! | an error or schema referenced in a temporal contract does not exist | `check_references` |
//! | the default configuration differs from the registry | `check_settings` |
//! | a provider advertises a temporal capability absent from its contract metadata | `check_sources` |
//!
//! Beside them sits the internal consistency of the six files against each other, which §36.4
//! does not enumerate and which is where a registry actually rots: an inverse label that went
//! missing, a rule requiring an evidence strength nobody declares, a source naming an evidence
//! class that does not exist.
//!
//! A missing `docs/contracts/temporal/` directory is not a problem: registries arrive with the
//! phase that needs them (AGENTS.md §14). A directory that exists and is missing one of §36's
//! six required files is, because §36 lists them as required and a half-written contract set is
//! worse than none.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_yaml_ng::Value as Yaml;

pub use crate::scan::Problem;

/// §36's six required registries.
const REQUIRED_REGISTRIES: [&str; 6] = [
    "temporal.yaml",
    "events.yaml",
    "evidence.yaml",
    "causality.yaml",
    "sources.yaml",
    "recorder.yaml",
];

/// The seventeen canonical event kinds of v0.5 §6.1, in the order the specification lists them.
///
/// The list is closed: "Providers and plugins MAY define namespaced subtypes, but the top-level
/// semantics MUST map to one of these classes." `events.yaml` is compared against it in both
/// directions, so neither the registry nor a future `EventKind` enum can grow a kind alone.
pub const CANONICAL_EVENT_KINDS: [&str; 17] = [
    "object.observed",
    "object.appeared",
    "object.changed",
    "object.disappeared",
    "relation.added",
    "relation.removed",
    "action.requested",
    "action.authorized",
    "action.executed",
    "action.completed",
    "action.failed",
    "provider.event",
    "coverage.started",
    "coverage.ended",
    "checkpoint.created",
    "landmark.added",
    "landmark.removed",
];

/// The five evidence strengths of v0.5 §7.2, strongest first.
///
/// The order is the contract: `authoritative > asserted > derived > correlated > observational`,
/// and nothing may raise a strength (§7.2). `evidence.yaml` must declare them in this order, so
/// a reader of the file sees the ordering the type imposes rather than an alphabetised list that
/// happens to contain the same words.
pub const EVIDENCE_STRENGTHS: [&str; 5] = [
    "authoritative",
    "asserted",
    "derived",
    "correlated",
    "observational",
];

/// The nine evidence source classes of v0.5 §7.1.
pub const EVIDENCE_SOURCE_CLASSES: [&str; 9] = [
    "ono.session",
    "ono.recorder",
    "linux.procfs",
    "linux.netlink",
    "linux.systemd-dbus",
    "linux.journald",
    "adapter:<adapter-id>",
    "remote:<link-id>/<provider-id>",
    "kuang:<package-id>/<provider-id>",
];

/// The eight gap reasons of v0.5 §7.5.
pub const GAP_REASONS: [&str; 8] = [
    "not_recorded",
    "retention_expired",
    "provider_unavailable",
    "permission_denied",
    "source_disconnected",
    "clock_uncertain",
    "corrupt_segment",
    "unsupported",
];

/// The five causal relationship classes of v0.5 §15.1 with their inverse labels, and whether the
/// class is causal.
///
/// `causal` is the predicate a renderer keys its edge style on (§45.3) and the predicate that
/// decides whether causal wording is permitted at all (§15.6).
pub const RELATION_CLASSES: [(&str, &str, bool); 5] = [
    ("caused_by", "caused", true),
    ("triggered_by", "triggered", true),
    ("resulted_in", "result", true),
    ("correlated_with", "correlated_with", false),
    ("preceded_by", "followed_by", false),
];

/// The seven capability keys of `TemporalCapabilities` (v0.5 §21.1).
pub const TEMPORAL_CAPABILITY_KEYS: [&str; 7] = [
    "current_snapshot",
    "live_events",
    "historical_query",
    "exhaustive_events",
    "causal_tokens",
    "checkpointable",
    "retained_history",
];

/// The built-in causal rules v0.5 §15.2 and §15.8 require to be registered and inspectable.
///
/// This is the well-known list the registry is held against until the causal engine exposes its
/// own. When it does — as a `&'static [CausalRuleId]` of every rule the engine will run — this
/// constant is replaced by a read of it and the check binds to the implementation rather than to
/// a second copy of the specification (ADR-0627).
pub const BUILTIN_CAUSAL_RULES: [&str; 7] = [
    "ono.action-to-transaction",
    "ono.systemd-job-result",
    "ono.systemd-job-to-unit-state",
    "ono.process-parent",
    "ono.service-controls-process",
    "ono.action-launched-process",
    "ono.provider-causal-token",
];

/// The built-in correlation rules of v0.5 §15.5. Held the same way, and separately, because a
/// correlation rule may never emit a causal relation.
pub const BUILTIN_CORRELATION_RULES: [&str; 3] = [
    "ono.change-before-failure",
    "ono.resource-pressure-overlap",
    "ono.remote-endpoint-retry-spike",
];

/// The four words §15.6 forbids for a relation that is not causal.
pub const FORBIDDEN_CAUSAL_WORDS: [&str; 4] = ["because", "therefore", "led to", "caused"];

/// The two context modes of v0.5 §3.9.
const CONTEXT_MODES: [&str; 2] = ["present", "historical"];

/// The registries, parsed once, so every check reads the same document.
struct Registries {
    documents: BTreeMap<String, Yaml>,
    text: BTreeMap<String, String>,
}

impl Registries {
    fn get(&self, file: &str) -> Option<&Yaml> {
        self.documents.get(file)
    }
}

/// Checks `docs/contracts/temporal/` against itself, against the shared registries and against
/// the implementation (v0.5 §36.4).
///
/// Returns an empty vector when the contracts agree. A missing `docs/contracts/temporal/`
/// directory returns empty: registries arrive with the phase that needs them (AGENTS.md §14).
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("contracts").join("temporal");
    if !directory.is_dir() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    let mut documents = BTreeMap::new();
    let mut text = BTreeMap::new();

    for file in REQUIRED_REGISTRIES {
        let location = format!("docs/contracts/temporal/{file}");
        match std::fs::read_to_string(directory.join(file)) {
            Err(_) => problems.push(Problem::new(
                location,
                "does not exist; v0.5 §36 lists it as a required registry, and a half-written \
                 contract set makes a promise nobody can check",
            )),
            Ok(body) => match serde_yaml_ng::from_str::<Yaml>(&body) {
                Ok(document) => {
                    documents.insert(file.to_owned(), document);
                    text.insert(file.to_owned(), body);
                }
                Err(error) => problems.push(Problem::new(
                    location,
                    format!("is not valid YAML: {error}"),
                )),
            },
        }
    }

    let registries = Registries { documents, text };
    problems.extend(check_temporal(&registries));
    problems.extend(check_event_kinds(root, &registries));
    problems.extend(check_evidence(&registries));
    problems.extend(check_causal_rules(&registries));
    problems.extend(check_sources(root, &registries));
    problems.extend(check_recorder(&registries));
    problems.extend(check_settings(&registries));
    problems.extend(check_command_inventory(root, &registries));
    problems.extend(check_references(root, &registries));
    problems.extend(check_inventory(root));
    problems
}

/// `temporal.yaml`'s own vocabulary: the two context modes, the prompt markers, the read-only
/// policy's four classes and the seven capability keys of §21.1.
fn check_temporal(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/temporal.yaml";
    let Some(document) = registries.get("temporal.yaml") else {
        return Vec::new();
    };
    let mut problems = Vec::new();

    let modes: Vec<String> = sequence(document, "context_modes")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    if modes != CONTEXT_MODES {
        problems.push(Problem::new(
            location,
            format!(
                "declares the context modes {modes:?}; v0.5 §3.9 and §4 fix them as \
                 {CONTEXT_MODES:?}, and a session is in exactly one of them"
            ),
        ));
    }

    let markers: BTreeSet<String> = sequence(document, "prompt_markers")
        .into_iter()
        .filter_map(|row| string_at(row, "marker"))
        .collect();
    for marker in ["[PAST]", "[PAST?]"] {
        if !markers.contains(marker) {
            problems.push(Problem::new(
                location,
                format!("declares no `{marker}` prompt marker (v0.5 §4.6, §8.6)"),
            ));
        }
    }
    let past = sequence(document, "prompt_markers")
        .into_iter()
        .find(|row| string_at(row, "marker").as_deref() == Some("[PAST]"))
        .and_then(|row| string_at(row, "requires"));
    if past.as_deref() != Some("supported_coverage") {
        problems.push(Problem::new(
            location,
            "`[PAST]` must require `supported_coverage`; v0.5 §8.6: \"A prompt MUST NOT display \
             `[PAST]` merely because at least one event exists near that time.\"",
        ));
    }

    let covered: BTreeSet<String> = document
        .get("read_only_policy")
        .map(|policy| {
            sequence(policy, "covered_operations")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    for class in [
        "native_mutation",
        "kuang_mutation_tool",
        "remote_mutation",
        "shell_state_change",
    ] {
        if !covered.contains(class) {
            problems.push(Problem::new(
                location,
                format!(
                    "the read-only policy says nothing about `{class}`; v0.5 §4.7 names four \
                     classes of operation the rule applies to, and a class nobody wrote down is \
                     a class nobody guards"
                ),
            ));
        }
    }

    let keys: Vec<String> = sequence(document, "capabilities")
        .into_iter()
        .filter_map(|row| string_at(row, "key"))
        .collect();
    if keys != TEMPORAL_CAPABILITY_KEYS {
        problems.push(Problem::new(
            location,
            format!(
                "declares the temporal capability keys {keys:?}; v0.5 §21.1 fixes them as \
                 {TEMPORAL_CAPABILITY_KEYS:?}"
            ),
        ));
    }

    for window in [
        "timeline.default_window",
        "timeline.centered_half_window",
        "timeline.default_depth",
    ] {
        if !sequence(document, "default_windows")
            .into_iter()
            .any(|row| string_at(row, "id").as_deref() == Some(window))
        {
            problems.push(Problem::new(
                location,
                format!("declares no default window `{window}` (v0.5 §11.3, §11.8, §16.7)"),
            ));
        }
    }

    let forms: BTreeSet<String> = sequence(document, "time_selectors")
        .into_iter()
        .filter_map(|row| string_at(row, "form"))
        .collect();
    for form in [
        "absolute_rfc3339",
        "local_date_time",
        "local_time_today",
        "relative_past",
        "event_reference",
    ] {
        if !forms.contains(form) {
            problems.push(Problem::new(
                location,
                format!("declares no time selector form `{form}` (v0.5 §4.4)"),
            ));
        }
    }
    if !sequence(document, "time_selector_rules")
        .into_iter()
        .any(|row| string_at(row, "id").as_deref() == Some("no_future_relative"))
    {
        problems.push(Problem::new(
            location,
            "states no rule forbidding a future relative selector; v0.5 §4.4: \"Relative future \
             selectors are invalid for historical context.\"",
        ));
    }

    problems
}

/// `events.yaml`'s kinds against the canonical seventeen, in both directions (§36.4).
///
/// Two comparisons, and the registry has to satisfy both. [`CANONICAL_EVENT_KINDS`] is v0.5 §6.1
/// verbatim, so a registry that drifts from the specification fails whatever the code does; and
/// `ono_temporal_core::EventKind::ALL` is what the implementation will actually emit, so a kind
/// renamed in Rust and not here — or here and not in Rust — fails too. §36.4's second rule asks
/// for exactly the second of those: "the implementation emits an undocumented canonical event
/// kind".
fn check_event_kinds(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/events.yaml";
    let Some(document) = registries.get("events.yaml") else {
        return Vec::new();
    };
    let mut problems = Vec::new();

    let declared: BTreeSet<String> = sequence(document, "kinds")
        .into_iter()
        .filter_map(|row| string_at(row, "kind"))
        .collect();
    let canonical: BTreeSet<String> = CANONICAL_EVENT_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect();
    // The implementation's own vocabulary, which is the half §36.4 asks for. It agrees with
    // §6.1 today; comparing both ways means it cannot stop agreeing quietly.
    let implemented: BTreeSet<String> = ono_temporal_core::EventKind::ALL
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();
    for kind in implemented.difference(&declared) {
        problems.push(Problem::new(
            location,
            format!(
                "`ono_temporal_core::EventKind` carries `{kind}` and this registry omits it.                  §36.4: an implementation that emits an undocumented canonical event kind fails                  the gate"
            ),
        ));
    }
    for kind in declared.difference(&implemented) {
        problems.push(Problem::new(
            location,
            format!(
                "declares the event kind `{kind}`, which `ono_temporal_core::EventKind` does not                  carry. A kind nothing can emit is a contract nobody serves"
            ),
        ));
    }
    for kind in canonical.difference(&declared) {
        problems.push(Problem::new(
            location,
            format!("v0.5 §6.1 defines the event kind `{kind}` and this registry omits it"),
        ));
    }
    for kind in declared.difference(&canonical) {
        problems.push(Problem::new(
            location,
            format!(
                "declares the event kind `{kind}`, which v0.5 §6.1 does not define. The list is \
                 closed; a provider refinement goes in `subtype`"
            ),
        ));
    }

    for row in sequence(document, "kinds") {
        let Some(kind) = string_at(row, "kind") else {
            problems.push(Problem::new(location, "an event kind row names no `kind`"));
            continue;
        };
        match string_at(row, "subject").as_deref() {
            Some("required" | "optional" | "forbidden") => {}
            other => problems.push(Problem::new(
                location,
                format!(
                    "`{kind}` says its subject is {other:?}; the word is `required`, `optional` \
                     or `forbidden`, because §6 asks each kind to say whether it is about an \
                     identified object"
                ),
            )),
        }
        for field in ["rule", "spec"] {
            if string_at(row, field).unwrap_or_default().trim().is_empty() {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{kind}` states no `{field}`; §6 gives every kind a constraint and a \
                         section that fixes it"
                    ),
                ));
            }
        }
    }

    // §6.3, as a check rather than a paragraph: `object.disappeared` needs an authoritative
    // source event or a complete-snapshot contract, and a polling gap is neither.
    let contract = document.get("disappearance_contract");
    let permitted: BTreeSet<String> = contract
        .map(|node| {
            sequence(node, "permitted_when")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    for required in ["authoritative_source_event", "complete_snapshot_contract"] {
        if !permitted.contains(required) {
            problems.push(Problem::new(
                location,
                format!(
                    "`object.disappeared` does not name `{required}` as a condition that permits \
                     it; v0.5 §6.3 permits the kind only on an authoritative source event or a \
                     provider contract defining missing-from-a-complete-snapshot as absence"
                ),
            ));
        }
    }
    let forbidden: BTreeSet<String> = contract
        .map(|node| {
            sequence(node, "forbidden_when")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    if !forbidden.contains("polling_gap") {
        problems.push(Problem::new(
            location,
            "`object.disappeared` does not forbid `polling_gap`; v0.5 §6.3: \"A polling gap MUST \
             NOT automatically emit disappearance\"",
        ));
    }

    problems.extend(check_event_kinds_against_core(root, &declared));
    problems
}

/// The registry's kinds against `ono-temporal-core`'s own, where that crate declares them.
///
/// The comparison is on the string literals the crate carries, because `xtask` must keep
/// compiling while `ono-temporal-core` is being written: a `use` of a type that does not exist
/// yet breaks the whole gate rather than reporting one problem. This is the shape
/// `contracts::check_declared_options` already uses for command options.
fn check_event_kinds_against_core(root: &Path, declared: &BTreeSet<String>) -> Vec<Problem> {
    let source = root.join("crates").join("ono-temporal-core").join("src");
    let mut body = String::new();
    collect_rust(&source, &mut body);
    if !body.contains("enum EventKind") {
        // The crate has not landed its vocabulary yet. AGENTS.md §14: an artifact arrives with
        // the phase that needs it, and a check that fails on its absence would make every other
        // agent's gate red for work that is not theirs.
        return Vec::new();
    }
    let mut problems = Vec::new();
    for kind in declared {
        if !body.contains(&format!("\"{kind}\"")) {
            problems.push(Problem::new(
                "crates/ono-temporal-core/src",
                format!(
                    "`docs/contracts/temporal/events.yaml` declares the canonical event kind \
                     `{kind}` and `EventKind` names it nowhere; v0.5 §36.4 fails the gate when \
                     the implementation and the registry disagree about the kinds"
                ),
            ));
        }
    }
    problems
}

/// `evidence.yaml`: the nine source classes, the five strengths in strength order, the eight gap
/// reasons and §7.4's negative-evidence rule.
fn check_evidence(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/evidence.yaml";
    let Some(document) = registries.get("evidence.yaml") else {
        return Vec::new();
    };
    let mut problems = Vec::new();

    let classes: BTreeSet<String> = sequence(document, "source_classes")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    for class in EVIDENCE_SOURCE_CLASSES {
        if !classes.contains(class) {
            problems.push(Problem::new(
                location,
                format!("v0.5 §7.1 defines the evidence source class `{class}` and this registry omits it"),
            ));
        }
    }
    for class in &classes {
        if !EVIDENCE_SOURCE_CLASSES.contains(&class.as_str()) {
            problems.push(Problem::new(
                location,
                format!("declares the evidence source class `{class}`, which v0.5 §7.1 does not"),
            ));
        }
    }
    for row in sequence(document, "source_classes") {
        let Some(id) = string_at(row, "id") else {
            continue;
        };
        if string_at(row, "grammar").is_none() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` states no `grammar`; v0.5 §7.1: \"Sources MUST have stable \
                     inspectable identity\""
                ),
            ));
        }
    }

    let strengths: Vec<String> = sequence(document, "strengths")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    if strengths != EVIDENCE_STRENGTHS {
        problems.push(Problem::new(
            location,
            format!(
                "declares the evidence strengths {strengths:?}; v0.5 §7.2 fixes them, in \
                 strength order, as {EVIDENCE_STRENGTHS:?}"
            ),
        ));
    }
    for (position, row) in sequence(document, "strengths").into_iter().enumerate() {
        let id = string_at(row, "id").unwrap_or_else(|| "a strength".to_owned());
        let rank = row.get("rank").and_then(Yaml::as_u64);
        let expected = u64::try_from(position + 1).unwrap_or(0);
        if rank != Some(expected) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` is written {expected} from the top and declares rank {rank:?}; the \
                     ordering of v0.5 §7.2 is the contract, so the file's order and the ranks \
                     must be one statement"
                ),
            ));
        }
    }
    if !sequence(document, "strength_rules")
        .into_iter()
        .any(|row| string_at(row, "id").as_deref() == Some("no_promotion"))
    {
        problems.push(Problem::new(
            location,
            "states no `no_promotion` rule; v0.5 §7.2: \"Evidence strength MUST NOT be \
             automatically upgraded by renderers, AI assistants or plugins.\"",
        ));
    }

    let reasons: BTreeSet<String> = sequence(document, "gap_reasons")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    for reason in GAP_REASONS {
        if !reasons.contains(reason) {
            problems.push(Problem::new(
                location,
                format!("v0.5 §7.5 defines the gap reason `{reason}` and this registry omits it"),
            ));
        }
    }
    for reason in &reasons {
        if !GAP_REASONS.contains(&reason.as_str()) {
            problems.push(Problem::new(
                location,
                format!("declares the gap reason `{reason}`, which v0.5 §7.5 does not"),
            ));
        }
    }

    let negative = document.get("negative_evidence");
    let requires: BTreeSet<String> = negative
        .map(|node| {
            sequence(node, "requires")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    if !requires.contains("coverage_capable_of_proving_absence") {
        problems.push(Problem::new(
            location,
            "states no precondition for a negative historical claim; v0.5 §7.4 permits one only \
             where an authoritative or sufficiently complete source had coverage capable of \
             proving the absence",
        ));
    }

    problems
}

/// `causality.yaml`: the classes and their inverses, and the built-in rules against the
/// registered set (§36.4's third rule), plus every internal cross-reference.
fn check_causal_rules(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/causality.yaml";
    let Some(document) = registries.get("causality.yaml") else {
        return Vec::new();
    };
    let strengths: BTreeSet<String> = registries
        .get("evidence.yaml")
        .map(|evidence| {
            sequence(evidence, "strengths")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    let event_kinds: BTreeSet<String> = registries
        .get("events.yaml")
        .map(|events| {
            sequence(events, "kinds")
                .into_iter()
                .filter_map(|row| string_at(row, "kind"))
                .collect()
        })
        .unwrap_or_default();
    let mut problems = Vec::new();

    let mut classes: BTreeMap<String, bool> = BTreeMap::new();
    for row in sequence(document, "relation_classes") {
        let Some(id) = string_at(row, "id") else {
            problems.push(Problem::new(location, "a relation class row names no `id`"));
            continue;
        };
        let causal = row.get("causal").and_then(Yaml::as_bool);
        classes.insert(id.clone(), causal.unwrap_or(false));
        let inverse = string_at(row, "inverse_label").unwrap_or_default();
        if inverse.trim().is_empty() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` declares no `inverse_label`; v0.5 §15.1 gives every class one, and a \
                     graph a user can only read in one direction is half a graph"
                ),
            ));
        }
        match RELATION_CLASSES.iter().find(|(name, _, _)| *name == id) {
            None => problems.push(Problem::new(
                location,
                format!("declares the relation class `{id}`, which v0.5 §15.1 does not define"),
            )),
            Some((_, expected_inverse, expected_causal)) => {
                if !inverse.trim().is_empty() && inverse != *expected_inverse {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` has the inverse label `{inverse}`; v0.5 §15.1 gives it \
                             `{expected_inverse}`"
                        ),
                    ));
                }
                if causal != Some(*expected_causal) {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` declares causal {causal:?}; v0.5 §15.1 and §15.6 make it \
                             {expected_causal}, and a renderer keys its wording on that predicate"
                        ),
                    ));
                }
            }
        }
    }
    for (id, _, _) in RELATION_CLASSES {
        if !classes.contains_key(id) {
            problems.push(Problem::new(
                location,
                format!("v0.5 §15.1 defines the relation class `{id}` and this registry omits it"),
            ));
        }
    }

    // The engine's own list, which is what §36.4 asks the registry to be held against: a rule the
    // causal engine runs and nobody registered cannot be audited, and a rule the registry
    // declares and nothing runs is a causal claim Ono cannot make. The specification's list stays
    // beside it, so a registry that drifts from §15.2 fails whatever the engine does.
    let implemented_causal: BTreeSet<&str> = ono_temporal_query::causal::BUILTIN_CAUSAL_RULE_IDS
        .iter()
        .copied()
        .collect();
    let implemented_correlation: BTreeSet<&str> =
        ono_temporal_query::causal::BUILTIN_CORRELATION_RULE_IDS
            .iter()
            .copied()
            .collect();
    for (kind, declared, implemented) in [
        (
            "causal",
            BUILTIN_CAUSAL_RULES.as_slice(),
            &implemented_causal,
        ),
        (
            "correlation",
            BUILTIN_CORRELATION_RULES.as_slice(),
            &implemented_correlation,
        ),
    ] {
        for id in declared {
            if !implemented.contains(id) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "v0.5 names the built-in {kind} rule `{id}` and the causal engine runs no                          rule by that id"
                    ),
                ));
            }
        }
        for id in implemented {
            if !declared.contains(id) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "the causal engine runs the {kind} rule `{id}`, which v0.5 §15.8 has no                          row for; every rule that emits a relation must be inspectable"
                    ),
                ));
            }
        }
    }

    let mut declared_rules = BTreeSet::new();
    for (key, expected, causal_only) in [
        ("rules", BUILTIN_CAUSAL_RULES.as_slice(), true),
        (
            "correlation_rules",
            BUILTIN_CORRELATION_RULES.as_slice(),
            false,
        ),
    ] {
        let mut seen = BTreeSet::new();
        for row in sequence(document, key) {
            let Some(id) = string_at(row, "rule_id") else {
                problems.push(Problem::new(
                    location,
                    format!("a `{key}` row names no `rule_id`"),
                ));
                continue;
            };
            seen.insert(id.clone());
            declared_rules.insert(id.clone());
            problems.extend(check_rule_row(
                location,
                &id,
                row,
                &classes,
                &strengths,
                &event_kinds,
                causal_only,
            ));
        }
        for id in expected {
            if !seen.contains(*id) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "the built-in rule `{id}` is not registered under `{key}`; v0.5 §15.8 \
                         requires every built-in rule to be machine-readable and inspectable"
                    ),
                ));
            }
        }
        for id in &seen {
            if !expected.contains(&id.as_str()) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{key}` registers `{id}`, which is not one of the built-in rules the \
                         causal engine runs; a rule in the registry that nothing implements is a \
                         causal claim Ono cannot make"
                    ),
                ));
            }
        }
    }
    if declared_rules.len() != BUILTIN_CAUSAL_RULES.len() + BUILTIN_CORRELATION_RULES.len() {
        problems.push(Problem::new(
            location,
            "a rule id is registered twice; a rule id identifies one rule, and an explanation \
             naming an ambiguous rule cannot be inspected (v0.5 §15.8)",
        ));
    }

    let wording = document.get("renderer_wording");
    let forbidden: BTreeSet<String> = wording
        .map(|node| string_sequence(node, "forbidden_for_non_causal"))
        .unwrap_or_default()
        .into_iter()
        .collect();
    for word in FORBIDDEN_CAUSAL_WORDS {
        if !forbidden.contains(word) {
            problems.push(Problem::new(
                location,
                format!(
                    "does not forbid the word `{word}` for a non-causal relation; v0.5 §15.6 \
                     lists it, and §15.8 says no renderer may create causal language outside \
                     this registry"
                ),
            ));
        }
    }
    let connectors = wording.and_then(|node| node.get("connector_style"));
    let causal_style = connectors
        .and_then(|node| node.get("causal"))
        .and_then(|node| string_at(node, "style"));
    let non_causal_style = connectors
        .and_then(|node| node.get("non_causal"))
        .and_then(|node| string_at(node, "style"));
    match (causal_style, non_causal_style) {
        (Some(causal), Some(non_causal)) if causal != non_causal => {}
        _ => problems.push(Problem::new(
            location,
            "does not give a causal edge and a non-causal edge distinct connector styles; \
             v0.5 §45.3: \"A renderer MUST never use the same edge style/label for both.\"",
        )),
    }

    if document
        .get("unknown_cause")
        .and_then(|node| node.get("is_error"))
        .and_then(Yaml::as_bool)
        != Some(false)
    {
        problems.push(Problem::new(
            location,
            "does not state that an unknown cause is not an error; v0.5 §15.7: \"Unknown cause is \
             a valid outcome, not an error.\"",
        ));
    }

    problems
}

/// The seven fields §15.8 says a rule records, and the cross-references each one carries.
fn check_rule_row(
    location: &str,
    id: &str,
    row: &Yaml,
    classes: &BTreeMap<String, bool>,
    strengths: &BTreeSet<String>,
    event_kinds: &BTreeSet<String>,
    causal_only: bool,
) -> Vec<Problem> {
    let mut problems = Vec::new();

    // §15.8 requires every rule that emits a causal relation to be inspectable, and a row saying
    // `declared` about a rule the engine runs is inspectable and wrong. The word is checked
    // against the engine's own list, which is what `check_causality` compares the ids against.
    let implemented = BUILTIN_CAUSAL_RULES.contains(&id) || BUILTIN_CORRELATION_RULES.contains(&id);
    match string_at(row, "status").as_deref() {
        Some("implemented") if implemented => {}
        Some("declared") if !implemented => {}
        Some("implemented") => problems.push(Problem::new(
            location,
            format!("`{id}` says `status: implemented` and the causal engine runs no such rule"),
        )),
        Some("declared") => problems.push(Problem::new(
            location,
            format!(
                "`{id}` says `status: declared` and the causal engine runs it. A registry that \
                 misdescribes the code is §15.8's inspectability with the inspection removed"
            ),
        )),
        Some(other) => problems.push(Problem::new(
            location,
            format!("`{id}` says `status: {other}`; the word is `declared` or `implemented`"),
        )),
        None => problems.push(Problem::new(
            location,
            format!("`{id}` states no `status`, so nothing says whether the engine runs it"),
        )),
    }

    for field in [
        "input_event_kinds",
        "required_evidence_strengths",
        "identity_constraints",
        "time_constraints",
        "output_relation",
        "provider_source_constraints",
    ] {
        if row.get(field).is_none() {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` records no `{field}`; v0.5 §15.8 fixes the seven fields a rule \
                     records, and a rule missing one cannot be inspected against its inputs"
                ),
            ));
        }
    }

    for kind in string_sequence(row, "input_event_kinds") {
        if !event_kinds.is_empty() && !event_kinds.contains(&kind) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` reads the event kind `{kind}`, which `events.yaml` does not declare"
                ),
            ));
        }
    }

    for (input, required) in mapping_entries(row, "required_evidence_strengths") {
        let Some(strength) = required.as_str() else {
            problems.push(Problem::new(
                location,
                format!("`{id}` gives `{input}` a required strength that is not a name"),
            ));
            continue;
        };
        if !strengths.is_empty() && !strengths.contains(strength) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` requires the evidence strength `{strength}`, which `evidence.yaml` \
                     does not declare (v0.5 §7.2)"
                ),
            ));
        }
        if !event_kinds.is_empty() && !event_kinds.contains(&input) {
            problems.push(Problem::new(
                location,
                format!(
                    "`{id}` requires a strength for `{input}`, which is not a canonical event kind"
                ),
            ));
        }
    }

    match string_at(row, "output_relation") {
        None => {}
        Some(output) => match classes.get(&output) {
            None => problems.push(Problem::new(
                location,
                format!(
                    "`{id}` emits `{output}`, which is not a relation class this registry declares"
                ),
            )),
            Some(causal) => {
                if causal_only && !causal {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` is registered as a causal rule and emits the non-causal \
                             relation `{output}`; v0.5 §15.8 registers the rules that emit \
                             `caused_by`, `triggered_by` or `resulted_in`"
                        ),
                    ));
                }
                if !causal_only && *causal {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` is registered as a correlation rule and emits the causal \
                             relation `{output}`; v0.5 §15.5 gives correlation no causal \
                             evidence, so it may emit `correlated_with` only"
                        ),
                    ));
                }
                if !causal_only && output != "correlated_with" {
                    problems.push(Problem::new(
                        location,
                        format!("`{id}` is a correlation rule and must emit `correlated_with`"),
                    ));
                }
            }
        },
    }

    if !causal_only && row.get("window").is_none() {
        problems.push(Problem::new(
            location,
            format!(
                "`{id}` is a correlation rule and states no `window`; v0.5 §15.5's association \
                 is temporal, so the span it looks across is the rule"
            ),
        ));
    }

    problems
}

/// `sources.yaml` against §21.1's keys, `evidence.yaml`'s classes, §21.5 and §22.1's
/// prohibitions, and the provider contracts (§36.4's last rule).
fn check_sources(root: &Path, registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/sources.yaml";
    let Some(document) = registries.get("sources.yaml") else {
        return Vec::new();
    };
    let classes: BTreeSet<String> = registries
        .get("evidence.yaml")
        .map(|evidence| {
            sequence(evidence, "source_classes")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    let keys: BTreeSet<String> = registries
        .get("temporal.yaml")
        .map(|temporal| {
            sequence(temporal, "capabilities")
                .into_iter()
                .filter_map(|row| string_at(row, "key"))
                .collect()
        })
        .unwrap_or_default();
    let providers = declared_providers(root);
    let mut problems = Vec::new();

    for row in sequence(document, "sources") {
        let Some(id) = string_at(row, "id") else {
            problems.push(Problem::new(location, "a source row names no `id`"));
            continue;
        };
        match string_at(row, "evidence_class") {
            None => problems.push(Problem::new(
                location,
                format!("`{id}` names no `evidence_class` (v0.5 §7.1)"),
            )),
            Some(class) => {
                if !classes.is_empty() && !classes.contains(&class) {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` belongs to the evidence source class `{class}`, which \
                             `evidence.yaml` does not declare"
                        ),
                    ));
                }
            }
        }

        let claimed: BTreeMap<String, &Yaml> = row
            .get("capabilities")
            .and_then(Yaml::as_mapping)
            .map(|mapping| {
                mapping
                    .iter()
                    .filter_map(|(key, value)| Some((key.as_str()?.to_owned(), value)))
                    .collect()
            })
            .unwrap_or_default();
        for key in claimed.keys() {
            if !keys.is_empty() && !keys.contains(key) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{id}` claims the temporal capability `{key}`, which v0.5 §21.1 does not \
                         define"
                    ),
                ));
            }
        }
        for key in &keys {
            if !claimed.contains_key(key) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{id}` says nothing about `{key}`; v0.5 §21.1 makes capabilities \
                         inspectable, and an unstated capability reads as an unanswered question"
                    ),
                ));
            }
        }

        let claims = |key: &str| claimed.get(key).and_then(|value| value.as_bool());
        let completeness = row
            .get("coverage")
            .and_then(|coverage| string_at(coverage, "completeness"));
        let sampled = row
            .get("coverage")
            .and_then(|coverage| coverage.get("sampled"))
            .and_then(Yaml::as_bool);

        if claims("exhaustive_events") == Some(true) {
            if sampled != Some(false) {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{id}` claims `exhaustive_events` and samples; v0.5 §21.5: \
                         \"`exhaustive_events` is a strong contract... Providers MUST NOT \
                         advertise it merely because events usually arrive\""
                    ),
                ));
            }
            if completeness.as_deref() != Some("complete") {
                problems.push(Problem::new(
                    location,
                    format!(
                        "`{id}` claims `exhaustive_events` and its coverage is \
                         {completeness:?}; v0.5 §21.5 makes the claim one about sequence \
                         continuity supporting absence claims, which only `complete` coverage is"
                    ),
                ));
            }
        }
        if id == "linux.procfs" && claims("historical_query") != Some(false) {
            problems.push(Problem::new(
                location,
                "`linux.procfs` claims `historical_query`; v0.5 §22.1: \"The reference provider \
                 MUST NOT claim native historical process coverage.\" Process history comes from \
                 the recorder comparing snapshots, with provenance `snapshot_diff`",
            ));
        }

        match string_at(row, "provider").filter(|name| name != "null") {
            None => {}
            Some(provider) => {
                if !providers.contains_key(&provider) {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{id}` is fronted by the provider `{provider}`, which no file in \
                             `docs/contracts/providers/` declares"
                        ),
                    ));
                } else {
                    // §36.4: a provider advertising a temporal capability its contract metadata
                    // does not carry fails the gate. One provider id may front several target
                    // groups with different `temporal:` blocks — netlink pushes link, address
                    // and route changes and polls neighbours — so the source-level claim is the
                    // disjunction: the source can do it if any of its groups can.
                    for key in TEMPORAL_CAPABILITY_KEYS {
                        // `retained_history` is a duration rather than a claim, and a source's
                        // effective retention is its configuration's, not its contract's.
                        if key == "retained_history" {
                            continue;
                        }
                        let advertised = providers
                            .get(&provider)
                            .map(|rows| {
                                rows.iter().any(|row| {
                                    row.get("temporal")
                                        .and_then(|block| block.get(key))
                                        .and_then(Yaml::as_bool)
                                        == Some(true)
                                })
                            })
                            .unwrap_or(false);
                        if claims(key) != Some(advertised) {
                            problems.push(Problem::new(
                                location,
                                format!(
                                    "`{id}` claims `{key}` is {:?} and the provider `{provider}` \
                                     advertises {advertised} in `docs/contracts/providers/`; \
                                     v0.5 §36.4 fails the gate when a provider advertises a \
                                     temporal capability its contract metadata does not carry",
                                    claims(key)
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    for rule in [
        "exhaustive_events_is_a_contract",
        "procfs_is_not_historical",
        "failure_is_coverage_loss",
    ] {
        if !sequence(document, "claim_rules")
            .into_iter()
            .any(|row| string_at(row, "id").as_deref() == Some(rule))
        {
            problems.push(Problem::new(
                location,
                format!("states no claim rule `{rule}` (v0.5 §21.5, §21.8, §22.1)"),
            ));
        }
    }

    problems
}

/// `recorder.yaml`: §10.6's two lists, §30's file permissions and §44.1's restart procedure.
fn check_recorder(registries: &Registries) -> Vec<Problem> {
    let location = "docs/contracts/temporal/recorder.yaml";
    let Some(document) = registries.get("recorder.yaml") else {
        return Vec::new();
    };
    let mut problems = Vec::new();

    if document
        .get("defaults")
        .and_then(|node| node.get("enabled"))
        .and_then(Yaml::as_bool)
        != Some(false)
    {
        problems.push(Problem::new(
            location,
            "does not record that the recorder is disabled by default; v0.5 §10.2: \"Persistent \
             recording MUST be disabled by default.\"",
        ));
    }

    let forbidden: BTreeSet<String> = sequence(document, "must_not_persist")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    for entry in [
        "command_output_bodies",
        "file_contents",
        "environment_dumps",
        "secrets",
        "secret_bearing_argv",
        "packet_payloads",
        "unlimited_metrics_samples",
    ] {
        if !forbidden.contains(entry) {
            problems.push(Problem::new(
                location,
                format!(
                    "does not forbid persisting `{entry}`; v0.5 §10.6 lists seven things the \
                     recorder MUST NOT persist by default, and a privacy boundary nobody wrote \
                     down is not a boundary"
                ),
            ));
        }
    }
    if sequence(document, "collects").is_empty() {
        problems.push(Problem::new(
            location,
            "states nothing the recorder collects; v0.5 §10.6's first list is half the policy",
        ));
    }

    let privilege = document.get("privilege");
    let prohibitions: BTreeSet<String> = privilege
        .map(|node| {
            sequence(node, "must_not")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    for entry in [
        "setuid",
        "automatic_sudo",
        "privileged_daemon_for_visibility",
        "read_beyond_user",
    ] {
        if !prohibitions.contains(entry) {
            problems.push(Problem::new(
                location,
                format!("does not forbid `{entry}`; v0.5 §10.5 lists four things the recorder MUST NOT do"),
            ));
        }
    }
    if privilege
        .and_then(|node| string_at(node, "runs_as"))
        .as_deref()
        != Some("user")
    {
        problems.push(Problem::new(
            location,
            "does not say the recorder runs with the user's privileges; v0.5 §10.5 requires it",
        ));
    }

    let storage = document.get("storage");
    for (field, expected) in [("directory_mode", "0700"), ("database_mode", "0600")] {
        let mode = storage.and_then(|node| string_at(node, field));
        if mode.as_deref() != Some(expected) {
            problems.push(Problem::new(
                location,
                format!(
                    "declares `{field}` as {mode:?}; v0.5 §30.2 fixes the reference Linux \
                     permission as {expected}, because the ledger directory MUST be user-private"
                ),
            ));
        }
    }

    if document
        .get("process_argv")
        .and_then(|node| node.get("default"))
        .and_then(Yaml::as_bool)
        != Some(false)
    {
        problems.push(Problem::new(
            location,
            "does not record that argv persistence is off by default; v0.5 §30.4 permits the \
             option and disables it by default because argv carries secrets",
        ));
    }
    if document
        .get("process_argv")
        .and_then(|node| string_at(node, "setting"))
        .as_deref()
        != Some("temporal.record.process_argv")
    {
        problems.push(Problem::new(
            location,
            "does not name the setting that enables argv persistence; v0.5 §30.4's option is \
             `temporal.record.process_argv`",
        ));
    }

    let ledger = document.get("session_ledger");
    if ledger
        .and_then(|node| node.get("max_events"))
        .and_then(Yaml::as_u64)
        != Some(100_000)
    {
        problems.push(Problem::new(
            location,
            "does not carry §10.7's session ledger bound of 100000 events",
        ));
    }

    let steps: Vec<String> = document
        .get("restart_procedure")
        .map(|node| {
            sequence(node, "steps")
                .into_iter()
                .filter_map(|row| string_at(row, "id"))
                .collect()
        })
        .unwrap_or_default();
    let expected = [
        "validate_store_metadata",
        "restore_source_sequence_checkpoints",
        "mark_downtime_as_gap",
        "fresh_checkpoint",
        "continue_without_pretending",
    ];
    if steps != expected {
        problems.push(Problem::new(
            location,
            format!(
                "declares the restart procedure {steps:?}; v0.5 §44.1 fixes it, in order, as \
                 {expected:?}"
            ),
        ));
    }

    let lifecycle = document.get("lifecycle");
    let start = lifecycle
        .map(|node| sequence(node, "commands"))
        .unwrap_or_default()
        .into_iter()
        .find(|row| string_at(row, "id").as_deref() == Some("ono.recorder.start"));
    if start
        .and_then(|row| row.get("idempotent"))
        .and_then(Yaml::as_bool)
        != Some(true)
    {
        problems.push(Problem::new(
            location,
            "does not record that `start recorder` is idempotent; v0.5 §10.8 requires it",
        ));
    }

    problems
}

/// `temporal.yaml`'s settings block against `ono_cli::settings::CATALOGUE`, in both directions
/// and on key, type and default (§36.4's fifth rule).
fn check_settings(registries: &Registries) -> Vec<Problem> {
    use ono_cli::settings::{CATALOGUE, SettingSpec};

    let location = "docs/contracts/temporal/temporal.yaml";
    let Some(document) = registries.get("temporal.yaml") else {
        return Vec::new();
    };
    let mut problems = Vec::new();

    let catalogue: BTreeMap<&str, &SettingSpec> = CATALOGUE
        .iter()
        .filter(|setting| setting.key.starts_with("temporal."))
        .map(|setting| (setting.key, setting))
        .collect();
    let mut declared = BTreeSet::new();

    for row in sequence(document, "settings") {
        let Some(key) = string_at(row, "key") else {
            problems.push(Problem::new(location, "a settings row names no `key`"));
            continue;
        };
        declared.insert(key.clone());
        let Some(setting) = catalogue.get(key.as_str()) else {
            problems.push(Problem::new(
                location,
                format!(
                    "declares the setting `{key}` and `ono_cli::settings::CATALOGUE` has no such \
                     key; v0.5 §33 makes every setting typed and inspectable, and a key only the \
                     registry knows cannot be set"
                ),
            ));
            continue;
        };
        let ty = string_at(row, "type").unwrap_or_default();
        if ty != setting.ty.name() {
            problems.push(Problem::new(
                location,
                format!(
                    "declares `{key}` as `{ty}` and the shell declares it as `{}`",
                    setting.ty.name()
                ),
            ));
            continue;
        }
        let Some(given) = row.get("default") else {
            problems.push(Problem::new(
                location,
                format!("`{key}` states no `default`"),
            ));
            continue;
        };
        match parse_default(&ty, given) {
            None => problems.push(Problem::new(
                location,
                format!("`{key}` has a default that is not a `{ty}`: {given:?}"),
            )),
            Some(value) => {
                if value != setting.default_value() {
                    problems.push(Problem::new(
                        location,
                        format!(
                            "`{key}` defaults to {value} here and to {} in the shell; v0.5 §36.4 \
                             fails the gate when the default configuration differs from the \
                             registry",
                            setting.default_value()
                        ),
                    ));
                }
            }
        }
    }

    for key in catalogue.keys() {
        if !declared.contains(*key) {
            problems.push(Problem::new(
                location,
                format!(
                    "the shell declares the setting `{key}` and this registry omits it; v0.5 §33 \
                     requires every canonical setting to be in machine-readable configuration \
                     metadata"
                ),
            ));
        }
    }

    problems
}

/// A default written the way a user types it, read in its declared type.
fn parse_default(ty: &str, given: &Yaml) -> Option<ono_value::Value> {
    use ono_value::{ByteSize, Duration, Value};

    match ty {
        "bool" => given.as_bool().map(Value::Bool),
        "int" => given.as_i64().map(|number| Value::Int(i128::from(number))),
        "string" => given.as_str().map(Value::string),
        "bytesize" => given
            .as_str()
            .and_then(|text| ByteSize::parse(text).ok())
            .map(Value::ByteSize),
        "duration" => given
            .as_str()
            .and_then(|text| Duration::parse(text).ok())
            .map(Value::Duration),
        _ => None,
    }
}

/// Every command in `docs/contracts/commands/temporal.yaml` against `temporal.yaml`'s inventory
/// (§36.4's first rule).
///
/// The command family file arrives with the implementations that bind it, so this passes
/// vacuously until then and bites the moment the file lands.
fn check_command_inventory(root: &Path, registries: &Registries) -> Vec<Problem> {
    let path = root
        .join("docs")
        .join("contracts")
        .join("commands")
        .join("temporal.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(commands) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let Some(document) = registries.get("temporal.yaml") else {
        return Vec::new();
    };

    let inventory: BTreeSet<String> = sequence(document, "commands")
        .into_iter()
        .filter_map(|row| string_at(row, "id"))
        .collect();
    let mut problems = Vec::new();
    for row in sequence(&commands, "commands") {
        let Some(id) = string_at(row, "id") else {
            continue;
        };
        // A `planned` command describes the whole product rather than the part that exists
        // today (ADR-0012); anything stable must be in the inventory.
        if string_at(row, "stability").as_deref() == Some("planned") {
            continue;
        }
        if !inventory.contains(&id) {
            problems.push(Problem::new(
                "docs/contracts/temporal/temporal.yaml",
                format!(
                    "`{id}` is a stable temporal command and this registry's inventory omits it; \
                     v0.5 §36.4 fails the gate on exactly that"
                ),
            ));
        }
    }
    problems
}

/// Every error name and schema id a temporal registry mentions, resolved against the shared
/// registries (§36.4's fourth rule).
fn check_references(root: &Path, registries: &Registries) -> Vec<Problem> {
    let contracts = root.join("docs").join("contracts");
    let errors: BTreeSet<String> = std::fs::read_to_string(contracts.join("errors.yaml"))
        .ok()
        .and_then(|text| serde_yaml_ng::from_str::<Yaml>(&text).ok())
        .map(|document| {
            sequence(&document, "errors")
                .into_iter()
                .filter_map(|row| string_at(row, "name"))
                .collect()
        })
        .unwrap_or_default();

    let mut problems = Vec::new();
    for (file, body) in &registries.text {
        let location = format!("docs/contracts/temporal/{file}");
        for name in error_names(body) {
            if !errors.is_empty() && !errors.contains(&name) {
                problems.push(Problem::new(
                    location.clone(),
                    format!(
                        "names the error `{name}`, which `docs/contracts/errors.yaml` does not \
                         declare (v0.5 §34, §36.4)"
                    ),
                ));
            }
        }
        for id in schema_ids(body) {
            let Some(file_name) = schema_file_name(&id) else {
                continue;
            };
            if !contracts.join("schemas").join(&file_name).is_file() {
                problems.push(Problem::new(
                    location.clone(),
                    format!(
                        "names the schema `{id}`, and `docs/contracts/schemas/{file_name}` does \
                         not exist (v0.5 §35, §36.4)"
                    ),
                ));
            }
        }
    }
    problems
}

/// Every temporal registry against `docs/contracts/hardening/registries.yaml`.
///
/// v0.4.1 §52.3 asks the gate to validate every machine-readable contract, and the inventory is
/// what says which they are. It indexes `docs/contracts/hardening/` by name; a temporal registry
/// is indexed by a path relative to that directory, so one index covers both and a registry
/// still arrives with its validator or does not arrive (ADR-0625).
fn check_inventory(root: &Path) -> Vec<Problem> {
    let location = "docs/contracts/hardening/registries.yaml";
    let Ok(text) = std::fs::read_to_string(root.join("docs/contracts/hardening/registries.yaml"))
    else {
        return Vec::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let indexed: BTreeSet<String> = sequence(&document, "registries")
        .into_iter()
        .filter_map(|row| string_at(row, "file"))
        .collect();
    let mut problems = Vec::new();
    for file in REQUIRED_REGISTRIES {
        let relative = format!("../temporal/{file}");
        if !indexed.contains(&relative) {
            problems.push(Problem::new(
                location,
                format!(
                    "says nothing about `docs/contracts/temporal/{file}`, which is a \
                     machine-readable contract v0.5 §36 requires. v0.4.1 §52.3 asks the gate to \
                     validate every one of them, so a registry arrives with its validator or it \
                     does not arrive"
                ),
            ));
        }
    }
    problems
}

/// Every `temporal.<name>` token in a registry that is an error name rather than a setting key.
///
/// A setting of v0.5 §33 has three segments (`temporal.retention.max_age`) and an error of §34
/// has two (`temporal.read_only`), so the shape separates them without a list to keep in step.
fn error_names(body: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (index, _) in body.match_indices("temporal.") {
        let rest = &body[index + "temporal.".len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .unwrap_or(rest.len());
        let (name, tail) = rest.split_at(end);
        if name.is_empty() || tail.starts_with('.') || name == "yaml" {
            continue;
        }
        // `docs/contracts/temporal/…` and `ono-temporal-core` are paths, not error names.
        if body[..index].ends_with(['/', '-', '.']) {
            continue;
        }
        names.insert(format!("temporal.{name}"));
    }
    names
}

/// Every `ono.<name>/<version>` schema id a registry mentions.
fn schema_ids(body: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for (index, _) in body.match_indices("ono.") {
        let rest = &body[index + "ono.".len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .unwrap_or(rest.len());
        let (name, tail) = rest.split_at(end);
        let Some(version) = tail.strip_prefix('/') else {
            continue;
        };
        let digits = version
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(version.len());
        if name.is_empty() || digits == 0 {
            continue;
        }
        ids.insert(format!("ono.{name}/{}", &version[..digits]));
    }
    ids
}

/// `ono.recorder-status/1` lives in `recorder-status.v1.yaml`.
fn schema_file_name(id: &str) -> Option<String> {
    let (name, version) = id.split_once('/')?;
    let short = name.strip_prefix("ono.").unwrap_or(name);
    Some(format!("{}.v{version}.yaml", short.replace('.', "-")))
}

/// Every provider `docs/contracts/providers/` declares, by id.
///
/// One id may appear several times — `linux.netlink` fronts three target groups with different
/// temporal behaviour — so the rows are kept rather than the last one winning.
fn declared_providers(root: &Path) -> BTreeMap<String, Vec<Yaml>> {
    let mut providers: BTreeMap<String, Vec<Yaml>> = BTreeMap::new();
    let directory = root.join("docs").join("contracts").join("providers");
    for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
            continue;
        };
        for row in sequence(&document, "providers") {
            if let Some(id) = string_at(row, "id") {
                providers.entry(id).or_default().push(row.clone());
            }
        }
    }
    providers
}

/// Every `.rs` file under `directory`, concatenated, for a literal scan.
fn collect_rust(directory: &Path, body: &mut String) {
    for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust(&path, body);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            body.push_str(&text);
        }
    }
}

fn sequence<'a>(document: &'a Yaml, key: &str) -> Vec<&'a Yaml> {
    document
        .get(key)
        .and_then(Yaml::as_sequence)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn string_at(document: &Yaml, key: &str) -> Option<String> {
    document
        .get(key)
        .and_then(Yaml::as_str)
        .map(str::to_owned)
        .or_else(|| {
            document
                .get(key)
                .filter(|value| value.is_null())
                .map(|_| "null".to_owned())
        })
}

fn string_sequence(document: &Yaml, key: &str) -> Vec<String> {
    sequence(document, key)
        .into_iter()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn mapping_entries<'a>(document: &'a Yaml, key: &str) -> Vec<(String, &'a Yaml)> {
    document
        .get(key)
        .and_then(Yaml::as_mapping)
        .map(|mapping| {
            mapping
                .iter()
                .filter_map(|(name, value)| Some((name.as_str()?.to_owned(), value)))
                .collect()
        })
        .unwrap_or_default()
}
