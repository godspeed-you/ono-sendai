//! The canonical spatial topology and discovery-before-naming, from
//! `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — none of it exists in this
//! build, so every test here is `#[ignore]`d until the increment that delivers it.
//!
//! Sections covered: §2 (core spatial invariants), §3 (`SpatialObject`, `SpatialScope`, `Place`,
//! `HierarchicalEdge`, `RelationshipEdge`, `Neighborhood`, `Landmark`), §4 (canonical system
//! topology), §7 (root `SYSTEM` and the six domains), §9 (discovery without prior names), §12
//! (process spaces), §13 (service spaces), §17 (identity spaces), §18 (device spaces), §35.2
//! (permission states), §44.1 and §44.2 (the two discovery acceptance scenarios). The
//! containerised form of §44.1/§44.2 belongs to the acceptance suite; these are the integration
//! forms of the same scenarios.
//!
//! Everything is asserted through the non-interactive surface §29.1 fixes — `look --json`,
//! `map --json`, `near`, `find place`, `trail --json` MUST answer without a TTY — except the two
//! completion tests, where the behaviour is interactive by nature (§43.4) and a pseudo-terminal
//! is the only honest observation point.
//!
//! Fixtures are built by the test: a `sleep` child whose parent is this test process, and a
//! `TcpListener` on `127.0.0.1:0`. Nothing here depends on a process, service, mount or device
//! that only exists on one machine; where the environment cannot supply a service manager, the
//! test asserts the honest degradation §35.2 and invariant §2.17 require (unavailable and
//! denied are distinct from empty) instead of skipping.
//!
//! Readings chosen where v0.4 is silent — the integrator turns these into ADRs:
//!
//! - `look --json` prints exactly one JSON document; the record describing the current place is
//!   that document or its `place` member, and the `Neighborhood` of §3.6 is its `neighborhood`
//!   member or the document itself when the document already carries `groups`.
//! - Names in spatial records are read from `display_name` (§3.1), falling back to `name`.
//! - §3.1 does not name a field for the permission state, and §35.2 fixes the vocabulary
//!   (`available`, `empty`, `unknown`, `permission_denied`, `unsupported`, `stale`). These tests
//!   require it under `permission` on a place and under `state` on a neighborhood group, because
//!   `state` on a process or service place already means "running"/"failed".
//! - The six canonical domains of §4 appear at the root as `domains` (the `SystemPlace` field of
//!   §7.1) or, equivalently, as the root neighborhood's groups (§3.6).
//! - Domain names are compared case-insensitively: §4 spells them `COMPUTE`, §6.3 spells the
//!   same move `enter compute`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ono_process::{Command as PtyCommand, Executor, PtySession, WindowSize};
use ono_testkit::Shell;
use serde_yaml_ng::Value;

/// The six canonical domains, in the order spec §4 draws them.
const DOMAINS: [&str; 6] = [
    "COMPUTE",
    "NETWORK",
    "STORAGE",
    "CONTAINERS",
    "IDENTITY",
    "DEVICES",
];

/// The permission/availability vocabulary of spec §35.2. These states MUST remain distinct, and
/// `empty` is never the answer for something that could not be asked.
const PERMISSION_STATES: [&str; 6] = [
    "available",
    "empty",
    "unknown",
    "permission_denied",
    "unsupported",
    "stale",
];

/// The states that mean "this could not be answered" rather than "there is nothing here".
const UNANSWERED_STATES: [&str; 4] = ["unknown", "permission_denied", "unsupported", "stale"];

/// The `--where` predicate that selects a running service by its visible state.
///
/// Spec §44.2 asks for "the running web service", chosen by visible metadata. The inherited v0.2
/// contract `ono.service/1` spells "running" in two fields, and neither of them is a `state`
/// called `running`: `state` is the high-level activity state
/// (`active`/`reloading`/`inactive`/`failed`/`activating`/`deactivating`/`unknown`), and
/// `substate` carries the provider's own word for what the unit is doing inside that state
/// (`running`, `exited`, `dead`, `plugged`, …). A service that is up and executing is therefore
/// `active` *and* `running` — the pair the scenario's next step, "follow one of its processes",
/// depends on, because an `active`/`exited` oneshot unit has no process left to follow.
const RUNNING_SERVICE: &str = r#"state == "active" and substate == "running""#;

// --- harness -------------------------------------------------------------------------------

/// Runs one spatial script with `ono -c`, which is the non-interactive surface §29.1 governs.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// The JSON document a `--json` spatial command printed.
///
/// Navigation before it (`home`, `enter …`) may print a rendered header, so the document is read
/// from the first line that opens one, to the end of standard output.
fn document(run: &ono_testkit::Run) -> Value {
    let stdout = run.stdout();
    let start = stdout
        .char_indices()
        .find(|&(index, character)| {
            (character == '{' || character == '[')
                && (index == 0 || stdout.as_bytes()[index - 1] == b'\n')
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| {
            panic!(
                "spec §29.1: a `--json` spatial command prints a JSON document without a \
                 terminal\n--- stdout ---\n{stdout}\n--- stderr ---\n{}",
                run.stderr()
            )
        });
    serde_yaml_ng::from_str(&stdout[start..]).unwrap_or_else(|error| {
        panic!("spec §29.1: the `--json` output is one JSON document: {error}\n{stdout}")
    })
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_mapping().and_then(|mapping| mapping.get(name))
}

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    field(value, name).and_then(Value::as_str)
}

