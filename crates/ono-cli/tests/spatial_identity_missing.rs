//! Stable identity, lifetime, tombstones and permission honesty for the v0.4 Spatial Systems
//! Interface — the part of `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` that
//! decides whether a *place* can be trusted to be the same place tomorrow.
//!
//! Sections covered: §3.1 (`SpatialObject`: `spatial_id` opaque, display name is not identity),
//! §3.2 (scope), §10 (identity tiers, process identity, tombstones), §11.3 (no forced single
//! parent), §11.4 (relationship explainability), §11.5 (confidence), §25.3 (event freshness, as
//! far as identity across change needs it), §33 (the index is a cache, providers are
//! authoritative), §35 (permission boundaries and the six neighborhood states), §37.1 (adapter
//! identity merge), §40 (structured `spatial.*` errors), §42 (provider conformance: identity,
//! reuse safety, relation integrity, permission), §43.2 (the identity properties), §44.7
//! (identity replacement) and §44.8 (permission honesty). §53 settles the product questions
//! these tests encode: *"PID alone is insufficient identity"*, *"Restarted service process? Old
//! process tombstones; stable service remains; new process has new identity"* and
//! *"Unknown/denied data? Distinct from empty."*
//!
//! None of this exists today. The whole spatial verb set (`look`, `near`, `map`, `up`, `back`,
//! `jump`, `trail`, `follow`, `home`) is absent — `look` and `find` currently resolve to the
//! external `/usr/bin/look` and `/usr/bin/find`, the rest to `Ono-Sendai-E0101 command not
//! found`. Every test below therefore fails on the missing spatial surface, not on its own
//! scaffolding, and each carries the section that governs it.
//!
//! ## Spellings these tests pin, and why
//!
//! * A process becomes the current place through the v0.2 spelling `enter process <pid>`
//!   (spec §6.3 `enter <selector>` moving one place; v0.2 §14.3 already makes that statement
//!   succeed) so that the only thing missing is the *spatial* answer, not the navigation verb.
//! * The place itself is read with `look --json` → `PlaceView` (§6.1, §29.1: it MUST work with
//!   no TTY).
//! * The one-hop neighborhood, including per-edge provenance, is read with `near | to json`.
//!   §11.4 requires every displayed relationship to support inspection "or equivalent
//!   structured selection", and §29.4 makes `near` an ordinary object stream that composes with
//!   the v0.2 pipeline. That composition is the structured selection.
//! * `PlaceView` nesting is not fixed by the spec, so `look_field` accepts the field at the
//!   document root, under `place`/`object`, or anywhere below — the tests assert the *contents*
//!   §3.1 and §6.1 name, never a shape the spec does not.
//!
//! ## Fixtures
//!
//! Every process this file navigates to is one it spawned itself: `sh -c 'sleep … & echo $!'`,
//! so the `sh` exits immediately and the `sleep` is orphaned and reparented, which means a kill
//! genuinely removes it from `/proc` instead of leaving a zombie this test would have to reap.
//! Nothing here depends on a service, a name or a process that only exists on one machine.
//!
//! §43.3 asks for a restarting-service fixture. An unprivileged, offline test cannot restart a
//! system service, so §44.7 is proven with a process the test restarts itself: the old process
//! exits, a new process with the same command line and a new pid takes over, and the rule under
//! test — the old place tombstones, the new place is a different identity, history never
//! confuses them — is exactly the same rule. The comment on each such test says so.
//!
//! Real PID reuse cannot be forced unprivileged either (it needs the pid space to wrap, or a
//! private pid namespace that may not be available). §43.2's property "PID reuse → different
//! lifetime SpatialId" is therefore asserted through its two observable consequences, which is
//! what §42.2 actually demands: a tombstoned place never resolves to a live object, and two
//! process lifetimes never share a `SpatialId`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use ono_testkit::Shell;
use serde_yaml_ng::Value;

/// The six neighborhood-group states of spec §35.2. They "MUST remain distinct".
const PERMISSION_STATES: [&str; 6] = [
    "available",
    "empty",
    "unknown",
    "permission_denied",
    "unsupported",
    "stale",
];

/// The confidence vocabulary of spec §11.5.
const CONFIDENCES: [&str; 5] = ["exact", "strong", "inferred", "user_declared", "unknown"];

/// The freshness vocabulary of spec §25.3, normalised — the spec writes `event-driven`.
const FRESHNESS: [&str; 5] = ["event_driven", "polled", "cached", "stale", "partial"];

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// The uid this test runs as, read from the kernel rather than from the environment.
fn uid() -> u32 {
    std::fs::metadata("/proc/self")
        .expect("/proc is mounted on every Linux test host")
        .uid()
}

