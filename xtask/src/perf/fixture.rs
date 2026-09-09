//! The deterministic fixture ledger of v0.5 §49, and the contract builders it is made of
//! (work packages TEST-001 and TEST-003).
//!
//! §49 asks for release evidence measured against a ledger of at least a million events, a
//! hundred thousand lifetimes, half a million relation changes and ten thousand action records,
//! and it says what the exercise is for:
//!
//! > The purpose is to catch architectural scaling failures, not to claim production
//! > observability scale.
//!
//! Three properties follow from that sentence, and each of them is a rule this module keeps.
//!
//! **It is a real store.** The fixture is a `ono-temporal-ledger` SQLite database written through
//! [`LedgerWrite`], one batch at a time, with the production schema, the production encoders and
//! the production indexes. A hand-written file would measure the file. v0.4.1 §32.2 states the
//! same rule for the topology fixtures — *"provider/planner code exercised by the benchmark MUST
//! match production logic"* — and a ledger is where it bites hardest, because the thing under
//! measurement *is* the index.
//!
//! **It is deterministic.** Nothing here reads a clock, a hostname, a process id or the operating
//! system's randomness. The instants come from [`ono_testkit::temporal::Clock`]'s fixed origin,
//! the variation comes from [`ono_testkit::Rng`] seeded with the number the profile declares, and
//! the identities are content digests over those. The same seed therefore produces the same
//! events with the same [`EventId`]s on any machine in any year, which is what makes two
//! measurements a fortnight apart comparable at all.
//!
//! **It is honest.** A builder here fills every field the contract requires and none that the
//! source would not have known. An `object.changed` event carries the evidence its field
//! transition rests on, because §7.2 makes an unevidenced claim inadmissible and because a
//! fixture that quietly omitted the evidence reference would let `why` pass a benchmark it could
//! never pass in production.

#![allow(
    clippy::expect_used,
    reason = "this module builds a fixture, and every `expect` here names a fact about the \
              shipped contracts — the process schema exists, `pid` is one of its fields, \
              `linux.procfs` is a built-in evidence source. None of them is reachable from user \
              input, and a fixture that quietly built a different shape would make the \
              measurement above it meaningless (v0.4.1 section 2.6)"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration as WallDuration, Instant};

