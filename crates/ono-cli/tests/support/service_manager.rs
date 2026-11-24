//! A service manager a test owns: a private `dbus-daemon`, and on it `org.freedesktop.systemd1`
//! answering for exactly the units it was started with (ADR-0576, ADR-0880, ADR-1080).
//!
//! The host's unit list is not a fixture: a container starting beside a suite registers systemd
//! scopes while it runs, and a test that read the list twice measured the machine (issues #155 and
//! #300). The shell under test reaches this manager through `DBUS_SYSTEM_BUS_ADDRESS` — the same
//! production provider, the same D-Bus calls, and a population that is a number the test chose.
//!
//! It implements the part of systemd's D-Bus API the shell reads and writes — `Manager.Version`,
//! `Manager.ListUnits`, `Manager.LoadUnit`, the `Unit` and `Service` properties of each unit (every
//! unit but the first `Wants` the first, so a unit's neighbourhood reaches the list), and
//! the job methods `StartUnit`, `StopUnit`, `RestartUnit` and `ReloadUnit` — and counts every
//! request at that boundary, so a test can state how often the shell asked the service manager
//! anything without knowing which code path asked. A job is accepted and recorded; nothing on
//! the host is started, stopped or restarted by it. Both the daemon and the thread serving the
//! manager end when it is dropped (ADR-0516).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly, and not every suite that includes the \
              fixture reads every one of its counters (AGENTS.md section 16)"
)]

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ono_testkit::Scratch;
use zbus::zvariant::OwnedObjectPath;

/// How often the shell asked the fixture's service manager each kind of question.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requests {
    /// `Manager.ListUnits` calls: one per enumeration of the whole unit list.
    pub list_units: u64,
    /// `Manager.LoadUnit` calls: one per unit asked for by name.
    pub load_unit: u64,
    /// Reads of a unit's `Unit` properties (a `GetAll` or a `Get` of `Id`): one per unit read.
    pub unit_reads: u64,
    /// Jobs queued: `StartUnit`, `StopUnit`, `RestartUnit`, `ReloadUnit`.
    pub jobs: u64,
}

impl Requests {
    /// Every request counted.
    pub fn total(&self) -> u64 {
        self.list_units + self.load_unit + self.unit_reads + self.jobs
    }
}

#[derive(Default)]
struct Counters {
    list_units: AtomicU64,
    load_unit: AtomicU64,
    unit_reads: AtomicU64,
    jobs: AtomicU64,
}

impl Counters {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

/// The fixture itself. See the module documentation.
pub struct FixtureServiceManager {
    daemon: Child,
    address: String,
    counters: Arc<Counters>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    server: Option<std::thread::JoinHandle<()>>,
    _home: Scratch,
}

impl FixtureServiceManager {
    /// Starts the bus and a manager holding `units` units named `fixture-<n>.service`, or `None`
    /// where this host has no `dbus-daemon`.
    pub fn try_start(units: i64) -> Option<Self> {
        let names = (0..units)
            .map(|index| format!("fixture-{index}.service"))
            .collect();
        Self::try_start_with(names)
    }

    /// Starts the bus and a manager holding exactly the units `names`, or `None` where this host
    /// has no `dbus-daemon`.
    pub fn try_start_with(names: Vec<String>) -> Option<Self> {
        let home = ono_testkit::socket_scratch();
        let socket = home.path().join("bus");
        let config = home.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>custom</type><listen>unix:path={}</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )
        .expect("the bus configuration is written");
        let daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let counters = Arc::new(Counters::default());
        let mut manager = Self {
            daemon,
            address: format!("unix:path={}", socket.display()),
            counters: Arc::clone(&counters),
            stop: None,
            server: None,
            _home: home,
        };
        let deadline = Instant::now() + ono_testkit::under_load(Duration::from_secs(10));
        while !socket.exists() {
            assert!(
                Instant::now() < deadline,
                "the fixture's dbus-daemon never opened {}",
                socket.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let address = manager.address.clone();
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime for the fixture's manager");
            runtime.block_on(async move {
                match serve(&address, names, counters).await {
                    Ok(connection) => {
                        let _ = ready_tx.send(Ok(()));
                        // Held until the fixture is dropped; the connection serves meanwhile.
                        let _ = tokio::task::spawn_blocking(move || stop_rx.recv()).await;
                        drop(connection);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                    }
                }
            });
        });
        manager.stop = Some(stop_tx);
        manager.server = Some(server);
        match ready_rx.recv_timeout(ono_testkit::under_load(Duration::from_secs(10))) {
            Ok(Ok(())) => Some(manager),
            Ok(Err(error)) => panic!("the fixture's service manager could not start: {error}"),
            Err(error) => panic!("the fixture's service manager did not start: {error}"),
        }
    }

    /// The bus address the shell under test is given as its system bus.
    pub fn address(&self) -> String {
        self.address.clone()
    }

    /// How often the manager has been asked each kind of question since it started.
    pub fn requests(&self) -> Requests {
        Requests {
            list_units: self.counters.list_units.load(Ordering::SeqCst),
            load_unit: self.counters.load_unit.load(Ordering::SeqCst),
            unit_reads: self.counters.unit_reads.load(Ordering::SeqCst),
            jobs: self.counters.jobs.load(Ordering::SeqCst),
        }
    }
}

