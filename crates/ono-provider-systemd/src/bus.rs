//! The systemd D-Bus surface this provider uses, as a trait it owns.
//!
//! Spec §23.3 requires the service provider to "use systemd D-Bus APIs where available rather
//! than shelling out to `systemctl` and parsing text", and spec §50 forbids parsing unstable
//! human-readable output at all. Everything below is therefore a typed rendering of what
//! `org.freedesktop.systemd1.Manager` and the `org.freedesktop.DBus.Properties` interface return
//! — never of what a command-line tool printed.
//!
//! The trait exists because systemd is the outside world, and the outside world is absent from
//! the machines this crate is tested on. A recorded implementation of [`SystemdBus`] is a fake of
//! that outside world in the sense AGENTS.md §11 permits, in the same way a procfs fixture is;
//! it is not a mock of a layer this crate wrote.

use std::fmt;

use ono_core::ErrorCode;
use ono_value::ErrorValue;

/// One row of `org.freedesktop.systemd1.Manager.ListUnits`.
///
/// `ListUnits` returns ten columns; the five kept here are the ones a query can be narrowed on
/// before the per-unit property read that follows. The rest — the followed unit, the object path
/// and the pending job — describe the call, not the service.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitListing {
    /// The unit name, suffix included: `nginx.service`.
    pub name: String,
    /// `Description`, the human sentence the unit file declares.
    pub description: Option<String>,
    /// `LoadState`: `loaded`, `not-found`, `masked`, `error`.
    pub load_state: Option<String>,
    /// `ActiveState`: `active`, `activating`, `deactivating`, `inactive`, `failed`, `reloading`.
    pub active_state: Option<String>,
    /// `SubState`, the unit-type-specific state such as `running`, `exited` or `dead`.
    pub sub_state: Option<String>,
    /// The unit's D-Bus object path, which `ListUnits` already answered with.
    ///
    /// Reading a unit's properties needs its path, and asking `Manager.LoadUnit` for a path the
    /// listing has already given is a round trip per unit that buys nothing (ADR-0561).
    pub path: Option<String>,
}

/// The properties of one unit, read from `org.freedesktop.DBus.Properties.GetAll`.
///
/// Every field is optional because systemd genuinely does not expose every property for every
/// unit: a `.timer` has no `MainPID`, a unit outside a cgroup has no `MemoryCurrent`, and a unit
/// that has never run has no `Result`. `None` means systemd did not say, and it becomes a null
/// rather than a zero (spec §35.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitProperties {
    /// `Id` — the unit name, suffix included.
    pub name: String,
    /// `Description`.
    pub description: Option<String>,
    /// `LoadState`.
    pub load_state: Option<String>,
    /// `ActiveState`.
    pub active_state: Option<String>,
    /// `SubState`.
    pub sub_state: Option<String>,
    /// `UnitFileState`: `enabled`, `disabled`, `masked`, `static`, `indirect`, …
    pub unit_file_state: Option<String>,
    /// `FragmentPath` — the unit file backing the unit, empty for a transient or generated one.
    pub fragment_path: Option<String>,
    /// `StateChangeTimestamp`, in microseconds since the Unix epoch: when the unit last moved.
    pub state_change_usec: Option<u64>,
    /// `MainPID`. Zero is systemd's way of saying there is no main process.
    pub main_pid: Option<u32>,
    /// `MemoryCurrent`, in bytes. `u64::MAX` is systemd's way of saying it does not know.
    pub memory_current: Option<u64>,
    /// `TasksCurrent`. `u64::MAX` is systemd's way of saying it does not know.
    pub tasks_current: Option<u64>,
    /// `Result`: `success`, `exit-code`, `signal`, `timeout`, `oom-kill`, `core-dump`, …
    ///
    /// This is what turns "the unit failed" into "the unit failed *because*", which spec §33.2
    /// shows as the `DETAIL` column of a failed-service listing.
    pub result: Option<String>,
    /// `ExecMainStatus` — the exit status of the last main process, where one has exited.
    pub exec_main_status: Option<i32>,
    /// The units this one requires: `Requires`, `Requisite`, `BindsTo` and `Wants`, merged and
    /// sorted. Ordering (`After`, `Before`) is deliberately not among them (ADR-0239).
    pub dependencies: Vec<String>,
}

impl UnitProperties {
    /// The listing form of these properties, for a provider that already read them.
    #[must_use]
    pub fn listing(&self) -> UnitListing {
        UnitListing {
            name: self.name.clone(),
            description: self.description.clone(),
            load_state: self.load_state.clone(),
            active_state: self.active_state.clone(),
            sub_state: self.sub_state.clone(),
            // A listing made from properties already in hand is not one `ListUnits` answered,
            // so it names no path: whoever holds these properties has nothing left to read.
            path: None,
        }
    }
}

/// A job the `Manager` interface can be asked to queue for a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// `StartUnit`.
    Start,
    /// `StopUnit`.
    Stop,
    /// `RestartUnit`.
    Restart,
    /// `ReloadUnit`.
    Reload,
}

impl JobKind {
    /// The `org.freedesktop.systemd1.Manager` method that queues this job.
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            JobKind::Start => "StartUnit",
            JobKind::Stop => "StopUnit",
            JobKind::Restart => "RestartUnit",
            JobKind::Reload => "ReloadUnit",
        }
    }

    /// The operation name a command uses for this job, as `docs/contracts/commands/service.yaml`
    /// spells it.
    #[must_use]
    pub const fn operation(self) -> &'static str {
        match self {
            JobKind::Start => "start",
            JobKind::Stop => "stop",
            JobKind::Restart => "restart",
            JobKind::Reload => "reload",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.operation())
    }
}

