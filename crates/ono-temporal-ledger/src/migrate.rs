//! Versioned, forward-only store migrations (v0.5 §31.6, §44.3).
//!
//! §31.6: "store schema migrations MUST be versioned and tested against fixtures from every
//! shipped v0.5 store version", and migration "MUST preserve `EventId`, `EvidenceId`, `ActionId`
//! and causal references". Both properties come from the same rule: a migration adds structure and
//! never rewrites an identity. The identities are content digests (ADR-0620), so a migration that
//! recomputed one would have changed what the event *is*, which §6.7 forbids outside a migration
//! "that preserves semantic identity".
//!
//! The steps are a list, applied in order, each inside its own transaction. A store at version `n`
//! reaching an Ono that ships version `m > n` runs steps `n+1 ..= m`; a store at a version this
//! Ono does not know is refused rather than downgraded, because §44.3 requires the shell to keep
//! working with temporal persistence disabled instead of writing new-version events into an old
//! store.

/// One forward migration step: the version it produces and the statements that produce it.
pub(crate) struct Step {
    /// The store version this step leaves behind.
    pub(crate) version: u32,
    /// What the step does. Executed as one batch inside one transaction.
    pub(crate) sql: &'static str,
}

/// The version a freshly created store carries, and the version every older store migrates to.
pub const STORE_VERSION: u32 = 3;

/// The metadata key holding the store's schema version.
pub(crate) const VERSION_KEY: &str = "store_version";
/// The metadata key holding the instant the store was created.
pub(crate) const CREATED_KEY: &str = "created_at";
/// The metadata key holding the identity of this store, so a diagnostic can name it (§31.7).
pub(crate) const STORE_ID_KEY: &str = "store_id";

/// The ten logical sets of §31.3, in the order the specification lists them.
///
/// §31.3 permits a different physical normalisation and permits no missing set, so this is the
/// list `LedgerStore::logical_sets` answers against.
pub const LOGICAL_SETS: &[&str] = &[
    "events",
    "evidence",
    "causal_links",
    "coverage_intervals",
    "checkpoints",
    "checkpoint_objects",
    "checkpoint_relations",
    "source_sequences",
    "actions",
    "metadata",
];

