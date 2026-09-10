//! The persistent plan store (spec v0.6 §36.1, §36.2, §36.4, §41.2, §42.3, §42.4).
//!
//! §36.1: *"Sealed plans MUST be persisted so they survive shell exit and context compaction."*
//! §36.2 asks for SQLite with a versioned schema. What the store holds is decided by three other
//! sections, and each of them shows up in the API:
//!
//! - **§41.2, resume.** *"On shell crash/restart, plan state MUST be reconstructable from
//!   persisted action records and provider evidence."* Every action's settled status is written as
//!   its own row by [`PlanStore::record_action_status`], and [`PlanStore::get`] lays those rows
//!   over the stored plan. An action whose outcome was never established comes back
//!   [`ono_change_core::ActionStatus::Unknown`], which Appendix F.2 makes an uncertainty boundary
//!   rather than a failure or a success.
//! - **§42.4, concurrency.** *"Plan store MUST prevent two sessions from applying the same sealed
//!   plan concurrently."* [`PlanStore::claim`] takes an exclusive claim and refuses a second with
//!   `change.plan_already_applying`, naming the holder. §42.3 bounds it: the claim carries a lease,
//!   so a session that died holding one does not keep the plan forever.
//! - **§36.4, references.** A plan is named by an identity or by an unambiguous prefix of one, and
//!   [`PlanStore::reference_width`] widens the printed form exactly as far as the store requires.
//!
//! The plan itself travels as the `ono.change-plan/1` record of §46.1, encoded with
//! `ono_value::to_json`'s tagged form, so a byte size stays a byte size and a timestamp stays a
//! timestamp across a restart. A row that will not decode is `change.plan_store_corrupt` rather
//! than a partially-read plan: §36.2's versioned schema is what should have prevented it, and a
//! half-decoded plan would describe a change that is not the one the operator approved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use jiff::Timestamp;
use ono_change_core::{
    ActionId, ActionStatus, ChangePlan, ImpactGraph, PlanId, PlanKind, PlanState, RecoveryAsset,
    RecoveryAssetId, error, value,
};
use ono_value::{ErrorValue, RecordValue, SchemaRegistry, Value, builtin_schemas};
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

use crate::migrate::{STEPS, STORE_ID_KEY, STORE_VERSION, VERSION_KEY};
use crate::references::{Reference, plan_reference_width, render_asset, render_plan};
use crate::secrets::{SecretRedaction, rebuild_record};

/// How long a writer waits for another process's transaction before refusing.
const BUSY_TIMEOUT_MS: u64 = 5_000;

/// How long an apply claim lives before another session may take it over (§42.3).
///
/// §42.3: *"Locks MUST be bounded and released on failure."* A [`Claim`] releases itself on drop,
/// which covers every failure the process survives. The lease covers the one it does not: a
/// session that was killed between PREPARE and APPLY left a row nobody will ever delete, and
/// without an expiry the plan would be unappliable forever. Five minutes is longer than any
/// individual provider operation §52 budgets and short enough that an operator waits rather than
/// reaches for a flag.
pub const CLAIM_LEASE: Duration = Duration::from_secs(300);

/// How to open a plan store (§36.2).
#[derive(Debug, Clone)]
pub struct StoreOptions {
    path: PathBuf,
    redaction: SecretRedaction,
    schemas: Option<SchemaRegistry>,
}

impl StoreOptions {
    /// A store at `path`, with §36.3's default redaction and the built-in schemas.
    #[must_use]
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            redaction: SecretRedaction::new(),
            schemas: None,
        }
    }

    /// The same options with a different declared set of sensitive arguments (§36.3).
    #[must_use]
    pub fn redacting(mut self, redaction: SecretRedaction) -> Self {
        self.redaction = redaction;
        self
    }

    /// The same options resolving record schemas through `schemas` rather than the built-in set.
    ///
    /// A stored record carries its schema id; reading it back needs the contract behind that id.
    /// A host that has loaded contributed schemas passes a registry that also answers for those.
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

/// What `get plan` filters the store by (§5.5, §36.4).
#[derive(Debug, Clone, Default)]
pub struct PlanFilter {
    session: Option<Arc<str>>,
    state: Option<PlanState>,
    kind: Option<PlanKind>,
    limit: Option<usize>,
    every_revision: bool,
}

impl PlanFilter {
    /// Every plan the store holds, newest first, latest revision only.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// Only plans created by `session`.
    #[must_use]
    pub fn in_session(mut self, session: impl Into<Arc<str>>) -> Self {
        self.session = Some(session.into());
        self
    }

    /// Only plans in this lifecycle state (§4.1).
    #[must_use]
    pub const fn in_state(mut self, state: PlanState) -> Self {
        self.state = Some(state);
        self
    }

    /// Only change plans, or only recovery plans (§24.1).
    #[must_use]
    pub const fn of_kind(mut self, kind: PlanKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// At most this many rows.
    #[must_use]
    pub const fn limited_to(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Every revision rather than the latest of each plan (§7.5).
    #[must_use]
    pub const fn including_superseded(mut self) -> Self {
        self.every_revision = true;
        self
    }
}

/// One row of `get plan` (§5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSummary {
    /// The plan's identity.
    pub id: PlanId,
    /// The revision this row describes (§3.2).
    pub revision: u32,
    /// Whether it changes the system or restores it (§24.1).
    pub kind: PlanKind,
    /// Where it is in §4.1.
    pub state: PlanState,
    /// The session that created it.
    pub session: Arc<str>,
    /// The intent, as a person reads it (§3.1).
    pub intent: Arc<str>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it was sealed, where it has been.
    pub sealed_at: Option<Timestamp>,
    /// When it stops being appliable (§5.6).
    pub expires_at: Option<Timestamp>,
    /// The seal digest (§4.4).
    pub digest: Option<Arc<str>>,
    /// The reference to print, widened as far as this store requires (§36.4).
    pub reference: Arc<str>,
}

/// An exclusive, leased claim on applying one sealed plan (§42.3, §42.4).
///
/// The claim is released when it is dropped, which is what makes §42.3's "released on failure"
/// hold for every failure the process survives — an early return, a refusal, a panic unwinding
/// through the executor. The lease covers the process that does not survive.
#[derive(Debug)]
pub struct Claim<'store> {
    store: &'store PlanStore,
    plan: PlanId,
    session: Arc<str>,
    claimed_at: Timestamp,
    expires_at: Timestamp,
}

