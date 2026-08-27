//! What one history entry remembers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use ono_core::ExitStatus;
use serde::{Deserialize, Serialize};

/// One command, and what happened when it ran.
///
/// Spec §20.1: "History records semantics, not only strings". An entry knows where it ran, how
/// it ended and how long it took, so a later session can answer questions about it without
/// re-running anything. The context snapshot and structured result reference of §20.1 arrive
/// with the context stack in phase E; the fields that phase A can honestly fill are filled now,
/// and nothing is fabricated (spec §35.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    id: String,
    at: Timestamp,
    command: String,
    cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    session: String,
}

impl Entry {
    pub(crate) fn new(
        id: String,
        session: String,
        command: String,
        cwd: PathBuf,
        outcome: Option<Outcome>,
    ) -> Self {
        Self {
            id,
            at: Timestamp::now(),
            command,
            cwd,
            status: outcome.map(|outcome| outcome.status.code()),
            duration_ms: outcome
                .map(|outcome| u64::try_from(outcome.duration.as_millis()).unwrap_or(u64::MAX)),
            session,
        }
    }

    /// The entry's identity, stable across sessions, so a result or a note can refer to it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// When the command ran.
    #[must_use]
    pub fn at(&self) -> Timestamp {
        self.at
    }

    /// The command exactly as it will be recalled — after redaction, never before (spec §17.5).
    #[must_use]
    pub fn command_text(&self) -> &str {
        &self.command
    }

    /// The working directory the command ran in.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// How the command ended, if the shell waited for it.
    #[must_use]
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.status.map(ExitStatus::from_code)
    }

    /// How long the command took, if it was measured.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(Duration::from_millis)
    }

    /// The shell session that ran it, so a timeline can group by session (spec §20.3).
    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }
}

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    status: ExitStatus,
    duration: Duration,
}

impl Outcome {
    /// An outcome with the given status and duration.
    #[must_use]
    pub fn new(status: ExitStatus, duration: Duration) -> Self {
        Self { status, duration }
    }

    /// The command's exit status.
    #[must_use]
    pub fn status(self) -> ExitStatus {
        self.status
    }

    /// How long it took.
    #[must_use]
    pub fn duration(self) -> Duration {
        self.duration
    }
}
