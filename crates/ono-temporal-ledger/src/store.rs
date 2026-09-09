//! The persistent temporal ledger: SQLite in WAL mode at the canonical user-private path
//! (v0.5 §31).
//!
//! §31.1 makes SQLite in WAL mode normative for the reference implementation, "so tests and
//! recovery behavior are deterministic". §31.5 fixes the durability contract: WAL and transactions,
//! a process crash losing nothing committed, and a power loss possibly losing the most recent
//! unflushed interval while never corrupting what came before. `synchronous = NORMAL` is exactly
//! that contract and nothing stronger — ADR-0631 records why.
//!
//! Everything a caller sees is [`LedgerRead`] and [`LedgerWrite`]; the SQL never leaves this
//! module, which is what §39.4 asks of the layer above it.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialScope};
use ono_temporal_core::{
    ActionEvent, Appended, CausalLink, Checkpoint, CoverageQuery, EventId, EventQuery, Evidence,
    EvidenceId, EvidenceSource, GapReason, LedgerRead, LedgerWrite, QueryOrder, RetentionState,
    TemporalCompleteness, TemporalCoverage, TemporalEvent, TimeRange, distinguishing_length, error,
};
use ono_value::{ByteSize, ErrorValue, SchemaRegistry, builtin_schemas};
use rusqlite::types::Value::{Integer, Text};
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, Row, Transaction, params};

use crate::codec::{Decoded, corrupt, read_scope_path, scope_path, scope_upper_bound};
use crate::integrity::{IntegrityFinding, IntegrityReport};
use crate::migrate::{CREATED_KEY, STEPS, STORE_ID_KEY, STORE_VERSION, VERSION_KEY};
use crate::retention::{RetentionPolicy, Swept};
use crate::rows::{
    ActionColumns, CheckpointColumns, CoverageColumns, EventColumns, EvidenceColumns, LinkColumns,
    ObjectColumns, RelationColumns, action_columns, checkpoint_columns, coverage_columns,
    event_columns, evidence_columns, from_nanos, link_columns, nanos, object_columns, read_action,
    read_checkpoint, read_coverage, read_event, read_evidence, read_link, read_object,
    read_relation, relation_columns,
};
use crate::sequences::SourceSequence;

/// How long a writer waits for another process's transaction before refusing (§32.6).
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// The capability a retention boundary covers: all of them, which is what expiry removes.
const RETENTION_CAPABILITY: &str = "*";

/// How the store trades durability for speed (§31.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    /// `synchronous = NORMAL`: §31.5's own contract, and the default (ADR-0631).
    ///
    /// A process crash loses nothing committed, because the write-ahead log is a file the
    /// operating system holds whether or not Ono is alive. A power loss may lose the most recent
    /// unflushed interval, which §31.5 permits, and cannot corrupt earlier history, because WAL
    /// recovery replays whole frames.
    #[default]
    Normal,
    /// `synchronous = FULL`: every commit reaches the platter before it is acknowledged.
    ///
    /// Stronger than §31.5 requires and costs a device flush per commit, so it is offered rather
    /// than imposed — an installation on unreliable power may want it.
    Full,
}

impl Durability {
    /// The SQLite setting this durability is.
    const fn pragma(self) -> &'static str {
        match self {
            Durability::Normal => "NORMAL",
            Durability::Full => "FULL",
        }
    }
}

/// How to open a store.
#[derive(Debug, Clone)]
pub struct StoreOptions {
    path: PathBuf,
    retention: RetentionPolicy,
    durability: Durability,
    schemas: Option<SchemaRegistry>,
}