impl Claim<'_> {
    /// The plan this claim covers.
    #[must_use]
    pub const fn plan(&self) -> &PlanId {
        &self.plan
    }

    /// The session holding it.
    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }

    /// When it was taken.
    #[must_use]
    pub const fn claimed_at(&self) -> Timestamp {
        self.claimed_at
    }

    /// When another session may take it over (§42.3).
    #[must_use]
    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    /// Extends the lease from `now`, for an apply that is still making progress (§42.3).
    ///
    /// # Errors
    ///
    /// Returns `change.plan_already_applying` when the lease had already expired and another
    /// session took the plan over, and `change.plan_store_unavailable` where the store cannot be
    /// written.
    pub fn renew(&mut self, now: Timestamp) -> Result<(), ErrorValue> {
        let expires_at = self
            .store
            .take_claim(&self.plan, &self.session, now, CLAIM_LEASE)?;
        self.expires_at = expires_at;
        Ok(())
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        // §42.3: a lock is released on failure. Dropping is the only path every failure takes,
        // and a release that could not be written leaves the lease to expire instead.
        let _ = self.store.release(&self.plan, &self.session);
    }
}

/// The persistent plan store (§36.1).
#[derive(Debug)]
pub struct PlanStore {
    path: PathBuf,
    version: u32,
    redaction: SecretRedaction,
    schemas: SchemaRegistry,
    connection: Mutex<Connection>,
}

impl PlanStore {
    /// Opens or creates the store at `path` (§36.1, §36.2).
    ///
    /// The directory is created `0700` and the database `0600` before SQLite sees either (§43.1),
    /// write-ahead logging is verified by reading back the mode SQLite selected, the file is
    /// integrity-checked, and the schema is migrated forward to [`STORE_VERSION`].
    ///
    /// # Errors
    ///
    /// - `change.plan_store_unavailable` where the path cannot be created or opened, where SQLite
    ///   refuses write-ahead logging, or where the store is at a version this build does not know;
    /// - `change.plan_store_corrupt` where SQLite's integrity check fails.
    pub fn open(path: &Path) -> Result<Self, ErrorValue> {
        Self::open_with(&StoreOptions::at(path))
    }

    /// Opens or creates the store described by `options`.
    ///
    /// # Errors
    ///
    /// As [`PlanStore::open`].
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
        .map_err(|failure| unavailable(&path, &failure))?;
        connection
            .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
            .map_err(|failure| unavailable(&path, &failure))?;