use jiff::{Span, Timestamp};
use ono_spatial_core::{
    BootIdentity, Confidence, PermissionState, ScopeKind, SpatialId, SpatialIdentity, SpatialScope,
    SpatialType,
};
use ono_temporal_core::{
    ActionEvent, ActionId, AuthorizationDecision, AuthorizationSummary, CausalLink, CausalLinkId,
    CausalRelation, CausalRuleId, ChangeCertainty, Checkpoint, CheckpointId, ClockDomain,
    EventKind, EventSeed, EventTimes, Evidence, EvidenceClaim, EvidenceId, EvidenceSource,
    EvidenceStrength, FieldChange, LedgerWrite, ObjectState, Redactable, RedactedCommandSummary,
    RelationState, SpatialRef, TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_temporal_ledger::{LedgerStore, RetentionPolicy, StoreOptions};
use ono_testkit::Rng;
use ono_testkit::temporal::{Clock, TemporalProfile};
use ono_value::{MapValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};
use sha2::Digest as _;

/// The host the fixture's history belongs to.
///
/// A name rather than this machine's, because §49's fixture is a *deterministic* one and a
/// hostname is the first thing that makes two runs disagree.
const FIXTURE_HOST: &str = "fixture-host";

/// The boot the fixture's clock domain names (§25.1).
const FIXTURE_BOOT: &str = "4d0a1f2b-0000-4000-8000-0000000f1c00";

/// How many places the fixture's objects live in.
///
/// A history all in one place would make every scope-filtered query a full scan of the window,
/// which measures a filter nobody uses: §11.3 scopes a timeline to the current place, and the
/// cost that matters is the index skipping the rest. Sixteen is enough that a place holds a
/// legible fraction of the traffic and few enough that the scope column stays selective.
const PLACES: usize = 16;

/// How far apart two consecutive events are, in nanoseconds.
///
/// A million events across §10.4's twenty-four-hour default retention window is 11.6 events a
/// second, which is a busy recorder rather than an impossible one — and "on a ledger within
/// default retention" is exactly the condition §32.3 states its budgets under.
const STEP_NANOS: i64 = 86_400_000;

/// The window one event cycle covers: twenty slots, fixing the mix of kinds (§6.1).
const CYCLE: usize = 20;

/// How often the fixture writes a checkpoint (§31.9's default cadence).
const CHECKPOINT_MINUTES: i64 = 5;

/// How many objects one checkpoint captures.
///
/// A checkpoint of the whole hundred-thousand-object world every five minutes would be a fixture
/// about checkpoint width; §9.1's reconstruction cost is *checkpoint plus replay*, and the replay
/// is what a scaling failure hides in. Two hundred is the size of a real place's object set.
const CHECKPOINT_OBJECTS: usize = 200;

/// How many relations one checkpoint captures.
const CHECKPOINT_RELATIONS: usize = 100;

/// How many events are written per transaction.
const BATCH: usize = 2_000;

/// The fields an `object.changed` event may transition.
const FIELDS: [&str; 4] = ["active_state", "cpu_percent", "state", "sub_state"];

/// The relations the fixture's edges carry (§6.4).
const RELATIONS: [&str; 3] = [
    "process.owns_socket",
    "service.owns_process",
    "process.child_of",
];

/// A built fixture ledger, and what building it cost.
#[derive(Debug, Clone)]
pub struct FixtureLedger {
    path: PathBuf,
    profile: TemporalProfile,
    origin: Timestamp,
    horizon: Timestamp,
    events: u64,
    evidence: u64,
    actions: u64,
    checkpoints: u64,
    digest: String,
    built_in: WallDuration,
    bytes: u64,
    reused: bool,
}

impl FixtureLedger {
    /// Where the store is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The cardinality it was built at.
    #[must_use]
    pub const fn profile(&self) -> TemporalProfile {
        self.profile
    }

    /// The instant its history starts at.
    #[must_use]
    pub const fn origin(&self) -> Timestamp {
        self.origin
    }

    /// The instant its history ends at — the "now" a query against it is asked at.
    #[must_use]
    pub const fn horizon(&self) -> Timestamp {
        self.horizon
    }

    /// How many events it holds.
    #[must_use]
    pub const fn events(&self) -> u64 {
        self.events
    }

    /// How many evidence records it holds.
    #[must_use]
    pub const fn evidence(&self) -> u64 {
        self.evidence
    }

    /// How many action records it holds.
    #[must_use]
    pub const fn actions(&self) -> u64 {
        self.actions
    }

    /// How many checkpoints it holds.
    #[must_use]
    pub const fn checkpoints(&self) -> u64 {
        self.checkpoints
    }

    /// The digest over every event identity, in write order.
    ///
    /// This is what "deterministic" is checked against. Two builds from one seed agree here, and
    /// the figure is cheap enough to state beside a measurement — a benchmark record that names
    /// it says which history it was measured over.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// How long the construction took. Zero where the store was reused rather than rebuilt.
    #[must_use]
    pub const fn built_in(&self) -> WallDuration {
        self.built_in
    }

    /// How large the store is on disk, including its write-ahead log.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether an already-built store was reused rather than written again.
    #[must_use]
    pub const fn reused(&self) -> bool {
        self.reused
    }

    /// The root scope every place nests inside.
    #[must_use]
    pub fn root_scope(&self) -> SpatialScope {
        root_scope()
    }

    /// The scope of place `index`.
    #[must_use]
    pub fn place_scope(&self, index: usize) -> SpatialScope {
        place_scope(index)
    }

    /// The identity of object `index`.
    #[must_use]
    pub fn subject(&self, index: usize) -> SpatialId {
        subject(index)
    }

    /// Opens the built store for reading, with retention `policy`.
    ///
    /// # Errors
    ///
    /// Returns the reason the store could not be opened.
    pub fn open(&self, policy: RetentionPolicy) -> Result<LedgerStore, String> {
        LedgerStore::open_with(&StoreOptions::at(&self.path).with_retention(policy))
            .map_err(|error| format!("cannot open the fixture ledger: {error}"))
    }
}

/// The scope the fixture's history belongs to.
fn root_scope() -> SpatialScope {
    SpatialScope::host(FIXTURE_HOST, BootIdentity::new(FIXTURE_HOST, FIXTURE_BOOT))
}

/// The scope of place `index`, nested inside the host.
fn place_scope(index: usize) -> SpatialScope {
    root_scope().nest(ScopeKind::Container, &format!("service-{index:02}"))
}

/// The identity of object `index`, lifetime-bound the way §5.2 requires.
fn subject(index: usize) -> SpatialId {
    SpatialIdentity::lifetime(
        SpatialType::Process,
        [
            ("boot", FIXTURE_BOOT.to_owned()),
            ("pid", (1_000 + index).to_string()),
        ],
    )
    .spatial_id()
}

/// The clock domain every event of the fixture is timed in (§25.1).
fn domain() -> ClockDomain {
    ClockDomain {
        host: Arc::from(FIXTURE_HOST),
        boot_id: Some(Arc::from(FIXTURE_BOOT)),
    }
}

/// Where a value the fixture writes says it came from.
fn provenance(provider: &str, schema: &str) -> Provenance {
    Provenance::local(provider, SchemaId::new(schema, 1))
}

// ------------------------------------------------------------------------------------------
// TEST-001: builders that are valid by construction.
// ------------------------------------------------------------------------------------------

/// The times of an event observed at `at`, in the fixture's clock domain.
///
/// `source_sequence` is stated because the fixture's source is one that numbers its reports, and
/// §26.2 makes that the only ordering evidence two events of one source have. Leaving it out
/// would make every pair in the fixture `Concurrent` and quietly turn the causality benchmark
/// into a measurement of nothing.
#[must_use]
pub fn times(at: Timestamp, sequence: u64) -> EventTimes {
    EventTimes {
        source_time: Some(at),
        observed_at: at,
        ingested_at: at,
        source_sequence: Some(sequence),
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: domain(),
    }
}

/// An event seed carrying everything §3.3 requires and nothing it does not.
#[must_use]
pub fn event_seed(
    kind: EventKind,
    scope: SpatialScope,
    at: Timestamp,
    sequence: u64,
    provider: &str,
) -> EventSeed {
    EventSeed {
        kind,
        subtype: None,
        scope,
        subject: None,
        related: Vec::new(),
        times: times(at, sequence),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(provider, "ono.temporal-event"),
    }
}

/// A resolved subject reference — the canonical identity plus what it was called (§5.5).
#[must_use]
pub fn resolved(id: SpatialId, label: &str) -> SpatialRef {
    SpatialRef::Resolved {
        id,
        object_type: SpatialType::Process,
        label: Arc::from(label),
    }
}

/// Evidence for one field reading, with the identity §7.1 derives from its content.
///
/// The identity is a digest over source, instant, scope, subject and claim, so two sources
/// reporting the same reading collide rather than double-count — the same rule [`EventId`]
/// follows for events.
#[must_use]
pub fn field_evidence(
    source: &EvidenceSource,
    at: Timestamp,
    scope: &SpatialScope,
    subject: &SpatialId,
    field: &str,
    value: &str,
    strength: EvidenceStrength,
) -> Evidence {
    let claim = EvidenceClaim::FieldValue {
        field: Arc::from(field),
        value: Value::string(value),
        at,
    };
    Evidence {
        evidence_id: EvidenceId::of(source, at, scope, Some(subject), &claim),
        source: source.clone(),
        observed_at: at,
        source_time: Some(at),
        scope: scope.clone(),
        subject: Some(subject.clone()),
        claim,
        strength,
        raw_ref: None,
        derived_from: Vec::new(),
        provenance: provenance("ono.recorder", "ono.temporal-evidence"),
    }
}

/// A coverage interval one source claims over one capability (§8.1).
#[must_use]
pub fn coverage_interval(
    scope: SpatialScope,
    capability: &str,
    from: Timestamp,
    until: Timestamp,
    completeness: TemporalCompleteness,
) -> TemporalCoverage {
    TemporalCoverage {
        scope,
        capability: Arc::from(capability),
        from,
        until,
        completeness,
        sampling_interval: Some(ono_value::Duration::from_nanoseconds(i128::from(
            STEP_NANOS,
        ))),
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

/// A causal link, minted through the identity its rule, class and endpoints determine (§15.2).
#[must_use]
pub fn causal_link(
    rule: &str,
    relation: CausalRelation,
    cause: &TemporalEvent,
    effect: &TemporalEvent,
    evidence: Vec<EvidenceId>,
) -> CausalLink {
    let rule = CausalRuleId::new(rule);
    CausalLink {
        link_id: CausalLinkId::of(&rule, relation, &cause.event_id, &effect.event_id),
        relation,
        cause: cause.event_id.clone(),
        effect: effect.event_id.clone(),
        rule,
        evidence,
        strength: EvidenceStrength::Authoritative,
        source: EvidenceSource::recorder(),
    }
}

/// An action record as §17 shapes it, with its command already redacted (§17.5).
///
/// The raw command text never reaches this function: [`RedactedCommandSummary::of`] takes the
/// verb, the target and the arguments as [`Redactable`]s, so a secret-shaped argument is replaced
/// before the summary exists rather than scrubbed afterwards.
#[must_use]
pub fn action(
    at: Timestamp,
    session: &str,
    operation: &str,
    target: SpatialId,
    arguments: &[Redactable],
) -> ActionEvent {
    ActionEvent {
        action_id: ActionId::of(session, at, operation, Some(&target)),
        command: RedactedCommandSummary::of(operation, Some("service"), arguments),
        actor: Arc::from("fixture"),
        session_id: Arc::from(session),
        requested_at: at,
        target: Some(target),
        operation: Arc::from(operation),
        authorization: AuthorizationSummary {
            decision: AuthorizationDecision::Confirmed,
            risk: Arc::from("service_affecting"),
            capability: Some(Arc::from("service.manage")),
            reason: Some(Arc::from("a service restart interrupts its clients")),
        },
        result: None,
        external_transaction: Some(Arc::from("systemd:/org/freedesktop/systemd1/job/4821")),
        provenance: provenance("ono.session", "ono.temporal-action"),
    }
}

/// The canonical process record a checkpoint captures for object `index`.
///
/// # Panics
///
/// Panics if the process contract is missing from the built-in registry, which is a broken build.
#[must_use]
pub fn object_record(index: usize) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.process", 1))
        .expect("the process contract ships with the shell");
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("linux.procfs", schema.id().clone()),
    )
    .set("pid", Value::Int(i128::from(1_000 + index as i64)))
    .expect("pid is a declared field")
    .build()
}

/// A checkpoint of `scope` at `at`, holding a bounded slice of the world (§3.6).
///
/// # Panics
///
/// Panics if the built-in source classes do not name `linux.procfs`, which is a broken build.
#[must_use]
pub fn checkpoint(scope: &SpatialScope, at: Timestamp, first_object: usize) -> Checkpoint {
    let source = EvidenceSource::builtin("linux.procfs").expect("a built-in source class");
    let objects = (0..CHECKPOINT_OBJECTS)
        .map(|offset| {
            let index = first_object + offset;
            ObjectState {
                id: subject(index),
                object_type: SpatialType::Process,
                label: Arc::from(format!("process {}", 1_000 + index)),
                record: object_record(index),
                observed_at: at,
                source: source.clone(),
            }
        })
        .collect();
    let relations = (0..CHECKPOINT_RELATIONS)
        .map(|offset| {
            let index = first_object + offset;
            RelationState {
                from: subject(index),
                to: subject(index + 1),
                relation: Arc::from(RELATIONS[offset % RELATIONS.len()]),
                confidence: Confidence::Exact,
                observed_at: at,
                source: source.clone(),
            }
        })
        .collect();
    Checkpoint {
        checkpoint_id: CheckpointId::of(scope, at),
        scope: scope.clone(),
        captured_at: at,
        coverage: vec![coverage_interval(
            scope.clone(),
            "process.existence",
            at,
            at,
            TemporalCompleteness::Complete,
        )],
        objects,
        relations,
        provenance: provenance("ono.recorder", "ono.temporal-checkpoint"),
    }
}

// ------------------------------------------------------------------------------------------
// TEST-003: the fixture ledger itself.
// ------------------------------------------------------------------------------------------

/// What one event slot of the cycle is.
const fn kind_of(slot: usize) -> EventKind {
    match slot {
        0..=4 => EventKind::RelationAdded,
        5..=9 => EventKind::RelationRemoved,
        10..=14 => EventKind::ObjectChanged,
        15 | 16 => EventKind::ObjectAppeared,
        17 => EventKind::ObjectDisappeared,
        _ => EventKind::ObjectObserved,
    }
}

/// Builds — or reuses — the fixture ledger `profile` declares, under `root`.
///
/// The store is reused when a manifest beside it records the same seed, the same cardinality and
/// the same event digest. That is a cache with a correctness argument rather than a shortcut: the
/// digest is over the identities of every event in write order, so a manifest that matches
/// describes the same history, and one that does not is rebuilt.
///
/// # Errors
///
/// Returns the reason the store could not be created or written.
pub fn build(profile: TemporalProfile, root: &Path) -> Result<FixtureLedger, String> {
    let directory = root.join(format!("temporal-{}-{}", profile.name, profile.seed));
    let path = directory.join("ledger.sqlite3");
    let manifest_path = directory.join("fixture.json");

    if let Some(found) = reuse(&manifest_path, &path, profile) {
        return Ok(found);
    }
    // A partial store from an interrupted run would be measured as if it were complete.
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;

    let started = Instant::now();
    let written = write_fixture(profile, &path)?;
    let built_in = started.elapsed();
    let bytes = on_disk(&path);

    let fixture = FixtureLedger {
        path,
        profile,
        origin: written.origin,
        horizon: written.horizon,
        events: written.events,
        evidence: written.evidence,
        actions: written.actions,
        checkpoints: written.checkpoints,
        digest: written.digest,
        built_in,
        bytes,
        reused: false,
    };
    write_manifest(&manifest_path, &fixture)?;
    Ok(fixture)
}

/// What one build wrote.
struct Written {
    origin: Timestamp,
    horizon: Timestamp,
    events: u64,
    evidence: u64,
    actions: u64,
    checkpoints: u64,
    digest: String,
}

/// Writes the whole fixture through the ledger's public write path.
#[allow(
    clippy::too_many_lines,
    reason = "the generator is one linear description of one history; splitting it into per-kind \
              helpers would hide the cycle that fixes the mix of kinds"
)]
fn write_fixture(profile: TemporalProfile, path: &Path) -> Result<Written, String> {
    // Retention is unlimited *while building*: §10.4's default bounds would sweep the fixture's
    // oldest hours away as it was written, and the fixture's cardinality is §49's, not §10.4's.
    // Every measurement then opens the same store with whichever policy it is about.
    let store = LedgerStore::open_with(
        &StoreOptions::at(path)
            .with_retention(RetentionPolicy::unlimited())
            .with_durability(ono_temporal_ledger::Durability::Normal),
    )
    .map_err(|error| format!("cannot create the fixture ledger: {error}"))?;

    let clock = Clock::fixed();
    let origin = clock.now();
    let mut rng = Rng::seeded(profile.seed);
    let mut digest = sha2::Sha256::new();
    let recorder = EvidenceSource::recorder();

    let mut appeared = 0usize;
    let mut events = Vec::with_capacity(BATCH);
    let mut evidence = Vec::with_capacity(BATCH);
    let mut written_events = 0u64;
    let mut written_evidence = 0u64;

    for index in 0..profile.events {
        let at = origin
            .checked_add(Span::new().nanoseconds(index as i64 * STEP_NANOS))
            .map_err(|error| format!("the fixture window left the representable range: {error}"))?;
        let kind = kind_of(index % CYCLE);
        // Every object appears exactly once, so the fixture holds exactly the declared number of
        // lifetimes rather than however many a random draw happened to touch.
        let object = if kind == EventKind::ObjectAppeared {
            let object = appeared % profile.objects;
            appeared += 1;
            object
        } else {
            rng.below(profile.objects)
        };
        let scope = place_scope(object % PLACES);
        let label = format!("process {}", 1_000 + object);
        let mut seed = event_seed(kind, scope.clone(), at, index as u64, "linux.procfs");
        seed.subject = Some(resolved(subject(object), &label));

        match kind {
            EventKind::RelationAdded | EventKind::RelationRemoved => {
                let other = (object + 1 + rng.below(64)) % profile.objects;
                seed.related = vec![resolved(
                    subject(other),
                    &format!("process {}", 1_000 + other),
                )];
                let mut payload = MapValue::new();
                payload.insert(
                    Arc::from("relation"),
                    Value::string(RELATIONS[index % RELATIONS.len()]),
                );
                payload.insert(Arc::from("confidence"), Value::string("exact"));
                seed.payload = Some(Value::Map(Arc::new(payload)));
            }
            EventKind::ObjectChanged => {
                let field = FIELDS[rng.below(FIELDS.len())];
                let before = if rng.chance(2) {
                    "active"
                } else {
                    "activating"
                };
                let after = if before == "active" {
                    "failed"
                } else {
                    "active"
                };
                let record = field_evidence(
                    &recorder,
                    at,
                    &scope,
                    &subject(object),
                    field,
                    after,
                    EvidenceStrength::Authoritative,
                );
                seed.changed_fields = vec![FieldChange {
                    field: Arc::from(field),
                    before: Some(Value::string(before)),
                    after: Some(Value::string(after)),
                    certainty: ChangeCertainty::Observed,
                }];
                seed.evidence = vec![record.evidence_id.clone()];
                evidence.push(record);
            }
            EventKind::ObjectAppeared | EventKind::ObjectDisappeared => {
                let record = field_evidence(
                    &recorder,
                    at,
                    &scope,
                    &subject(object),
                    "state",
                    if kind == EventKind::ObjectAppeared {
                        "running"
                    } else {
                        "gone"
                    },
                    EvidenceStrength::Authoritative,
                );
                seed.evidence = vec![record.evidence_id.clone()];
                evidence.push(record);
            }
            _ => {}
        }

        let event = seed.seal();
        digest.update(event.event_id.as_str().as_bytes());
        events.push(event);

        if events.len() == BATCH {
            written_evidence += flush(&store, &mut events, &mut evidence)?;
            written_events += BATCH as u64;
        }
    }
    if !events.is_empty() {
        written_events += events.len() as u64;
        written_evidence += flush(&store, &mut events, &mut evidence)?;
    }

    let span_nanos = profile.events as i64 * STEP_NANOS;
    let horizon = origin
        .checked_add(Span::new().nanoseconds(span_nanos))
        .map_err(|error| format!("the fixture horizon left the representable range: {error}"))?;

    // §8.1: coverage is per scope, per capability, per interval. One interval per place per hour
    // per capability, because a recorder that ran for a day wrote a day of intervals rather than
    // one, and composing them is part of what a timeline pays for.
    let mut intervals = Vec::new();
    for place in 0..PLACES {
        let scope = place_scope(place);
        for hour in 0..24 {
            let from = origin
                .checked_add(Span::new().hours(hour))
                .map_err(|error| format!("a coverage interval left the range: {error}"))?;
            let until = origin
                .checked_add(Span::new().hours(hour + 1))
                .map_err(|error| format!("a coverage interval left the range: {error}"))?;
            for capability in ["process.existence", "relation:process.owns_socket"] {
                intervals.push(coverage_interval(
                    scope.clone(),
                    capability,
                    from,
                    until,
                    TemporalCompleteness::Complete,
                ));
            }
        }
    }
    store
        .record_coverage(&intervals)
        .map_err(|error| format!("cannot record coverage: {error}"))?;

    // §31.9's five-minute cadence, in the root scope, so a reconstruction of the host finds one.
    let root = root_scope();
    let mut checkpoints = 0u64;
    let mut minute = 0i64;
    while minute * 60 * 1_000_000_000 < span_nanos {
        let at = origin
            .checked_add(Span::new().minutes(minute))
            .map_err(|error| format!("a checkpoint instant left the range: {error}"))?;
        let first = (checkpoints as usize * CHECKPOINT_OBJECTS) % profile.objects;
        store
            .write_checkpoint(&checkpoint(&root, at, first))
            .map_err(|error| format!("cannot write a checkpoint: {error}"))?;
        checkpoints += 1;
        minute += CHECKPOINT_MINUTES;
    }

    // §17: the action records, spread across the window.
    let action_step = span_nanos / profile.actions.max(1) as i64;
    for index in 0..profile.actions {
        let at = origin
            .checked_add(Span::new().nanoseconds(index as i64 * action_step))
            .map_err(|error| format!("an action instant left the range: {error}"))?;
        let target = subject(rng.below(profile.objects));
        store
            .record_action(&action(
                at,
                "fixture-session",
                "restart",
                target,
                &[
                    Redactable::plain("nginx.service"),
                    Redactable::secret("--token", "hunter2"),
                ],
            ))
            .map_err(|error| format!("cannot record an action: {error}"))?;
    }

    store
        .flush()
        .map_err(|error| format!("cannot flush the fixture ledger: {error}"))?;

    Ok(Written {
        origin,
        horizon,
        events: written_events,
        evidence: written_evidence,
        actions: profile.actions as u64,
        checkpoints,
        digest: hex(&digest.finalize()),
    })
}

