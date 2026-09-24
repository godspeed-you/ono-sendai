//! What a bounded orientation reads, and what it may say about what it did not read
//! (v0.4.1 §34.4, §33.2, §2.17; ADR-0576).
//!
//! §34.4 is one sentence and one obligation:
//!
//! > A local neighborhood query SHOULD NOT require construction of the complete system graph when
//! > provider APIs can answer the neighborhood incrementally.
//!
//! Reading every object of every target before saying where a user is standing is that complete
//! construction, and it is what kept `enter compute; look` outside §33.2's 150 ms: six hundred
//! systemd units at three D-Bus round trips each, paid on every orientation, to draw a hundred
//! places and a count.
//!
//! So an orientation reads `limits.orientation_objects` of a target and stops. The whole of this
//! suite is the other half of that sentence — §2.17's, which says the shell may not then be vague
//! or wrong about the size of what it described:
//!
//! * a target that can be counted is counted, and the figure is the provider's, not the sample's;
//! * a target that cannot be counted reports no count at all, and says why;
//! * the objects themselves are unchanged — `get service` is not an orientation and reads
//!   everything, so nothing a user asks for directly is bounded by this.
//!
//! The service tests read a service manager this suite owns rather than the host's. The host's
//! unit list is not a fixture: a container starting beside the suite registers systemd scopes
//! while it runs, and a count compared across two readings of it measured the machine
//! (issue #155, ADR-0880). [`FixtureServiceManager`] answers `org.freedesktop.systemd1` on a
//! private bus with exactly the units it was given, and the shell under test reaches it through
//! `DBUS_SYSTEM_BUS_ADDRESS` — the same production provider, the same D-Bus calls, and a
//! population that is a number the test chose.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "spatial")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, Shell, SkipReason, scratch, skipped};
use serde_yaml_ng::Value;
use zbus::zvariant::OwnedObjectPath;

use support::{field, items, json, search};

/// A bound small enough that any host in the world exceeds it for the targets under test.
const BOUND: &str = "3";

fn look(space: &str, bound: &str) -> ono_testkit::Run {
    bounded(bound)
        .args(["-c", &format!("enter {space}; look --json")])
        .run()
}

/// The shell with the orientation bound set to `bound`.
fn bounded(bound: &str) -> Shell {
    Shell::new()
        .env("ONO_LIMITS_ORIENTATION_OBJECTS", bound.to_owned())
        .env("ONO_LIMITS_ORIENTATION_CEILING", bound.to_owned())
}

/// The `count` and `detail` of one exit of a `look --json` answer.
fn group(document: &Value, name: &str) -> (Option<i64>, Option<String>) {
    let groups = field(document, "groups");
    for group in items(&groups) {
        if search(group, "name").and_then(|found| found.as_str().map(str::to_owned))
            == Some(name.to_owned())
        {
            let count = search(group, "count").and_then(|count| count.as_i64());
            let detail =
                search(group, "detail").and_then(|detail| detail.as_str().map(str::to_owned));
            return (count, detail);
        }
    }
    panic!("`look` in this place has no `{name}` exit: {document:?}");
}

/// The whole truth, asked for the way a user asks for it: `get <target> | count`.
fn population(target: &str) -> Option<i64> {
    let run = Shell::new()
        .args(["-c", &format!("get {target} | count | to json")])
        .run();
    if !run.status().is_success() {
        return None;
    }
    // `count | to json` emits the stream as an array, so the one value is the only element.
    items(&json(run.stdout().trim()))
        .first()
        .and_then(serde_yaml_ng::Value::as_i64)
}

#[test]
fn should_count_a_bounded_target_by_what_the_provider_says_is_there() {
    let Some(manager) = FixtureServiceManager::try_start(FIXTURE_UNITS) else {
        skipped(
            SkipReason::ExternalToolUnavailable,
            "counting a bounded service enumeration needs `dbus-daemon`, so the fixture can serve \
             a service manager with more units than the bound",
        );
        return;
    };
    let services = FIXTURE_UNITS;

    let answered = look_with(&manager, "compute", BOUND);

    answered.assert_success();
    let (count, detail) = group(&json(answered.stdout().trim()), "services");
    assert_eq!(
        count,
        Some(services),
        "v0.4.1 §2.17: an orientation that read three units of {services} reports {services}, \
         because the count is a fact about the place and not about the reading. §34.4 permits \
         the bounded read; nothing permits a bounded count. Detail was {detail:?}"
    );
}