        // WAL answers with the mode it selected, so it is a query rather than a statement, and
        // the answer is read back: a store that silently fell back to a rollback journal would
        // lose §36.1's durability without saying so.
        let mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(|failure| unavailable(&path, &failure))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(error::store_unavailable(&format!(
                "`{}` refused write-ahead logging and reports `{mode}`; §36.2 asks for the \
                 durability v0.5 storage practice has",
                path.display()
            )));
        }
        connection
            .execute_batch(
                "PRAGMA synchronous = NORMAL;\nPRAGMA foreign_keys = ON;\n\
                 PRAGMA auto_vacuum = INCREMENTAL;",
            )
            .map_err(|failure| unavailable(&path, &failure))?;

        let answer: Result<String, _> =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0));
        match answer {
            Ok(report) if report.eq_ignore_ascii_case("ok") => {}
            Ok(report) => {
                return Err(error::store_corrupt(&format!(
                    "`{}`: SQLite reports `{report}`. §36.2's versioned schema cannot be trusted \
                     over a damaged file.",
                    path.display()
                )));
            }
            Err(failure) => {
                return Err(error::store_corrupt(&format!(
                    "`{}`: the integrity check could not run: {failure}",
                    path.display()
                )));
            }
        }

        let mut connection = connection;
        let version = migrate(&mut connection, &path)?;

        Ok(Self {
            path,
            version,
            redaction: options.redaction.clone(),
            schemas: options
                .schemas
                .clone()
                .unwrap_or_else(|| builtin_schemas().clone()),
            connection: Mutex::new(connection),
        })
    }

    /// Where the store is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema version the store is at (§36.2).
    #[must_use]
    pub const fn store_version(&self) -> u32 {
        self.version
    }

    // ---- plans -------------------------------------------------------------------------------

    /// Persists `plan` at its own revision (§36.1, §36.3).
    ///
    /// A revision is written once and updated in place; a rebase (§7.5) inserts a new row and
    /// leaves the original where it was. Action-status rows survive an update of the same
    /// revision, because §41.2's evidence is what a re-put would otherwise erase.
    ///
    /// # Errors
    ///
    /// - `change.plan_store_unavailable` where the store cannot be written;
    /// - `ono.provider_schema_violation` where a contract of §46 is not in this build.
    pub fn put(&self, plan: &ChangePlan) -> Result<(), ErrorValue> {
        let record = value::plan_record(plan)?;
        // §36.3: the secret is replaced before the record reaches the database, not after.
        let record = map_actions(&record, &|action| {
            self.redaction.action_record(action).map(Some)
        })?;
        let encoded = encode(&record)?;
        let impact = if plan.impact().nodes().is_empty() && plan.impact().boundaries().is_empty() {
            None
        } else {
            Some(encode(&value::impact_record(plan.id(), plan.impact())?)?)
        };
        let assets: Vec<String> = plan
            .protection()
            .rows()
            .iter()
            .flat_map(|row| row.assets())
            .map(|asset| asset.as_str().to_owned())
            .collect();

        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|failure| self.unavailable(&failure))?;
        transaction
            .execute(
                "INSERT INTO plans (plan_id, revision, kind, state, session, intent, \
                 created_nanos, sealed_nanos, expires_nanos, digest, record, impact) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT (plan_id, revision) DO UPDATE SET \
                 kind = excluded.kind, state = excluded.state, session = excluded.session, \
                 intent = excluded.intent, created_nanos = excluded.created_nanos, \
                 sealed_nanos = excluded.sealed_nanos, expires_nanos = excluded.expires_nanos, \
                 digest = excluded.digest, record = excluded.record, impact = excluded.impact",
                params![
                    plan.id().as_str(),
                    i64::from(plan.revision()),
                    plan.kind().as_str(),
                    plan.state().as_str(),
                    plan.session(),
                    plan.intent().text(),
                    nanos(plan.created_at()),
                    plan.sealed_at().map(nanos),
                    plan.expires_at().map(nanos),
                    plan.digest(),
                    encoded,
                    impact,
                ],
            )
            .map_err(|failure| self.unavailable(&failure))?;
        transaction
            .execute(
                "DELETE FROM plan_assets WHERE plan_id = ?1 AND revision = ?2",
                params![plan.id().as_str(), i64::from(plan.revision())],
            )
            .map_err(|failure| self.unavailable(&failure))?;
        for asset in &assets {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO plan_assets (plan_id, revision, asset_id) \
                     VALUES (?1, ?2, ?3)",
                    params![plan.id().as_str(), i64::from(plan.revision()), asset],
                )
                .map_err(|failure| self.unavailable(&failure))?;
        }
        transaction
            .commit()
            .map_err(|failure| self.unavailable(&failure))
    }

    /// The latest revision of `plan`, with §41.2's persisted action statuses applied.
    ///
    /// # Errors
    ///
    /// - `change.plan_not_found` where the store holds no such plan;
    /// - `change.plan_store_corrupt` where the stored record will not decode.
    pub fn get(&self, plan: &PlanId) -> Result<ChangePlan, ErrorValue> {
        let revision = self.latest_revision(plan)?;
        self.get_revision(plan, revision)
    }

    /// One particular revision of `plan` (§7.5).
    ///
    /// # Errors
    ///
    /// As [`PlanStore::get`].
    pub fn get_revision(&self, plan: &PlanId, revision: u32) -> Result<ChangePlan, ErrorValue> {
        let stored: Option<(String, Option<String>)> = self
            .locked()
            .query_row(
                "SELECT record, impact FROM plans WHERE plan_id = ?1 AND revision = ?2",
                params![plan.as_str(), i64::from(revision)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?;
        let Some((encoded, impact)) = stored else {
            return Err(error::plan_not_found(&render_plan(
                plan,
                plan.as_str().len(),
            )));
        };
        let record = self.decode(&encoded, "record")?;
        let statuses = self.action_statuses(plan, revision)?;
        let record = overlay_statuses(&record, &statuses)?;
        let restored = value::plan_from_record(&record)?;
        let Some(impact) = impact else {
            return Ok(restored);
        };
        let graph = value::impact_from_record(&self.decode(&impact, "impact")?)?;
        Ok(restored.with_impact(graph))
    }

    /// The impact graph stored beside `plan`, or an empty one where none was (§9.6, §46.7).
    ///
    /// # Errors
    ///
    /// As [`PlanStore::get`].
    pub fn impact_of(&self, plan: &PlanId) -> Result<ImpactGraph, ErrorValue> {
        Ok(self.get(plan)?.impact().clone())
    }

    /// Resolves a reference to exactly one plan (§36.4).
    ///
    /// # Errors
    ///
    /// - `change.plan_not_found` where nothing matches, or where the text is not an identity at
    ///   all — a reference is refused rather than searched for;
    /// - `change.plan_reference_ambiguous` where more than one plan shares the prefix, naming the
    ///   candidates so the operator can widen it.
    pub fn resolve(&self, reference: &str) -> Result<PlanId, ErrorValue> {
        let parsed = Reference::parse(reference)?;
        if !parsed.names_a_plan() {
            return Err(error::plan_not_found(reference));
        }
        let matches = self.matching_ids("plans", "plan_id", parsed.body())?;
        match matches.len() {
            0 => Err(error::plan_not_found(reference)),
            1 => PlanId::parse(&matches[0]).ok_or_else(|| {
                error::record_malformed("plan_id", "the stored identity is not hexadecimal")
            }),
            _ => Err(error::plan_reference_ambiguous(reference, &matches)),
        }
    }

    /// Resolves a reference to exactly one recovery asset (§37.5, §36.4).
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` where nothing matches or the text is not an identity, and
    /// `change.plan_reference_ambiguous` where more than one asset shares the prefix.
    pub fn resolve_asset(&self, reference: &str) -> Result<RecoveryAssetId, ErrorValue> {
        let parsed = Reference::parse(reference)?;
        if !parsed.names_an_asset() {
            return Err(error::asset_not_found(reference));
        }
        let matches = self.matching_ids("assets", "asset_id", parsed.body())?;
        match matches.len() {
            0 => Err(error::asset_not_found(reference)),
            1 => RecoveryAssetId::parse(&matches[0]).ok_or_else(|| {
                error::record_malformed("asset_id", "the stored identity is not hexadecimal")
            }),
            _ => Err(error::plan_reference_ambiguous(reference, &matches)),
        }
    }

    /// The width a printed plan reference needs so that it still resolves here (§36.4).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn reference_width(&self) -> Result<usize, ErrorValue> {
        let identities = self.all_plan_ids()?;
        let borrowed: Vec<&str> = identities.iter().map(String::as_str).collect();
        Ok(plan_reference_width(&borrowed))
    }

    /// One plan reference, rendered so a reader can type it back (§36.4).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn render_reference(&self, plan: &PlanId) -> Result<String, ErrorValue> {
        Ok(render_plan(plan, self.reference_width()?))
    }

    /// One asset reference, rendered so a reader can type it back (§37.5).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn render_asset_reference(&self, asset: &RecoveryAssetId) -> Result<String, ErrorValue> {
        let identities = self.all_asset_ids()?;
        let borrowed: Vec<&str> = identities.iter().map(String::as_str).collect();
        Ok(render_asset(asset, plan_reference_width(&borrowed)))
    }

    /// The plans `filter` selects, newest first (§5.5's `get plan`).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn list(&self, filter: &PlanFilter) -> Result<Vec<PlanSummary>, ErrorValue> {
        // The reference width is computed over every plan in the store rather than over the
        // filtered rows, so a reference printed under one filter still resolves under another
        // (§36.4).
        let width = self.reference_width()?;
        let mut rows: Vec<PlanSummary> = Vec::new();
        {
            let connection = self.locked();
            let mut statement = connection
                .prepare(
                    "SELECT plan_id, revision, kind, state, session, intent, created_nanos, \
                     sealed_nanos, expires_nanos, digest FROM plans ORDER BY created_nanos DESC, \
                     plan_id ASC, revision DESC",
                )
                .map_err(|failure| self.unavailable(&failure))?;
            let mapped = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                    ))
                })
                .map_err(|failure| self.unavailable(&failure))?;
            for row in mapped {
                let row = row.map_err(|failure| self.unavailable(&failure))?;
                rows.push(summary(row, width)?);
            }
        }
        if !filter.every_revision {
            let mut seen: Vec<PlanId> = Vec::new();
            rows.retain(|row| {
                if seen.contains(&row.id) {
                    return false;
                }
                seen.push(row.id.clone());
                true
            });
        }
        rows.retain(|row| {
            filter
                .session
                .as_ref()
                .is_none_or(|session| row.session.as_ref() == session.as_ref())
                && filter.state.is_none_or(|state| row.state == state)
                && filter.kind.is_none_or(|kind| row.kind == kind)
        });
        if let Some(limit) = filter.limit {
            rows.truncate(limit);
        }
        Ok(rows)
    }

    /// Removes a plan and everything the store held beside it.
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be written.
    pub fn remove(&self, plan: &PlanId) -> Result<(), ErrorValue> {
        self.locked()
            .execute(
                "DELETE FROM plans WHERE plan_id = ?1",
                params![plan.as_str()],
            )
            .map(|_| ())
            .map_err(|failure| self.unavailable(&failure))
    }

    // ---- action status (§41.2) ---------------------------------------------------------------

    /// Records what is known about one action's outcome (§41.2, Appendix F.2).
    ///
    /// This is written as each action settles rather than when the plan finishes, because §41.2's
    /// reconstruction happens after a crash and a plan payload written at the end would not exist.
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be written, including where no such
    /// plan revision exists — the foreign key refuses evidence about a plan nobody stored.
    pub fn record_action_status(
        &self,
        plan: &PlanId,
        revision: u32,
        action: &ActionId,
        ordinal: usize,
        status: ActionStatus,
        at: Timestamp,
        detail: Option<&str>,
    ) -> Result<(), ErrorValue> {
        self.locked()
            .execute(
                "INSERT INTO action_status (plan_id, revision, action_id, ordinal, status, \
                 settled_nanos, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT (plan_id, revision, action_id) DO UPDATE SET \
                 status = excluded.status, settled_nanos = excluded.settled_nanos, \
                 detail = excluded.detail",
                params![
                    plan.as_str(),
                    i64::from(revision),
                    action.as_str(),
                    i64::try_from(ordinal).unwrap_or(i64::MAX),
                    status.as_str(),
                    nanos(at),
                    detail,
                ],
            )
            .map(|_| ())
            .map_err(|failure| self.unavailable(&failure))
    }

    /// Records the lifecycle state a plan revision has reached (§4.1, §22.2, §41.2).
    ///
    /// The state is written where the transition happens rather than at the end, so a shell that
    /// stops in the middle leaves `applying` behind and not `sealed`. §41.2 reads it back: a plan
    /// that says `sealed` after it mutated is a plan `apply` would run a second time, and §2.7's
    /// "a sealed plan applies once" is only true if the store remembers that it did.
    ///
    /// The stored record travels with it, because `state` is a column *and* a field of the
    /// record: [`PlanStore::get_revision`] decodes the record, so a column nobody mirrored into
    /// it would be read back as the state the plan was written with.
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be written, and
    /// `change.plan_not_found` where no such revision exists.
    pub fn record_state(
        &self,
        plan: &PlanId,
        revision: u32,
        state: PlanState,
    ) -> Result<(), ErrorValue> {
        let stored: Option<String> = self
            .locked()
            .query_row(
                "SELECT record FROM plans WHERE plan_id = ?1 AND revision = ?2",
                params![plan.as_str(), i64::from(revision)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?;
        let Some(encoded) = stored else {
            return Err(error::plan_not_found(&render_plan(
                plan,
                plan.as_str().len(),
            )));
        };
        let record = self.decode(&encoded, "record")?;
        let record = rebuild_record(&record, "state", Some(Value::string(state.as_str())))?;
        self.locked()
            .execute(
                "UPDATE plans SET state = ?3, record = ?4 WHERE plan_id = ?1 AND revision = ?2",
                params![
                    plan.as_str(),
                    i64::from(revision),
                    state.as_str(),
                    encode(&record)?,
                ],
            )
            .map(|_| ())
            .map_err(|failure| self.unavailable(&failure))
    }

    /// Every persisted action status of one plan revision, by action identity (§41.2).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read, and
    /// `change.plan_store_corrupt` where a stored status is not a word §4.7 spells.
    pub fn action_statuses(
        &self,
        plan: &PlanId,
        revision: u32,
    ) -> Result<BTreeMap<String, ActionStatus>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(
                "SELECT action_id, status FROM action_status WHERE plan_id = ?1 AND revision = ?2",
            )
            .map_err(|failure| self.unavailable(&failure))?;
        let mapped = statement
            .query_map(params![plan.as_str(), i64::from(revision)], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|failure| self.unavailable(&failure))?;
        let mut statuses = BTreeMap::new();
        for row in mapped {
            let (id, status) = row.map_err(|failure| self.unavailable(&failure))?;
            let status = ActionStatus::from_name(&status).ok_or_else(|| {
                error::record_malformed(
                    "status",
                    &format!(
                        "`{status}` is not a status §4.7 spells, so the plan cannot be \
                              reconstructed from it"
                    ),
                )
            })?;
            statuses.insert(id, status);
        }
        Ok(statuses)
    }

    // ---- the apply claim (§42.3, §42.4) ------------------------------------------------------

    /// Takes the exclusive claim on applying `plan` (§42.4).
    ///
    /// # Errors
    ///
    /// `change.plan_already_applying`, naming the session that holds it, when a live claim belongs
    /// to somebody else. An expired claim is taken over: §42.3 bounds a lock, and a dead session's
    /// row is not a reason to make a plan permanently unappliable.
    pub fn claim(
        &self,
        plan: &PlanId,
        session: &str,
        now: Timestamp,
    ) -> Result<Claim<'_>, ErrorValue> {
        self.claim_for(plan, session, now, CLAIM_LEASE)
    }

    /// Takes the exclusive claim with a lease of `lease` (§42.3).
    ///
    /// # Errors
    ///
    /// As [`PlanStore::claim`].
    pub fn claim_for(
        &self,
        plan: &PlanId,
        session: &str,
        now: Timestamp,
        lease: Duration,
    ) -> Result<Claim<'_>, ErrorValue> {
        let expires_at = self.take_claim(plan, session, now, lease)?;
        Ok(Claim {
            store: self,
            plan: plan.clone(),
            session: Arc::from(session),
            claimed_at: now,
            expires_at,
        })
    }

    /// The session holding a live claim on `plan` as of `now`, where one does (§42.4).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn claim_holder(
        &self,
        plan: &PlanId,
        now: Timestamp,
    ) -> Result<Option<Arc<str>>, ErrorValue> {
        let held: Option<(String, i64)> = self
            .locked()
            .query_row(
                "SELECT session, expires_nanos FROM apply_claims WHERE plan_id = ?1",
                params![plan.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?;
        Ok(held
            .filter(|(_, expires)| *expires > nanos(now))
            .map(|(session, _)| Arc::from(session.as_str())))
    }

    /// Writes the claim row, refusing a live one held by another session (§42.4).
    fn take_claim(
        &self,
        plan: &PlanId,
        session: &str,
        now: Timestamp,
        lease: Duration,
    ) -> Result<Timestamp, ErrorValue> {
        let expires_at = plus(now, lease);
        let mut connection = self.locked();
        let transaction = connection
            .transaction()
            .map_err(|failure| self.unavailable(&failure))?;
        let held: Option<(String, i64)> = transaction
            .query_row(
                "SELECT session, expires_nanos FROM apply_claims WHERE plan_id = ?1",
                params![plan.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?;
        if let Some((holder, expires)) = held
            && expires > nanos(now)
            && holder != session
        {
            return Err(error::plan_already_applying(plan, &holder));
        }
        transaction
            .execute(
                "INSERT INTO apply_claims (plan_id, session, claimed_nanos, expires_nanos) \
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT (plan_id) DO UPDATE SET \
                 session = excluded.session, claimed_nanos = excluded.claimed_nanos, \
                 expires_nanos = excluded.expires_nanos",
                params![plan.as_str(), session, nanos(now), nanos(expires_at),],
            )
            .map_err(|failure| self.unavailable(&failure))?;
        transaction
            .commit()
            .map_err(|failure| self.unavailable(&failure))?;
        Ok(expires_at)
    }

    /// Releases a claim this session holds. A claim somebody else took over is left alone.
    fn release(&self, plan: &PlanId, session: &str) -> Result<(), ErrorValue> {
        self.locked()
            .execute(
                "DELETE FROM apply_claims WHERE plan_id = ?1 AND session = ?2",
                params![plan.as_str(), session],
            )
            .map(|_| ())
            .map_err(|failure| self.unavailable(&failure))
    }

    // ---- recovery assets (§11, §37) ----------------------------------------------------------

    /// Persists one recovery asset (§37.5).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be written, and
    /// `ono.provider_schema_violation` where §11.1's contract is not in this build.
    pub fn put_asset(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        let encoded = encode(&value::asset_record(asset)?)?;
        self.locked()
            .execute(
                "INSERT INTO assets (asset_id, provider, asset_type, state, source_plan, \
                 created_nanos, expires_nanos, record) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (asset_id) DO UPDATE SET provider = excluded.provider, \
                 asset_type = excluded.asset_type, state = excluded.state, \
                 source_plan = excluded.source_plan, created_nanos = excluded.created_nanos, \
                 expires_nanos = excluded.expires_nanos, record = excluded.record",
                params![
                    asset.id().as_str(),
                    asset.provider(),
                    asset.asset_type().as_str(),
                    asset.state().as_str(),
                    asset.source_plan().map(PlanId::as_str),
                    nanos(asset.created_at()),
                    asset.expires_at().map(nanos),
                    encoded,
                ],
            )
            .map(|_| ())
            .map_err(|failure| self.unavailable(&failure))
    }

    /// One recovery asset by identity (§37.5).
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` where the store holds none, and `change.plan_store_corrupt`
    /// where the stored record will not decode.
    pub fn get_asset(&self, asset: &RecoveryAssetId) -> Result<RecoveryAsset, ErrorValue> {
        let stored: Option<String> = self
            .locked()
            .query_row(
                "SELECT record FROM assets WHERE asset_id = ?1",
                params![asset.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?;
        let Some(encoded) = stored else {
            return Err(error::asset_not_found(&asset.to_string()));
        };
        value::asset_from_record(&self.decode(&encoded, "record")?)
    }

    /// Every recovery asset the store holds, newest first (§37.5).
    ///
    /// # Errors
    ///
    /// As [`PlanStore::get_asset`].
    pub fn list_assets(&self) -> Result<Vec<RecoveryAsset>, ErrorValue> {
        let encoded = {
            let connection = self.locked();
            let mut statement = connection
                .prepare("SELECT record FROM assets ORDER BY created_nanos DESC, asset_id ASC")
                .map_err(|failure| self.unavailable(&failure))?;
            let mapped = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|failure| self.unavailable(&failure))?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(|failure| self.unavailable(&failure))?);
            }
            rows
        };
        let mut assets = Vec::with_capacity(encoded.len());
        for text in &encoded {
            assets.push(value::asset_from_record(&self.decode(text, "record")?)?);
        }
        Ok(assets)
    }

    /// Every asset `plan` rests on: the ones its coverage cites and the ones it created (§10.3).
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn assets_for(&self, plan: &PlanId) -> Result<Vec<RecoveryAssetId>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(
                "SELECT asset_id FROM plan_assets WHERE plan_id = ?1 \
                 UNION SELECT asset_id FROM assets WHERE source_plan = ?1 ORDER BY asset_id ASC",
            )
            .map_err(|failure| self.unavailable(&failure))?;
        let mapped = statement
            .query_map(params![plan.as_str()], |row| row.get::<_, String>(0))
            .map_err(|failure| self.unavailable(&failure))?;
        let mut assets = Vec::new();
        for row in mapped {
            let id = row.map_err(|failure| self.unavailable(&failure))?;
            if let Some(parsed) = RecoveryAssetId::parse(&id) {
                assets.push(parsed);
            }
        }
        Ok(assets)
    }

    /// Every plan whose recovery rests on `asset` (§37.3, §2.15).
    ///
    /// This is what a cleanup preview shows before removing an asset, and what
    /// `recovery.cleanup_blocked` names when policy refuses the removal: §2.15 forbids recovery
    /// assets a retained plan requires from being deleted silently.
    ///
    /// # Errors
    ///
    /// `change.plan_store_unavailable` where the store cannot be read.
    pub fn plans_depending_on(&self, asset: &RecoveryAssetId) -> Result<Vec<PlanId>, ErrorValue> {
        let connection = self.locked();
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT plan_id FROM plan_assets WHERE asset_id = ?1 \
                 UNION SELECT source_plan FROM assets \
                 WHERE asset_id = ?1 AND source_plan IS NOT NULL ORDER BY plan_id ASC",
            )
            .map_err(|failure| self.unavailable(&failure))?;
        let mapped = statement
            .query_map(params![asset.as_str()], |row| row.get::<_, String>(0))
            .map_err(|failure| self.unavailable(&failure))?;
        let mut plans = Vec::new();
        for row in mapped {
            let id = row.map_err(|failure| self.unavailable(&failure))?;
            if let Some(parsed) = PlanId::parse(&id) {
                plans.push(parsed);
            }
        }
        Ok(plans)
    }

    // ---- internals ---------------------------------------------------------------------------

    /// The highest revision of `plan`, or `change.plan_not_found`.
    fn latest_revision(&self, plan: &PlanId) -> Result<u32, ErrorValue> {
        let revision: Option<i64> = self
            .locked()
            .query_row(
                "SELECT MAX(revision) FROM plans WHERE plan_id = ?1",
                params![plan.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|failure| self.unavailable(&failure))?
            .flatten();
        revision
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| error::plan_not_found(&render_plan(plan, plan.as_str().len())))
    }

    /// Every distinct identity in `table` whose `column` begins with `prefix`.
    fn matching_ids(
        &self,
        table: &str,
        column: &str,
        prefix: &str,
    ) -> Result<Vec<String>, ErrorValue> {
        // `table` and `column` are compile-time constants at every call site, and `prefix` is
        // bound as a parameter, so nothing a user typed reaches the statement text.
        let sql = format!(
            "SELECT DISTINCT {column} FROM {table} WHERE substr({column}, 1, ?2) = ?1 \
             ORDER BY {column} ASC"
        );
        let connection = self.locked();
        let mut statement = connection
            .prepare(&sql)
            .map_err(|failure| self.unavailable(&failure))?;
        let mapped = statement
            .query_map(
                params![prefix, i64::try_from(prefix.len()).unwrap_or(i64::MAX)],
                |row| row.get::<_, String>(0),
            )
            .map_err(|failure| self.unavailable(&failure))?;
        let mut identities = Vec::new();
        for row in mapped {
            identities.push(row.map_err(|failure| self.unavailable(&failure))?);
        }
        Ok(identities)
    }

    /// Every plan identity the store holds, for §36.4's widening rule.
    fn all_plan_ids(&self) -> Result<Vec<String>, ErrorValue> {
        self.matching_ids("plans", "plan_id", "")
    }

    /// Every asset identity the store holds, for §37.5's widening rule.
    fn all_asset_ids(&self) -> Result<Vec<String>, ErrorValue> {
        self.matching_ids("assets", "asset_id", "")
    }

    /// Reads one stored record back, refusing a row this build cannot decode (§36.2).
    fn decode(&self, encoded: &str, field: &str) -> Result<RecordValue, ErrorValue> {
        let value = ono_value::from_json_str(encoded, &self.schemas).map_err(|failure| {
            error::record_malformed(
                field,
                &format!("the stored value is not readable: {failure}"),
            )
        })?;
        match value {
            Value::Record(record) => Ok((*record).clone()),
            other => Err(error::record_malformed(
                field,
                &format!(
                    "the store holds a {} where §46 declares a record",
                    other.type_name()
                ),
            )),
        }
    }

    fn locked(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn unavailable(&self, failure: &rusqlite::Error) -> ErrorValue {
        unavailable(&self.path, failure)
    }
}