/// Appends one batch and empties the buffers, answering with how much evidence went with it.
fn flush(
    store: &LedgerStore,
    events: &mut Vec<TemporalEvent>,
    evidence: &mut Vec<Evidence>,
) -> Result<u64, String> {
    let count = evidence.len() as u64;
    store
        .append(events, evidence)
        .map_err(|error| format!("cannot append a batch to the fixture ledger: {error}"))?;
    events.clear();
    evidence.clear();
    Ok(count)
}

/// The manifest beside a built store, or `None` where it describes a different fixture.
fn reuse(manifest: &Path, path: &Path, profile: TemporalProfile) -> Option<FixtureLedger> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let document: serde_json::Value = serde_json::from_str(&text).ok()?;
    let same = document.get("seed")?.as_u64()? == profile.seed
        && document.get("declared_events")?.as_u64()? == profile.events as u64
        && document.get("declared_objects")?.as_u64()? == profile.objects as u64
        && document.get("declared_actions")?.as_u64()? == profile.actions as u64;
    if !same || !path.is_file() {
        return None;
    }
    let clock = Clock::fixed();
    let origin = clock.now();
    Some(FixtureLedger {
        path: path.to_path_buf(),
        profile,
        origin,
        horizon: origin
            .checked_add(Span::new().nanoseconds(profile.events as i64 * STEP_NANOS))
            .ok()?,
        events: document.get("events")?.as_u64()?,
        evidence: document.get("evidence")?.as_u64()?,
        actions: document.get("actions")?.as_u64()?,
        checkpoints: document.get("checkpoints")?.as_u64()?,
        digest: document.get("digest")?.as_str()?.to_owned(),
        built_in: WallDuration::from_secs_f64(document.get("built_in_seconds")?.as_f64()?),
        bytes: document.get("bytes")?.as_u64()?,
        reused: true,
    })
}

