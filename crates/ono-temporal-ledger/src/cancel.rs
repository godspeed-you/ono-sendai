//! Asking, from the middle of a long read, whether anybody still wants the answer (v0.5 §32.6).
//!
//! §32.6: "Long historical queries MUST be cancellable using normal Ono cancellation semantics."
//! A query over a large ledger is one synchronous scan: the caller is inside `events` for as long
//! as it takes, and a shell waiting for it cannot answer a Ctrl-C it has already been given. §32.3
//! makes a query bounded work, so the scan reads in batches, and a batch boundary is where it can
//! be asked to stop.
//!
//! The store does not know what cancellation *means* — spec §18.5 is the shell's, and §39.4 keeps
//! this crate out of it — so the shell says how to ask, once, and the scan asks.

use std::sync::OnceLock;

/// How the store asks whether the work it is doing has been abandoned.
static WATCH: OnceLock<fn() -> bool> = OnceLock::new();

/// How many rows a scan reads between two questions.
///
/// Large enough that an ordinary read pays the question once, small enough that a scan over a
/// hundred thousand events answers a Ctrl-C in the time it takes to decode a few hundred rows.
pub(crate) const BATCH: usize = 256;

/// Teaches the store how to ask whether the caller has been asked to stop.
///
/// The first caller wins and later ones are ignored, so a process has one answer to the question
/// rather than one per shell. Nothing is watched until somebody sets this: a store used by a test,
/// a migration or a recorder flush scans to the end as it always did.
///
/// ```
/// ono_temporal_ledger::watch_for_cancellation(|| false);
/// ```
pub fn watch_for_cancellation(check: fn() -> bool) {
    let _ = WATCH.set(check);
}

/// Whether the caller has been asked to stop. `false` where nobody is watching.
pub(crate) fn cancelled() -> bool {
    WATCH.get().is_some_and(|check| check())
}

/// The refusal a scan that was told to stop answers with.
///
/// `stream.cancelled` rather than a temporal condition: the query was not refused by the store,
/// it was abandoned by the shell, and §18.5's vocabulary is what the shell turns back into
/// 128 + SIGINT.
pub(crate) fn cancelled_read() -> ono_value::ErrorValue {
    ono_value::ErrorValue::new(
        ono_core::ErrorCode::StreamCancelled,
        "the historical query was interrupted",
    )
}
