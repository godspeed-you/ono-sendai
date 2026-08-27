//! The one implementation of [`SystemdBus`] that talks to a real service manager.
//!
//! Everything here is `org.freedesktop.systemd1` over D-Bus: `Manager.ListUnits`,
//! `Manager.LoadUnit`, the job methods, the unit-file methods, and
//! `org.freedesktop.DBus.Properties.GetAll` for the per-unit properties. Nothing here runs a
//! program or reads its output (spec §23.3, §50).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, Proxy};

use crate::{BusError, JobKind, SystemdBus, UnitListing, UnitProperties};

const DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_INTERFACE: &str = "org.freedesktop.systemd1.Service";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// `replace` is the job mode `systemctl` uses: supersede a pending job for the same unit.
const JOB_MODE: &str = "replace";

/// How long any one call may take before the provider gives up on it.
///
/// A shell that blocks forever on a wedged bus is a shell nobody can use (spec §34). Ten seconds
/// is long enough for a slow `StartUnit` that waits on dependencies and short enough that a user
/// gets an answer rather than a hang.
const CALL_BUDGET: Duration = Duration::from_secs(10);

/// Where the D-Bus system bus socket is, when the environment does not say otherwise.
const SYSTEM_BUS_SOCKETS: [&str; 2] = [
    "/run/dbus/system_bus_socket",
    "/var/run/dbus/system_bus_socket",
];

/// A connection to the system bus, and the `Manager` proxy on it.
pub struct SystemBus {
    connection: Connection,
    manager: Proxy<'static>,
}

impl fmt::Debug for SystemBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemBus")
            .field("destination", &DESTINATION)
            .finish()
    }
}

impl SystemBus {
    /// Opens the system bus and builds the `Manager` proxy.
    ///
    /// # Errors
    ///
    /// [`BusError::Unavailable`], naming what was missing, when there is no bus socket or the
    /// connection cannot be established.
    pub async fn connect() -> Result<Self, BusError> {
        let connection = open_system_bus().await?;
        let manager = budgeted(
            "building the org.freedesktop.systemd1.Manager proxy",
            Proxy::new(&connection, DESTINATION, MANAGER_PATH, MANAGER_INTERFACE),
        )
        .await?;
        Ok(Self {
            connection,
            manager,
        })
    }

    /// Every property of one unit, merged from the interfaces that carry them.
    async fn properties(
        &self,
        path: &OwnedObjectPath,
        unit: &str,
    ) -> Result<HashMap<String, OwnedValue>, BusError> {
        let proxy = budgeted(
            "building the D-Bus properties proxy",
            Proxy::new(
                &self.connection,
                DESTINATION,
                path.clone(),
                PROPERTIES_INTERFACE,
            ),
        )
        .await?;

        let mut properties: HashMap<String, OwnedValue> = budgeted(
            "reading org.freedesktop.systemd1.Unit properties",
            proxy.call::<_, _, HashMap<String, OwnedValue>>("GetAll", &(UNIT_INTERFACE,)),
        )
        .await?;

        // Only a `.service` carries the Service interface; asking any other unit for it would be
        // a round trip that can only fail.
        if unit.ends_with(".service") {
            let service: HashMap<String, OwnedValue> = budgeted(
                "reading org.freedesktop.systemd1.Service properties",
                proxy.call::<_, _, HashMap<String, OwnedValue>>("GetAll", &(SERVICE_INTERFACE,)),
            )
            .await?;
            properties.extend(service);
        }
        Ok(properties)
    }

    async fn load(&self, unit: &str) -> Result<OwnedObjectPath, BusError> {
        budgeted(
            "org.freedesktop.systemd1.Manager.LoadUnit",
            self.manager
                .call::<_, _, OwnedObjectPath>("LoadUnit", &(unit,)),
        )
        .await
    }
}

#[async_trait::async_trait]
impl SystemdBus for SystemBus {
    async fn manager_version(&self) -> Result<String, BusError> {
        budgeted(
            "reading org.freedesktop.systemd1.Manager.Version",
            self.manager.get_property::<String>("Version"),
        )
        .await
        .map_err(|error| {
            BusError::Unavailable(format!(
                "the D-Bus system bus answered but org.freedesktop.systemd1.Manager did not: {}",
                error.message()
            ))
        })
    }