/// One summary row out of the columns the query selected.
type SummaryRow = (
    String,
    i64,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    Option<String>,
);

fn summary(row: SummaryRow, width: usize) -> Result<PlanSummary, ErrorValue> {
    let (id, revision, kind, state, session, intent, created, sealed, expires, digest) = row;
    let plan = PlanId::parse(&id)
        .ok_or_else(|| error::record_malformed("plan_id", "the stored identity is not an id"))?;
    Ok(PlanSummary {
        reference: Arc::from(render_plan(&plan, width).as_str()),
        id: plan,
        revision: u32::try_from(revision).unwrap_or(u32::MAX),
        kind: PlanKind::from_name(&kind).ok_or_else(|| {
            error::record_malformed("kind", &format!("`{kind}` is not a plan kind §3.2 spells"))
        })?,
        state: PlanState::from_name(&state).ok_or_else(|| {
            error::record_malformed("state", &format!("`{state}` is not a state §4.1 draws"))
        })?,
        session: Arc::from(session.as_str()),
        intent: Arc::from(intent.as_str()),
        created_at: instant(created, "created_nanos")?,
        sealed_at: sealed.map(|at| instant(at, "sealed_nanos")).transpose()?,
        expires_at: expires.map(|at| instant(at, "expires_nanos")).transpose()?,
        digest: digest.map(|text| Arc::from(text.as_str())),
    })
}