/// Why a call to systemd did not produce an answer.
///
/// The variants exist because a user needs them apart: "there is no service manager here" is a
/// different sentence from "systemd knows that unit and refused you", and collapsing them is the
/// conflation between absence and denial that spec §10.5 exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusError {
    /// No service manager answered: no bus socket, no `org.freedesktop.systemd1` on it, or the
    /// connection could not be established.
    Unavailable(String),
    /// systemd understood the request and refused it for want of authorisation — the polkit
    /// answer an unprivileged caller gets for `StartUnit`.
    PermissionDenied(String),
    /// systemd does not know the unit.
    NoSuchUnit(String),
    /// systemd refused for a reason of its own: a masked unit, a unit that cannot reload, a
    /// dependency that failed.
    Refused(String),
    /// The call did not complete within the provider's budget. A shell that blocks on a wedged
    /// bus is a shell nobody can use (spec §34).
    TimedOut(String),
}

impl BusError {
    /// What systemd said, in its own words.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            BusError::Unavailable(message)
            | BusError::PermissionDenied(message)
            | BusError::NoSuchUnit(message)
            | BusError::Refused(message)
            | BusError::TimedOut(message) => message,
        }
    }

    /// The taxonomy code this failure carries into a pipeline (spec §43).
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            BusError::Unavailable(_) | BusError::TimedOut(_) => ErrorCode::ProviderUnavailable,
            BusError::PermissionDenied(_) => ErrorCode::IoPermissionDenied,
            BusError::NoSuchUnit(_) => ErrorCode::IoNotFound,
            // The taxonomy of spec §43 is closed and additive (ADR-0006) and has no code for
            // "the service manager declined". `provider.unsupported` is the nearest true
            // statement: this provider cannot carry out that operation on that object. The
            // D-Bus error name systemd gave is kept verbatim in the message.
            BusError::Refused(_) => ErrorCode::ProviderUnsupported,
        }
    }

    /// The failure as the structured error value a command reports (spec §16.1).
    #[must_use]
    pub fn into_error(self) -> ErrorValue {
        let error = ErrorValue::new(self.code(), self.message().to_owned());
        match self {
            BusError::PermissionDenied(_) => error.with_help(
                "systemd asks polkit before it changes a unit. Run this as a user the policy \
                 admits, or grant the action with a polkit rule.",
            ),
            BusError::Unavailable(_) => error.with_help(
                "the provider exists but no service manager answers here. This is not the same \
                 as there being no services.",
            ),
            BusError::TimedOut(_) => error.with_retryable(true),
            BusError::NoSuchUnit(_) | BusError::Refused(_) => error,
        }
    }
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for BusError {}

/// The part of `org.freedesktop.systemd1` this provider speaks.
///
/// Implemented once against the real system bus, and once in the crate's tests against recorded
/// responses. Both are the outside world; neither is a layer of this crate.
#[async_trait::async_trait]
pub trait SystemdBus: Send + Sync + fmt::Debug {
    /// Reads `Manager.Version`.
    ///
    /// This is the availability probe: it succeeds only if a bus exists, something owns
    /// `org.freedesktop.systemd1` on it, and that something answers the `Manager` interface.
    ///
    /// # Errors
    ///
    /// [`BusError::Unavailable`] when no service manager answers.
    async fn manager_version(&self) -> Result<String, BusError>;

    /// Calls `Manager.ListUnits`.
    ///
    /// # Errors
    ///
    /// Whatever the bus reported.
    async fn list_units(&self) -> Result<Vec<UnitListing>, BusError>;

    /// Reads every property of one unit, loading it if it is not loaded yet.
    ///
    /// Returns `Ok(None)` when systemd has no such unit — an answer, not a failure.
    ///
    /// # Errors
    ///
    /// Whatever the bus reported, other than "no such unit".
    async fn unit_properties(&self, unit: &str) -> Result<Option<UnitProperties>, BusError>;

    /// The same, for a unit whose object path is already known.
    ///
    /// An enumeration has the path from `ListUnits` and does not have to ask `LoadUnit` for it
    /// again. The default answers through [`SystemdBus::unit_properties`], so a bus that has no
    /// cheaper way to do it — and every test double — is correct without knowing about this.
    ///
    /// # Errors
    ///
    /// As [`SystemdBus::unit_properties`].
    async fn unit_properties_at(
        &self,
        unit: &str,
        path: &str,
    ) -> Result<Option<UnitProperties>, BusError> {
        let _ = path;
        self.unit_properties(unit).await
    }

    /// Queues a job through the `Manager` interface, in `replace` mode.
    ///
    /// # Errors
    ///
    /// [`BusError::PermissionDenied`] when polkit refuses, [`BusError::NoSuchUnit`] when the unit
    /// is unknown, [`BusError::Refused`] when systemd declines for a reason of its own.
    async fn queue_job(&self, unit: &str, job: JobKind) -> Result<(), BusError>;

    /// Calls `EnableUnitFiles` or `DisableUnitFiles`, and reports whether systemd listed any
    /// change. An empty change list is systemd saying the unit files were already that way.
    ///
    /// # Errors
    ///
    /// As [`SystemdBus::queue_job`].
    async fn set_unit_file_enabled(&self, unit: &str, enabled: bool) -> Result<bool, BusError>;
}
