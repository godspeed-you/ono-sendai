//! The prompt as a place indicator, and the fact that decides whether a view may open at all
//! (spec v0.4 §5, §21, §29.1, §29.3).
//!
//! §21.1 fixes the semantic components a prompt renderer must be able to reach — link/host, the
//! current place, the privilege state and any context warning — and §21.2 fixes how much of the
//! place is shown: `<host>/<current-place-kind>/<display-name>`, never the whole trail, because
//! `trail` is where the history lives. Both are answered here, from the session's spatial state
//! and nothing else.
//!
//! The second thing this module owns is smaller and matters more: whether this process is an
//! interactive shell at a terminal. §29.1 forbids a hidden TUI dependency and §29.3 forbids a
//! script from ever opening a picker, so every full-screen view and every picker in this crate
//! asks [`at_terminal`] first. It is false in `ono -c`, false when a script is read from a pipe,
//! and false the moment either standard stream is not a terminal.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once, by the interactive loop, before the first prompt is drawn.
static INTERACTIVE: AtomicBool = AtomicBool::new(false);

/// Records that this process is running the interactive loop (spec v0.4 §29.1).
pub fn mark_interactive() {
    INTERACTIVE.store(true, Ordering::Relaxed);
}

/// Whether a full-screen view or a picker may take the terminal over.
///
/// Three things must hold together: the interactive loop is running, keys can be read, and what
/// is drawn can be seen. A pipeline inside an interactive session is still interactive — the
/// shell is — but a command whose values are consumed rather than shown never asks this
/// question, because the evaluator tells it so ([`ono_command::Invocation::displays`]).
#[must_use]
pub fn at_terminal() -> bool {
    INTERACTIVE.load(Ordering::Relaxed)
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
}

/// The place segment of the prompt: `local`, `local/compute`, `local/process/nginx` (§21.1, §21.2).
///
/// Read from what the last holder of the spatial state published rather than from the state
/// itself: the state is held behind the prompt by the reads ADR-0984 and ADR-0982 start, and a
/// prompt that waited for it would wait on a provider, while one that gave up named the root
/// over a session standing elsewhere (issue #311, ADR-1032).
#[must_use]
pub fn place_segment() -> Option<String> {
    Some(crate::spatial::session::standing().path.clone())
}

/// The host segment of the prompt where the session stands on a linked host (§19.2, §21.1);
/// `None` on the local one.
///
/// Read from the same publication as [`place_segment`], so the two always describe one moment:
/// a command crossing a link changes the geography the moment it moves, and a prompt drawn
/// before it lets the state go named the new host beside the old place (ADR-1032).
#[must_use]
pub fn host_segment() -> Option<String> {
    crate::spatial::session::standing().host.clone()
}