fn sequence<'a>(value: &'a Value, name: &str) -> Option<&'a [Value]> {
    field(value, name)
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
}

/// The record describing the current place (spec §3.3), read through the documented reading.
fn place(document: &Value) -> &Value {
    field(document, "place").unwrap_or(document)
}

/// The bounded, ranked `Neighborhood` of spec §3.6.
fn neighborhood(document: &Value) -> &Value {
    field(document, "neighborhood").unwrap_or(document)
}

/// The display name of any spatial record (spec §3.1: the display name is not identity, but it is
/// what a user reads).
fn name_of(value: &Value) -> String {
    text(value, "display_name")
        .or_else(|| text(value, "name"))
        .unwrap_or_else(|| {
            panic!("spec §3.1: every spatial record carries a display name, got {value:?}")
        })
        .to_owned()
}

/// The permission state of a neighborhood group (spec §35.2), under `state` or `permission`.
fn permission_of(value: &Value) -> String {
    text(value, "state")
        .or_else(|| text(value, "permission"))
        .unwrap_or_else(|| {
            panic!(
                "spec §35.2 and §2.17: a group carries one of {PERMISSION_STATES:?} so that \
                 unknown is visible, got {value:?}"
            )
        })
        .to_owned()
}

/// The groups of the current place's neighborhood (spec §3.6), lower-cased by name.
fn groups(document: &Value) -> Vec<Value> {
    sequence(neighborhood(document), "groups")
        .unwrap_or_else(|| {
            panic!(
                "spec §3.6: a Neighborhood has `groups`, and §24.2 renders them as the place's \
                 exits, got {document:?}"
            )
        })
        .to_vec()
}

fn group_names(document: &Value) -> Vec<String> {
    groups(document)
        .iter()
        .map(|group| name_of(group).to_lowercase())
        .collect()
}

fn group_named<'a>(document: &'a Value, wanted: &str, groups: &'a [Value]) -> &'a Value {
    groups
        .iter()
        .find(|group| name_of(group).to_lowercase() == wanted)
        .unwrap_or_else(|| {
            panic!(
                "spec §7: this place offers a `{wanted}` group, saw {:?} in {document:?}",
                groups.iter().map(name_of).collect::<Vec<_>>()
            )
        })
}

/// The six domain summaries at the root (spec §7.1 `domains`, or the root neighborhood's groups).
fn domain_summaries(document: &Value) -> Vec<Value> {
    sequence(document, "domains").map_or_else(|| groups(document), <[Value]>::to_vec)
}

/// The single number a `| count` script prints.
fn count(script: &str) -> i64 {
    let run = ono(script);
    run.assert_success();
    let line = run
        .stdout()
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_else(|| panic!("`{script}` printed a count, got {:?}", run.output()));
    line.trim()
        .parse()
        .unwrap_or_else(|error| panic!("`{script}` printed a number: {error} ({line:?})"))
}

/// The rows of a `… | to json` script.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let document = document(run);
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §29.4: `near` and `find place` are ordinary object streams, so `| to json` is \
                 array, got {document:?}"
            )
        })
        .clone()
}

/// Whether a service manager answers on this host at all. Spec §44.2 needs one; §35.2 fixes what
/// must happen when there is none, and the tests below assert that instead of skipping.
fn service_manager_answers() -> bool {
    let run = ono("get service | count");
    if !run.status().is_success() {
        return false;
    }
    run.stdout()
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .and_then(|line| line.trim().parse::<i64>().ok())
        .is_some_and(|count| count > 0)
}

/// A `sleep` child of *this test process*, so a predicate over its parentage identifies it
/// without anyone ever typing its name (spec §9.3, §44.1).
struct SleepChild(Child);