/// Applies one function to every action record inside a `ono.change-plan/1` record.
///
/// Both §36.3's redaction and §41.2's status overlay are the same surgery — replace the action
/// list with a mapped one and leave the rest of the plan, including the digest the seal was taken
/// over, exactly as it was.
fn map_actions(
    record: &RecordValue,
    map: &dyn Fn(&RecordValue) -> Result<Option<RecordValue>, ErrorValue>,
) -> Result<RecordValue, ErrorValue> {
    let Some(Value::List(actions)) = record.get("actions") else {
        return Ok(record.clone());
    };
    let mut mapped = Vec::with_capacity(actions.len());
    let mut changed = false;
    for item in actions.iter() {
        let Value::Record(action) = item else {
            mapped.push(item.clone());
            continue;
        };
        match map(action)? {
            Some(next) => {
                changed = true;
                mapped.push(Value::Record(Arc::new(next)));
            }
            None => mapped.push(item.clone()),
        }
    }
    if !changed {
        return Ok(record.clone());
    }
    rebuild_record(record, "actions", Some(Value::list(mapped)))
}

/// Lays the persisted action statuses of §41.2 over a stored plan record.
fn overlay_statuses(
    record: &RecordValue,
    statuses: &BTreeMap<String, ActionStatus>,
) -> Result<RecordValue, ErrorValue> {
    if statuses.is_empty() {
        return Ok(record.clone());
    }
    map_actions(record, &|action| {
        let Some(Value::String(id)) = action.get("id") else {
            return Ok(None);
        };
        let Some(status) = statuses.get(id.as_ref()) else {
            return Ok(None);
        };
        rebuild_record(action, "status", Some(Value::string(status.as_str()))).map(Some)
    })
}