/// Every JSON document a script printed, in order. JSON is YAML, so the workspace's YAML
/// parser reads it; `to json` and `--json` both emit one document per line (spec §29.1).
fn json_docs(run: &ono_testkit::Run) -> Vec<Value> {
    run.stdout()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{') || line.starts_with('['))
        .map(|line| {
            serde_yaml_ng::from_str(line).unwrap_or_else(|error| {
                panic!("spec §29.1: structured spatial output is one JSON document per line ({error}): {line}")
            })
        })
        .collect()
}

/// The single JSON document a `look --json` (or one `to json`) printed.
fn one_doc(run: &ono_testkit::Run, what: &str) -> Value {
    let docs = json_docs(run);
    assert_eq!(
        docs.len(),
        1,
        "spec §29.1: `{what}` answers with exactly one structured document without a TTY, got {:?}",
        run.output()
    );
    docs.into_iter().next().expect("length checked above")
}

/// A field of a `PlaceView`. Spec §6.1 names what `look --json` carries and §3.1 names the
/// `SpatialObject` fields it carries them from, but neither fixes the nesting, so the root, the
/// conventional `place`/`object` wrappers and any deeper position all count.
fn look_field<'a>(view: &'a Value, name: &str) -> Option<&'a Value> {
    if let Some(direct) = view.get(name) {
        return Some(direct);
    }
    for wrapper in ["place", "object", "spatial_object", "current"] {
        if let Some(found) = view.get(wrapper).and_then(|inner| inner.get(name)) {
            return Some(found);
        }
    }
    find_key(view, name)
}

/// Depth-first search for a mapping key anywhere in a document.
fn find_key<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Mapping(map) => {
            for (key, child) in map {
                if key.as_str() == Some(name) {
                    return Some(child);
                }
                if let Some(found) = find_key(child, name) {
                    return Some(found);
                }
            }
            None
        }
        Value::Sequence(items) => items.iter().find_map(|item| find_key(item, name)),
        _ => None,
    }
}

/// The opaque `SpatialId` of the place a `PlaceView` describes (spec §3.1).
fn spatial_id(view: &Value) -> String {
    let raw = look_field(view, "spatial_id").unwrap_or_else(|| {
        panic!("spec §3.1: every SpatialObject carries an opaque `spatial_id`, got {view:?}")
    });
    let id = as_text(raw);
    assert!(
        !id.is_empty() && raw != &Value::Null,
        "spec §3.1: `spatial_id` identifies the object and is never empty or null, got {view:?}"
    );
    id
}

fn as_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => serde_yaml_ng::to_string(other)
            .expect("a value serialises")
            .trim()
            .to_owned(),
    }
}

/// A named neighborhood group of a `PlaceView` — `files`, `sockets`, `children`, … (spec §6.1,
/// §12). §35.2 makes each one a *state*, not a bare count.
fn group<'a>(view: &'a Value, name: &str) -> &'a Value {
    look_field(view, name).unwrap_or_else(|| {
        panic!("spec §12: a process place exposes a `{name}` group among its exits, got {view:?}")
    })
}

/// The §35.2 state of a neighborhood group, which MUST be one of the six defined values.
fn group_state(view: &Value, name: &str) -> String {
    let group = group(view, name);
    let state = group.get("state").unwrap_or_else(|| {
        panic!(
            "spec §35.2: the `{name}` group carries one of {PERMISSION_STATES:?} as its state, got {group:?}"
        )
    });
    let state = as_text(state);
    assert!(
        PERMISSION_STATES.contains(&state.as_str()),
        "spec §35.2: `{name}` is in one of the six defined states {PERMISSION_STATES:?}, got {state:?}"
    );
    state
}

/// The rows of a `near | to json` stream.
fn neighbors(run: &ono_testkit::Run) -> Vec<Value> {
    let doc = one_doc(run, "near | to json");
    doc.as_sequence()
        .unwrap_or_else(|| {
            panic!("spec §6.2, §29.4: `near` is a stream of SpatialNeighbor values, got {doc:?}")
        })
        .clone()
}

/// A statement that kills `pid` and waits, bounded, until the kernel has taken it out of
/// `/proc` — so the next spatial statement in the same script observes a genuinely gone object
/// rather than a race. No `$` appears in it, so the shell's quoting cannot change its meaning.
fn kill_statement(pid: u32) -> String {
    format!(
        "sh -c \"kill -9 {pid}; for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do [ -d /proc/{pid} ] || break; sleep 0.2; done\""
    )
}

/// A `sleep` process the test owns, orphaned at birth so that killing it removes it from
/// `/proc` for good instead of leaving a zombie this process would have to reap.
struct Orphan {
    pid: u32,
}