impl StoreOptions {
    /// A store at `path`, with §10.4's retention defaults and §31.5's durability.
    #[must_use]
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            retention: RetentionPolicy::default(),
            durability: Durability::default(),
            schemas: None,
        }
    }

    /// The same options with a different retention policy (§10.4).
    #[must_use]
    pub fn with_retention(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// The same options with a different durability setting (§31.5).
    #[must_use]
    pub fn with_durability(mut self, durability: Durability) -> Self {
        self.durability = durability;
        self
    }

    /// The same options resolving record schemas through `schemas` rather than the built-in set.
    ///
    /// A stored record carries its schema id; reading it back needs the contract behind that id.
    /// The built-in registry answers for everything Ono ships, and a host that has loaded
    /// contributed schemas passes a registry that also answers for those.
    #[must_use]
    pub fn with_schemas(mut self, schemas: SchemaRegistry) -> Self {
        self.schemas = Some(schemas);
        self
    }

    /// Where the store is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The persistent temporal ledger (§31).
#[derive(Debug)]
pub struct LedgerStore {
    path: PathBuf,
    retention: RetentionPolicy,
    schemas: SchemaRegistry,
    integrity: Mutex<IntegrityReport>,
    version: u32,
    connection: Mutex<Connection>,
}

impl LedgerStore {
    /// Opens or creates the store at `path`, with §10.4's retention and §31.5's durability.
    ///
    /// The directory is created `0700` and the database `0600` (§30.2), and the store is migrated
    /// forward to [`STORE_VERSION`] before anything is written into it (§44.3).
    ///
    /// # Errors
    ///
    /// Returns `temporal.store_unavailable` where the path cannot be created or opened, and
    /// `temporal.store_corrupt` where the store is damaged beyond use (§31.7).
    pub fn open(path: &Path) -> Result<Self, ErrorValue> {
        Self::open_with(&StoreOptions::at(path))
    }

    /// Opens or creates the store described by `options`.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::open`].
    pub fn open_with(options: &StoreOptions) -> Result<Self, ErrorValue> {
        let path = options.path.clone();
        if let Some(directory) = path.parent()
            && !directory.as_os_str().is_empty()
        {
            crate::path::create_private_directory(directory)?;
        }
        crate::path::create_private_file(&path)?;

        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| unavailable(&path, &error))?;

        connection
            .busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))
            .map_err(|error| unavailable(&path, &error))?;
        // WAL answers with the mode it selected, so it is a query rather than a statement.
        let mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(|error| unavailable(&path, &error))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(error::store_unavailable(&format!(
                "`{}` refused write-ahead logging and reports `{mode}`; §31.5 requires it",
                path.display()
            )));
        }
        connection
            .execute_batch(&format!(
                "PRAGMA synchronous = {};\nPRAGMA foreign_keys = ON;\n\
                 PRAGMA auto_vacuum = INCREMENTAL;",
                options.durability.pragma()
            ))
            .map_err(|error| unavailable(&path, &error))?;

        let mut integrity = IntegrityReport::default();
        check_file_integrity(&connection, &path, &mut integrity);
        if integrity.is_fatal() {
            return Err(error::store_corrupt(
                &path.display().to_string(),
                &integrity.guidance(),
            ));
        }

        let mut connection = connection;
        let version = migrate(&mut connection, &path)?;

        Ok(Self {
            path,
            retention: options.retention,
            schemas: options
                .schemas
                .clone()
                .unwrap_or_else(|| builtin_schemas().clone()),
            integrity: Mutex::new(integrity),
            version,
            connection: Mutex::new(connection),
        })
    }

    /// Where the store is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema version the store is at (§31.6).
    #[must_use]
    pub const fn store_version(&self) -> u32 {
        self.version
    }

    /// The retention policy in force (§10.4).
    #[must_use]
    pub const fn policy(&self) -> RetentionPolicy {
        self.retention
    }

    /// What is wrong with the store, from opening it and from every row that has since refused
    /// to decode (§31.7).
    ///
    /// The report grows during reads because §31.7's second and fifth obligations are about
    /// *segments*: a database SQLite opens happily may still hold one payload a scribble has made
    /// unreadable, and the only place that is discovered is the read that needed it.
    #[must_use]
    pub fn integrity(&self) -> IntegrityReport {
        self.integrity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every gap the store knows of: intervals discarded as corrupt (§31.7) and breaks in a
    /// sequence a source declared contiguous (§43.2).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn gaps(&self) -> Result<Vec<ono_temporal_core::TemporalGap>, ErrorValue> {
        let mut gaps = self.integrity().gaps;
        gaps.extend(self.sequence_gaps()?);
        Ok(gaps)
    }

    /// Records a row that would not decode, and the interval its loss costs (§31.7).
    fn note_corrupt(&self, segment: &str, detail: &str, scope: &str, from: i64, until: i64) {
        let Ok(scope) = read_scope_path(scope) else {
            let mut report = self
                .integrity
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            report
                .findings
                .push(IntegrityFinding::new(segment, detail, false));
            return;
        };
        let (Ok(from), Ok(until)) = (from_nanos(from), from_nanos(until)) else {
            let mut report = self
                .integrity
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            report
                .findings
                .push(IntegrityFinding::new(segment, detail, false));
            return;
        };
        let mut report = self
            .integrity
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        report.note(
            IntegrityFinding::new(segment, detail, false),
            &scope,
            from,
            until,
            EvidenceSource::recorder(),
        );
    }

    /// The logical sets of §31.3 the store actually holds.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal where the catalogue cannot be read.
    pub fn logical_sets(&self) -> Result<BTreeSet<String>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| self.unavailable(&error))?;
        let mut held = BTreeSet::new();
        for name in rows {
            held.insert(name.map_err(|error| self.unavailable(&error))?);
        }
        Ok(held)
    }

    /// How SQLite plans the timeline query of §32.3, so the index is a test rather than a comment.
    ///
    /// §32.3 budgets 100 ms p95 for a fifteen-minute timeline of one place. That budget is met by
    /// the `events_by_place` index and by nothing else, so the plan is something a test can assert
    /// on rather than a property a reviewer has to take on trust.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal where the planner cannot be consulted.
    pub fn explain_timeline_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT event_id FROM events \
             WHERE scope_path >= ?1 AND scope_path < ?2 \
               AND presentation_nanos >= ?3 AND presentation_nanos < ?4 \
             ORDER BY presentation_nanos ASC LIMIT 500",
            &[
                Text("host\u{1e}web01\u{1e}web01/boot\u{1f}".to_owned()),
                Text("host\u{1e}web01\u{1e}web01/boot\u{20}".to_owned()),
                Integer(0),
                Integer(i64::MAX),
            ],
        )
    }

    /// How SQLite plans the one-hour changes query of §32.3.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::explain_timeline_plan`].
    pub fn explain_changes_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT event_id FROM events \
             WHERE presentation_nanos >= ?1 AND presentation_nanos < ?2 \
               AND kind IN ('object.changed', 'relation.added') \
             ORDER BY presentation_nanos ASC",
            &[Integer(0), Integer(i64::MAX)],
        )
    }

    /// How SQLite plans the nearest-checkpoint-before lookup of §9.1.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::explain_timeline_plan`].
    pub fn explain_checkpoint_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT checkpoint_id FROM checkpoints \
             WHERE scope_path >= ?1 AND scope_path < ?2 AND captured_nanos <= ?3 \
             ORDER BY captured_nanos DESC LIMIT 1",
            &[
                Text("host\u{1e}web01\u{1e}web01/boot\u{1f}".to_owned()),
                Text("host\u{1e}web01\u{1e}web01/boot\u{20}".to_owned()),
                Integer(i64::MAX),
            ],
        )
    }

    /// How SQLite plans the causal-links-by-effect lookup of §16.7.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::explain_timeline_plan`].
    pub fn explain_causal_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT link_id FROM causal_links WHERE effect = ?1",
            &[Text("e00000000000000000000000".to_owned())],
        )
    }

    /// How SQLite plans the event-by-identity lookup of §11.6.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::explain_timeline_plan`].
    pub fn explain_event_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT kind FROM events WHERE event_id >= ?1 AND event_id < ?2 LIMIT 2",
            &[Text("e0000".to_owned()), Text("e0000\u{10FFFF}".to_owned())],
        )
    }

    /// How SQLite plans retention's oldest-first sweep of §31.8.
    ///
    /// # Errors
    ///
    /// As [`LedgerStore::explain_timeline_plan`].
    pub fn explain_retention_plan(&self) -> Result<Vec<String>, ErrorValue> {
        self.explain(
            "SELECT event_id FROM events WHERE presentation_nanos < ?2 \
             ORDER BY presentation_nanos ASC, event_id ASC LIMIT ?1",
            &[Integer(256), Integer(i64::MAX)],
        )
    }

    fn explain(
        &self,
        sql: &str,
        binds: &[rusqlite::types::Value],
    ) -> Result<Vec<String>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(binds.iter()), |row| {
                row.get::<_, String>(3)
            })
            .map_err(|error| self.unavailable(&error))?;
        let mut plan = Vec::new();
        for step in rows {
            plan.push(step.map_err(|error| self.unavailable(&error))?);
        }
        Ok(plan)
    }

    /// The state of every source sequence the store has seen (§25.4, §44.1).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn source_sequences(&self) -> Result<Vec<SourceSequence>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(
                "SELECT source, domain_host, domain_boot, lowest, highest, seen, contiguous, \
                        declared, last_nanos, scope_path \
                 FROM source_sequences ORDER BY source, domain_host, domain_boot",
            )
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map([], read_sequence_row)
            .map_err(|error| self.unavailable(&error))?;
        let mut sequences = Vec::new();
        for row in rows {
            let row = row.map_err(|error| self.unavailable(&error))?;
            sequences.push(row.map_err(|detail| corrupt("source_sequences", &detail))?);
        }
        Ok(sequences)
    }

    /// The sequence state of one source in one clock domain (§44.1's second restart step).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn source_sequence(
        &self,
        source: &EvidenceSource,
        host: &str,
        boot: Option<&str>,
    ) -> Result<Option<SourceSequence>, ErrorValue> {
        Ok(self.source_sequences()?.into_iter().find(|sequence| {
            &sequence.source == source
                && sequence.domain.host.as_ref() == host
                && sequence.domain.boot_id.as_deref() == boot
        }))
    }

    /// The gaps a break in a declared-contiguous sequence leaves (§43.2, §7.5).
    ///
    /// A source that declares its sequence contiguous and then skips a number lost events, and
    /// §43.2 prefers an explicit gap to pretended continuity. `reason` is
    /// `source_disconnected` where the store recorded a disconnection and `not_recorded`
    /// otherwise.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn sequence_gaps(&self) -> Result<Vec<ono_temporal_core::TemporalGap>, ErrorValue> {
        let mut gaps = Vec::new();
        for sequence in self.source_sequences()? {
            if !sequence.has_lost_events() {
                continue;
            }
            let reason = if sequence.source.is_remote() {
                GapReason::SourceDisconnected
            } else {
                GapReason::NotRecorded
            };
            gaps.push(sequence.gap(sequence.last_seen_at, reason));
        }
        Ok(gaps)
    }

    /// Declares that `source` numbers its events contiguously in this clock domain (§21.5, §43.2).
    ///
    /// Only a source that makes this claim can lose events by skipping a number; a source that
    /// never claimed it may legitimately have holes.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn declare_contiguous(
        &self,
        source: &EvidenceSource,
        host: &str,
        boot: Option<&str>,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<(), ErrorValue> {
        let connection = self.locked();
        let at = nanos(at).map_err(|detail| corrupt("source_sequences", &detail))?;
        connection
            .execute(
                "INSERT INTO source_sequences \
                    (source, domain_host, domain_boot, highest, lowest, seen, contiguous, \
                     declared, last_nanos, scope_path) \
                 VALUES (?1, ?2, ?3, 0, 0, 0, 1, 1, ?4, ?5) \
                 ON CONFLICT (source, domain_host, domain_boot) DO UPDATE SET declared = 1",
                params![
                    source.as_str(),
                    host,
                    boot.unwrap_or_default(),
                    at,
                    scope_path(scope)
                ],
            )
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }

    /// Removes expired history, oldest first, leaving no dangling reference (§10.4, §31.8).
    ///
    /// `now` is a parameter because §39.2 keeps the clock out of everything but the recorder and
    /// the shell. One call removes at most [`RetentionPolicy::batch`] events and reports whether
    /// the bounds now hold, so the caller drives it from a timer instead of blocking on it.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn sweep(&self, now: Timestamp) -> Result<Swept, ErrorValue> {
        let mut connection = self.locked();
        let policy = self.retention;
        if !policy.is_bounded() {
            return Ok(Swept {
                complete: true,
                ..Swept::default()
            });
        }
        let horizon = policy
            .max_age
            .map(|age| {
                let cutoff = now.as_nanosecond().saturating_sub(age.nanoseconds());
                i64::try_from(cutoff).unwrap_or(i64::MIN)
            })
            .unwrap_or(i64::MIN);

        let over_size = |connection: &Connection| -> Result<bool, ErrorValue> {
            match policy.max_size {
                None => Ok(false),
                Some(limit) => Ok(measure(connection, &self.path)? > limit),
            }
        };

        let mut swept = Swept::default();
        let mut removed = 0_usize;
        loop {
            if removed >= policy.batch {
                swept.complete = false;
                break;
            }
            let take = (policy.batch - removed).min(256);
            let doomed = oldest_events(&connection, horizon, over_size(&connection)?, take)
                .map_err(|error| self.unavailable(&error))?;
            if doomed.is_empty() {
                swept.complete = true;
                break;
            }
            removed += doomed.len();
            let transaction = connection
                .transaction()
                .map_err(|error| self.unavailable(&error))?;
            for id in &doomed {
                transaction
                    .execute("DELETE FROM events WHERE event_id = ?1", params![id])
                    .map_err(|error| self.unavailable(&error))?;
                swept.events += 1;
            }
            transaction
                .commit()
                .map_err(|error| self.unavailable(&error))?;
            // The size bound is measured on reclaimed pages, so the chunk is returned to the
            // filesystem before the next one is chosen. Removing more than the bound needs would
            // be the same defect as removing less (§10.4).
            if policy.max_size.is_some() {
                reclaim(&connection).map_err(|error| self.unavailable(&error))?;
            }
        }

        // §31.8: nothing may point at what has gone. Causal links and the join tables went with
        // their events by cascade; evidence, checkpoints, actions and coverage are swept against
        // the boundary the surviving events now define.
        let surviving: Option<i64> = connection
            .query_row("SELECT MIN(presentation_nanos) FROM events", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|error| self.unavailable(&error))?
            .flatten();
        // The boundary is normally the oldest event still held. Where retention removed *every*
        // event, that answer is `None` and the whole sweep below would be skipped — leaving
        // coverage rows that go on claiming `complete` for a window whose record has been
        // deleted. A reconstruction reading them concludes that nothing happened in that window,
        // which is a stronger and worse claim than §55.5's silent gap: it is a confident wrong
        // answer where §7.5 has a word for the truth. So a sweep that emptied the store falls
        // back to the age boundary, and the coverage goes with the events it described.
        let boundary: Option<i64> =
            surviving.or_else(|| (swept.events > 0 && policy.max_age.is_some()).then_some(horizon));
        let transaction = connection
            .transaction()
            .map_err(|error| self.unavailable(&error))?;
        // Evidence goes when it is older than the retained boundary *and* nothing surviving cites
        // it: not an event, not a causal link, and not the derivation chain of another record
        // (§7.3). Evidence a source has just written and no event references yet is younger than
        // the boundary, so it stays.
        let evidence_boundary = boundary.unwrap_or(i64::MAX);
        swept.evidence = u64::try_from(
            transaction
                .execute(
                    "DELETE FROM evidence WHERE observed_nanos < ?1 \
                     AND evidence_id NOT IN (SELECT evidence_id FROM event_evidence) \
                     AND evidence_id NOT IN (SELECT evidence_id FROM causal_link_evidence) \
                     AND evidence_id NOT IN (SELECT derived_from FROM evidence_derivation)",
                    params![evidence_boundary],
                )
                .map_err(|error| self.unavailable(&error))?,
        )
        .unwrap_or_default();
        if let Some(boundary) = boundary {
            swept.checkpoints = u64::try_from(
                transaction
                    .execute(
                        // §9.1 reconstructs by selecting the nearest checkpoint *at or before*
                        // the requested instant and applying events forward from it, so the
                        // newest checkpoint below the boundary is exactly the base state for the
                        // earliest instants still retained. Deleting it destroys the store's
                        // ability to answer inside its own retention window. What goes is every
                        // checkpoint below the boundary that a newer one below the boundary has
                        // already superseded (ADR-0773 supersedes ADR-0634 §3's reasoning).
                        "DELETE FROM checkpoints \
                          WHERE captured_nanos < ?1 \
                            AND EXISTS ( \
                              SELECT 1 FROM checkpoints AS newer \
                               WHERE newer.scope_path = checkpoints.scope_path \
                                 AND newer.captured_nanos > checkpoints.captured_nanos \
                                 AND newer.captured_nanos < ?1)",
                        params![boundary],
                    )
                    .map_err(|error| self.unavailable(&error))?,
            )
            .unwrap_or_default();
            swept.actions = u64::try_from(
                transaction
                    .execute(
                        "DELETE FROM actions WHERE requested_nanos < ?1",
                        params![boundary],
                    )
                    .map_err(|error| self.unavailable(&error))?,
            )
            .unwrap_or_default();
            // One boundary row per scope that lost coverage, because a composition for a place
            // has to find an interval whose scope contains it. A store-wide row would decode
            // into no scope at all and be dropped on the way back out.
            let expiring: Vec<(String, i64)> = {
                let mut statement = transaction
                    .prepare(
                        "SELECT scope_path, MIN(from_nanos) FROM coverage_intervals \
                          WHERE until_nanos < ?1 GROUP BY scope_path",
                    )
                    .map_err(|error| self.unavailable(&error))?;
                let rows = statement
                    .query_map(params![boundary], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|error| self.unavailable(&error))?;
                let mut collected = Vec::new();
                for row in rows {
                    collected.push(row.map_err(|error| self.unavailable(&error))?);
                }
                collected
            };
            swept.coverage = u64::try_from(
                transaction
                    .execute(
                        "DELETE FROM coverage_intervals WHERE until_nanos < ?1",
                        params![boundary],
                    )
                    .map_err(|error| self.unavailable(&error))?,
            )
            .unwrap_or_default();
            // §55.5 and §7.5: a window that was recorded, was complete, and was then removed by
            // retention must not compose to "nothing was watching". Deleting the intervals and
            // leaving nothing behind is exactly that — the composed reason falls back to
            // `not_recorded`, which renders as `recorder not running` and is an affirmative claim
            // about a period the recorder was in fact covering. One `unavailable` interval over
            // the swept span says what actually happened, and §34 has the word for it.
            for (scope_path, from) in expiring {
                transaction
                    .execute(
                        "INSERT OR REPLACE INTO coverage_intervals \
                            (scope_path, capability, from_nanos, until_nanos, completeness, \
                             sampling_nanos, source, permission) \
                         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
                        params![
                            scope_path,
                            RETENTION_CAPABILITY,
                            from,
                            boundary,
                            TemporalCompleteness::Unavailable.as_str(),
                            EvidenceSource::recorder().as_str(),
                            PermissionState::Available.as_str(),
                        ],
                    )
                    .map_err(|error| self.unavailable(&error))?;
            }
        }
        swept.links = u64::try_from(
            transaction
                .execute(
                    "DELETE FROM causal_links WHERE cause NOT IN (SELECT event_id FROM events) \
                        OR effect NOT IN (SELECT event_id FROM events)",
                    [],
                )
                .map_err(|error| self.unavailable(&error))?,
        )
        .unwrap_or_default();
        transaction
            .commit()
            .map_err(|error| self.unavailable(&error))?;

        if swept.removed_anything() {
            let evicted: u64 = swept.events;
            let previous = read_metadata_u64(&connection, "evicted").unwrap_or_default();
            write_metadata(&connection, "evicted", &(previous + evicted).to_string())
                .map_err(|error| self.unavailable(&error))?;
            // Reclaiming the pages is what makes the size bound mean anything. Incremental
            // vacuum is the bounded form §31.8 asks for and is what a store created by this crate
            // uses; a store from an installation that had it switched off falls back to the whole
            // vacuum, which is slower and still bounded by the store's own size.
            reclaim(&connection).map_err(|error| self.unavailable(&error))?;
        }
        // A bound smaller than an empty store is unattainable, so a sweep with nothing left to
        // remove is finished whatever the bound says. Reporting otherwise would ask the caller to
        // sweep for ever (§31.8).
        if swept.complete
            && policy.max_size.is_some()
            && over_size(&connection)?
            && holds_events(&connection).unwrap_or_default()
        {
            swept.complete = false;
        }
        Ok(swept)
    }

    /// Discards the whole local ledger (§30.8's `remove temporal-history`).
    ///
    /// §30.9 makes this the granularity v0.5 offers: removing one event from an evidence chain
    /// destroys the integrity of every claim derived from it, so what is offered is the whole
    /// store or nothing.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn remove_all(&self) -> Result<(), ErrorValue> {
        let connection = self.locked();
        connection
            .execute_batch(
                "DELETE FROM event_subjects; DELETE FROM event_evidence; \
                 DELETE FROM causal_link_evidence; DELETE FROM causal_links; \
                 DELETE FROM checkpoint_objects; DELETE FROM checkpoint_relations; \
                 DELETE FROM checkpoints; DELETE FROM coverage_intervals; \
                 DELETE FROM source_sequences; DELETE FROM actions; \
                 DELETE FROM evidence; DELETE FROM events;",
            )
            .map_err(|error| self.unavailable(&error))?;
        connection
            .execute_batch("VACUUM;")
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }

    fn locked(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn unavailable(&self, error: &rusqlite::Error) -> ErrorValue {
        unavailable(&self.path, error)
    }
}