#[test]
fn should_report_no_count_for_a_bounded_target_whose_count_it_cannot_keep_true() {
    // A provider states one population per query, and `socket` answers both `network.listeners`
    // and `network.connections`; splitting that figure between them would be inventing it. So a
    // bounded read of that target has no count for either exit — `null`, with the reason — which
    // is §42.4's rule that a missing figure is never reported as zero.
    let Some(sockets) = population("socket").filter(|count| *count > 3) else {
        skipped(
            SkipReason::MissingPrivilege,
            "the socket table must hold more sockets than the bound for the bound to show",
        );
        return;
    };

    let answered = look("network", BOUND);

    answered.assert_success();
    let (count, detail) = group(&json(answered.stdout().trim()), "listeners");
    assert_eq!(
        count, None,
        "{sockets} sockets serve two kinds of place, so a bounded read of them counts neither. \
         A count here would be the number of sockets that happened to be read"
    );
    let detail = detail.unwrap_or_default();
    assert!(
        detail.contains("orientation bound"),
        "§42.4: the group says why it has no count instead of leaving a reader to guess, got \
         {detail:?}"
    );
}

#[test]
fn should_leave_what_a_user_asks_for_directly_unbounded() {
    let Some(manager) = FixtureServiceManager::try_start(FIXTURE_UNITS) else {
        skipped(
            SkipReason::ExternalToolUnavailable,
            "this needs `dbus-daemon`, so the fixture can serve a service manager with more \
             units than the bound",
        );
        return;
    };
    let services = FIXTURE_UNITS;

    let asked = bounded(BOUND)
        .env("DBUS_SYSTEM_BUS_ADDRESS", manager.address())
        .args(["-c", "get service | count | to json"])
        .run();

    asked.assert_success();
    assert_eq!(
        items(&json(asked.stdout().trim()))
            .first()
            .and_then(serde_yaml_ng::Value::as_i64),
        Some(services),
        "`limits.orientation_objects` bounds an orientation and nothing else. `get service` is a \
         question about services, and answering three of them because a *view* is budgeted would \
         be the wrong answer to the question that was asked (§34.4, §2.17)"
    );
}

/// How many units the fixture's service manager holds: well above the bound, and a number no
/// host happens to have by accident.
const FIXTURE_UNITS: i64 = 17;

/// `look` in `space` against the fixture's service manager.
fn look_with(manager: &FixtureServiceManager, space: &str, bound: &str) -> ono_testkit::Run {
    bounded(bound)
        .env("DBUS_SYSTEM_BUS_ADDRESS", manager.address())
        .args(["-c", &format!("enter {space}; look --json")])
        .run()
}

/// A service manager this test owns: a private `dbus-daemon`, and on it
/// `org.freedesktop.systemd1` answering for exactly the units it was started with.
///
/// It implements the part of systemd's D-Bus API the shell reads — `Manager.Version`,
/// `Manager.ListUnits`, `Manager.LoadUnit`, and the `Unit` and `Service` properties of each
/// unit — so the production provider runs unchanged against a population nothing else can
/// change. Both the daemon and the thread serving the manager end when it is dropped
/// (ADR-0516).
struct FixtureServiceManager {
    daemon: Child,
    address: String,
    stop: Option<std::sync::mpsc::Sender<()>>,
    server: Option<std::thread::JoinHandle<()>>,
    _home: Scratch,
}

impl FixtureServiceManager {
    /// Starts the bus and the manager, or `None` where this host has no `dbus-daemon`.
    fn try_start(units: i64) -> Option<Self> {
        let home = scratch();
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
        let mut manager = Self {
            daemon,
            address: format!("unix:path={}", socket.display()),
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
                match serve(&address, units).await {
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
    fn address(&self) -> String {
        self.address.clone()
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
async fn serve(address: &str, units: i64) -> zbus::Result<zbus::Connection> {
    let names: Vec<String> = (0..units)
        .map(|index| format!("fixture-{index}.service"))
        .collect();
    let mut builder = zbus::connection::Builder::address(address)?
        .name("org.freedesktop.systemd1")?
        .serve_at(
            "/org/freedesktop/systemd1",
            FixtureManager {
                units: names.clone(),
            },
        )?;
    for name in names {
        let path = unit_path(&name);
        builder = builder
            .serve_at(path.clone(), FixtureUnit { id: name.clone() })?
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
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl FixtureManager {
    #[zbus(property)]
    fn version(&self) -> String {
        "fixture".to_owned()
    }

    fn list_units(&self) -> Vec<UnitRow> {
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
        if self.units.iter().any(|unit| unit == name) {
            Ok(unit_path(name))
        } else {
            Err(zbus::fdo::Error::Failed(format!("Unit {name} not found.")))
        }
    }
}

struct FixtureUnit {
    id: String,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl FixtureUnit {
    #[zbus(property, name = "Id")]
    fn id(&self) -> String {
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
}

struct FixtureService;

#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl FixtureService {
    #[zbus(property, name = "MainPID")]
    fn main_pid(&self) -> u32 {
        0
    }
}