/// Records what was built beside the store it was built into.
fn write_manifest(path: &Path, fixture: &FixtureLedger) -> Result<(), String> {
    let document = serde_json::json!({
        "note": "The deterministic fixture ledger of v0.5 section 49, written by \
                 `cargo run -p xtask -- perf`. `digest` is the SHA-256 of every event identity in \
                 write order: two builds from one seed agree on it, and a build that does not is \
                 a different history.",
        "profile": fixture.profile.name,
        "seed": fixture.profile.seed,
        "declared_events": fixture.profile.events as u64,
        "declared_objects": fixture.profile.objects as u64,
        "declared_relation_changes": fixture.profile.relation_changes as u64,
        "declared_actions": fixture.profile.actions as u64,
        "events": fixture.events,
        "evidence": fixture.evidence,
        "actions": fixture.actions,
        "checkpoints": fixture.checkpoints,
        "digest": fixture.digest,
        "built_in_seconds": fixture.built_in.as_secs_f64(),
        "bytes": fixture.bytes,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot render the fixture manifest: {error}"))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// How large the store is, counting the write-ahead log and the shared-memory index beside it.
fn on_disk(path: &Path) -> u64 {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let mut candidate = path.as_os_str().to_owned();
        candidate.push(suffix);
        if let Ok(meta) = std::fs::metadata(std::path::PathBuf::from(candidate)) {
            total += meta.len();
        }
    }
    total
}

/// A digest as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