fn unavailable(path: &Path, error: &rusqlite::Error) -> ErrorValue {
    error::store_unavailable(&format!("`{}`: {error}", path.display()))
}

// ---- opening -----------------------------------------------------------------------------------

/// `PRAGMA integrity_check`, plus the header check SQLite passes but a truncated file fails.
fn check_file_integrity(connection: &Connection, path: &Path, report: &mut IntegrityReport) {
    let answer: Result<String, _> =
        connection.query_row("PRAGMA integrity_check", [], |row| row.get(0));
    match answer {
        Ok(answer) if answer.eq_ignore_ascii_case("ok") => {}
        Ok(answer) => report.findings.push(IntegrityFinding::new(
            &path.display().to_string(),
            &format!("SQLite reports `{answer}`"),
            true,
        )),
        Err(error) => report.findings.push(IntegrityFinding::new(
            &path.display().to_string(),
            &format!("the integrity check could not run: {error}"),
            true,
        )),
    }
}

/// Applies every migration step the store has not yet had (§31.6, §44.3).
fn migrate(connection: &mut Connection, path: &Path) -> Result<u32, ErrorValue> {
    let has_metadata: bool = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| unavailable(path, &error))?
        > 0;
    let current = if has_metadata {
        read_metadata_u64(connection, VERSION_KEY).unwrap_or_default()
    } else {
        0
    };
    let current = u32::try_from(current).unwrap_or(u32::MAX);
    if current > STORE_VERSION {
        return Err(error::store_unavailable(&format!(
            "`{}` was written by a newer Ono at store version {current}; this one reads up to \
             {STORE_VERSION}. The shell keeps working without persistent history (§44.3).",
            path.display()
        )));
    }
    for step in STEPS {
        if step.version <= current {
            continue;
        }
        let transaction = connection
            .transaction()
            .map_err(|error| unavailable(path, &error))?;
        transaction
            .execute_batch(step.sql)
            .map_err(|error| unavailable(path, &error))?;
        set_metadata(&transaction, VERSION_KEY, &step.version.to_string())
            .map_err(|error| unavailable(path, &error))?;
        transaction
            .commit()
            .map_err(|error| unavailable(path, &error))?;
    }
    if current == 0 {
        write_metadata(connection, CREATED_KEY, &Timestamp::UNIX_EPOCH.to_string())
            .map_err(|error| unavailable(path, &error))?;
        write_metadata(connection, STORE_ID_KEY, &path.display().to_string())
            .map_err(|error| unavailable(path, &error))?;
    }
    Ok(STORE_VERSION)
}