impl SleepChild {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("3117")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sleep` is available on every test host");
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// A `--where` predicate over visible process metadata that matches *this* child and nothing
    /// else on the host.
    ///
    /// Parentage alone is not enough: every test in this binary spawns children of the same test
    /// process, and cargo runs them concurrently, so `ppid == <test pid>` also matches the `ono`
    /// shells and sleep children of whichever tests happen to overlap. The child's own pid is
    /// known to the fixture, and §9's "discovery without prior names" is about never naming the
    /// *object* — its command name — not about being unable to point at one's own fixture.
    fn selector(&self) -> String {
        let parent = std::process::id();
        let pid = self.pid();
        format!("ppid == {parent} and pid == {pid}")
    }
}

impl Drop for SleepChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A pseudo-terminal running the interactive shell, for the two behaviours that only exist
/// interactively (spec §43.4).
fn interactive_shell() -> PtySession {
    let mut executor = Executor::detached();
    let command = PtyCommand::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal")
}

/// Reads from `shell` until `needle` appears or the budget runs out, accumulating into `seen`.
fn wait_for(shell: &mut PtySession, seen: &mut String, needle: &str, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    let mut buffer = [0u8; 8192];
    while Instant::now() < deadline {
        if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
            seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
        if seen.contains(needle) {
            return true;
        }
    }
    false
}

// --- the root and the canonical domains (§4, §7.1) -------------------------------------------

#[test]
fn should_report_the_system_root_as_the_current_place_when_home_runs() {
    // §7.1: `home` MUST resolve to a SystemPlace for the active host. §2.2: Ono must always be
    // able to explain the current spatial context, so `look` after `home` names it.
    let run = ono("home; look --json");
    run.assert_success();

    let document = document(&run);
    let place = place(&document);
    assert_eq!(
        name_of(place).to_uppercase(),
        "SYSTEM",
        "§7.1: `home` resolves to the root SYSTEM place, got {document:?}"
    );
    let hostname = text(&document, "hostname")
        .or_else(|| text(place, "hostname"))
        .unwrap_or_else(|| panic!("§7.1: a SystemPlace carries the hostname, got {document:?}"));
    assert!(
        !hostname.is_empty(),
        "§7.1: the root names the host it represents, got {hostname:?}"
    );
}

#[test]
fn should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root() {
    // §4 and §53 ("Root geography? Six canonical domains"): the root offers COMPUTE, NETWORK,
    // STORAGE, CONTAINERS, IDENTITY and DEVICES — all six, and nothing else beside them.
    let run = ono("home; look --json");
    run.assert_success();

    let document = document(&run);
    let mut seen: Vec<String> = domain_summaries(&document)
        .iter()
        .map(|domain| name_of(domain).to_uppercase())
        .collect();
    seen.sort_unstable();
    let mut expected: Vec<String> = DOMAINS.iter().map(|name| (*name).to_owned()).collect();
    expected.sort_unstable();
    assert_eq!(
        seen, expected,
        "§4/§53: the root lists exactly the six canonical domains, got {document:?}"
    );
}

#[test]
fn should_carry_a_permission_state_on_every_domain_so_an_unavailable_one_stays_visible() {
    // §4: "Unavailable domains remain visible but carry an `unavailable`, `unsupported` or
    // `permission_denied` state rather than disappearing silently." §2.17: unknown is visible.
    // §35.2 fixes the vocabulary, so the state of every domain must come from it.
    let run = ono("home; look --json");
    run.assert_success();

    let document = document(&run);
    for domain in domain_summaries(&document) {
        let name = name_of(&domain);
        let state = permission_of(&domain);
        assert!(
            PERMISSION_STATES.contains(&state.as_str()),
            "§35.2: domain {name} reports one of {PERMISSION_STATES:?}, got {state:?}"
        );
    }
}

#[test]
fn should_bound_the_root_horizon_instead_of_listing_every_known_object() {
    // §7.1: "The root MUST never be a flat list of every object known to Ono." §2.9: the horizon
    // is bounded. §3.6: a Neighborhood is a bounded, ranked projection, not all adjacent nodes.
    let processes = count("get process | count");
    assert!(
        processes > 64,
        "precondition: this host runs more processes than a bounded horizon may show, saw \
         {processes}"
    );

    let neighbors = count("home; near | count");
    assert!(
        neighbors <= 64,
        "§2.9/§7.1: the root shows a bounded neighborhood, not {neighbors} objects"
    );
    assert!(
        neighbors >= 6,
        "§4: the six canonical domains are always reachable from the root, saw {neighbors}"
    );
}

#[test]
fn should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking() {
    // §3.1: a SpatialObject carries `spatial_id`, `object_type`, `display_name` and `scope`.
    // §3.2: a scope names the execution and discovery boundary — here the local host. §35.2 adds
    // the permission state, which §2.17 requires to be visible rather than implied.
    for script in ["home; look --json", "home; enter compute; look --json"] {
        let run = ono(script);
        run.assert_success();
        let document = document(&run);
        let place = place(&document);

        let id = text(place, "spatial_id")
            .unwrap_or_else(|| panic!("§3.1: a place carries a spatial_id, got {document:?}"));
        assert!(
            !id.is_empty(),
            "§3.1: the spatial id identifies the place, got {id:?} from `{script}`"
        );
        assert!(
            text(place, "object_type").is_some_and(|kind| !kind.is_empty()),
            "§3.1: a place carries its object_type, got {document:?} from `{script}`"
        );
        assert!(
            !name_of(place).is_empty(),
            "§3.1: a place carries a display name, got {document:?} from `{script}`"
        );

        let scope = field(place, "scope")
            .unwrap_or_else(|| panic!("§3.1/§3.2: a place carries its scope, got {document:?}"));
        let scope_kind = text(scope, "kind")
            .or_else(|| scope.as_str())
            .unwrap_or_else(|| panic!("§3.2: a scope names its kind, got {scope:?}"));
        assert!(
            scope_kind.to_lowercase().contains("host"),
            "§3.2: a local place belongs to the HostScope, got {scope_kind:?} from `{script}`"
        );

        let permission = text(place, "permission").unwrap_or_else(|| {
            panic!("§35.2/§2.17: a place carries its permission state, got {document:?}")
        });
        assert!(
            PERMISSION_STATES.contains(&permission),
            "§35.2: the permission state is one of {PERMISSION_STATES:?}, got {permission:?}"
        );
    }
}

#[test]
fn should_keep_the_same_spatial_id_for_the_root_across_separate_sessions() {
    // §3.1: a SpatialId is "stable for as long as the implementation can truthfully identify the
    // same conceptual object". The root of one host is one conceptual object, so two runs of the
    // same script must not invent two identities for it.
    let first = ono("home; look --json");
    first.assert_success();
    let second = ono("home; look --json");
    second.assert_success();

    let left = document(&first);
    let right = document(&second);
    assert_eq!(
        text(place(&left), "spatial_id"),
        text(place(&right), "spatial_id"),
        "§3.1: the root's spatial id is stable across sessions, got {left:?} then {right:?}"
    );
}

// --- the domains themselves (§7.2 - §7.7) -----------------------------------------------------

#[test]
fn should_enter_every_canonical_domain_when_named_at_the_root() {
    // §2.3: movement changes context, it does not merely print an object. §6.3: `enter` resolves
    // one place. Every one of the six domains of §4 is enterable, including the ones no provider
    // on this host contributes to (§4: unavailable domains remain visible).
    for domain in DOMAINS {
        let script = format!("home; enter {}; look --json", domain.to_lowercase());
        let run = ono(&script);
        run.assert_success();
        let document = document(&run);
        assert_eq!(
            name_of(place(&document)).to_uppercase(),
            domain,
            "§2.3/§6.3: `enter {}` makes {domain} the current place, got {document:?}",
            domain.to_lowercase()
        );
    }
}

#[test]
fn should_offer_the_compute_groups_the_spec_names_when_entering_compute() {
    // §7.2: "COMPUTE MUST provide access to: processes services jobs workloads cgroups". The same
    // section calls `workloads` a spatial aggregate Ono MAY form when evidence supports it, so it
    // is not required here; the other four are.
    let run = ono("home; enter compute; look --json");
    run.assert_success();

    let document = document(&run);
    let names = group_names(&document);
    for wanted in ["processes", "services", "jobs", "cgroups"] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§7.2: COMPUTE provides access to `{wanted}`, saw {names:?}"
        );
    }
}

#[test]
fn should_offer_the_network_groups_the_spec_names_when_entering_network() {
    // §7.3: NETWORK MUST provide access to interfaces, addresses, routes, neighbors, listeners,
    // connections and namespaces — all seven are MUST, none is conditional on a provider.
    let run = ono("home; enter network; look --json");
    run.assert_success();

    let document = document(&run);
    let names = group_names(&document);
    for wanted in [
        "interfaces",
        "addresses",
        "routes",
        "neighbors",
        "listeners",
        "connections",
        "namespaces",
    ] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§7.3: NETWORK provides access to `{wanted}`, saw {names:?}"
        );
    }
}

#[test]
fn should_offer_the_storage_groups_the_spec_names_when_entering_storage() {
    // §7.4: STORAGE MUST provide access to filesystems, mounts, volumes/devices where known and
    // directory roots. `filesystems` and `mounts` are unconditional; the directory entry point is
    // required too (§15.1 keeps the Unix path tree), and is matched under any name beginning
    // `director…` because §7.4 writes it as prose rather than an identifier.
    let run = ono("home; enter storage; look --json");
    run.assert_success();

    let document = document(&run);
    let names = group_names(&document);
    for wanted in ["filesystems", "mounts"] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§7.4: STORAGE provides access to `{wanted}`, saw {names:?}"
        );
    }
    assert!(
        names.iter().any(|name| name.starts_with("director")),
        "§7.4/§15.1: STORAGE offers directory roots, so the Unix path tree stays reachable, \
         saw {names:?}"
    );
}

#[test]
fn should_offer_the_identity_groups_the_spec_names_when_entering_identity() {
    // §7.6: IDENTITY MUST provide access to users, groups and active sessions. §17 adds that
    // identity spaces never reveal secrets, credentials or environment contents — this test only
    // establishes the entry points; the non-disclosure rule is asserted where those places are.
    let run = ono("home; enter identity; look --json");
    run.assert_success();

    let document = document(&run);
    let names = group_names(&document);
    for wanted in ["users", "groups", "sessions"] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§7.6: IDENTITY provides access to `{wanted}`, saw {names:?}"
        );
    }
}

#[test]
fn should_keep_containers_and_devices_enterable_with_a_state_when_no_provider_contributes() {
    // §7.5 (provider-neutral containers), §7.7 (devices only where a provider supplies meaning)
    // and §4 (an unavailable domain stays visible with a state). An unprivileged host with no
    // container runtime must still be able to stand in CONTAINERS and be told why it is empty —
    // §2.17 and §35.2: unavailable is never rendered as absence.
    for domain in ["containers", "devices"] {
        let script = format!("home; enter {domain}; look --json");
        let run = ono(&script);
        run.assert_success();

        let document = document(&run);
        let permission = text(place(&document), "permission").unwrap_or_else(|| {
            panic!("§4/§35.2: {domain} reports its availability, got {document:?}")
        });
        assert!(
            PERMISSION_STATES.contains(&permission),
            "§35.2: {domain} reports one of {PERMISSION_STATES:?}, got {permission:?}"
        );
    }
}

// --- the spatial layer composes provider facts, it does not invent them (§2.16) ---------------

#[test]
fn should_show_the_users_the_user_provider_answers_for_when_entering_identity_users() {
    // §2.16: "Providers own facts. Ono's spatial layer composes provider data; it MUST NOT become
    // an undocumented source of system truth." So the users visible as places are a subset of
    // what `get user` answers, and uid 0 — the one account every Linux host has — is among them.
    let known = rows(&ono("get user | to json"));
    let known: Vec<String> = known.iter().map(name_of).collect();
    assert!(
        known.iter().any(|name| name == "root"),
        "precondition: the user provider answers for uid 0, saw {known:?}"
    );

    let run = ono("home; enter identity; enter users; near | to json");
    run.assert_success();
    let seen: Vec<String> = rows(&run).iter().map(name_of).collect();

    assert!(
        seen.iter().any(|name| name == "root"),
        "§17/§7.6: the user the provider answers for is a place, saw {seen:?}"
    );
    for name in &seen {
        assert!(
            known.contains(name),
            "§2.16: the spatial layer shows only what the provider knows; `{name}` is not in \
             {known:?}"
        );
    }
}

#[test]
fn should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts() {
    // §2.16 again, on the mount table: every mount that is a place must be a mount `get mount`
    // answers for, and `/` is always one of them (§15.3: mount boundaries are real objects).
    let known = rows(&ono("get mount | to json"));
    let known: Vec<String> = known
        .iter()
        .filter_map(|mount| text(mount, "target").map(str::to_owned))
        .collect();
    assert!(
        known.iter().any(|target| target == "/"),
        "precondition: the mount provider answers for the root filesystem, saw {known:?}"
    );

    let run = ono("home; enter storage; enter mounts; near | to json");
    run.assert_success();
    let seen: Vec<String> = rows(&run).iter().map(name_of).collect();

    assert!(
        seen.iter().any(|target| target == "/"),
        "§7.4/§15.3: the root mount is a place, saw {seen:?}"
    );
    for target in &seen {
        assert!(
            known.contains(target),
            "§2.16: `{target}` is a place the mount provider does not know: {known:?}"
        );
    }
}

#[test]
fn should_show_a_block_device_the_device_provider_answers_for_when_entering_devices() {
    // §18: devices are spatial objects only where a provider supplies stable identity and useful
    // relationships, and "/dev/* path existence alone is insufficient". A block device the v0.2
    // device provider already answers for — kind and subsystem included — is exactly such an
    // object, so it must appear as a place rather than be re-derived or dropped.
    let blocks = rows(&ono("get device | where kind == \"block\" | to json"));
    let names: Vec<String> = blocks.iter().map(name_of).collect();

    let run = ono("home; enter devices; look --json");
    run.assert_success();
    let document = document(&run);
    let permission = text(place(&document), "permission")
        .unwrap_or_else(|| panic!("§4/§35.2: DEVICES reports its availability, got {document:?}"));

    if names.is_empty() {
        assert!(
            UNANSWERED_STATES.contains(&permission) || permission == "empty",
            "§35.2: with no block devices the domain is `empty` or reports why it cannot say, \
             got {permission:?}"
        );
        return;
    }

    assert_eq!(
        permission,
        "available",
        "§35.2: the device provider answers for {} devices, so DEVICES is available, got \
         {permission:?}",
        names.len()
    );
    let seen: Vec<String> = rows(&ono("home; enter devices; near --all | to json"))
        .iter()
        .map(name_of)
        .collect();
    assert!(
        names.iter().any(|name| seen.contains(name)),
        "§18/§2.16: a block device the provider answers for is a place, saw {seen:?} for {names:?}"
    );
}

// --- the neighborhood is bounded, ranked and explains what it hides (§3.6, §3.7) --------------

#[test]
fn should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors() {
    // §3.6: a Neighborhood carries `hidden_count`, `completeness` and `generated_at`; it "is not
    // simply all adjacent nodes". §6.2: `near` ranks and bounds by default, `--all` asks for the
    // complete one-hop neighborhood. The process list is the natural stress case.
    let processes = count("get process | count");
    assert!(
        processes > 64,
        "precondition: this host runs enough processes to overflow a horizon, saw {processes}"
    );

    let bounded = count("home; enter compute; enter processes; near | count");
    let complete = count("home; enter compute; enter processes; near --all | count");
    assert!(
        bounded < complete,
        "§6.2/§3.6: the default neighborhood is bounded below the complete one, saw {bounded} \
         and {complete}"
    );

    let run = ono("home; enter compute; enter processes; look --json");
    run.assert_success();
    let document = document(&run);
    let neighborhood = neighborhood(&document);
    let hidden = field(neighborhood, "hidden_count")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("§3.6: a Neighborhood carries hidden_count, got {document:?}"));
    assert!(
        hidden > 0,
        "§3.6/§2.9: what the bounded horizon left out is counted, not silently dropped, got \
         {hidden} with {processes} processes"
    );
    assert!(
        text(neighborhood, "completeness").is_some(),
        "§3.6: a Neighborhood states its completeness, got {document:?}"
    );
}

#[test]
fn should_expose_a_reason_on_every_landmark_when_a_place_reports_landmarks() {
    // §3.7: "A landmark MUST always expose its reason", drawn from the built-in vocabulary.
    // §2.11: highlighting is driven by real state, change, importance or pinning — so a landmark
    // without a reason is a decoration, which §49.1 forbids.
    let reasons = [
        "high_cpu",
        "high_memory",
        "failed",
        "restarting",
        "recently_changed",
        "public_listener",
        "privileged",
        "storage_pressure",
        "connection_spike",
        "new_object",
        "removed_object",
        "security_boundary",
        "remote_boundary",
        "user_pinned",
    ];

    for script in [
        "home; look --json",
        "home; enter compute; look --json",
        "home; enter network; look --json",
    ] {
        let run = ono(script);
        run.assert_success();
        let document = document(&run);
        let landmarks = sequence(&document, "landmarks")
            .or_else(|| sequence(neighborhood(&document), "landmarks"))
            .unwrap_or_else(|| {
                panic!(
                    "§3.6/§7.1: a place reports its landmarks, even as an empty list: {document:?}"
                )
            })
            .to_vec();
        for landmark in landmarks {
            let reason = text(&landmark, "reason").unwrap_or_else(|| {
                panic!("§3.7: a landmark always exposes its reason, got {landmark:?}")
            });
            assert!(
                reasons.contains(&reason),
                "§3.7: the landmark reason comes from the built-in vocabulary {reasons:?}, got \
                 {reason:?} from `{script}`"
            );
        }
    }
}

#[test]
fn should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root() {
    // §29.4: `near` and `find place` return normal structured streams that participate in object
    // pipelines. §6.2 shows the shape: a relation, the object, its state. §2.20: spatial state is
    // inspectable and scriptable, which means the v0.2 stages compose with it unchanged.
    let run = ono("home; near | to json");
    run.assert_success();
    let neighbors = rows(&run);
    assert!(
        neighbors.len() >= 6,
        "§4: the six canonical domains are neighbors of the root, got {neighbors:?}"
    );
    for neighbor in &neighbors {
        assert!(
            text(neighbor, "relation").is_some(),
            "§6.2: a neighbor names the relation that reaches it, got {neighbor:?}"
        );
        assert!(
            !name_of(neighbor).is_empty(),
            "§6.2: a neighbor names the object, got {neighbor:?}"
        );
    }

    let selected = ono("home; near | select relation | count");
    selected.assert_success();
    assert_eq!(
        count("home; near | select relation | count"),
        i64::try_from(neighbors.len()).unwrap_or(i64::MAX),
        "§29.4/§2.20: `near` composes with the v0.2 pipeline unchanged, got {}",
        selected.output()
    );
}

// --- discovery without prior names (§9, §44.1, §44.2) -----------------------------------------

#[test]
fn should_answer_look_near_and_map_without_an_object_name_when_at_the_root() {
    // §9.2: `look`, `near` and `map` "MUST work without an object name". §29.1: all of them must
    // work non-interactively, which is how this test observes them. §2.1: discovery before naming
    // is an invariant, and this is its smallest form.
    for script in [
        "home; look",
        "home; look --json",
        "home; near",
        "home; map",
        "home; map --json",
        "home; trail --json",
    ] {
        let run = ono(script);
        run.assert_success();
        assert!(
            !run.stdout().trim().is_empty(),
            "§9.2/§29.1: `{script}` answers without an object name and without a terminal, got \
             {:?}",
            run.output()
        );
    }
}

#[test]
fn should_reach_a_process_it_never_names_when_only_a_predicate_over_visible_metadata_is_known() {
    // §44.1 (cold-start discovery) and §9.3 (active global discovery) in their smallest honest
    // form: the test spawns a child of itself and then walks home -> COMPUTE -> processes and
    // selects it by parentage — metadata that is visible in the place — never by the name
    // `sleep`. §28.2: a structured result containing spatial objects is enterable, and `@-1` is
    // the v0.2 reference to the previous result.
    let child = SleepChild::spawn();
    let selector = child.selector();

    let script = format!(
        "home; enter compute; enter processes; find place --type process --where {selector}; \
         enter @-1; look --json"
    );
    let run = ono(&script);
    run.assert_success();

    let document = document(&run);
    let place = place(&document);
    assert_eq!(
        field(place, "pid").and_then(Value::as_u64),
        Some(u64::from(child.pid())),
        "§9.3/§44.1: a predicate over visible metadata reaches the process the user never named, \
         got {document:?}"
    );
    assert!(
        text(place, "object_type").is_some_and(|kind| kind.to_lowercase().contains("process")),
        "§3.1/§12: the place reached is a process place, got {document:?}"
    );
}

#[test]
fn should_offer_the_process_exits_the_spec_names_when_a_discovered_process_becomes_the_place() {
    // §12: a process place has at minimum the groups identity, state, parent, children, service,
    // user, cgroup, namespaces, files, sockets/connections and recent changes. §35.2: a group the
    // user may not read reports `permission_denied`, never `0` — the child here is the test's own
    // process, so its open files are legitimately readable and the group must say so.
    let child = SleepChild::spawn();
    let selector = child.selector();

    let script = format!(
        "home; enter compute; enter processes; find place --type process --where {selector}; \
         enter @-1; look --json"
    );
    let run = ono(&script);
    run.assert_success();

    let document = document(&run);
    let groups = groups(&document);
    let names: Vec<String> = groups
        .iter()
        .map(|group| name_of(group).to_lowercase())
        .collect();
    for wanted in [
        "parent",
        "children",
        "user",
        "cgroup",
        "namespaces",
        "files",
        "sockets",
    ] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§12: a process place offers `{wanted}` as an exit, saw {names:?} for pid {}",
            child.pid()
        );
    }

    let files = group_named(&document, "files", &groups);
    let state = permission_of(files);
    assert!(
        PERMISSION_STATES.contains(&state.as_str()),
        "§35.2: the files group states its permission, got {state:?}"
    );
    assert_ne!(
        state, "unknown",
        "§35.1/§35.2: the test owns this process, so its open files are legitimately queryable \
         and the group is not `unknown`: {files:?}"
    );
}

#[test]
fn should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner() {
    // §2.5 and §3.5: a relationship edge (`process --parent-of--> process`) is real and
    // traversable; §6.4: `follow` traverses a relationship edge and records it in the trail.
    // The spawner is this test process, whose pid the test knows without ever naming it to Ono.
    let child = SleepChild::spawn();
    let parent = std::process::id();
    let selector = child.selector();

    let script = format!(
        "home; enter compute; enter processes; find place --type process --where {selector}; \
         enter @-1; follow parent; look --json"
    );
    let run = ono(&script);
    run.assert_success();

    let document = document(&run);
    assert_eq!(
        field(place(&document), "pid").and_then(Value::as_u64),
        Some(u64::from(parent)),
        "§6.4/§3.5: `follow parent` traverses the parent-of edge to the spawning process, got \
         {document:?}"
    );

    let trail = ono(&format!("{script}; trail --json"));
    trail.assert_success();
    assert!(
        trail.stdout().contains("parent"),
        "§6.4/§6.7: the relation traversed is recorded in the trail, got {:?}",
        trail.output()
    );
}

#[test]
fn should_discover_a_listening_socket_by_its_port_and_follow_it_to_its_owning_process() {
    // The half of §44.2 that needs no service manager: "follow its listening socket". The test
    // binds the listener itself, so the port is fixture-owned and the walk home -> NETWORK ->
    // listeners selects it by visible metadata (§9.3), never by a name. §14.3 makes the listener
    // a place; §3.5 makes `process --owns--> socket` a real edge, and the socket belongs to this
    // test's own process, so §35.1 permits resolving its owner.
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("a bound address").port();
    let owner = std::process::id();

    let script = format!(
        "home; enter network; enter listeners; find place --where local.port == {port}; \
         enter @-1; look --json"
    );
    let run = ono(&script);
    run.assert_success();

    let listener_view = document(&run);
    assert!(
        text(place(&listener_view), "object_type")
            .is_some_and(|kind| kind.to_lowercase().contains("socket")),
        "§14.3/§44.2: the listener discovered by port is a socket place, got {listener_view:?}"
    );

    let run = ono(&format!(
        "home; enter network; enter listeners; find place --where local.port == {port}; \
         enter @-1; follow process; look --json"
    ));
    run.assert_success();
    let followed = document(&run);
    assert_eq!(
        field(place(&followed), "pid").and_then(Value::as_u64),
        Some(u64::from(owner)),
        "§3.5/§44.2: `follow process` reaches the process that owns the socket, got {followed:?}"
    );

    drop(listener);
}

#[test]
fn should_reach_a_running_service_by_its_visible_state_when_a_service_manager_answers() {
    // §44.2 (the unknown-nginx scenario): home -> compute -> services -> select the running
    // service by visible metadata -> enter it -> follow one of its processes. Where no service
    // manager answers — the container the acceptance suite runs in, for one — §35.2 and §2.17
    // require the services group to say so rather than report an empty system, and that is what
    // this test asserts instead. §13 fixes the groups a service place offers.
    if !service_manager_answers() {
        let run = ono("home; enter compute; enter services; look --json");
        run.assert_success();
        let document = document(&run);
        let permission = text(place(&document), "permission").unwrap_or_else(|| {
            panic!("§4/§35.2: the services place reports its availability, got {document:?}")
        });
        assert!(
            UNANSWERED_STATES.contains(&permission),
            "§2.17/§35.2: with no service manager the services place is `unknown`, \
             `permission_denied`, `unsupported` or `stale` — never `empty` or `available`, got \
             {permission:?}"
        );
        return;
    }

    let counted = format!(
        "home; enter compute; enter services; find place --where {RUNNING_SERVICE} | count"
    );
    let found = ono(&counted);
    found.assert_success();
    assert!(
        count(&counted) >= 1,
        "§44.2/§9.3: a running service is discoverable by its visible state alone, got {:?}",
        found.output()
    );

    let run = ono(&format!(
        "home; enter compute; enter services; find place --where {RUNNING_SERVICE}; enter @-1; \
         look --json"
    ));
    run.assert_success();
    let document = document(&run);
    assert!(
        text(place(&document), "object_type")
            .is_some_and(|kind| kind.to_lowercase().contains("service")),
        "§44.2: entering the discovered result lands on a service place, got {document:?}"
    );
    let names = group_names(&document);
    for wanted in ["processes", "dependencies", "dependents", "cgroup"] {
        assert!(
            names.iter().any(|name| name == wanted),
            "§13: a service place offers `{wanted}` as an exit, saw {names:?}"
        );
    }

    let followed = ono(&format!(
        "home; enter compute; enter services; find place --where {RUNNING_SERVICE}; enter @-1; \
         follow processes; look --json"
    ));
    followed.assert_success();
    assert!(
        !followed.stdout().trim().is_empty(),
        "§44.2/§13: one of the service's processes is reachable by following the edge, got {:?}",
        followed.output()
    );
}

#[test]
fn should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider() {
    // §35.2: the six states are distinct, and §2.17 forbids rendering "unknown" as absence. This
    // is the invariant that makes every count in a spatial view trustworthy: a group that reports
    // `available` with zero members is a claim that there is nothing there.
    let run = ono("home; enter compute; look --json");
    run.assert_success();
    let document = document(&run);
    let groups = groups(&document);
    let services = group_named(&document, "services", &groups);
    let state = permission_of(services);
    let members = field(services, "count")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("§3.6: a neighborhood group carries its size, got {services:?}"));

    if service_manager_answers() {
        assert_eq!(
            state, "available",
            "§35.2: a service manager answers here, so the group is available, got {services:?}"
        );
        assert!(
            members > 0,
            "§2.16: the group reports what the provider answers, got {members}"
        );
    } else {
        assert!(
            UNANSWERED_STATES.contains(&state.as_str()),
            "§2.17/§35.2: no service manager means the group is unavailable, not empty, got \
             {services:?}"
        );
        assert_ne!(
            state, "empty",
            "§35.2: `empty` claims there are no services; `unsupported` says nobody could answer"
        );
    }
}

#[test]
fn should_resolve_find_as_a_spatial_verb_while_the_external_tool_stays_reachable_by_path() {
    // §6 fixes `find` and `look` as normative spatial verbs, and §9.3 makes the spatial search
    // the global discovery path. Both names collide with a Unix tool that exists on every host,
    // and §2.15 ("Unix remains underneath") plus §44.10 (raw shell continuity) forbid breaking
    // it. ADR-0124 resolves the collision the way the v0.2 registry already resolves it, by verb
    // plus target: `find place …` is the spatial search, bare `find` keeps reaching findutils,
    // and `/usr/bin/find` still runs the external tool by path.
    let run = ono("home; find place --type process --where pid == 1 | count");
    run.assert_success();
    assert_eq!(
        count("home; find place --type process --where pid == 1 | count"),
        1,
        "§9.3 with ADR-0124: `find place` is the spatial search and returns a structured \
         stream, got {:?}",
        run.output()
    );

    let external = ono("/usr/bin/find /etc/hostname -maxdepth 0");
    external.assert_success();
    external.assert_stdout_contains("/etc/hostname");
}

// --- completion as spatial discovery (§9.4) ---------------------------------------------------

#[test]
fn should_complete_the_places_of_the_current_neighborhood_when_tab_follows_enter() {
    // §9.4: "completion MUST prioritize services visible in the current neighborhood and then
    // offer broader matches" — completion is a lightweight local map, not token completion.
    // §9.1 lists it among the passive discovery paths, so at the root the six canonical domains
    // of §4 are what `enter <TAB>` teaches. Interactive by nature, hence the PTY (§43.4).
    let mut shell = interactive_shell();
    let mut seen = String::new();
    assert!(
        wait_for(&mut shell, &mut seen, ">", Duration::from_secs(10)),
        "a prompt"
    );

    shell.write_all(b"home\n").expect("the terminal accepts it");
    assert!(
        wait_for(&mut shell, &mut seen, "SYSTEM", Duration::from_secs(8)),
        "§7.1/§5: the root place announces itself; saw:\n{seen}"
    );

    seen.clear();
    shell.write_all(b"enter \t\t").expect("two tabs");
    let listed = wait_for(&mut shell, &mut seen, "network", Duration::from_secs(8));
    assert!(
        listed && seen.to_lowercase().contains("compute"),
        "§9.4: `enter <TAB>` at the root offers the neighborhood's places — the six domains of \
         §4; saw:\n{seen}"
    );

    shell.write_all(b"\x03exit\n").expect("leave");
    let _ = shell.wait();
}

#[test]
fn should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow() {
    // §9.4, second half: at a process place, `follow <TAB>` "MUST show actual available relation
    // types" — parent, child, user, cgroup, namespace, socket, file, service. Not the whole
    // relation vocabulary: the ones this place actually has (§3.5, §12). The fixture is a child
    // of the test, so `parent` and `user` are certain to exist.
    let child = SleepChild::spawn();
    let selector = child.selector();

    let mut shell = interactive_shell();
    let mut seen = String::new();
    assert!(
        wait_for(&mut shell, &mut seen, ">", Duration::from_secs(10)),
        "a prompt"
    );

    let walk = format!(
        "home; enter compute; enter processes; find place --type process --where {selector}; enter @-1\n"
    );
    shell
        .write_all(walk.as_bytes())
        .expect("the terminal accepts the walk");
    assert!(
        wait_for(
            &mut shell,
            &mut seen,
            &child.pid().to_string(),
            Duration::from_secs(10)
        ),
        "§9.3: the discovered process becomes the current place; saw:\n{seen}"
    );

    seen.clear();
    shell.write_all(b"follow \t\t").expect("two tabs");
    // Both relations are waited for, because the completion table arrives in as many writes as
    // the terminal happens to give it: reading until `parent` is on screen and then asserting on
    // `user` in the same breath fails whenever the rest of the table is still in flight.
    let listed = wait_for(&mut shell, &mut seen, "parent", Duration::from_secs(8))
        && wait_for(&mut shell, &mut seen, "user", Duration::from_secs(8));
    assert!(
        listed,
        "§9.4: `follow <TAB>` lists the relations this place actually has; saw:\n{seen}"
    );

    shell.write_all(b"\x03exit\n").expect("leave");
    let _ = shell.wait();
}
