//! A recorded systemd: the outside world, written down.
//!
//! systemd does not run in the acceptance container, in CI, or on a great many machines Ono is
//! used from. The provider's positive path is nonetheless a contract, so it is tested against
//! recorded `org.freedesktop.systemd1` responses rather than against a machine that happens to
//! have systemd. This is the procfs-fixture pattern of AGENTS.md §11 — a fake of the system
//! being read, not a mock of a layer this crate wrote — and the recorded units cover the shapes
//! that break naive providers: a running unit, a failed unit with a result and an exit code, a
//! masked unit, a unit that has no main process at all, and — on demand — a unit file on disk
//! that is not loaded, and a stub that is enumerated although its unit file is gone.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    dead_code,
    reason = "a fixture states its preconditions the way a test does; not every helper is used by every test file"
)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use ono_provider_systemd::{BusError, JobKind, SystemdBus, UnitListing, UnitProperties};

/// The microsecond timestamp `nginx.service` last changed state at, fixed so the assertion on
/// `since` is a value and not a tolerance.
pub const NGINX_STATE_CHANGE_USEC: u64 = 1_787_000_000_000_000;

/// The microsecond timestamp `postgresql.service` failed at.
pub const POSTGRES_STATE_CHANGE_USEC: u64 = 1_787_000_252_000_000;

/// The resident memory systemd reports for `nginx.service`, in bytes.
pub const NGINX_MEMORY_BYTES: u64 = 41_943_040;

/// A systemd whose answers are recorded rather than observed.
#[derive(Debug)]
pub struct RecordedSystemd {
    /// The units in memory: what `ListUnits` enumerates.
    units: Mutex<BTreeMap<String, UnitProperties>>,
    /// The unit files on disk that are *not* loaded. systemd unloads inactive units from
    /// memory, so a disabled-but-present service lives here: `ListUnits` never mentions it,
    /// and only `LoadUnit` reaches it. Each entry is recorded as it reads once loaded.
    unit_files: Mutex<BTreeMap<String, UnitProperties>>,
    version: Result<String, BusError>,
    authorised: bool,
}

impl RecordedSystemd {
    /// A service manager that answers, holding the four recorded units.
    pub fn running() -> Self {
        let units = [
            nginx(),
            postgresql(),
            masked(),
            timer_without_main_process(),
        ]
        .into_iter()
        .map(|unit| (unit.name.clone(), unit))
        .collect();
        Self {
            units: Mutex::new(units),
            unit_files: Mutex::new(BTreeMap::new()),
            version: Ok("257 (257.5-1)".to_owned()),
            authorised: true,
        }
    }

    /// A machine with no service manager: the probe fails and nothing else is ever asked.
    pub fn absent(reason: &str) -> Self {
        Self {
            units: Mutex::new(BTreeMap::new()),
            unit_files: Mutex::new(BTreeMap::new()),
            version: Err(BusError::Unavailable(reason.to_owned())),
            authorised: true,
        }
    }

    /// The same service manager, with one more unit file on disk that is not loaded.
    ///
    /// This is the shape `systemctl status` finds and a `ListUnits` filter misses: the unit
    /// exists, systemd just has no reason to keep it in memory.
    pub fn with_unit_file_on_disk(self, unit: UnitProperties) -> Self {
        self.unit_files
            .lock()
            .expect("the recorded unit files are not poisoned")
            .insert(unit.name.clone(), unit);
        self
    }

    /// The same service manager, with a unit kept in memory that has no unit file at all.
    ///
    /// Recorded from a real service manager: when another unit references a name whose file is
    /// gone, `ListUnits` enumerates a stub for it with `LoadState` `not-found` — the very stub
    /// `LoadUnit` answers for a name it has never heard of. Such a stub is not a service.
    pub fn with_dangling_reference(self, name: &str) -> Self {
        self.units
            .lock()
            .expect("the recorded units are not poisoned")
            .insert(name.to_owned(), not_found(name));
        self
    }

    /// A service manager that answers queries and refuses every mutation, the way polkit
    /// refuses an unprivileged caller.
    pub fn refusing_authorisation() -> Self {
        Self {
            authorised: false,
            ..Self::running()
        }
    }