/// Returns the pages retention has freed to the filesystem (§31.8).
///
/// The write-ahead log is checkpointed first, because until it is, the pages the deletes freed are
/// still on disk and the size bound would never be reached however much history was removed.
fn reclaim(connection: &Connection) -> rusqlite::Result<()> {
    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    let mode: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    if mode == 2 {
        connection.execute_batch("PRAGMA incremental_vacuum;")
    } else {
        connection.execute_batch("VACUUM;")
    }
}

/// Whether any event is left to remove.
fn holds_events(connection: &Connection) -> rusqlite::Result<bool> {
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM events LIMIT 1", [], |row| row.get(0))?;
    Ok(count > 0)
}

fn read_metadata_u64(connection: &Connection, key: &str) -> Option<u64> {
    connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse().ok())
}

fn write_metadata(connection: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn set_metadata(transaction: &Transaction<'_>, key: &str, value: &str) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// How much space the history occupies (§10.4's `temporal.retention.max_size`).
///
/// The measure is SQLite's own: pages in use times the page size. The write-ahead log is left out
/// deliberately — it is a transient buffer that a checkpoint returns to the database file, so
/// counting it would make the bound depend on when the last checkpoint happened rather than on how
/// much history is retained.
fn measure(connection: &Connection, path: &Path) -> Result<ByteSize, ErrorValue> {
    let pages: i64 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|error| unavailable(path, &error))?;
    let free: i64 = connection
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap_or_default();
    let size: i64 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|error| unavailable(path, &error))?;
    let used = pages.saturating_sub(free).max(0);
    Ok(ByteSize::from_bytes(
        u128::from(used.unsigned_abs()) * u128::from(size.unsigned_abs()),
    ))
}