/// Version 1: the ten logical sets of §31.3 and the indexes §32.3's budgets need.
///
/// The normalisation is the one §31.4 asks for. Every scalar a query filters or orders on is a
/// column — the presentation instant, the scope path, the kind, the subject — and everything else
/// is one CBOR payload per row. The join tables exist because retention has to find an orphan
/// without decoding a payload (§31.8): an evidence record no surviving event or link cites is a
/// row count, not a scan.
///
/// `scope_path` is the one column whose shape is a decision rather than a copy. It renders a
/// scope and its ancestors outermost first with a terminator after each level, so
/// `SpatialScope::contains` is a prefix comparison and `timeline` for one place is an index range
/// scan rather than a filter over every event (§32.3's 100 ms budget).
const V1: &str = "
CREATE TABLE metadata (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE events (
    event_id           TEXT PRIMARY KEY NOT NULL,
    kind               TEXT NOT NULL,
    scope_path         TEXT NOT NULL,
    subject            TEXT,
    presentation_nanos INTEGER NOT NULL,
    source_nanos       INTEGER,
    observed_nanos     INTEGER NOT NULL,
    ingested_nanos     INTEGER NOT NULL,
    source_sequence    INTEGER,
    monotonic_nanos    INTEGER,
    uncertainty_nanos  INTEGER,
    domain_host        TEXT NOT NULL,
    domain_boot        TEXT,
    source             TEXT NOT NULL,
    body               BLOB NOT NULL
) STRICT;

CREATE INDEX events_by_place ON events (scope_path, presentation_nanos);
CREATE INDEX events_by_time  ON events (presentation_nanos);
CREATE INDEX events_by_kind  ON events (kind, presentation_nanos);

CREATE TABLE event_subjects (
    event_id           TEXT NOT NULL REFERENCES events (event_id) ON DELETE CASCADE,
    subject            TEXT NOT NULL,
    role               TEXT NOT NULL,
    presentation_nanos INTEGER NOT NULL,
    PRIMARY KEY (subject, presentation_nanos, event_id, role)
) STRICT;

CREATE INDEX event_subjects_by_event ON event_subjects (event_id);

CREATE TABLE evidence (
    evidence_id    TEXT PRIMARY KEY NOT NULL,
    source         TEXT NOT NULL,
    observed_nanos INTEGER NOT NULL,
    scope_path     TEXT NOT NULL,
    subject        TEXT,
    strength       TEXT NOT NULL,
    body           BLOB NOT NULL
) STRICT;

CREATE INDEX evidence_by_time ON evidence (observed_nanos);

CREATE TABLE event_evidence (
    event_id    TEXT NOT NULL REFERENCES events (event_id) ON DELETE CASCADE,
    evidence_id TEXT NOT NULL,
    position    INTEGER NOT NULL,
    PRIMARY KEY (event_id, position)
) STRICT;

CREATE INDEX event_evidence_by_evidence ON event_evidence (evidence_id);

CREATE TABLE evidence_derivation (
    evidence_id  TEXT NOT NULL REFERENCES evidence (evidence_id) ON DELETE CASCADE,
    derived_from TEXT NOT NULL,
    position     INTEGER NOT NULL,
    PRIMARY KEY (evidence_id, position)
) STRICT;

CREATE INDEX evidence_derivation_by_parent ON evidence_derivation (derived_from);

CREATE TABLE causal_links (
    link_id  TEXT PRIMARY KEY NOT NULL,
    relation TEXT NOT NULL,
    cause    TEXT NOT NULL REFERENCES events (event_id) ON DELETE CASCADE,
    effect   TEXT NOT NULL REFERENCES events (event_id) ON DELETE CASCADE,
    rule     TEXT NOT NULL,
    strength TEXT NOT NULL,
    source   TEXT NOT NULL
) STRICT;

CREATE INDEX causal_links_by_effect ON causal_links (effect);
CREATE INDEX causal_links_by_cause  ON causal_links (cause);

CREATE TABLE causal_link_evidence (
    link_id     TEXT NOT NULL REFERENCES causal_links (link_id) ON DELETE CASCADE,
    evidence_id TEXT NOT NULL,
    position    INTEGER NOT NULL,
    PRIMARY KEY (link_id, position)
) STRICT;

CREATE INDEX causal_link_evidence_by_evidence ON causal_link_evidence (evidence_id);

CREATE TABLE coverage_intervals (
    scope_path     TEXT NOT NULL,
    capability     TEXT NOT NULL,
    from_nanos     INTEGER NOT NULL,
    until_nanos    INTEGER NOT NULL,
    completeness   TEXT NOT NULL,
    sampling_nanos INTEGER,
    source         TEXT NOT NULL,
    permission     TEXT NOT NULL,
    PRIMARY KEY (scope_path, capability, source, from_nanos, until_nanos, completeness)
) STRICT;

CREATE INDEX coverage_by_time ON coverage_intervals (from_nanos, until_nanos);

CREATE TABLE checkpoints (
    checkpoint_id  TEXT PRIMARY KEY NOT NULL,
    scope_path     TEXT NOT NULL,
    captured_nanos INTEGER NOT NULL,
    body           BLOB NOT NULL
) STRICT;

CREATE INDEX checkpoints_by_place ON checkpoints (scope_path, captured_nanos);

CREATE TABLE checkpoint_objects (
    checkpoint_id  TEXT NOT NULL REFERENCES checkpoints (checkpoint_id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    spatial_id     TEXT NOT NULL,
    object_type    TEXT NOT NULL,
    label          TEXT NOT NULL,
    observed_nanos INTEGER NOT NULL,
    source         TEXT NOT NULL,
    body           BLOB NOT NULL,
    PRIMARY KEY (checkpoint_id, position)
) STRICT;

CREATE TABLE checkpoint_relations (
    checkpoint_id  TEXT NOT NULL REFERENCES checkpoints (checkpoint_id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    from_id        TEXT NOT NULL,
    to_id          TEXT NOT NULL,
    relation       TEXT NOT NULL,
    confidence     TEXT NOT NULL,
    observed_nanos INTEGER NOT NULL,
    source         TEXT NOT NULL,
    PRIMARY KEY (checkpoint_id, position)
) STRICT;

CREATE TABLE source_sequences (
    source        TEXT NOT NULL,
    domain_host   TEXT NOT NULL,
    domain_boot   TEXT NOT NULL,
    highest       INTEGER NOT NULL,
    lowest        INTEGER NOT NULL,
    seen          INTEGER NOT NULL,
    contiguous    INTEGER NOT NULL,
    declared      INTEGER NOT NULL,
    last_nanos    INTEGER NOT NULL,
    scope_path    TEXT NOT NULL,
    PRIMARY KEY (source, domain_host, domain_boot)
) STRICT;

CREATE TABLE actions (
    action_id            TEXT PRIMARY KEY NOT NULL,
    requested_nanos      INTEGER NOT NULL,
    actor                TEXT NOT NULL,
    session_id           TEXT NOT NULL,
    operation            TEXT NOT NULL,
    target               TEXT,
    external_transaction TEXT,
    body                 BLOB NOT NULL
) STRICT;

CREATE INDEX actions_by_time ON actions (requested_nanos);
";

/// Version 2: the significance flag §18.4's event stepping and §27.1's landmarks read.
///
/// "Significant event stepping" needs to move to the next *significant* event without decoding
/// every payload in between, and a landmark transition is exactly such an event. The flag is a
/// column with its own index because that is the query: give me the next significant event in this
/// place after this instant. Existing rows take the default, which says only that nothing has yet
/// classified them — a v1 store migrates without any event changing what it is.
const V2: &str = "
ALTER TABLE events ADD COLUMN significance INTEGER NOT NULL DEFAULT 0;
CREATE INDEX events_by_significance ON events (scope_path, significance, presentation_nanos);
";

/// Version 3: the index that lets a place-scoped query walk time instead of sorting it.
///
/// `events_by_place` leads on `scope_path`, and every scoped query bounds the scope as a *prefix
/// range* rather than an equality, because a place includes everything under it. An index leading
/// on a range gives no usable order for the `ORDER BY presentation_nanos` that follows, so SQLite
/// answered §32.3's queries with `USE TEMP B-TREE FOR ORDER BY`: it read every event in the place,
/// sorted all of them, and then took the hundred it had been asked for. On the §49 fixture that
/// was 62,500 rows sorted to answer a `why` about a hundred, and it is the whole of why the row
/// missed its budget.
///
/// Leading on `presentation_nanos` and carrying `scope_path` beside it inverts that: the walk is
/// in the order the query asks for, the scope is tested from the index rather than from the table,
/// and the walk stops at the limit. Both indexes stay — `events_by_place` still serves a query
/// that wants everything about a place regardless of order (ADR-0776).
const V3: &str = "
CREATE INDEX events_by_time_place ON events (presentation_nanos, scope_path);
";

/// Every migration, in order.
pub(crate) const STEPS: &[Step] = &[
    Step {
        version: 1,
        sql: V1,
    },
    Step {
        version: 2,
        sql: V2,
    },
    Step {
        version: 3,
        sql: V3,
    },
];