    async fn list_units(&self) -> Result<Vec<UnitListing>, BusError> {
        /// The ten columns of `ListUnits`: name, description, load, active, sub, followed unit,
        /// object path, job id, job type, job path.
        type Row = (
            String,
            String,
            String,
            String,
            String,
            String,
            OwnedObjectPath,
            u32,
            String,
            OwnedObjectPath,
        );

        let rows: Vec<Row> = budgeted(
            "org.freedesktop.systemd1.Manager.ListUnits",
            self.manager.call::<_, _, Vec<Row>>("ListUnits", &()),
        )
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| UnitListing {
                name: row.0,
                description: non_empty(row.1),
                load_state: non_empty(row.2),
                active_state: non_empty(row.3),
                sub_state: non_empty(row.4),
            })
            .collect())
    }

    async fn unit_properties(&self, unit: &str) -> Result<Option<UnitProperties>, BusError> {
        let path = match self.load(unit).await {
            Ok(path) => path,
            Err(BusError::NoSuchUnit(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let properties = match self.properties(&path, unit).await {
            Ok(properties) => properties,
            Err(BusError::NoSuchUnit(_)) => return Ok(None),
            Err(error) => return Err(error),
        };

        Ok(Some(UnitProperties {
            name: text(&properties, "Id").unwrap_or_else(|| unit.to_owned()),
            description: text(&properties, "Description"),
            load_state: text(&properties, "LoadState"),
            active_state: text(&properties, "ActiveState"),
            sub_state: text(&properties, "SubState"),
            unit_file_state: text(&properties, "UnitFileState"),
            fragment_path: text(&properties, "FragmentPath"),
            state_change_usec: number::<u64>(&properties, "StateChangeTimestamp"),
            main_pid: number::<u32>(&properties, "MainPID"),
            memory_current: number::<u64>(&properties, "MemoryCurrent"),
            tasks_current: number::<u64>(&properties, "TasksCurrent"),
            result: text(&properties, "Result"),
            exec_main_status: number::<i32>(&properties, "ExecMainStatus"),
        }))
    }

    async fn queue_job(&self, unit: &str, job: JobKind) -> Result<(), BusError> {
        budgeted(
            job.method(),
            self.manager
                .call::<_, _, OwnedObjectPath>(job.method(), &(unit, JOB_MODE)),
        )
        .await
        .map(|_| ())
    }

    async fn set_unit_file_enabled(&self, unit: &str, enabled: bool) -> Result<bool, BusError> {
        /// The `(type, file, destination)` triples systemd reports as its changes.
        type Change = (String, String, String);

        let changes: Vec<Change> = if enabled {
            let (_carries_install_info, changes): (bool, Vec<Change>) = budgeted(
                "org.freedesktop.systemd1.Manager.EnableUnitFiles",
                self.manager.call::<_, _, (bool, Vec<Change>)>(
                    "EnableUnitFiles",
                    &(vec![unit], false, true),
                ),
            )
            .await?;
            changes
        } else {
            budgeted(
                "org.freedesktop.systemd1.Manager.DisableUnitFiles",
                self.manager
                    .call::<_, _, Vec<Change>>("DisableUnitFiles", &(vec![unit], false)),
            )
            .await?
        };
        Ok(!changes.is_empty())
    }
}

/// Opens the D-Bus system bus, or says which socket is missing.
///
/// Shared by every provider of this crate that lives behind the system bus, so that "no bus
/// here" is one sentence rather than several.
///
/// # Errors
///
/// [`BusError::Unavailable`], naming what was missing, when there is no bus socket or the
/// connection cannot be established.
pub(crate) async fn open_system_bus() -> Result<Connection, BusError> {
    if let Some(expected) = missing_system_bus_socket() {
        return Err(BusError::Unavailable(format!(
            "the D-Bus system bus socket {} does not exist, so no service manager can be \
             asked here; this is normal in a container, under WSL, or on a machine that uses \
             another init",
            expected.display()
        )));
    }
    budgeted("connecting to the D-Bus system bus", Connection::system())
        .await
        .map_err(|error| {
            BusError::Unavailable(format!(
                "the D-Bus system bus could not be opened: {}",
                error.message()
            ))
        })
}

/// The system bus socket that ought to exist but does not, or `None` if one does.
///
/// `DBUS_SYSTEM_BUS_ADDRESS` wins where it names a unix socket. Where it names anything else —
/// a TCP address, a launchd endpoint — there is no path to check, and the connection attempt
/// itself is the test.
fn missing_system_bus_socket() -> Option<PathBuf> {
    if let Ok(address) = std::env::var("DBUS_SYSTEM_BUS_ADDRESS") {
        let path = address
            .split(';')
            .find_map(|part| part.trim().strip_prefix("unix:path="))
            .map(|path| PathBuf::from(path.split(',').next().unwrap_or(path)))?;
        return if is_socket(&path) { None } else { Some(path) };
    }
    if SYSTEM_BUS_SOCKETS
        .iter()
        .any(|path| is_socket(Path::new(path)))
    {
        return None;
    }
    SYSTEM_BUS_SOCKETS.first().map(PathBuf::from)
}

fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}

/// Runs one bus call under [`CALL_BUDGET`], translating whatever comes back.
pub(crate) async fn budgeted<T>(
    what: &str,
    call: impl Future<Output = zbus::Result<T>>,
) -> Result<T, BusError> {
    match tokio::time::timeout(CALL_BUDGET, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(translate(&error, what)),
        Err(_) => Err(BusError::TimedOut(format!(
            "{what} did not answer within {} seconds",
            CALL_BUDGET.as_secs()
        ))),
    }
}

/// A D-Bus failure as the distinction a user needs.
///
/// The D-Bus error *name* is the structured part of the answer; the detail is systemd's own
/// sentence, and it is carried through verbatim rather than replaced by one of ours.
fn translate(error: &zbus::Error, what: &str) -> BusError {
    let zbus::Error::MethodError(name, detail, _) = error else {
        return BusError::Unavailable(format!("{what} failed: {error}"));
    };
    let said = detail
        .clone()
        .unwrap_or_else(|| format!("{} (no detail given)", name.as_str()));
    match name.as_str() {
        "org.freedesktop.DBus.Error.AccessDenied"
        | "org.freedesktop.DBus.Error.AuthFailed"
        | "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired" => {
            BusError::PermissionDenied(said)
        }
        "org.freedesktop.systemd1.NoSuchUnit"
        | "org.freedesktop.DBus.Error.UnknownObject"
        | "org.freedesktop.DBus.Error.FileNotFound" => BusError::NoSuchUnit(said),
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner"
        | "org.freedesktop.DBus.Error.Disconnected"
        | "org.freedesktop.DBus.Error.NoServer" => BusError::Unavailable(said),
        "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => {
            BusError::TimedOut(said)
        }
        other => BusError::Refused(format!("{other}: {said}")),
    }
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

pub(crate) fn text(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    properties
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
        .filter(|text| !text.is_empty())
}

pub(crate) fn number<T>(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    properties
        .get(key)
        .and_then(|value| T::try_from(value.clone()).ok())
}