fn oldest_events(
    connection: &Connection,
    horizon: i64,
    over_size: bool,
    take: usize,
) -> rusqlite::Result<Vec<String>> {
    let take = i64::try_from(take).unwrap_or(i64::MAX);
    if over_size {
        // The size bound removes the oldest events whatever their age (§10.4's "whichever bound
        // removes data first").
        let mut statement = connection.prepare(
            "SELECT event_id FROM events ORDER BY presentation_nanos ASC, event_id ASC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![take], |row| row.get::<_, String>(0))?;
        return rows.collect();
    }
    let mut statement = connection.prepare(
        "SELECT event_id FROM events WHERE presentation_nanos < ?2 \
         ORDER BY presentation_nanos ASC, event_id ASC LIMIT ?1",
    )?;
    let rows = statement.query_map(params![take, horizon], |row| row.get::<_, String>(0))?;
    rows.collect()
}

fn read_sequence_row(row: &Row<'_>) -> rusqlite::Result<Decoded<SourceSequence>> {
    let source: String = row.get(0)?;
    let host: String = row.get(1)?;
    let boot: String = row.get(2)?;
    let lowest: i64 = row.get(3)?;
    let highest: i64 = row.get(4)?;
    let seen: i64 = row.get(5)?;
    let contiguous: i64 = row.get(6)?;
    let declared: i64 = row.get(7)?;
    let last: i64 = row.get(8)?;
    let scope: String = row.get(9)?;
    Ok((|| {
        Ok(SourceSequence {
            source: crate::rows::read_source(&source)?,
            domain: ono_temporal_core::ClockDomain {
                host: std::sync::Arc::from(host.as_str()),
                boot_id: (!boot.is_empty()).then(|| std::sync::Arc::from(boot.as_str())),
            },
            lowest: u64::try_from(lowest).unwrap_or_default(),
            highest: u64::try_from(highest).unwrap_or_default(),
            seen: u64::try_from(seen).unwrap_or_default(),
            contiguous: contiguous != 0,
            declared_contiguous: declared != 0,
            last_seen_at: from_nanos(last)?,
            scope: read_scope_path(&scope)?,
        })
    })())
}

// ---- reading -----------------------------------------------------------------------------------

/// The columns every event query selects, in the order [`read_event_row`] reads them.
const EVENT_COLUMNS: &str = "event_id, kind, scope_path, subject, presentation_nanos, \
     source_nanos, observed_nanos, ingested_nanos, source_sequence, monotonic_nanos, \
     uncertainty_nanos, domain_host, domain_boot, source, body";

fn read_event_row(row: &Row<'_>) -> rusqlite::Result<EventColumns> {
    Ok(EventColumns {
        event_id: row.get(0)?,
        kind: row.get(1)?,
        scope: row.get(2)?,
        subject: row.get(3)?,
        presentation_nanos: row.get(4)?,
        source_nanos: row.get(5)?,
        observed_nanos: row.get(6)?,
        ingested_nanos: row.get(7)?,
        source_sequence: row.get(8)?,
        monotonic_nanos: row.get(9)?,
        uncertainty_nanos: row.get(10)?,
        domain_host: row.get(11)?,
        domain_boot: row.get(12)?,
        source: row.get(13)?,
        body: row.get(14)?,
    })
}

/// The SQL a query becomes, and the values it binds.
///
/// §32.3's budgets are met by never materialising more than the answer: the window, the place, the
/// kinds and the limit are all in the statement, so a bounded question over a million events reads
/// the rows it answers with and no others.
struct Selection {
    sql: String,
    binds: Vec<rusqlite::types::Value>,
}

fn event_selection(query: &EventQuery) -> Decoded<Selection> {
    use rusqlite::types::Value as Bound;
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<Bound> = Vec::new();
    if let Some(scope) = &query.scope {
        let prefix = scope_path(scope);
        let upper = scope_upper_bound(&prefix);
        binds.push(Bound::Text(prefix));
        clauses.push(format!("scope_path >= ?{}", binds.len()));
        binds.push(Bound::Text(upper));
        clauses.push(format!("scope_path < ?{}", binds.len()));
    }
    match (query.range.from, query.range.until) {
        (Some(from), Some(until)) if from == until => {
            binds.push(Bound::Integer(nanos(from)?));
            clauses.push(format!("presentation_nanos = ?{}", binds.len()));
        }
        (from, until) => {
            if let Some(from) = from {
                binds.push(Bound::Integer(nanos(from)?));
                clauses.push(format!("presentation_nanos >= ?{}", binds.len()));
            }
            if let Some(until) = until {
                binds.push(Bound::Integer(nanos(until)?));
                clauses.push(format!("presentation_nanos < ?{}", binds.len()));
            }
        }
    }
    if !query.kinds.is_empty() {
        let mut names = Vec::new();
        for kind in &query.kinds {
            binds.push(Bound::Text(kind.as_str().to_owned()));
            names.push(format!("?{}", binds.len()));
        }
        clauses.push(format!("kind IN ({})", names.join(", ")));
    }
    if !query.subjects.is_empty() {
        let mut ids = Vec::new();
        for subject in &query.subjects {
            binds.push(Bound::Text(subject.as_str().to_owned()));
            ids.push(format!("?{}", binds.len()));
        }
        clauses.push(format!(
            "event_id IN (SELECT event_id FROM event_subjects WHERE subject IN ({}))",
            ids.join(", ")
        ));
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let order = match query.order {
        QueryOrder::Ascending => "ASC",
        QueryOrder::Descending => "DESC",
    };
    let limit = match query.limit {
        Some(limit) => {
            binds.push(Bound::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
            format!(" LIMIT ?{}", binds.len())
        }
        None => String::new(),
    };
    // A scope is a *prefix range* — a place includes everything under it — so `events_by_place`,
    // which leads on `scope_path`, gives no usable order for the `ORDER BY presentation_nanos`
    // that follows. Left to itself SQLite reads every event in the place, sorts all of them and
    // then takes the few that were asked for: on §49's fixture, 62,500 rows sorted to answer a
    // question about a hundred, which is 83 ms of pure sorting and the whole of why §32.3's `why`
    // row missed its budget. `events_by_time_place` leads on the instant and carries the scope
    // beside it, so the walk is already in the order the query wants, the scope is tested from the
    // index, and the walk stops when the limit is full — 0.3 ms for the same answer (ADR-0776).
    //
    // Only where the question names an instant or a count. Then the walk can seek to the window
    // and stop at its end, and the cost is the window rather than the store: `changes --since 1h`
    // costs an hour of events whatever the ledger holds. A question that names neither — every
    // event of a place, unbounded — has to read the place whatever the order, and the planner's
    // own choice is the right one there.
    let bounded =
        query.limit.is_some() || query.range.from.is_some() || query.range.until.is_some();
    let index = if query.scope.is_some() && bounded {
        " INDEXED BY events_by_time_place"
    } else {
        ""
    };
    Ok(Selection {
        sql: format!(
            "SELECT {EVENT_COLUMNS} FROM events{index}{where_clause} \
             ORDER BY presentation_nanos {order}, event_id {order}{limit}"
        ),
        binds,
    })
}

impl LedgerRead for LedgerStore {
    fn events(&self, query: &EventQuery) -> Result<Vec<TemporalEvent>, ErrorValue> {
        let selection = event_selection(query).map_err(|detail| corrupt("events", &detail))?;
        let connection = self.locked();
        let mut statement = connection
            .prepare(&selection.sql)
            .map_err(|error| self.unavailable(&error))?;
        let binds = rusqlite::params_from_iter(selection.binds.iter());
        let rows = statement
            .query_map(binds, read_event_row)
            .map_err(|error| self.unavailable(&error))?;
        let mut events = Vec::new();
        for (read, row) in rows.enumerate() {
            // v0.5 §32.6: this scan is the whole of a long historical query, and the shell that
            // asked for it is inside this call until it returns. A batch boundary is where it can
            // be told the answer is no longer wanted; the partial read is dropped rather than
            // returned, because half a query is not a smaller query.
            if read % crate::cancel::BATCH == 0 && crate::cancel::cancelled() {
                return Err(crate::cancel::cancelled_read());
            }
            let columns = row.map_err(|error| self.unavailable(&error))?;
            // §31.7's first obligation: a row that does not decode is refused rather than
            // rendered with its broken fields blanked. The query still answers (§31.7's third),
            // and the interval the refused row covered becomes a gap (§31.7's fifth).
            match read_event(&columns, &self.schemas) {
                Ok(event) => events.push(event),
                Err(detail) => self.note_corrupt(
                    &format!("events:{}", columns.event_id),
                    &detail,
                    &columns.scope,
                    columns.presentation_nanos,
                    columns.presentation_nanos,
                ),
            }
        }
        Ok(events)
    }

    fn event(&self, id: &EventId) -> Result<Option<TemporalEvent>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {EVENT_COLUMNS} FROM events WHERE event_id >= ?1 AND event_id < ?2 LIMIT 2"
            ))
            .map_err(|error| self.unavailable(&error))?;
        let prefix = id.as_str().to_owned();
        let upper = upper_bound(&prefix);
        let rows = statement
            .query_map(params![prefix, upper], read_event_row)
            .map_err(|error| self.unavailable(&error))?;
        let mut found = Vec::new();
        for row in rows {
            found.push(row.map_err(|error| self.unavailable(&error))?);
        }
        match found.as_slice() {
            [] => Ok(None),
            [only] => read_event(only, &self.schemas)
                .map(Some)
                .map_err(|detail| corrupt("events", &detail)),
            many => Err(error::ambiguous_event(
                &many
                    .iter()
                    .filter_map(|columns| EventId::parse(&columns.event_id))
                    .collect::<Vec<_>>(),
            )),
        }
    }

    fn shortest_unique_prefixes(
        &self,
        ids: &[EventId],
        minimum: usize,
    ) -> Result<Vec<usize>, ErrorValue> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // The identity is the table's primary key, so the retained identities either side of one
        // in identity order are two index seeks, and the shortest prefix nothing else answers to
        // follows from those two alone: everything sharing a prefix with an identity is
        // contiguous with it. That is two seeks per rendered row, over two statements prepared
        // once for the whole batch, rather than a query per row per lengthening step (§32.3).
        let floor = minimum.max(1);
        let connection = self.locked();
        let mut below = connection
            .prepare(
                "SELECT event_id FROM events WHERE event_id < ?1 ORDER BY event_id DESC LIMIT 1",
            )
            .map_err(|error| self.unavailable(&error))?;
        let mut above = connection
            .prepare(
                "SELECT event_id FROM events WHERE event_id > ?1 ORDER BY event_id ASC LIMIT 1",
            )
            .map_err(|error| self.unavailable(&error))?;
        let mut lengths = Vec::with_capacity(ids.len());
        for id in ids {
            let text = id.as_str();
            let mut length = floor.min(text.len());
            for statement in [&mut below, &mut above] {
                let neighbour: Option<String> = statement
                    .query_row(params![text], |row| row.get(0))
                    .optional()
                    .map_err(|error| self.unavailable(&error))?;
                if let Some(neighbour) = neighbour {
                    length = length.max(distinguishing_length(text, &neighbour));
                }
            }
            lengths.push(length);
        }
        Ok(lengths)
    }

    fn evidence(&self, ids: &[EvidenceId]) -> Result<Vec<Evidence>, ErrorValue> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=ids.len()).map(|index| format!("?{index}")).collect();
        let connection = self.locked();
        let mut statement = connection
            .prepare(&format!(
                "SELECT evidence_id, source, observed_nanos, scope_path, subject, strength, body \
                 FROM evidence WHERE evidence_id IN ({}) ORDER BY observed_nanos ASC",
                placeholders.join(", ")
            ))
            .map_err(|error| self.unavailable(&error))?;
        let binds = rusqlite::params_from_iter(ids.iter().map(|id| id.as_str().to_owned()));
        let rows = statement
            .query_map(binds, |row| {
                Ok(EvidenceColumns {
                    evidence_id: row.get(0)?,
                    source: row.get(1)?,
                    observed_nanos: row.get(2)?,
                    scope: row.get(3)?,
                    subject: row.get(4)?,
                    strength: row.get(5)?,
                    body: row.get(6)?,
                })
            })
            .map_err(|error| self.unavailable(&error))?;
        let mut found = Vec::new();
        for row in rows {
            let columns = row.map_err(|error| self.unavailable(&error))?;
            match read_evidence(&columns, &self.schemas) {
                Ok(evidence) => found.push(evidence),
                Err(detail) => self.note_corrupt(
                    &format!("evidence:{}", columns.evidence_id),
                    &detail,
                    &columns.scope,
                    columns.observed_nanos,
                    columns.observed_nanos,
                ),
            }
        }
        Ok(found)
    }

    fn causal_links(&self, effect: &EventId) -> Result<Vec<CausalLink>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(
                "SELECT link_id, relation, cause, effect, rule, strength, source \
                 FROM causal_links WHERE effect = ?1 ORDER BY link_id",
            )
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map(params![effect.as_str()], |row| {
                Ok(LinkColumns {
                    link_id: row.get(0)?,
                    relation: row.get(1)?,
                    cause: row.get(2)?,
                    effect: row.get(3)?,
                    rule: row.get(4)?,
                    strength: row.get(5)?,
                    source: row.get(6)?,
                    evidence: Vec::new(),
                })
            })
            .map_err(|error| self.unavailable(&error))?;
        let mut links = Vec::new();
        for row in rows {
            let mut columns = row.map_err(|error| self.unavailable(&error))?;
            columns.evidence = link_evidence(&connection, &columns.link_id)
                .map_err(|error| self.unavailable(&error))?;
            if let Ok(link) = read_link(&columns) {
                links.push(link);
            }
        }
        Ok(links)
    }

    fn coverage(&self, query: &CoverageQuery) -> Result<Vec<TemporalCoverage>, ErrorValue> {
        use rusqlite::types::Value as Bound;
        let mut clauses = Vec::new();
        let mut binds: Vec<Bound> = Vec::new();
        if let Some(scope) = &query.scope {
            let prefix = scope_path(scope);
            let upper = scope_upper_bound(&prefix);
            binds.push(Bound::Text(prefix));
            clauses.push(format!("scope_path >= ?{}", binds.len()));
            binds.push(Bound::Text(upper));
            clauses.push(format!("scope_path < ?{}", binds.len()));
        }
        if !query.capabilities.is_empty() {
            let mut names = Vec::new();
            for capability in &query.capabilities {
                binds.push(Bound::Text(capability.to_string()));
                names.push(format!("?{}", binds.len()));
            }
            clauses.push(format!("capability IN ({})", names.join(", ")));
        }
        if let Some(from) = query.range.from {
            binds.push(Bound::Integer(
                nanos(from).map_err(|detail| corrupt("coverage_intervals", &detail))?,
            ));
            clauses.push(format!("until_nanos >= ?{}", binds.len()));
        }
        if let Some(until) = query.range.until {
            binds.push(Bound::Integer(
                nanos(until).map_err(|detail| corrupt("coverage_intervals", &detail))?,
            ));
            clauses.push(format!("from_nanos <= ?{}", binds.len()));
        }
        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let connection = self.locked();
        let mut statement = connection
            .prepare(&format!(
                "SELECT scope_path, capability, from_nanos, until_nanos, completeness, \
                        sampling_nanos, source, permission \
                 FROM coverage_intervals{where_clause} ORDER BY from_nanos ASC, until_nanos ASC"
            ))
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(binds.iter()), |row| {
                Ok(CoverageColumns {
                    scope: row.get(0)?,
                    capability: row.get(1)?,
                    from_nanos: row.get(2)?,
                    until_nanos: row.get(3)?,
                    completeness: row.get(4)?,
                    sampling_nanos: row.get(5)?,
                    source: row.get(6)?,
                    permission: row.get(7)?,
                })
            })
            .map_err(|error| self.unavailable(&error))?;
        let mut intervals = Vec::new();
        for row in rows {
            let columns = row.map_err(|error| self.unavailable(&error))?;
            if let Ok(interval) = read_coverage(&columns) {
                intervals.push(interval);
            }
        }
        Ok(intervals)
    }

    fn checkpoint_before(
        &self,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Option<Checkpoint>, ErrorValue> {
        let prefix = scope_path(scope);
        let upper = scope_upper_bound(&prefix);
        let at = nanos(at).map_err(|detail| corrupt("checkpoints", &detail))?;
        let connection = self.locked();
        let header = connection
            .query_row(
                "SELECT checkpoint_id, scope_path, captured_nanos, body FROM checkpoints \
                 WHERE scope_path >= ?1 AND scope_path < ?2 AND captured_nanos <= ?3 \
                 ORDER BY captured_nanos DESC LIMIT 1",
                params![prefix, upper, at],
                |row| {
                    Ok(CheckpointColumns {
                        checkpoint_id: row.get(0)?,
                        scope: row.get(1)?,
                        captured_nanos: row.get(2)?,
                        body: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|error| self.unavailable(&error))?;
        let Some(header) = header else {
            return Ok(None);
        };
        let mut objects = Vec::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT spatial_id, object_type, label, observed_nanos, source, body \
                     FROM checkpoint_objects WHERE checkpoint_id = ?1 ORDER BY position",
                )
                .map_err(|error| self.unavailable(&error))?;
            let rows = statement
                .query_map(params![header.checkpoint_id], |row| {
                    Ok(ObjectColumns {
                        spatial_id: row.get(0)?,
                        object_type: row.get(1)?,
                        label: row.get(2)?,
                        observed_nanos: row.get(3)?,
                        source: row.get(4)?,
                        body: row.get(5)?,
                    })
                })
                .map_err(|error| self.unavailable(&error))?;
            for row in rows {
                let columns = row.map_err(|error| self.unavailable(&error))?;
                objects.push(
                    read_object(&columns, &self.schemas)
                        .map_err(|detail| corrupt("checkpoint_objects", &detail))?,
                );
            }
        }
        let mut relations = Vec::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT from_id, to_id, relation, confidence, observed_nanos, source \
                     FROM checkpoint_relations WHERE checkpoint_id = ?1 ORDER BY position",
                )
                .map_err(|error| self.unavailable(&error))?;
            let rows = statement
                .query_map(params![header.checkpoint_id], |row| {
                    Ok(RelationColumns {
                        from_id: row.get(0)?,
                        to_id: row.get(1)?,
                        relation: row.get(2)?,
                        confidence: row.get(3)?,
                        observed_nanos: row.get(4)?,
                        source: row.get(5)?,
                    })
                })
                .map_err(|error| self.unavailable(&error))?;
            for row in rows {
                let columns = row.map_err(|error| self.unavailable(&error))?;
                relations.push(
                    read_relation(&columns)
                        .map_err(|detail| corrupt("checkpoint_relations", &detail))?,
                );
            }
        }
        read_checkpoint(&header, objects, relations)
            .map(Some)
            .map_err(|detail| corrupt("checkpoints", &detail))
    }

    fn actions(&self, range: TimeRange) -> Result<Vec<ActionEvent>, ErrorValue> {
        use rusqlite::types::Value as Bound;
        let mut clauses = Vec::new();
        let mut binds: Vec<Bound> = Vec::new();
        if let Some(from) = range.from {
            binds.push(Bound::Integer(
                nanos(from).map_err(|detail| corrupt("actions", &detail))?,
            ));
            clauses.push(format!("requested_nanos >= ?{}", binds.len()));
        }
        if let Some(until) = range.until {
            binds.push(Bound::Integer(
                nanos(until).map_err(|detail| corrupt("actions", &detail))?,
            ));
            clauses.push(if range.from == Some(until) {
                format!("requested_nanos <= ?{}", binds.len())
            } else {
                format!("requested_nanos < ?{}", binds.len())
            });
        }
        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let connection = self.locked();
        let mut statement = connection
            .prepare(&format!(
                "SELECT action_id, requested_nanos, actor, session_id, operation, target, \
                        external_transaction, body \
                 FROM actions{where_clause} ORDER BY requested_nanos ASC, action_id ASC"
            ))
            .map_err(|error| self.unavailable(&error))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(binds.iter()), |row| {
                Ok(ActionColumns {
                    action_id: row.get(0)?,
                    requested_nanos: row.get(1)?,
                    actor: row.get(2)?,
                    session_id: row.get(3)?,
                    operation: row.get(4)?,
                    target: row.get(5)?,
                    external_transaction: row.get(6)?,
                    body: row.get(7)?,
                })
            })
            .map_err(|error| self.unavailable(&error))?;
        let mut actions = Vec::new();
        for row in rows {
            let columns = row.map_err(|error| self.unavailable(&error))?;
            if let Ok(action) = read_action(&columns) {
                actions.push(action);
            }
        }
        Ok(actions)
    }

    fn retention(&self) -> RetentionState {
        let connection = self.locked();
        let bounds: Option<(Option<i64>, Option<i64>, i64)> = connection
            .query_row(
                "SELECT MIN(presentation_nanos), MAX(presentation_nanos), COUNT(*) FROM events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();
        let (earliest, latest, count) = bounds.unwrap_or((None, None, 0));
        RetentionState {
            earliest: earliest.and_then(|nanos| from_nanos(nanos).ok()),
            latest: latest.and_then(|nanos| from_nanos(nanos).ok()),
            events: u64::try_from(count).unwrap_or_default(),
            evicted: read_metadata_u64(&connection, "evicted").unwrap_or_default(),
            stored_size: measure(&connection, &self.path).ok(),
            max_age: self.retention.max_age,
            max_size: self.retention.max_size,
        }
    }
}

