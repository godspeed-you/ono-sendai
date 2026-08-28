//! The spatial commands the shell dispatches (spec v0.4 §6, §45.6, §46).
//!
//! §45.6: "`ono-cli` should parse/dispatch spatial commands and own session current-place state,
//! but SHOULD NOT implement graph selection, identity reconciliation or map layout directly."
//! That is the split here. The shell reads the arguments, knows which host and which boot the
//! session belongs to, asks the providers for the objects a query needs, and hands everything
//! else on:
//!
//! - which record is which place, and whether two records are one — `ono-spatial-index`'s
//!   provider bridge (§45.2);
//! - which places answer a query, in which order, and what a query may cost —
//!   `ono-spatial-query` (§45.3).
//!
//! What it does own is the state neither of those crates can have: the host and boot identity
//! every observation belongs to (§10.2), and the pins that outlive the session (§46.1).

pub mod commands;
pub mod find;
pub mod movement;
pub mod pins;
pub mod relations;
pub mod session;
pub mod storage;
pub mod view;

pub use commands::{Enter, Follow, Home, Look, Near, enter_observed};
pub use find::{FindPlace, local_scope, spatial_type};
pub use movement::{Back, Jump, Trail, Up};
pub use pins::{PinPlace, PinStore, UnpinPlace, pin_path};
pub use session::{SpatialSessionState, spatial_session};