impl Orphan {
    fn spawn() -> Self {
        let output = Command::new("sh")
            .arg("-c")
            .arg("sleep 300 >/dev/null 2>&1 </dev/null & echo $!")
            .output()
            .expect("`sh` and `sleep` exist on every test host");
        let pid: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .expect("`echo $!` prints the background pid");
        let orphan = Self { pid };
        orphan.wait_until_visible();
        orphan
    }

    fn pid(&self) -> u32 {
        self.pid
    }

    /// Waits, bounded, until the fixture process is visible in `/proc`.
    fn wait_until_visible(&self) {
        let path = format!("/proc/{}", self.pid);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if Path::new(&path).exists() {
                return;
            }
            sleep(Duration::from_millis(20));
        }
        panic!(
            "the fixture process {} should be visible in /proc within 5s",
            self.pid
        );
    }
}

impl Drop for Orphan {
    fn drop(&mut self) {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("kill -9 {} 2>/dev/null", self.pid))
            .status();
    }
}

// --- identity is stable, and two objects never share one -------------------------------------

#[test]
fn should_return_the_same_spatial_id_when_the_same_place_is_observed_twice() {
    // §42.1 identity test: "Repeated observations of the same live object MUST resolve to the
    // same SpatialId within the provider's advertised identity tier." §43.2 states it as a
    // property: same stable provider identity -> same SpatialId.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nlook --json\nlook --json",
        pid = child.pid()
    ));
    run.assert_success();
    let views = json_docs(&run);
    assert_eq!(
        views.len(),
        2,
        "spec §6.1: each `look --json` answers with a PlaceView, got {:?}",
        run.output()
    );
    assert_eq!(
        spatial_id(&views[0]),
        spatial_id(&views[1]),
        "spec §42.1: two observations of one live process resolve to the same SpatialId"
    );
}

#[test]
fn should_return_the_same_spatial_id_when_the_same_place_is_observed_by_two_shell_invocations() {
    // §3.1: the SpatialId is "stable for as long as the implementation can truthfully identify
    // the same conceptual object" — which outlives one shell process. A per-session counter
    // would satisfy §42.1 within a session and fail here; this test is what forbids it.
    let child = Orphan::spawn();
    let script = format!("enter process {pid}\nlook --json", pid = child.pid());

    let first = ono(&script);
    first.assert_success();
    let second = ono(&script);
    second.assert_success();

    assert_eq!(
        spatial_id(&one_doc(&first, "look --json")),
        spatial_id(&one_doc(&second, "look --json")),
        "spec §3.1, §42.1: the SpatialId of one unchanged process is the same in two sessions"
    );
}

#[test]
fn should_give_different_spatial_ids_to_two_processes_that_share_a_display_name() {
    // §3.1: "The display name is not identity." Both fixtures are `sleep 300`, so a display
    // name, a command line or an executable path used as identity would collide here.
    let first = Orphan::spawn();
    let second = Orphan::spawn();

    let look = |pid: u32| {
        let run = ono(&format!("enter process {pid}\nlook --json"));
        run.assert_success();
        one_doc(&run, "look --json")
    };
    let first_view = look(first.pid());
    let second_view = look(second.pid());

    assert_eq!(
        as_text(look_field(&first_view, "display_name").unwrap_or(&Value::Null)),
        as_text(look_field(&second_view, "display_name").unwrap_or(&Value::Null)),
        "the fixture is two processes with the same display name; that is the point of the test"
    );
    assert_ne!(
        spatial_id(&first_view),
        spatial_id(&second_view),
        "spec §3.1: two different objects never share a SpatialId, however alike they look"
    );
}