    fn with_unit<T>(&self, unit: &str, act: impl FnOnce(&mut UnitProperties) -> T) -> Option<T> {
        let mut units = self
            .units
            .lock()
            .expect("the recorded units are not poisoned");
        // Acting on an unloaded unit loads it: that is what `StartUnit` and its siblings do.
        if !units.contains_key(unit) {
            let mut unit_files = self
                .unit_files
                .lock()
                .expect("the recorded unit files are not poisoned");
            if let Some(loaded) = unit_files.remove(unit) {
                units.insert(unit.to_owned(), loaded);
            }
        }
        units.get_mut(unit).map(act)
    }

    fn guard(&self, unit: &str) -> Result<(), BusError> {
        if !self.authorised {
            return Err(BusError::PermissionDenied(
                "Interactive authentication required.".to_owned(),
            ));
        }
        let units = self
            .units
            .lock()
            .expect("the recorded units are not poisoned");
        let on_disk = self
            .unit_files
            .lock()
            .expect("the recorded unit files are not poisoned");
        // A dangling reference is in memory but has no file; systemd refuses to act on it.
        let real = units
            .get(unit)
            .is_some_and(|properties| properties.load_state.as_deref() != Some("not-found"));
        if real || on_disk.contains_key(unit) {
            Ok(())
        } else {
            Err(BusError::NoSuchUnit(format!("Unit {unit} not found.")))
        }
    }
}

#[async_trait::async_trait]
impl SystemdBus for RecordedSystemd {
    async fn manager_version(&self) -> Result<String, BusError> {
        self.version.clone()
    }

    async fn list_units(&self) -> Result<Vec<UnitListing>, BusError> {
        let units = self
            .units
            .lock()
            .expect("the recorded units are not poisoned");
        Ok(units.values().map(UnitProperties::listing).collect())
    }

    async fn unit_properties(&self, unit: &str) -> Result<Option<UnitProperties>, BusError> {
        // Recorded from a real service manager: `LoadUnit` rejects a name with no unit suffix
        // outright rather than answering "no such unit". A provider that treats that as a fatal
        // error can never resolve `get service nginx`.
        if !unit.contains('.') {
            return Err(BusError::Refused(format!(
                "org.freedesktop.DBus.Error.InvalidArgs: Unit name {unit} is not valid."
            )));
        }
        let units = self
            .units
            .lock()
            .expect("the recorded units are not poisoned");
        if let Some(properties) = units.get(unit) {
            return Ok(Some(properties.clone()));
        }
        // `GetUnit` would stop here: it answers only for loaded units. `LoadUnit` goes on to
        // the disk and loads the unit file on demand — which is why the provider speaks
        // `LoadUnit`, and why this fixture answers from the recorded unit files next. The
        // loaded unit is answered without being kept in memory: a unit nothing holds a
        // reference to is garbage-collected again straight away, so this is the steady state
        // the next call observes.
        let unit_files = self
            .unit_files
            .lock()
            .expect("the recorded unit files are not poisoned");
        if let Some(properties) = unit_files.get(unit) {
            return Ok(Some(properties.clone()));
        }
        // Also recorded from a real service manager: `LoadUnit` answers for a unit that does
        // not exist with a stub whose `LoadState` is `not-found`, rather than with an error.
        // A provider that took the stub at face value would report a service that is not
        // there.
        Ok(Some(not_found(unit)))
    }

    async fn queue_job(&self, unit: &str, job: JobKind) -> Result<(), BusError> {
        self.guard(unit)?;
        self.with_unit(unit, |properties| match job {
            JobKind::Start | JobKind::Restart => {
                properties.active_state = Some("active".to_owned());
                properties.sub_state = Some("running".to_owned());
                properties.main_pid = Some(4242);
                properties.result = Some("success".to_owned());
                properties.exec_main_status = None;
            }
            JobKind::Stop => {
                properties.active_state = Some("inactive".to_owned());
                properties.sub_state = Some("dead".to_owned());
                properties.main_pid = Some(0);
            }
            JobKind::Reload => {
                properties.sub_state = Some("running".to_owned());
            }
        });
        Ok(())
    }

    async fn set_unit_file_enabled(&self, unit: &str, enabled: bool) -> Result<bool, BusError> {
        self.guard(unit)?;
        let wanted = if enabled { "enabled" } else { "disabled" };
        Ok(self
            .with_unit(unit, |properties| {
                let changed = properties.unit_file_state.as_deref() != Some(wanted);
                properties.unit_file_state = Some(wanted.to_owned());
                changed
            })
            .unwrap_or(false))
    }
}

