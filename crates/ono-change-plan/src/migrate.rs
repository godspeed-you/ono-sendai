//! Versioned, forward-only plan-store migrations (spec v0.6 §36.2, §41.2).
//!
//! §36.2: *"The reference implementation SHOULD use SQLite with versioned schema, consistent with
//! v0.5 storage practice where practical."* v0.5's practice is a list of steps, applied in order,
//! each inside its own transaction, with the version recorded in a metadata table. A store at
//! version `n` reaching an Ono that ships version `m > n` runs steps `n+1 ..= m`; a store at a
//! version this Ono does not know is **refused, never downgraded**, because a newer build's rows
//! may mean something this one would misread, and a plan misread is a change misdescribed.
//!
//! The shape of version 1 follows from three sentences of the specification rather than from
//! database taste:
//!
//! - §41.2 requires plan state to be "reconstructable from persisted action records and provider
//!   evidence", so an action's settled status is a row of its own rather than a field inside the
//!   plan payload. A crash between two actions leaves the rows that were written.
//! - §7.5 creates revisions and §4.4 makes each of them immutable, so the primary key is
//!   `(plan_id, revision)` and a rebase inserts rather than updates.
//! - §42.4 forbids two sessions applying one sealed plan, so the claim is a row with a uniqueness
//!   constraint on the plan, and §42.3's bound is the expiry column beside it.

/// One forward migration step: the version it produces and the statements that produce it.
pub(crate) struct Step {
    /// The store version this step leaves behind.
    pub(crate) version: u32,
    /// What the step does. Executed as one batch inside one transaction.
    pub(crate) sql: &'static str,
}

/// The version a freshly created store carries, and the version every older store migrates to.
pub const STORE_VERSION: u32 = 1;

/// The metadata key holding the store's schema version.
pub(crate) const VERSION_KEY: &str = "store_version";

/// The metadata key holding the identity of this store, so a diagnostic can name it.
pub(crate) const STORE_ID_KEY: &str = "store_id";

/// Version 1: plans and their revisions, per-action status, recovery assets and the apply claim.
///
/// Every column outside a payload is one a query filters or orders on — the plan identity, the
/// revision, the state, the session, the instant — and the plan itself travels as one
/// `ono.change-plan/1` record encoded with `ono_value::to_json`'s tagged form. The record is the
/// contract of §46.1; encoding fields a second time as columns would give the store two answers
/// about one plan and eventually let them disagree.
///
/// `impact` is a column beside the plan rather than inside it because `ono.change-plan/1` carries
/// an impact *summary* and §46.7's graph is its own record (§9.6). A plan read back without its
/// graph is still the plan; a plan read back with a graph that disagrees with its summary is not.
///
/// `plan_assets` exists so that §37.3's cleanup preview and §2.15's refusal are a row count rather
/// than a decode of every stored plan: "which plans stop being recoverable if this asset goes" is
/// the question, and it has to be answerable before the asset is deleted.
const V1: &str = "
CREATE TABLE metadata (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE plans (
    plan_id       TEXT NOT NULL,
    revision      INTEGER NOT NULL,
    kind          TEXT NOT NULL,
    state         TEXT NOT NULL,
    session       TEXT NOT NULL,
    intent        TEXT NOT NULL,
    created_nanos INTEGER NOT NULL,
    sealed_nanos  INTEGER,
    expires_nanos INTEGER,
    digest        TEXT,
    record        TEXT NOT NULL,
    impact        TEXT,
    PRIMARY KEY (plan_id, revision)
) STRICT;

CREATE INDEX plans_by_session ON plans (session, created_nanos);
CREATE INDEX plans_by_state   ON plans (state, created_nanos);
CREATE INDEX plans_by_time    ON plans (created_nanos);

CREATE TABLE action_status (
    plan_id       TEXT NOT NULL,
    revision      INTEGER NOT NULL,
    action_id     TEXT NOT NULL,
    ordinal       INTEGER NOT NULL,
    status        TEXT NOT NULL,
    settled_nanos INTEGER NOT NULL,
    detail        TEXT,
    PRIMARY KEY (plan_id, revision, action_id),
    FOREIGN KEY (plan_id, revision) REFERENCES plans (plan_id, revision) ON DELETE CASCADE
) STRICT;

CREATE INDEX action_status_by_plan ON action_status (plan_id, revision, ordinal);

CREATE TABLE assets (
    asset_id      TEXT PRIMARY KEY NOT NULL,
    provider      TEXT NOT NULL,
    asset_type    TEXT NOT NULL,
    state         TEXT NOT NULL,
    source_plan   TEXT,
    created_nanos INTEGER NOT NULL,
    expires_nanos INTEGER,
    record        TEXT NOT NULL
) STRICT;

CREATE INDEX assets_by_plan  ON assets (source_plan);
CREATE INDEX assets_by_state ON assets (state, created_nanos);

CREATE TABLE plan_assets (
    plan_id  TEXT NOT NULL,
    revision INTEGER NOT NULL,
    asset_id TEXT NOT NULL,
    PRIMARY KEY (plan_id, revision, asset_id),
    FOREIGN KEY (plan_id, revision) REFERENCES plans (plan_id, revision) ON DELETE CASCADE
) STRICT;

CREATE INDEX plan_assets_by_asset ON plan_assets (asset_id);

CREATE TABLE apply_claims (
    plan_id       TEXT PRIMARY KEY NOT NULL,
    session       TEXT NOT NULL,
    claimed_nanos INTEGER NOT NULL,
    expires_nanos INTEGER NOT NULL
) STRICT;
";

/// Every migration, in order.
pub(crate) const STEPS: &[Step] = &[Step {
    version: 1,
    sql: V1,
}];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_ship_a_step_for_every_version_up_to_the_current_one() {
        let versions: Vec<u32> = STEPS.iter().map(|step| step.version).collect();
        assert_eq!(
            versions,
            (1..=STORE_VERSION).collect::<Vec<u32>>(),
            "§36.2: a versioned schema is only versioned if every version has a step"
        );
    }

    #[test]
    fn should_keep_the_steps_in_ascending_order() {
        assert!(
            STEPS.windows(2).all(|pair| pair[0].version < pair[1].version),
            "migrations are forward-only and applied in order (§36.2)"
        );
    }
}