#[test]
fn should_carry_a_lifetime_descriptor_rather_than_the_bare_pid_as_process_identity() {
    // §10.2: "PID alone MUST NOT be treated as a persistent spatial identity"; the identity
    // SHOULD include boot identity, pid, start time and pid namespace identity. §3.1 gives the
    // SpatialObject a `lifetime` descriptor. §53 settles it: "PID alone is insufficient
    // identity." An id that is literally the pid is therefore wrong on its face.
    let child = Orphan::spawn();
    let pid = child.pid();
    let run = ono(&format!("enter process {pid}\nlook --json"));
    run.assert_success();
    let view = one_doc(&run, "look --json");

    let id = spatial_id(&view);
    for pid_only in [
        format!("{pid}"),
        format!("process/{pid}"),
        format!("pid:{pid}"),
    ] {
        assert_ne!(
            id, pid_only,
            "spec §10.2, §53: the pid alone is not a spatial identity, got {id:?}"
        );
    }

    let lifetime = look_field(&view, "lifetime").unwrap_or_else(|| {
        panic!("spec §3.1: a SpatialObject carries a `lifetime` descriptor, got {view:?}")
    });
    assert_ne!(
        lifetime,
        &Value::Null,
        "spec §10.2: a live process has a known lifetime — its start time is what makes the \
         identity safe against pid reuse, got {view:?}"
    );
    assert!(
        find_key(lifetime, "started").is_some()
            || find_key(lifetime, "start_time").is_some()
            || find_key(lifetime, "since").is_some(),
        "spec §10.2: the lifetime identity of a local process includes its start time, got {lifetime:?}"
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/…v0.4…md §37.1); un-ignored by the increment that delivers it"]
fn should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id() {
    // §37.1: "Objects from adapters MUST be reconciled with canonical provider identities
    // before appearing as duplicate map nodes." v0.3 already turns `ps` into typed
    // `ono.process/1` values, so the same live process is observable through two providers;
    // the spatial layer must not make it two places.
    let child = Orphan::spawn();
    let pid = child.pid();

    let native = ono(&format!("enter process {pid}\nlook --json"));
    native.assert_success();
    let adapted = ono(&format!(
        "ps -p {pid} | take 1 | enter process\nlook --json"
    ));
    adapted.assert_success();

    assert_eq!(
        spatial_id(&one_doc(&native, "look --json")),
        spatial_id(&one_doc(&adapted, "look --json")),
        "spec §37.1: an adapted object reconciles to the canonical identity of the same process"
    );
}

// --- tombstones ------------------------------------------------------------------------------

#[test]
fn should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited() {
    // §10.3: a recently removed object remains as a short-lived tombstone with a state such as
    // "exited 12s ago". §33.2: the index is a cache, providers are authoritative — the first
    // `look` primes the index, and the second must not repeat its answer.
    let child = Orphan::spawn();
    let pid = child.pid();
    let run = ono(&format!(
        "enter process {pid}\nlook --json\n{kill}\nlook --json",
        kill = kill_statement(pid)
    ));
    let views = json_docs(&run);
    assert_eq!(
        views.len(),
        2,
        "spec §6.1: `look --json` answers at a tombstoned place too, got {:?}",
        run.output()
    );

    let alive = as_text(look_field(&views[0], "state").unwrap_or(&Value::Null));
    assert!(
        alive.contains("running") || alive.contains("sleeping"),
        "the fixture is alive before the kill; that is the baseline this test compares to, got {alive:?}"
    );

    let dead = &views[1];
    assert_eq!(
        spatial_id(dead),
        spatial_id(&views[0]),
        "spec §10.3: the tombstone is the same place, not a new one — the identity is retained"
    );
    let rendered = serde_yaml_ng::to_string(dead).expect("a place view serialises");
    assert!(
        rendered.contains("tombstone") || rendered.contains("exited"),
        "spec §10.3, §33.2: after the process exits the place is a tombstone and says so, \
         instead of the cached live state, got {rendered}"
    );
    assert!(
        !alive_state(dead),
        "spec §33.2: providers are authoritative — a gone process is never still `running`, got {dead:?}"
    );
}

/// Whether a place view still claims a live process state.
fn alive_state(view: &Value) -> bool {
    let state = as_text(look_field(view, "state").unwrap_or(&Value::Null));
    state == "running" || state == "sleeping" || state == "live"
}

#[test]
fn should_distinguish_a_tombstone_from_a_place_that_never_existed() {
    // §40 gives `spatial.not_found` and `spatial.destination_gone` as separate error codes, and
    // §53 settles that unknown/denied is distinct from empty. "It is gone" and "there is no
    // such thing" are different answers and the user must be able to tell them apart.
    let child = Orphan::spawn();
    let pid = child.pid();

    let gone = ono(&format!(
        "enter process {pid}\nlook --json\n{kill}\nenter process {pid}\nlook --json",
        kill = kill_statement(pid)
    ));
    // ADR-0191: the status read here is the refused `enter`'s own. A script that continues past
    // a failed statement exits with its last statement's status (ADR-0008), so a trailing `look`
    // would report the `look`, not the refusal.
    let never = ono("enter process 4000000");

    assert!(
        !never.status().is_success(),
        "spec §40: a place that never existed is `spatial.not_found`, got {:?}",
        never.output()
    );
    assert!(
        never.stderr().contains("spatial.not_found"),
        "spec §40: the error naming an unknown place is `spatial.not_found`, got {:?}",
        never.stderr()
    );

    let gone_report = gone.output();
    assert!(
        !gone_report.contains("spatial.not_found"),
        "spec §10.3, §40: a place visited this session that has since exited is a tombstone, \
         not an unknown place — `spatial.destination_gone` is the answer, got {gone_report}"
    );
    assert!(
        gone_report.contains("tombstone")
            || gone_report.contains("destination_gone")
            || gone_report.contains("exited"),
        "spec §10.3: revisiting a dead place reports the tombstone, got {gone_report}"
    );
}

#[test]
fn should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone() {
    // §10.3: a tombstone "MUST NOT accept actions that require a live object". Reading a dead
    // process's current relationships requires the live object, so `follow parent` — which
    // succeeds while the process lives, because §12 lists `parent` among a process's exits —
    // must be refused once it is a tombstone. §33.2 says the same from the cache side: actions
    // resolve and revalidate against the provider first.
    let child = Orphan::spawn();
    let pid = child.pid();

    let live = ono(&format!("enter process {pid}\nfollow parent\nlook --json"));
    live.assert_success();
    let parent_view = one_doc(&live, "look --json");
    assert_ne!(
        spatial_id(&parent_view),
        {
            let here = ono(&format!("enter process {pid}\nlook --json"));
            here.assert_success();
            spatial_id(&one_doc(&here, "look --json"))
        },
        "spec §6.4: `follow parent` traverses to another place while the process is alive"
    );

    let dead = ono(&format!(
        "enter process {pid}\n{kill}\nfollow parent",
        kill = kill_statement(pid)
    ));
    assert!(
        !dead.status().is_success(),
        "spec §10.3: a tombstone does not accept an action that needs a live object, got {:?}",
        dead.output()
    );
    assert!(
        dead.stderr().contains("spatial."),
        "spec §40: the refusal is a structured `spatial.*` error, got {:?}",
        dead.stderr()
    );
}

#[test]
fn should_never_resolve_a_tombstoned_place_to_a_live_object() {
    // §42.2 reuse-safety test: "the provider MUST prove that identifier reuse cannot silently
    // resolve a tombstoned place to a different object." §43.2 states the property as
    // "PID reuse -> different lifetime SpatialId".
    //
    // Real pid reuse cannot be forced by an unprivileged offline test: the pid space would have
    // to wrap, or the test would need a private pid namespace that may not be available. The
    // property is therefore asserted through the two consequences that make it observable —
    // the tombstoned identity is never reported as live, and no live place anywhere carries the
    // dead lifetime's SpatialId.
    let first = Orphan::spawn();
    let pid = first.pid();

    let run = ono(&format!(
        "enter process {pid}\nlook --json\n{kill}\nlook --json",
        kill = kill_statement(pid)
    ));
    let views = json_docs(&run);
    assert_eq!(views.len(), 2, "two PlaceViews, got {:?}", run.output());
    let dead_id = spatial_id(&views[1]);

    // A second, live process the test owns: it is a different lifetime and must be a different
    // identity, even though the shell would describe it with the same display name and type.
    let second = Orphan::spawn();
    let live = ono(&format!(
        "enter process {pid}\nlook --json",
        pid = second.pid()
    ));
    live.assert_success();
    let live_view = one_doc(&live, "look --json");

    assert_ne!(
        dead_id,
        spatial_id(&live_view),
        "spec §42.2, §43.2: a tombstoned lifetime and a live process never share a SpatialId"
    );
    assert!(
        alive_state(&live_view),
        "the second fixture is alive; that is what makes the comparison meaningful, got {live_view:?}"
    );
}

#[test]
fn should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place() {
    // §20.3 dead destinations: if `back` points at an object that no longer exists, Ono MUST
    // resolve a tombstone if available, inform the user before skipping, and "retain the
    // original trail record". §44.7: history must not confuse old and new identities.
    let child = Orphan::spawn();
    let pid = child.pid();

    let run = ono(&format!(
        "enter process {pid}\nlook --json\nhome\n{kill}\nback\nlook --json\ntrail --json",
        kill = kill_statement(pid)
    ));
    let docs = json_docs(&run);
    assert_eq!(
        docs.len(),
        3,
        "spec §6.1, §6.7: two PlaceViews and one trail document, got {:?}",
        run.output()
    );
    let (visited, returned, trail) = (&docs[0], &docs[1], &docs[2]);

    let visited_id = spatial_id(visited);
    assert_eq!(
        spatial_id(returned),
        visited_id,
        "spec §20.3: `back` returns to the place the trail recorded — as its tombstone, never \
         as some other object"
    );
    assert!(
        !alive_state(returned),
        "spec §20.3: the place `back` returns to is dead and says so, got {returned:?}"
    );

    let rendered = serde_yaml_ng::to_string(trail).expect("a trail serialises");
    assert!(
        rendered.contains(&visited_id),
        "spec §20.3: the original trail record is retained even though its destination died, \
         got {rendered}"
    );
}

#[test]
fn should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced() {
    // §44.7 identity replacement, as §53 settles it: "Old process tombstones; stable service
    // remains; new process has new identity." An unprivileged offline test cannot restart a
    // system service, so the restart is performed on a process the test owns: the old `sleep`
    // exits and a new `sleep` with the same command line takes over. The rule under test is
    // identical — the two lifetimes are two identities and history keeps them apart.
    let old = Orphan::spawn();
    let new = Orphan::spawn();

    let run = ono(&format!(
        "enter process {old}\nlook --json\n{kill}\nenter process {new}\nlook --json\ntrail --json",
        old = old.pid(),
        new = new.pid(),
        kill = kill_statement(old.pid())
    ));
    run.assert_success();
    let docs = json_docs(&run);
    assert_eq!(
        docs.len(),
        3,
        "spec §6.1, §6.7: two PlaceViews and one trail document, got {:?}",
        run.output()
    );
    let (before, after, trail) = (&docs[0], &docs[1], &docs[2]);

    let old_id = spatial_id(before);
    let new_id = spatial_id(after);
    assert_ne!(
        old_id, new_id,
        "spec §10.2, §44.7: the replacement process is a new identity, not the old place \
         wearing a new pid"
    );

    let rendered = serde_yaml_ng::to_string(trail).expect("a trail serialises");
    assert!(
        rendered.contains(&old_id) && rendered.contains(&new_id),
        "spec §6.7, §44.7: the trail records both places distinctly, so history cannot confuse \
         the exited process with its replacement, got {rendered}"
    );
}

// --- permission honesty ----------------------------------------------------------------------

#[test]
fn should_report_permission_denied_rather_than_zero_files_for_another_users_process() {
    // §44.8 and §35.2: a non-root user investigating a process with restricted file descriptors
    // must see unknown/permission_denied, never `files 0`. pid 1 is root-owned on every Linux
    // host and container, so `/proc/1/fd` is unreadable to this user — the v0.2 provider
    // already answers `io.permission_denied` there, and §35.1 forbids the spatial layer from
    // turning that into a count.
    if uid() == 0 {
        // The acceptance container runs as an unprivileged user (docs/ACCEPTANCE.md §2); as
        // root there is nothing this user may not read, so the denial cannot be provoked.
        return;
    }

    let run = ono("enter process 1\nlook --json");
    run.assert_success();
    let view = one_doc(&run, "look --json");

    let state = group_state(&view, "files");
    assert!(
        state == "permission_denied" || state == "unknown",
        "spec §44.8, §42.4: denied information is `permission_denied` or `unknown`, never a \
         false empty collection, got {state:?}"
    );

    let files = group(&view, "files");
    if let Some(count) = find_key(files, "count") {
        assert_ne!(
            count.as_i64(),
            Some(0),
            "spec §35.2: `files permission denied for N process FDs` is the required answer and \
             `files 0` is the forbidden one, got {files:?}"
        );
    }
    assert!(
        !run.stdout().contains("\"files\":0") && !run.stdout().contains("\"files\": 0"),
        "spec §35.2: an unreadable group is never rendered as zero, got {}",
        run.stdout()
    );
}

#[test]
fn should_report_a_real_file_list_for_a_process_this_user_owns() {
    // The other half of §44.8: honesty is only worth something if the readable case really is
    // readable. The fixture is this user's own process with stdin, stdout and stderr open, so
    // `/proc/<pid>/fd` is readable and the group is `available` with real members.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nlook --json",
        pid = child.pid()
    ));
    run.assert_success();
    let view = one_doc(&run, "look --json");

    assert_eq!(
        group_state(&view, "files"),
        "available",
        "spec §35.2: the user's own process has readable file descriptors, so the group is \
         `available`, got {view:?}"
    );
    let files = group(&view, "files");
    let count = find_key(files, "count")
        .and_then(Value::as_i64)
        .or_else(|| {
            find_key(files, "members").and_then(|m| m.as_sequence().map(|s| s.len() as i64))
        })
        .unwrap_or_else(|| {
            panic!("spec §6.1: an available group reports how many objects it holds, got {files:?}")
        });
    assert!(
        count >= 1,
        "spec §35.2: a process with stdin, stdout and stderr open has open files; `available` \
         with nothing in it would be the false-empty answer §42.4 forbids, got {files:?}"
    );
}