/// Encodes one record as the lossless tagged JSON of `ono_value::to_json`.
fn encode(record: &RecordValue) -> Result<String, ErrorValue> {
    ono_value::to_json_string(&Value::Record(Arc::new(record.clone())))
}

/// An instant as the store holds it.
fn nanos(at: Timestamp) -> i64 {
    i64::try_from(at.as_nanosecond()).unwrap_or(i64::MAX)
}

/// One stored instant, refusing a number that is not one (§36.2).
fn instant(nanos: i64, field: &str) -> Result<Timestamp, ErrorValue> {
    Timestamp::from_nanosecond(i128::from(nanos)).map_err(|failure| {
        error::record_malformed(
            field,
            &format!("a stored instant is not a timestamp: {failure}"),
        )
    })
}

/// `now` plus `lease`, saturating rather than wrapping (§42.3).
///
/// A lease that overflowed the representable range would be a lock with no bound, which is the
/// one thing §42.3 forbids. Saturating to `now` makes it expire immediately instead.
fn plus(now: Timestamp, lease: Duration) -> Timestamp {
    let span = i128::try_from(lease.as_nanos()).unwrap_or(i128::MAX);
    Timestamp::from_nanosecond(now.as_nanosecond().saturating_add(span)).unwrap_or(now)
}

