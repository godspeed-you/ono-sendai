//! Where a listening agent's audit events go by default (v0.4.1 §14).
//!
//! `ono-protocol` decides *what* is recorded, beside the loop that makes the decisions;
//! this module decides *where*, which is the listening agent's business and not the protocol's.
//!
//! The default is one line per event on stderr. §14 asks for structured events and §54.2 forbids
//! making an operator turn on debug logging to learn why a policy denied them, so the line is a
//! fixed `key=value` shape a script can cut, on the stream a listening agent already reports on —
//! stdout belongs to the wire when the agent is carried over stdio, and never carries diagnostics.

use std::io::Write as _;

use ono_protocol::{AuditEvent, AuditSink};

/// The default sink: one `ono-audit …` line per event on standard error.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrAudit;

impl AuditSink for StderrAudit {
    fn record(&self, event: &AuditEvent) {
        // A failure to write an audit line must not take the connection down with it: the
        // operator loses a line, and the decision the line described has already been made and
        // enforced.
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "{}", event.render());
    }
}