fn link_evidence(connection: &Connection, link_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT evidence_id FROM causal_link_evidence WHERE link_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map(params![link_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}

/// The exclusive upper bound of every string beginning with `prefix`.
fn upper_bound(prefix: &str) -> String {
    let mut bound = prefix.to_owned();
    bound.push('\u{10FFFF}');
    bound
}

// ---- writing -----------------------------------------------------------------------------------

impl LedgerWrite for LedgerStore {
    fn append(
        &self,
        events: &[TemporalEvent],
        evidence: &[Evidence],
    ) -> Result<Appended, ErrorValue> {
        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|error| self.unavailable(&error))?;
        let mut appended = Appended::default();
        let mut written: HashSet<String> = HashSet::new();
        for event in events {
            let columns = event_columns(event).map_err(|detail| corrupt("events", &detail))?;
            // §6.8: identity is a content digest, so an event already held is the same
            // observation, whatever its rendered row looks like.
            if written.contains(&columns.event_id) {
                appended.duplicates += 1;
                continue;
            }
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO events \
                        (event_id, kind, scope_path, subject, presentation_nanos, source_nanos, \
                         observed_nanos, ingested_nanos, source_sequence, monotonic_nanos, \
                         uncertainty_nanos, domain_host, domain_boot, source, body) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        columns.event_id,
                        columns.kind,
                        columns.scope,
                        columns.subject,
                        columns.presentation_nanos,
                        columns.source_nanos,
                        columns.observed_nanos,
                        columns.ingested_nanos,
                        columns.source_sequence,
                        columns.monotonic_nanos,
                        columns.uncertainty_nanos,
                        columns.domain_host,
                        columns.domain_boot,
                        columns.source,
                        columns.body,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
            if inserted == 0 {
                appended.duplicates += 1;
                continue;
            }
            written.insert(columns.event_id.clone());
            appended.stored += 1;
            index_subjects(&transaction, event, &columns)
                .map_err(|error| self.unavailable(&error))?;
            note_sequence(&transaction, event, &columns)
                .map_err(|error| self.unavailable(&error))?;
        }
        for record in evidence {
            let columns =
                evidence_columns(record).map_err(|detail| corrupt("evidence", &detail))?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO evidence \
                        (evidence_id, source, observed_nanos, scope_path, subject, strength, body) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        columns.evidence_id,
                        columns.source,
                        columns.observed_nanos,
                        columns.scope,
                        columns.subject,
                        columns.strength,
                        columns.body,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
            // §7.3's chains are what retention must not break, so they are a set rather than a
            // list inside a payload: a parent no derived claim cites any more is a row count.
            for (position, parent) in record.derived_from.iter().enumerate() {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO evidence_derivation \
                            (evidence_id, derived_from, position) VALUES (?1, ?2, ?3)",
                        params![
                            columns.evidence_id,
                            parent.as_str(),
                            i64::try_from(position).unwrap_or_default()
                        ],
                    )
                    .map_err(|error| self.unavailable(&error))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| self.unavailable(&error))?;
        Ok(appended)
    }

    fn append_links(&self, links: &[CausalLink]) -> Result<usize, ErrorValue> {
        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|error| self.unavailable(&error))?;
        let mut stored = 0;
        for link in links {
            // §15.2 permits `caused_by` only where evidence supports a direct causal statement,
            // and §15.8 requires every rule that emits one to be inspectable. The causal engine
            // enforces both before it hands a link out — but the engine is not the only way into
            // this table. A KUANG/11 contribution seam, a remote ingest path or the recorder all
            // hold a `LedgerWrite`, and a link written here is read back by `causal_links` and
            // rendered as a cause like any other. So the store is the second door and it needs
            // the same guard: a causal claim arrives with its evidence and its rule or it does
            // not arrive (ADR-0773).
            if link.relation.is_causal() {
                if link.evidence.is_empty() {
                    return Err(error::unsupported_source(
                        &link.source,
                        "a causal link with no evidence",
                    ));
                }
                if link.rule.as_str().trim().is_empty() {
                    return Err(error::unsupported_source(
                        &link.source,
                        "a causal link naming no rule",
                    ));
                }
            }
            let columns = link_columns(link);
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO causal_links \
                        (link_id, relation, cause, effect, rule, strength, source) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        columns.link_id,
                        columns.relation,
                        columns.cause,
                        columns.effect,
                        columns.rule,
                        columns.strength,
                        columns.source,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
            if inserted == 0 {
                continue;
            }
            stored += 1;
            for (position, evidence) in columns.evidence.iter().enumerate() {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO causal_link_evidence (link_id, evidence_id, position) \
                         VALUES (?1, ?2, ?3)",
                        params![
                            columns.link_id,
                            evidence,
                            i64::try_from(position).unwrap_or_default()
                        ],
                    )
                    .map_err(|error| self.unavailable(&error))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| self.unavailable(&error))?;
        Ok(stored)
    }

    fn record_coverage(&self, intervals: &[TemporalCoverage]) -> Result<(), ErrorValue> {
        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|error| self.unavailable(&error))?;
        for interval in intervals {
            let columns = coverage_columns(interval)
                .map_err(|detail| corrupt("coverage_intervals", &detail))?;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO coverage_intervals \
                        (scope_path, capability, from_nanos, until_nanos, completeness, \
                         sampling_nanos, source, permission) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        columns.scope,
                        columns.capability,
                        columns.from_nanos,
                        columns.until_nanos,
                        columns.completeness,
                        columns.sampling_nanos,
                        columns.source,
                        columns.permission,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
        }
        transaction
            .commit()
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }

    fn record_action(&self, action: &ActionEvent) -> Result<(), ErrorValue> {
        let columns = action_columns(action).map_err(|detail| corrupt("actions", &detail))?;
        let connection = self.locked();
        // §17.2's lifecycle updates one action rather than appending a second: the identity is
        // minted before execution and the result arrives later.
        connection
            .execute(
                "INSERT INTO actions \
                    (action_id, requested_nanos, actor, session_id, operation, target, \
                     external_transaction, body) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (action_id) DO UPDATE SET \
                    external_transaction = excluded.external_transaction, body = excluded.body",
                params![
                    columns.action_id,
                    columns.requested_nanos,
                    columns.actor,
                    columns.session_id,
                    columns.operation,
                    columns.target,
                    columns.external_transaction,
                    columns.body,
                ],
            )
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }

    fn write_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), ErrorValue> {
        let header =
            checkpoint_columns(checkpoint).map_err(|detail| corrupt("checkpoints", &detail))?;
        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|error| self.unavailable(&error))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO checkpoints \
                    (checkpoint_id, scope_path, captured_nanos, body) VALUES (?1, ?2, ?3, ?4)",
                params![
                    header.checkpoint_id,
                    header.scope,
                    header.captured_nanos,
                    header.body
                ],
            )
            .map_err(|error| self.unavailable(&error))?;
        transaction
            .execute(
                "DELETE FROM checkpoint_objects WHERE checkpoint_id = ?1",
                params![header.checkpoint_id],
            )
            .map_err(|error| self.unavailable(&error))?;
        transaction
            .execute(
                "DELETE FROM checkpoint_relations WHERE checkpoint_id = ?1",
                params![header.checkpoint_id],
            )
            .map_err(|error| self.unavailable(&error))?;
        for (position, object) in checkpoint.objects.iter().enumerate() {
            let columns =
                object_columns(object).map_err(|detail| corrupt("checkpoint_objects", &detail))?;
            transaction
                .execute(
                    "INSERT INTO checkpoint_objects \
                        (checkpoint_id, position, spatial_id, object_type, label, observed_nanos, \
                         source, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        header.checkpoint_id,
                        i64::try_from(position).unwrap_or_default(),
                        columns.spatial_id,
                        columns.object_type,
                        columns.label,
                        columns.observed_nanos,
                        columns.source,
                        columns.body,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
        }
        for (position, relation) in checkpoint.relations.iter().enumerate() {
            let columns = relation_columns(relation)
                .map_err(|detail| corrupt("checkpoint_relations", &detail))?;
            transaction
                .execute(
                    "INSERT INTO checkpoint_relations \
                        (checkpoint_id, position, from_id, to_id, relation, confidence, \
                         observed_nanos, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        header.checkpoint_id,
                        i64::try_from(position).unwrap_or_default(),
                        columns.from_id,
                        columns.to_id,
                        columns.relation,
                        columns.confidence,
                        columns.observed_nanos,
                        columns.source,
                    ],
                )
                .map_err(|error| self.unavailable(&error))?;
        }
        transaction
            .commit()
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }

    fn flush(&self) -> Result<(), ErrorValue> {
        let connection = self.locked();
        // §10.8: `stop recorder` MUST flush. Checkpointing the write-ahead log is what makes the
        // committed history readable by a second process and durable across a power loss.
        connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(|error| self.unavailable(&error))?;
        Ok(())
    }
}