fn unavailable(path: &Path, failure: &rusqlite::Error) -> ErrorValue {
    error::store_unavailable(&format!("`{}`: {failure}", path.display()))
}

/// Applies every migration step the store has not yet had (§36.2).
///
/// A store at a version this build does not know is refused rather than downgraded: its rows may
/// mean something this build would misread, and a plan misread is a change misdescribed.
fn migrate(connection: &mut Connection, path: &Path) -> Result<u32, ErrorValue> {
    let has_metadata: bool = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|failure| unavailable(path, &failure))?
        > 0;
    let current = if has_metadata {
        connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![VERSION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|failure| unavailable(path, &failure))?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or_default()
    } else {
        0
    };
    if current > STORE_VERSION {
        return Err(error::store_unavailable(&format!(
            "`{}` was written by a newer Ono at store version {current}; this one reads up to \
             {STORE_VERSION}. A store is never downgraded, because its rows may mean something \
             this build would misread (§36.2).",
            path.display()
        )));
    }
    for step in STEPS {
        if step.version <= current {
            continue;
        }
        let transaction = connection
            .transaction()
            .map_err(|failure| unavailable(path, &failure))?;
        transaction
            .execute_batch(step.sql)
            .map_err(|failure| unavailable(path, &failure))?;
        transaction
            .execute(
                "INSERT INTO metadata (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![VERSION_KEY, step.version.to_string()],
            )
            .map_err(|failure| unavailable(path, &failure))?;
        transaction
            .commit()
            .map_err(|failure| unavailable(path, &failure))?;
    }
    if current == 0 {
        connection
            .execute(
                "INSERT INTO metadata (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![STORE_ID_KEY, path.display().to_string()],
            )
            .map_err(|failure| unavailable(path, &failure))?;
    }
    Ok(STORE_VERSION)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use ono_change_core::{
        ActionRole, ConsistencyClass, DomainCoverage, DomainProtection, EffectDomain, Execution,
        Idempotency, Intent, PlanAction, PlanFragment, ProtectionSummary, RecoveryAssetType,
        RecoveryObjective, RecoveryScope, VerificationClass, VerificationContract,
    };
    use ono_core::ErrorCode;
    use tempfile::TempDir;

    use super::*;
    use crate::builder::PlanBuilder;
    use crate::freeze::ServiceTarget;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a valid instant")
    }

    fn store() -> (TempDir, PlanStore) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store =
            PlanStore::open(&directory.path().join("plans.sqlite3")).expect("a fresh store opens");
        (directory, store)
    }

    fn restart(plan: &PlanId, arguments: Vec<(Arc<str>, Value)>) -> PlanAction {
        PlanAction::new(
            plan,
            1,
            ActionRole::Mutate,
            "restart nginx.service",
            Execution::ProviderAction {
                provider: Arc::from("ono.service.systemd"),
                operation: Arc::from("ono.service.restart"),
                arguments,
            },
        )
        .on("systemd:nginx.service")
        .with_idempotency(Idempotency::Idempotent)
    }

    fn build(session: &str, arguments: Vec<(Arc<str>, Value)>) -> PlanBuilder {
        let builder = PlanBuilder::for_intent(
            Intent::new("restart nginx", "plan restart service nginx"),
            session.to_owned(),
            at(0),
        );
        let id = builder.plan_id().clone();
        let fragment = PlanFragment::empty()
            .at_version("255.7")
            .acting(restart(&id, arguments))
            .verifying(
                VerificationContract::new(
                    &id,
                    VerificationClass::Required,
                    "systemd:nginx.service",
                    "state == running",
                )
                .expecting(Value::string("running")),
            );
        builder
            .contributing(&fragment)
            .expect("a fragment is accepted")
            .resolve(vec![
                ServiceTarget::new("systemd", "nginx.service")
                    .freeze()
                    .expect("a namespaced unit freezes"),
            ])
            .expect("one target resolves")
    }

    fn sealed_plan(session: &str) -> ChangePlan {
        build(session, Vec::new())
            .seal(at(60))
            .expect("a plan seals")
    }

    fn plan_with_secret(session: &str) -> ChangePlan {
        build(
            session,
            vec![(Arc::from("password"), Value::string("hunter2"))],
        )
        .seal(at(60))
        .expect("a plan seals")
    }

    fn asset(created_at: Timestamp) -> RecoveryAsset {
        RecoveryAsset::proposed(
            "ono.recovery.zfs",
            RecoveryAssetType::ZfsSnapshot,
            "rpool/etc@ono-a82f",
            RecoveryScope::new("zfs-dataset", "rpool/etc", "host-1"),
            created_at,
        )
    }

    fn protected_plan(session: &str, recovery: &RecoveryAsset) -> ChangePlan {
        build(session, Vec::new())
            .with_protection(ProtectionSummary::of(vec![
                DomainCoverage::new(
                    EffectDomain::FilesystemPersistent,
                    RecoveryObjective::PreserveExact,
                    DomainProtection::Protected,
                    "the zfs snapshot holds the prior bytes",
                )
                .by_asset(recovery.id().clone())
                .at_consistency(ConsistencyClass::FilesystemConsistent),
            ]))
            .seal(at(60))
            .expect("a plan seals")
    }

    #[test]
    fn should_refuse_a_store_written_by_a_newer_build_rather_than_downgrade_it() {
        let temporary = tempfile::tempdir().expect("a temporary directory");
        let path = temporary.path().join("plans.sqlite3");
        {
            let store = PlanStore::open(&path).expect("a fresh store opens");
            store
                .locked()
                .execute(
                    "INSERT INTO metadata (key, value) VALUES (?1, ?2) \
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    params![VERSION_KEY, "99"],
                )
                .expect("the version is written");
        }
        let refusal = PlanStore::open(&path).expect_err("§36.2 refuses a version it cannot read");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangePlanStoreUnavailable,
            "§36.2: a store at an unknown version is refused, never downgraded"
        );
    }

    #[test]
    fn should_open_at_the_version_this_build_ships() {
        let (_directory, store) = store();
        assert_eq!(store.store_version(), STORE_VERSION);
    }

    #[test]
    fn should_refuse_a_plan_whose_stored_record_was_damaged() {
        let (_directory, store) = store();
        let plan = sealed_plan("session-1");
        store.put(&plan).expect("a sealed plan is persisted");
        store
            .locked()
            .execute(
                "UPDATE plans SET record = ?1 WHERE plan_id = ?2",
                params![
                    "{\"$record\": {\"schema\": \"nonsense\"}}",
                    plan.id().as_str()
                ],
            )
            .expect("the row is damaged");
        let refusal = store
            .get(plan.id())
            .expect_err("§36.2 refuses a row it cannot read");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangePlanStoreCorrupt,
            "§36.2: a corrupt row refuses rather than returning a partially-read plan"
        );
    }

    #[test]
    fn should_refuse_a_plan_whose_stored_record_is_not_json_at_all() {
        let (_directory, store) = store();
        let plan = sealed_plan("session-1");
        store.put(&plan).expect("a sealed plan is persisted");
        store
            .locked()
            .execute(
                "UPDATE plans SET record = ?1 WHERE plan_id = ?2",
                params!["not json", plan.id().as_str()],
            )
            .expect("the row is damaged");
        let refusal = store
            .get(plan.id())
            .expect_err("§36.2 refuses a row it cannot read");
        assert_eq!(refusal.code(), ErrorCode::ChangePlanStoreCorrupt);
    }

    #[test]
    fn should_refuse_a_stored_action_status_that_is_not_a_word_the_spec_spells() {
        let (_directory, store) = store();
        let plan = sealed_plan("session-1");
        store.put(&plan).expect("a sealed plan is persisted");
        store
            .locked()
            .execute(
                "INSERT INTO action_status (plan_id, revision, action_id, ordinal, status, \
                 settled_nanos) VALUES (?1, 1, ?2, 1, 'probably-fine', 0)",
                params![plan.id().as_str(), plan.actions()[0].id().as_str()],
            )
            .expect("the row is written");
        let refusal = store
            .get(plan.id())
            .expect_err("§4.7's vocabulary is closed");
        assert_eq!(refusal.code(), ErrorCode::ChangePlanStoreCorrupt);
    }

    #[test]
    fn should_refuse_action_evidence_about_a_plan_it_never_stored() {
        let (_directory, store) = store();
        let plan = sealed_plan("session-1");
        let refusal = store
            .record_action_status(
                plan.id(),
                1,
                plan.actions()[0].id(),
                1,
                ActionStatus::Succeeded,
                at(90),
                None,
            )
            .expect_err("§41.2's evidence is about a plan the store holds");
        assert_eq!(refusal.code(), ErrorCode::ChangePlanStoreUnavailable);
    }

    #[test]
    fn should_keep_the_store_and_its_database_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt as _;
        let (directory, store) = store();
        let mode = std::fs::metadata(store.path())
            .expect("the database exists")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(
            mode, 0o600,
            "§43.1: a plan names targets and arguments for changes that have not happened yet"
        );
        drop(directory);
    }

    #[test]
    fn should_persist_a_plan_that_carries_a_secret_as_a_handle() {
        let (_directory, store) = store();
        let plan = plan_with_secret("session-1");
        store.put(&plan).expect("a sealed plan is persisted");
        let stored: String = store
            .locked()
            .query_row(
                "SELECT record FROM plans WHERE plan_id = ?1",
                params![plan.id().as_str()],
                |row| row.get(0),
            )
            .expect("the row is there");
        assert!(
            !stored.contains("hunter2"),
            "§36.3: secrets MUST NOT be persisted in raw form"
        );
        assert!(
            stored.contains(crate::secrets::HANDLE_PREFIX),
            "§36.3: an opaque secret handle stands in its place"
        );
    }

    #[test]
    fn should_record_which_assets_a_plans_coverage_rests_on() {
        let (_directory, store) = store();
        let recovery = asset(at(30));
        let plan = protected_plan("session-1", &recovery);
        store.put_asset(&recovery).expect("an asset is persisted");
        store.put(&plan).expect("a sealed plan is persisted");
        assert_eq!(
            store.assets_for(plan.id()).expect("the store answers"),
            vec![recovery.id().clone()],
            "§10.3: the coverage matrix names the assets the plan's protection rests on"
        );
    }
}