impl Drop for FixtureServiceManager {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// Connects to the fixture's bus and serves the manager and its units there.
async fn serve(
    address: &str,
    names: Vec<String>,
    counters: Arc<Counters>,
) -> zbus::Result<zbus::Connection> {
    let mut builder = zbus::connection::Builder::address(address)?
        .name("org.freedesktop.systemd1")?
        .serve_at(
            "/org/freedesktop/systemd1",
            FixtureManager {
                units: names.clone(),
                counters: Arc::clone(&counters),
            },
        )?;
    let first = names.first().cloned();
    for name in names {
        let path = unit_path(&name);
        // Every unit but the first wants the first, as most units of a host want a few shared
        // ones: a dependency is what makes a unit's neighbourhood reach the rest of the list.
        let wants = first
            .iter()
            .filter(|first| **first != name)
            .cloned()
            .collect();
        builder = builder
            .serve_at(
                path.clone(),
                FixtureUnit {
                    id: name.clone(),
                    wants,
                    counters: Arc::clone(&counters),
                },
            )?
            .serve_at(path, FixtureService)?;
    }
    builder.build().await
}

/// The object path systemd gives a unit: its name with every byte outside `[A-Za-z0-9]` escaped.
fn unit_path(name: &str) -> OwnedObjectPath {
    let escaped: String = name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte).to_string()
            } else {
                format!("_{byte:02x}")
            }
        })
        .collect();
    OwnedObjectPath::try_from(format!("/org/freedesktop/systemd1/unit/{escaped}"))
        .expect("an escaped unit name is a valid object path")
}

/// One row of `ListUnits`: name, description, load, active, sub, followed unit, object path, job
/// id, job type, job path.
type UnitRow = (
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

struct FixtureManager {
    units: Vec<String>,
    counters: Arc<Counters>,
}

impl FixtureManager {
    /// A job on `name`: accepted and recorded, and nothing else happens.
    fn job(&self, name: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        Counters::bump(&self.counters.jobs);
        if !self.units.iter().any(|unit| unit == name) {
            return Err(zbus::fdo::Error::Failed(format!("Unit {name} not found.")));
        }
        let id = self.counters.jobs.load(Ordering::SeqCst);
        Ok(
            OwnedObjectPath::try_from(format!("/org/freedesktop/systemd1/job/{id}"))
                .expect("a job path is a valid object path"),
        )
    }
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl FixtureManager {
    #[zbus(property)]
    fn version(&self) -> String {
        "fixture".to_owned()
    }

    fn list_units(&self) -> Vec<UnitRow> {
        Counters::bump(&self.counters.list_units);
        let no_job = OwnedObjectPath::try_from("/").expect("`/` is an object path");
        self.units
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    format!("fixture unit {name}"),
                    "loaded".to_owned(),
                    "active".to_owned(),
                    "running".to_owned(),
                    String::new(),
                    unit_path(name),
                    0,
                    String::new(),
                    no_job.clone(),
                )
            })
            .collect()
    }

    fn load_unit(&self, name: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        Counters::bump(&self.counters.load_unit);
        if self.units.iter().any(|unit| unit == name) {
            Ok(unit_path(name))
        } else {
            Err(zbus::fdo::Error::Failed(format!("Unit {name} not found.")))
        }
    }

    fn start_unit(&self, name: &str, _mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        self.job(name)
    }

    fn stop_unit(&self, name: &str, _mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        self.job(name)
    }

    fn restart_unit(&self, name: &str, _mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        self.job(name)
    }

    fn reload_unit(&self, name: &str, _mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        self.job(name)
    }
}

struct FixtureUnit {
    id: String,
    wants: Vec<String>,
    counters: Arc<Counters>,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl FixtureUnit {
    // Every read of a unit asks for its `Id` — `GetAll` asks every property — so this is where a
    // read of one unit is counted.
    #[zbus(property, name = "Id")]
    fn id(&self) -> String {
        Counters::bump(&self.counters.unit_reads);
        self.id.clone()
    }

    #[zbus(property, name = "Description")]
    fn description(&self) -> String {
        format!("fixture unit {}", self.id)
    }

    #[zbus(property, name = "LoadState")]
    fn load_state(&self) -> String {
        "loaded".to_owned()
    }

    #[zbus(property, name = "ActiveState")]
    fn active_state(&self) -> String {
        "active".to_owned()
    }

    #[zbus(property, name = "SubState")]
    fn sub_state(&self) -> String {
        "running".to_owned()
    }

    // A unit with a unit file, as a unit an operator restarts has: it stays a unit for as long as
    // the test runs, where a transient scope does not.
    #[zbus(property, name = "FragmentPath")]
    fn fragment_path(&self) -> String {
        format!("/etc/systemd/system/{}", self.id)
    }

    #[zbus(property, name = "Wants")]
    fn wants(&self) -> Vec<String> {
        self.wants.clone()
    }

    #[zbus(property, name = "UnitFileState")]
    fn unit_file_state(&self) -> String {
        "enabled".to_owned()
    }
}

struct FixtureService;

#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl FixtureService {
    #[zbus(property, name = "MainPID")]
    fn main_pid(&self) -> u32 {
        0
    }
}