/// Indexes an event's subject and every subject it relates, so §11.2's subject filter is a lookup.
fn index_subjects(
    transaction: &Transaction<'_>,
    event: &TemporalEvent,
    columns: &EventColumns,
) -> rusqlite::Result<()> {
    let mut seen: HashSet<(String, &'static str)> = HashSet::new();
    let mut record = |id: &str, role: &'static str| -> rusqlite::Result<()> {
        if !seen.insert((id.to_owned(), role)) {
            return Ok(());
        }
        transaction.execute(
            "INSERT OR IGNORE INTO event_subjects \
                (event_id, subject, role, presentation_nanos) VALUES (?1, ?2, ?3, ?4)",
            params![columns.event_id, id, role, columns.presentation_nanos],
        )?;
        Ok(())
    };
    if let Some(id) = event
        .subject
        .as_ref()
        .and_then(ono_temporal_core::SpatialRef::spatial_id)
    {
        record(id.as_str(), "subject")?;
    }
    for related in &event.related {
        if let Some(id) = related.spatial_id() {
            record(id.as_str(), "related")?;
        }
    }
    for (position, evidence) in event.evidence.iter().enumerate() {
        transaction.execute(
            "INSERT OR IGNORE INTO event_evidence (event_id, evidence_id, position) \
             VALUES (?1, ?2, ?3)",
            params![
                columns.event_id,
                evidence.as_str(),
                i64::try_from(position).unwrap_or_default()
            ],
        )?;
    }
    Ok(())
}