#[test]
fn should_name_one_of_the_defined_permission_states_for_every_neighborhood_group() {
    // §35.2: the six states "MUST remain distinct", which means every group carries one of them
    // rather than collapsing absence, denial and emptiness into a missing key or a zero.
    // §54 asks the same as a review question: "Are permission-denied and unknown distinct from
    // empty?"
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nlook --all --json",
        pid = child.pid()
    ));
    run.assert_success();
    let view = one_doc(&run, "look --all --json");

    // §12 fixes the minimum groups a process place exposes.
    for name in [
        "parent",
        "children",
        "sockets",
        "files",
        "namespaces",
        "cgroup",
    ] {
        let state = group_state(&view, name);
        assert!(
            PERMISSION_STATES.contains(&state.as_str()),
            "spec §35.2: `{name}` is in one of {PERMISSION_STATES:?}, got {state:?}"
        );
    }
}

// --- hierarchy versus graph -------------------------------------------------------------------

#[test]
fn should_keep_every_relationship_parent_while_naming_one_canonical_parent() {
    // §11.3: "A spatial object MAY have one canonical parent for `up` while participating in
    // many relationships. … The canonical parent does not claim that other relationships are
    // less real." A process is contained by its parent process, its cgroup, its user and its
    // canonical hierarchy place at once; none of those may be dropped to make `up` simple.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nlook --json",
        pid = child.pid()
    ));
    run.assert_success();
    let view = one_doc(&run, "look --json");

    let canonical = look_field(&view, "canonical_parent").unwrap_or_else(|| {
        panic!("spec §11.3, §33.1: a spatial object names its canonical parent, got {view:?}")
    });
    assert_ne!(
        canonical,
        &Value::Null,
        "spec §11.3: the canonical parent for `up` is deterministic and present, got {view:?}"
    );

    let neighbors = ono(&format!(
        "enter process {pid}\nnear | to json",
        pid = child.pid()
    ));
    neighbors.assert_success();
    let rows = self::neighbors(&neighbors);
    let relations: Vec<String> = rows
        .iter()
        .map(|row| as_text(row.get("relation").unwrap_or(&Value::Null)))
        .collect();
    assert!(
        relations.iter().any(|relation| relation == "parent"),
        "spec §11.2, §12: the parent process remains a real relationship of its own, got {relations:?}"
    );
    assert!(
        relations.iter().any(|relation| relation == "cgroup")
            && relations.iter().any(|relation| relation == "user"),
        "spec §11.3: an object with several containing relations keeps them all — cgroup and \
         owning user are not dropped because a canonical parent was chosen, got {relations:?}"
    );
}

