//! The Ono-Sendai persistent temporal ledger: SQLite in WAL mode at the canonical user-private
//! path, with migrations, retention, integrity and source-sequence continuity (spec v0.5 §31, §39).
//!
//! §31.1 makes the store normative rather than incidental — "the reference implementation MUST use
//! SQLite in WAL mode for the local persistent temporal ledger" at
//! `~/.local/share/ono/temporal/ledger.sqlite3` — "so tests and recovery behavior are
//! deterministic". Everything above this crate speaks [`LedgerRead`](ono_temporal_core::LedgerRead)
//! and [`LedgerWrite`](ono_temporal_core::LedgerWrite), which `ono-temporal-core` owns, so no SQL
//! word crosses the boundary (§39.4).
//!
//! Four rules shape the crate, and each is a section of the specification rather than a
//! preference:
//!
//! - **Append-only.** §6.7: "persisted events MUST be append-only". There is no update path for an
//!   event; a correction is a new event or a new evidence record referring to the old one, and the
//!   only writer that rewrites anything is a migration "that preserves semantic identity" (§31.6).
//! - **Private by construction.** §30.2 gives the directory `0700` and the database `0600`, and
//!   both are created that way rather than narrowed afterwards. A file that was briefly
//!   world-readable was world-readable.
//! - **Honest about loss.** §43.2 prefers "an explicit coverage gap over pretending continuity",
//!   so a break in a sequence a source declared contiguous becomes a
//!   [`TemporalGap`](ono_temporal_core::TemporalGap), and so does an interval discarded because it
//!   was corrupt (§31.7).
//! - **Bounded.** §31.8 makes retention "bounded background work" and §32.3 budgets every query, so
//!   a sweep removes one batch per call and a query over a million events reads the rows it
//!   answers with.
//!
//! # The two ledgers
//!
//! [`Ledger`] is one handle over both: §10.7's bounded in-memory ledger, which
//! `ono-temporal-core` owns as `SessionLedger`, and [`LedgerStore`], the persistent one. Holding
//! the enum is what lets a caller stay ignorant of whether recording is on, and it is what keeps
//! §32.1's promise that a disabled recorder opens no file.
//!
//! ```
//! use ono_temporal_core::LedgerRead as _;
//!
//! // §10.2: recording is off by default, and the disabled path touches no filesystem.
//! let ledger = ono_temporal_ledger::Ledger::default();
//! assert!(!ledger.is_persistent());
//! assert_eq!(ledger.retention().events, 0);
//! ```

#![forbid(unsafe_code)]

mod codec;
mod integrity;
mod ledger;
mod migrate;
mod path;
mod retention;
mod rows;
mod sequences;
mod store;

pub use codec::decode_payload;
pub use integrity::{IntegrityFinding, IntegrityReport};
pub use ledger::Ledger;
pub use migrate::{LOGICAL_SETS, STORE_VERSION};
pub use path::{DATABASE_MODE, DATABASE_NAME, DIRECTORY_MODE, ledger_directory, ledger_path};
pub use retention::{
    DEFAULT_MAX_AGE, DEFAULT_MAX_SIZE, DEFAULT_RETENTION_BATCH, RetentionPolicy, Swept,
};
pub use sequences::SourceSequence;
pub use store::{Durability, LedgerStore, StoreOptions};