/// A running unit with everything systemd can expose.
pub fn nginx() -> UnitProperties {
    UnitProperties {
        name: "nginx.service".to_owned(),
        description: Some("A high performance web server and a reverse proxy server".to_owned()),
        load_state: Some("loaded".to_owned()),
        active_state: Some("active".to_owned()),
        sub_state: Some("running".to_owned()),
        unit_file_state: Some("enabled".to_owned()),
        fragment_path: Some("/lib/systemd/system/nginx.service".to_owned()),
        state_change_usec: Some(NGINX_STATE_CHANGE_USEC),
        main_pid: Some(812),
        memory_current: Some(NGINX_MEMORY_BYTES),
        tasks_current: Some(5),
        result: Some("success".to_owned()),
        exec_main_status: Some(0),
    }
}

/// A failed unit: the case spec §33.2 and §41.4 are about.
pub fn postgresql() -> UnitProperties {
    UnitProperties {
        name: "postgresql.service".to_owned(),
        description: Some("PostgreSQL RDBMS".to_owned()),
        load_state: Some("loaded".to_owned()),
        active_state: Some("failed".to_owned()),
        sub_state: Some("failed".to_owned()),
        unit_file_state: Some("enabled".to_owned()),
        fragment_path: Some("/lib/systemd/system/postgresql.service".to_owned()),
        state_change_usec: Some(POSTGRES_STATE_CHANGE_USEC),
        main_pid: Some(0),
        // systemd's "I do not know" for a cgroup accounting value, which must not become a zero.
        memory_current: Some(u64::MAX),
        tasks_current: Some(u64::MAX),
        result: Some("exit-code".to_owned()),
        exec_main_status: Some(1),
    }
}

/// A masked unit: present, never startable, and definitely not enabled at boot.
pub fn masked() -> UnitProperties {
    UnitProperties {
        name: "ono-blocked.service".to_owned(),
        description: Some("ono-blocked.service".to_owned()),
        load_state: Some("masked".to_owned()),
        active_state: Some("inactive".to_owned()),
        sub_state: Some("dead".to_owned()),
        unit_file_state: Some("masked".to_owned()),
        fragment_path: Some("/dev/null".to_owned()),
        state_change_usec: Some(0),
        main_pid: Some(0),
        memory_current: None,
        tasks_current: None,
        result: None,
        exec_main_status: None,
    }
}

/// A unit that runs no process, has no unit file of its own and has never changed state: every
/// optional field is genuinely absent rather than zero.
pub fn timer_without_main_process() -> UnitProperties {
    UnitProperties {
        name: "logrotate.timer".to_owned(),
        description: Some("Daily rotation of log files".to_owned()),
        load_state: Some("loaded".to_owned()),
        active_state: Some("inactive".to_owned()),
        sub_state: Some("dead".to_owned()),
        unit_file_state: Some("static".to_owned()),
        fragment_path: Some(String::new()),
        state_change_usec: Some(0),
        main_pid: None,
        memory_current: None,
        tasks_current: None,
        result: None,
        exec_main_status: None,
    }
}

/// A disabled-but-present unit, as it reads once `LoadUnit` has loaded it from disk.
///
/// systemd keeps no inactive unit in memory that nothing references, so this is the unit
/// `systemctl status certbot` finds and `ListUnits` does not: on disk, disabled, never run.
pub fn on_disk_only() -> UnitProperties {
    UnitProperties {
        name: "certbot.service".to_owned(),
        description: Some("Certbot".to_owned()),
        load_state: Some("loaded".to_owned()),
        active_state: Some("inactive".to_owned()),
        sub_state: Some("dead".to_owned()),
        unit_file_state: Some("disabled".to_owned()),
        fragment_path: Some("/lib/systemd/system/certbot.service".to_owned()),
        state_change_usec: Some(0),
        main_pid: Some(0),
        memory_current: None,
        tasks_current: None,
        result: None,
        exec_main_status: None,
    }
}

/// What `LoadUnit` answers for a unit systemd has never heard of: a stub, not an error.
pub fn not_found(name: &str) -> UnitProperties {
    UnitProperties {
        name: name.to_owned(),
        description: Some(name.to_owned()),
        load_state: Some("not-found".to_owned()),
        active_state: Some("inactive".to_owned()),
        sub_state: Some("dead".to_owned()),
        main_pid: Some(0),
        state_change_usec: Some(0),
        ..UnitProperties::default()
    }
}