#[test]
fn should_move_to_the_declared_canonical_parent_deterministically_when_going_up() {
    // §11.3: "The canonical parent MUST be deterministic for a given view profile." §43.2:
    // "up never traverses arbitrary graph edges." §6.6 fixes the reading with its own example —
    // `up` from a socket goes to NETWORK/SOCKETS, "not necessarily to the process" that owns
    // it — so the canonical parent is the hierarchy place, not the graph parent, and `up` must
    // land on exactly the id the place declared.
    let child = Orphan::spawn();
    let pid = child.pid();
    let script = format!("enter process {pid}\nlook --json\nup\nlook --json");

    let first = ono(&script);
    first.assert_success();
    let docs = json_docs(&first);
    assert_eq!(
        docs.len(),
        2,
        "spec §6.1: a PlaceView before and after `up`, got {:?}",
        first.output()
    );
    let declared = as_text(look_field(&docs[0], "canonical_parent").unwrap_or(&Value::Null));
    let arrived = spatial_id(&docs[1]);
    assert!(
        arrived == declared || declared.contains(&arrived),
        "spec §11.3: `up` moves to the canonical parent the place declared, got {declared:?} then {arrived:?}"
    );

    let second = ono(&script);
    second.assert_success();
    assert_eq!(
        arrived,
        spatial_id(&json_docs(&second)[1]),
        "spec §11.3: the canonical parent is deterministic, not whichever relation answered first"
    );

    let followed = ono(&format!("enter process {pid}\nfollow parent\nlook --json"));
    followed.assert_success();
    assert_ne!(
        arrived,
        spatial_id(&one_doc(&followed, "look --json")),
        "spec §6.6, §43.2: `up` follows the canonical hierarchy and never a graph edge — the \
         parent process is reached with `follow parent`, not with `up`"
    );
}

