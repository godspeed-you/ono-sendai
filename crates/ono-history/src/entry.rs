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
    /// Where the command ran. Absent for an entry a package wrote, which ran in no directory of
    /// the session's (ADR-1015); every entry the shell records itself has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    session: String,
    /// The adapters that shaped the command's programs (spec v0.3 §1.62), by full id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    adapters: Vec<String>,
    /// The argv each adapter actually ran, in the same order as `adapters`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    plans: Vec<String>,
    /// The KUANG/11 package that wrote the entry, set by the host and never by the package
    /// (spec §31.7 `history.write: {scope: plugin}`, ADR-1015). Absent means the operator's own
    /// entry, which is what every line written before attribution existed is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    package: Option<String>,
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
            cwd: Some(cwd),
            status: outcome.as_ref().map(|outcome| outcome.status.code()),
            duration_ms: outcome
                .as_ref()
                .map(|outcome| u64::try_from(outcome.duration.as_millis()).unwrap_or(u64::MAX)),
            session,
            adapters: outcome
                .as_ref()
                .map(|outcome| outcome.adapters.clone())
                .unwrap_or_default(),
            plans: outcome.map(|outcome| outcome.plans).unwrap_or_default(),
            package: None,
        }
    }

    /// An entry `package` wrote through the host, attributed by the host (ADR-1015).
    pub(crate) fn attributed(
        id: String,
        session: String,
        package: &str,
        command: String,
        status: Option<ExitStatus>,
        duration: Option<Duration>,
    ) -> Self {
        Self {
            id,
            at: Timestamp::now(),
            command,
            cwd: None,
            status: status.map(|status| status.code()),
            duration_ms: duration
                .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)),
            session,
            adapters: Vec::new(),
            plans: Vec::new(),
            package: Some(package.to_owned()),
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

    /// The working directory the command ran in; `None` for an entry a package wrote.
    #[must_use]
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// The package that wrote the entry, or `None` for the operator's own (ADR-1015).
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
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

    /// The adapters that shaped the command's programs, by full id (spec v0.3 §1.62).
    #[must_use]
    pub fn adapters(&self) -> &[String] {
        &self.adapters
    }

    /// What the entry can say about how it ran: each adapter and the argv it planned, or that
    /// no adapter took part. History records semantics (spec §20.1), so `explain` of a past
    /// command answers from the record and re-runs nothing.
    #[must_use]
    pub fn explain(&self) -> String {
        if self.adapters.is_empty() {
            return "no adapter took part".to_owned();
        }
        self.adapters
            .iter()
            .zip(
                self.plans
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::repeat("")),
            )
            .map(|(adapter, plan)| {
                if plan.is_empty() {
                    format!("adapted by {adapter}")
                } else {
                    format!("adapted by {adapter}: {plan}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    status: ExitStatus,
    duration: Duration,
    adapters: Vec<String>,
    plans: Vec<String>,
}

impl Outcome {
    /// An outcome with the given status and duration.
    #[must_use]
    pub fn new(status: ExitStatus, duration: Duration) -> Self {
        Self {
            status,
            duration,
            adapters: Vec::new(),
            plans: Vec::new(),
        }
    }

    /// Records which adapters shaped the command and what each one ran (spec v0.3 §1.62).
    #[must_use]
    pub fn adapted_by(
        mut self,
        adapters: impl IntoIterator<Item = String>,
        plans: impl IntoIterator<Item = String>,
    ) -> Self {
        self.adapters = adapters.into_iter().collect();
        self.plans = plans.into_iter().collect();
        self
    }

    /// The command's exit status.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        self.status
    }

    /// How long it took.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }
}