/// Keeps each source's sequence continuity current (§25.4, §44.1).
fn note_sequence(
    transaction: &Transaction<'_>,
    event: &TemporalEvent,
    columns: &EventColumns,
) -> rusqlite::Result<()> {
    let Some(sequence) = columns.source_sequence else {
        return Ok(());
    };
    let boot = columns.domain_boot.clone().unwrap_or_default();
    let existing: Option<(i64, i64, i64)> = transaction
        .query_row(
            "SELECT lowest, highest, seen FROM source_sequences \
             WHERE source = ?1 AND domain_host = ?2 AND domain_boot = ?3",
            params![columns.source, columns.domain_host, boot],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (lowest, highest, seen) = match existing {
        // A row with nothing seen is a declaration rather than an observation: `declare_contiguous`
        // writes one before the first event arrives, and its zeroes are not a sequence number.
        Some((_, _, 0)) | None => (sequence, sequence, 1),
        Some((lowest, highest, seen)) => (lowest.min(sequence), highest.max(sequence), seen + 1),
    };
    let span = highest.saturating_sub(lowest).saturating_add(1);
    let contiguous = i64::from(seen >= span);
    transaction.execute(
        "INSERT INTO source_sequences \
            (source, domain_host, domain_boot, highest, lowest, seen, contiguous, declared, \
             last_nanos, scope_path) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9) \
         ON CONFLICT (source, domain_host, domain_boot) DO UPDATE SET \
            highest = excluded.highest, lowest = excluded.lowest, seen = excluded.seen, \
            contiguous = excluded.contiguous, last_nanos = MAX(last_nanos, excluded.last_nanos), \
            scope_path = excluded.scope_path",
        params![
            columns.source,
            columns.domain_host,
            boot,
            highest,
            lowest,
            seen,
            contiguous,
            columns.presentation_nanos,
            columns.scope,
        ],
    )?;
    let _ = event;
    Ok(())
}