// --- relationship explainability and confidence -----------------------------------------------

#[test]
fn should_carry_source_provenance_and_confidence_on_every_relationship_edge() {
    // §11.4: "Every displayed relationship MUST support inspection … The result MUST include
    // relation, source, target, direction, provider, provenance, confidence, observed_at."
    // §29.4 makes `near` an ordinary stream, so piping it into `to json` is the structured
    // selection §11.4 allows in place of `inspect relation @edge-17`.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nnear | to json",
        pid = child.pid()
    ));
    run.assert_success();
    let rows = neighbors(&run);
    assert!(
        !rows.is_empty(),
        "spec §6.2, §12: a process place has neighbors — at least its parent, got {:?}",
        run.output()
    );

    for row in &rows {
        for field in [
            "relation",
            "source",
            "target",
            "direction",
            "provider",
            "provenance",
            "confidence",
            "observed_at",
        ] {
            let value = row.get(field).unwrap_or_else(|| {
                panic!("spec §11.4: an inspectable relationship carries `{field}`, got {row:?}")
            });
            assert_ne!(
                value,
                &Value::Null,
                "spec §11.4: `{field}` is part of the explanation of the edge, not an optional \
                 decoration, got {row:?}"
            );
        }
    }
}

#[test]
fn should_use_the_defined_confidence_vocabulary_and_never_call_an_inferred_edge_exact() {
    // §11.5 fixes the vocabulary and §53 settles the rule behind it: a relationship that was
    // derived rather than observed is "never silently exact". The parent edge is read straight
    // out of the kernel's own record for the process, so it is the one edge in this fixture
    // that may claim `exact`; anything whose provenance says it was inferred may not.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nnear | to json",
        pid = child.pid()
    ));
    run.assert_success();
    let rows = neighbors(&run);

    for row in &rows {
        let confidence = as_text(row.get("confidence").unwrap_or(&Value::Null));
        assert!(
            CONFIDENCES.contains(&confidence.as_str()),
            "spec §11.5: confidence is one of {CONFIDENCES:?}, got {confidence:?} in {row:?}"
        );
        let provenance = serde_yaml_ng::to_string(row.get("provenance").unwrap_or(&Value::Null))
            .expect("a provenance serialises");
        if provenance.contains("inferred") || provenance.contains("heuristic") {
            assert_ne!(
                confidence, "exact",
                "spec §11.5, §53: an inferred edge says so and is never labelled exact, got {row:?}"
            );
        }
    }

    let parent = rows
        .iter()
        .find(|row| as_text(row.get("relation").unwrap_or(&Value::Null)) == "parent")
        .unwrap_or_else(|| {
            panic!("spec §12: `parent` is an exit of a process place, got {rows:?}")
        });
    assert_eq!(
        as_text(parent.get("confidence").unwrap_or(&Value::Null)),
        "exact",
        "spec §11.5: an edge the kernel states directly is `exact`, not a guess, got {parent:?}"
    );
}

