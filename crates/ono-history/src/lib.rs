//! Semantic command history, and the retained result history beside it.
//!
//! Two different histories live here, and they are not the same thing. [`History`] is what was
//! *run* — the command text, where it ran and how it ended (spec §20.1), persisted as JSONL.
//! [`ResultHistory`] is what commands *produced* — the recent structured results `@-1` reaches
//! (spec §20.2, v0.4.1 §24), held in memory and bounded in four dimensions.
//!
//! Spec §20.1: "History records semantics, not only strings." An entry remembers where a command
//! ran, how it ended and how long it took, so a later session can answer questions about it
//! rather than only replay its text.
//!
//! The file is one JSON object per line. That format is chosen for a specific failure: a torn
//! write at the moment a machine loses power costs one entry, not the history. Reading tolerates
//! a line it cannot parse and keeps going, for the same reason.
//!
//! ```no_run
//! use std::path::Path;
//! use std::time::Duration;
//! use ono_core::ExitStatus;
//! use ono_history::{History, Outcome, Policy};
//!
//! let mut history = History::open(Path::new("/tmp/ono-history.jsonl"), Policy::default())?;
//! history.record(
//!     "get process",
//!     Path::new("/home/case"),
//!     Outcome::new(ExitStatus::SUCCESS, Duration::from_millis(4)),
//! );
//! history.flush()?;
//! # Ok::<(), ono_history::HistoryError>(())
//! ```

#![forbid(unsafe_code)]

mod cursor;
mod entry;
mod id;
mod policy;
mod results;
mod store;

pub use cursor::{Cursor, Direction};
pub use entry::{Entry, Outcome};
pub use policy::Policy;
pub use results::{ResultHistory, Retained, RetentionLimits, StoppedBy, value_size};
pub use store::{History, HistoryError};