#[test]
fn should_resolve_every_edge_endpoint_to_a_node_or_an_explicit_off_map_endpoint() {
    // §42.3 relation integrity: every edge target resolves to a known spatial object, an
    // explicit unresolved endpoint object, or a remote/opaque reference of the correct type —
    // "Dangling internal IDs are invalid." §43.2 states the same as a rendering property.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nmap --json",
        pid = child.pid()
    ));
    run.assert_success();
    let map = one_doc(&run, "map --json");

    let nodes = map
        .get("nodes")
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| panic!("spec §22: a SpatialMap carries `nodes`, got {map:?}"));
    let edges = map
        .get("edges")
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| panic!("spec §22: a SpatialMap carries `edges`, got {map:?}"));
    let known: Vec<String> = nodes
        .iter()
        .map(|node| {
            as_text(node.get("id").unwrap_or_else(|| {
                panic!("spec §22: every MapNode carries its SpatialId as `id`, got {node:?}")
            }))
        })
        .collect();

    for edge in edges {
        for end in ["source", "target"] {
            let id =
                as_text(edge.get(end).unwrap_or_else(|| {
                    panic!("spec §22: a MapEdge names its `{end}`, got {edge:?}")
                }));
            let off_map = find_key(edge, "off_map").is_some()
                || find_key(edge, "unresolved").is_some()
                || find_key(edge, "endpoint_kind").is_some();
            assert!(
                known.contains(&id) || off_map,
                "spec §42.3: the `{end}` of an edge resolves to a rendered node or is an \
                 explicit off-map endpoint; dangling internal ids are invalid, got {edge:?} \
                 against {known:?}"
            );
        }
    }
}

// --- cache honesty ----------------------------------------------------------------------------

#[test]
fn should_expose_how_fresh_the_data_behind_a_place_is() {
    // §33.2: "The index is a cache. Providers remain authoritative." §33.4: "`inspect` MUST
    // reveal source freshness when relevant." §25.3 fixes the vocabulary — event-driven,
    // polled, cached, stale, partial — and forbids presenting stale data as current. Without
    // this the identity guarantees above are unverifiable by the user: a place that looks live
    // may be a cached memory of one.
    let child = Orphan::spawn();
    let run = ono(&format!(
        "enter process {pid}\nlook --json",
        pid = child.pid()
    ));
    run.assert_success();
    let view = one_doc(&run, "look --json");

    let freshness = look_field(&view, "freshness").unwrap_or_else(|| {
        panic!("spec §33.4, §25.3: a place reveals the freshness of its source, got {view:?}")
    });
    let value = as_text(freshness).replace('-', "_");
    let named = FRESHNESS.contains(&value.as_str())
        || FRESHNESS.iter().any(|word| {
            serde_yaml_ng::to_string(freshness)
                .expect("a freshness serialises")
                .replace('-', "_")
                .contains(word)
        });
    assert!(
        named,
        "spec §25.3: freshness is one of {FRESHNESS:?}, got {freshness:?}"
    );
}
