//! Outcome tests for the storage half of the v0.4 spatial systems interface: the STORAGE domain,
//! mount boundaries, and the deliberate separation of the filesystem working directory from the
//! spatial place.
//!
//! Specification: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §7.4 (the
//! STORAGE domain), §15 (storage and filesystem spaces, mount boundaries, directory and file
//! places), §29.1 (`look --json`, `near` and `trail --json` work without a TTY), §30 (filesystem
//! `cd` versus spatial navigation, whole section), §40 (structured spatial errors), §44.3
//! (storage discovery without prior mount names) and §53 (the settled `cd`/place rules:
//! "Entering a directory changes cwd; entering other object types does not" and "Does `cd` always
//! change spatial place? No. Default `storage-only` synchronization").
//!
//! None of this exists in this build: `look`, `find` and `map` are answered by the external
//! programs of the same name or not at all, `home`/`near`/`trail` are unknown commands, and
//! `enter <path>` is refused with `Ono-Sendai-E0102`. Every helper therefore refuses those
//! answers first, so a missing spatial command can never be mistaken for a spatial one.
//!
//! What the tests know about the machine they run on, they learn from the shell itself: the
//! mounts and filesystems come from the v0.2 providers (`get mount`, `get filesystem`) at
//! runtime, never from a hard-coded device or mount name, which is also what invariant 16 asks
//! of the spatial layer — it composes provider facts rather than inventing its own.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ono_testkit::{Shell, scratch};
use serde_yaml_ng::Value;

/// Runs a one-liner in `directory`, so `pwd` has a value the test chose.
fn ono_in(directory: &Path, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .cwd(directory)
        .timeout(Duration::from_secs(30))
        .run()
}

/// Runs a one-liner wherever the suite runs, for the queries that do not touch the cwd.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Fails when the shell did not answer as the owner of the spatial command: a missing command,
/// an unenterable target, or an external program of the same name answering instead.
fn assert_spatial_command(run: &ono_testkit::Run, command: &str) {
    let stderr = run.stderr();
    assert!(
        !stderr.contains("Ono-Sendai-E0101"),
        "spec §6: `{command}` is a spatial command of the shell, not an unknown name; got stderr \
         {stderr:?}"
    );
    assert!(
        !stderr.contains("Ono-Sendai-E0102"),
        "spec §6.3/§15.1: `{command}` resolves a spatial place, and a filesystem path is one; got \
         stderr {stderr:?}"
    );
    assert!(
        !stderr.contains("/usr/bin/look") && !stderr.contains("/usr/bin/find"),
        "spec §6: `{command}` is the spatial command, never the external program of the same \
         name; got stderr {stderr:?}"
    );
}

/// A value rendered as text, for assertions that must not depend on field spelling.
fn rendered(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a spatial value serialises")
}

/// Whether `value` carries `key: expected` anywhere inside it, at any depth.
fn carries(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Mapping(mapping) => mapping.iter().any(|(name, nested)| {
            (name.as_str() == Some(key) && nested.as_str() == Some(expected))
                || carries(nested, key, expected)
        }),
        Value::Sequence(values) => values.iter().any(|nested| carries(nested, key, expected)),
        _ => false,
    }
}

/// The `PlaceView` a `look --json` printed, unwrapped from a one-element stream if the
/// implementation chose to emit it as one.
fn place_view(text: &str, run: &ono_testkit::Run) -> Value {
    let document: Value = serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!(
            "spec §29.1: `look --json` prints a JSON document without a TTY, got {text:?} \
             ({error}); stderr: {:?}",
            run.stderr()
        )
    });
    match document {
        Value::Sequence(mut values) if values.len() == 1 => values.remove(0),
        other => other,
    }
}

/// The current place inside a `PlaceView`: a nested `place` object where the view carries one,
/// the view itself otherwise.
fn place(view: &Value) -> Value {
    view.get("place").cloned().unwrap_or_else(|| view.clone())
}

/// The type the current place reports (§3.1 `object_type`), whatever field carries it.
fn place_type(view: &Value) -> String {
    let place = place(view);
    for field in ["type", "object_type", "kind", "schema", "place_type"] {
        if let Some(Value::String(value)) = place.get(field) {
            return value.to_lowercase();
        }
    }
    panic!(
        "spec §3.1/§6.1: `look --json` names the type of the current place in one of \
         `type`/`object_type`/`kind`/`schema`, got {view:?}"
    )
}

/// Splits the stdout of a script that printed `leading` plain lines, then a JSON document, then
/// `trailing` plain lines. The JSON document sits in the middle so a pretty-printed place view
/// cannot be confused with a `pwd` line.
fn split(
    run: &ono_testkit::Run,
    command: &str,
    leading: usize,
    trailing: usize,
) -> (Vec<String>, Vec<String>, Value) {
    assert_spatial_command(run, command);
    let lines: Vec<String> = run
        .stdout()
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    assert!(
        lines.len() > leading + trailing,
        "the script prints {leading} line(s), the place as JSON, then {trailing} line(s); got \
         stdout {:?} and stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let before = lines[..leading].to_vec();
    let after = lines[lines.len() - trailing..].to_vec();
    let document = lines[leading..lines.len() - trailing].join("\n");
    (before, after, place_view(&document, run))
}

/// A structured spatial failure (§40): the command exists, refuses, and names which of the §40
/// conditions occurred. The shell's error model carries a dotted name beside its numeric code
/// (v0.2 §43), so the name is what a caller can act on.
fn assert_spatial_error(run: &ono_testkit::Run, command: &str, error_name: &str) {
    assert_spatial_command(run, command);
    assert!(
        !run.status().is_success(),
        "spec §40: `{command}` fails with a structured error, got exit {:?} and stdout {:?}",
        run.status(),
        run.stdout()
    );
    let seen = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        seen.contains(error_name),
        "spec §40: `{command}` reports `{error_name}`, got {seen:?}"
    );
}

/// The values a `| to json` stage printed.
fn rows(run: &ono_testkit::Run, command: &str) -> Vec<Value> {
    assert_spatial_command(run, command);
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`{command} | to json` prints JSON, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §29.4: `{command}` is an ordinary stream, got {text:?}"))
        .clone()
}

/// One mount as the v0.2 provider reports it.
#[derive(Debug, Clone)]
struct Mount {
    target: String,
    source: String,
    filesystem: String,
}

/// The mounts of this host, straight from the v0.2 provider — the facts the spatial layer has to
/// compose rather than replace (invariant 16).
fn mounts() -> Vec<Mount> {
    let run = ono("get mount | to json");
    run.assert_success();
    let document: Value =
        serde_yaml_ng::from_str(run.stdout().trim()).expect("`get mount | to json` prints JSON");
    document
        .as_sequence()
        .expect("`to json` prints an array")
        .iter()
        .filter_map(|row| {
            Some(Mount {
                target: row.get("target")?.as_str()?.to_owned(),
                source: row.get("source")?.as_str()?.to_owned(),
                filesystem: row.get("filesystem")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// A mount other than `/` that hangs directly below it, so entering it from `/` crosses exactly
/// one boundary. Every Linux host has at least `/proc`; a host that has none cannot be asked the
/// question this test asks, and says so instead of pretending.
fn boundary_mount() -> Option<Mount> {
    mounts().into_iter().find(|mount| {
        let path = Path::new(&mount.target);
        mount.target != "/"
            && path.parent() == Some(Path::new("/"))
            && path.is_dir()
            && std::fs::read_dir(path).is_ok()
    })
}

/// A `sleep` child: a process place that is emphatically not a directory.
struct SleepChild(Child);

impl SleepChild {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("30")
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
}

impl Drop for SleepChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The canonical form of a scratch path, so a comparison against `pwd` cannot fail over a
/// symlinked temporary directory.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().expect("the scratch directory exists")
}

#[test]
fn should_change_the_working_directory_and_the_place_together_when_entering_a_directory() {
    // §30.2 and §53: "Entering a directory place changes both spatial place and cwd to that
    // directory." Both halves are asserted from the same run, because the place is session state
    // and a second process would not have it (§29.2, §46).
    let directory = scratch();
    let home = canonical(directory.path());
    let target = home.join("logs");
    std::fs::create_dir(&target).expect("create the directory to enter");

    let run = ono_in(
        &home,
        &format!("pwd; enter {}; look --json; pwd", target.display()),
    );
    let (before, after, view) = split(&run, "enter <directory>; look --json", 1, 1);
    assert_eq!(
        before[0],
        home.display().to_string(),
        "the run starts in the scratch directory"
    );
    assert_eq!(
        after[0],
        target.display().to_string(),
        "spec §30.2/§53: entering a directory place changes the working directory to it"
    );
    let shown = rendered(&place(&view));
    assert!(
        shown.contains(&target.display().to_string()),
        "spec §30.2: entering a directory place changes the spatial place to that directory, got \
         {shown}"
    );
    let kind = place_type(&view);
    assert!(
        kind.contains("dir"),
        "spec §15.4: the place is a directory place, got {kind:?}"
    );
}

#[test]
fn should_leave_the_working_directory_untouched_when_entering_a_process() {
    // §30.2: "Entering non-filesystem places MUST NOT change cwd", settled again in §53. The
    // place moves to the process; `pwd` before and after is the same directory.
    let directory = scratch();
    let home = canonical(directory.path());
    let child = SleepChild::spawn();

    let run = ono_in(
        &home,
        &format!("pwd; enter process {}; look --json; pwd", child.pid()),
    );
    let (before, after, view) = split(&run, "enter process; look --json", 1, 1);
    assert_eq!(
        before[0], after[0],
        "spec §30.2/§53: entering a process leaves the working directory where it was"
    );
    assert_eq!(
        after[0],
        home.display().to_string(),
        "spec §30.2: the working directory is still the one the run started in"
    );
    let kind = place_type(&view);
    assert!(
        kind.contains("process"),
        "spec §2 invariant 3: the movement still produced a new spatial context, got {kind:?}"
    );
    assert!(
        rendered(&place(&view)).contains(&child.pid().to_string()),
        "spec §12: the place is the process the test entered (pid {})",
        child.pid()
    );
}

#[test]
fn should_leave_the_working_directory_untouched_when_entering_a_file_or_a_socket() {
    // §53: "Entering a directory changes cwd; entering other object types does not." A file is
    // the sharp case — it is a filesystem object with a path, and entering it still must not
    // change the working directory, because it is not a directory (§15.5). A socket is the
    // second case the same rule covers (§14.3).
    let directory = scratch();
    let home = canonical(directory.path());
    let file = directory.write("nginx.conf", b"listen 8080;\n");
    let file = canonical(&file);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("the bound address").port();

    let opened = ono_in(
        &home,
        &format!("pwd; enter file {}; look --json; pwd", file.display()),
    );
    let (before, after, view) = split(&opened, "enter file; look --json", 1, 1);
    assert_eq!(
        before[0], after[0],
        "spec §53: entering a file is not entering a directory, so the working directory stands"
    );
    assert_eq!(
        after[0],
        home.display().to_string(),
        "spec §30.2: the working directory is still the one the run started in"
    );
    assert!(
        rendered(&place(&view)).contains(&file.display().to_string()),
        "spec §15.5: the place is the file the test entered, got {}",
        rendered(&place(&view))
    );

    let socket = ono_in(
        &home,
        &format!("pwd; enter socket {port}; look --json; pwd"),
    );
    let (before, after, view) = split(&socket, "enter socket; look --json", 1, 1);
    assert_eq!(
        before[0], after[0],
        "spec §30.2: entering a socket leaves the working directory where it was"
    );
    assert!(
        place_type(&view).contains("socket"),
        "spec §14.3: the place is the socket the test entered (:{port})"
    );
}

#[test]
fn should_move_the_place_with_cd_when_the_current_place_is_a_directory() {
    // §30.3 with the default `spatial.follow_cwd = storage-only`: while the current place is
    // inside the filesystem/storage family, `cd` updates the spatial place to the corresponding
    // directory place. §30.1 keeps `cd` itself what it has always been.
    let directory = scratch();
    let home = canonical(directory.path());
    let first = home.join("etc");
    let second = home.join("var");
    std::fs::create_dir(&first).expect("create the first directory");
    std::fs::create_dir(&second).expect("create the second directory");

    let run = ono_in(
        &home,
        &format!(
            "enter {}; cd {}; look --json; pwd",
            first.display(),
            second.display()
        ),
    );
    let (_, after, view) = split(&run, "cd inside storage; look --json", 0, 1);
    assert_eq!(
        after[0],
        second.display().to_string(),
        "spec §30.1: `cd` changes the working directory"
    );
    let shown = rendered(&place(&view));
    assert!(
        shown.contains(&second.display().to_string()),
        "spec §30.3: with the default `storage-only` synchronisation, `cd` inside the storage \
         family moves the place with it, got {shown}"
    );
    assert!(
        !shown.contains(&first.display().to_string()),
        "spec §30.3: the place left the directory the run entered first, got {shown}"
    );
}

#[test]
fn should_keep_the_place_when_cd_moves_while_the_place_is_a_process() {
    // §30.3, the other half of the same default: `spatial.follow_cwd = storage-only` exists so a
    // `cd` cannot throw the user out of a process investigation. The working directory moves, the
    // place does not.
    let directory = scratch();
    let home = canonical(directory.path());
    let elsewhere = home.join("var");
    std::fs::create_dir(&elsewhere).expect("create the directory to cd into");
    let child = SleepChild::spawn();

    let run = ono_in(
        &home,
        &format!(
            "enter process {}; cd {}; look --json; pwd",
            child.pid(),
            elsewhere.display()
        ),
    );
    let (_, after, view) = split(&run, "cd while investigating; look --json", 0, 1);
    assert_eq!(
        after[0],
        elsewhere.display().to_string(),
        "spec §30.1: `cd` changes the working directory wherever the place is"
    );
    let kind = place_type(&view);
    assert!(
        kind.contains("process"),
        "spec §30.3: the default `storage-only` synchronisation leaves a process investigation \
         where it is, got {kind:?}"
    );
    assert!(
        rendered(&place(&view)).contains(&child.pid().to_string()),
        "spec §30.3: the place is still pid {}, got {}",
        child.pid(),
        rendered(&place(&view))
    );
}

#[test]
fn should_keep_the_spatial_place_out_of_pwd_when_the_place_is_not_a_directory() {
    // §30.4: "Spatial place MUST NOT be encoded into `PWD`. `PWD` remains the filesystem working
    // directory." External commands read `PWD`, so a place leaking into it would break every one
    // of them (§44.10).
    let directory = scratch();
    let home = canonical(directory.path());
    let child = SleepChild::spawn();

    let run = ono_in(
        &home,
        &format!("enter process {}; look --json; echo $PWD", child.pid()),
    );
    let (_, after, view) = split(&run, "enter process; look --json", 0, 1);
    assert_eq!(
        after[0],
        home.display().to_string(),
        "spec §30.4: `PWD` is the filesystem working directory, never the spatial place"
    );
    assert!(
        place_type(&view).contains("process"),
        "spec §30.4: the place is the process, and `PWD` still says otherwise on purpose"
    );
}

#[test]
fn should_list_the_mounts_the_providers_report_when_walking_into_the_storage_domain() {
    // §44.3 and §7.4: without prior mount names, `home -> storage -> mounts` must reach the
    // mounts of this host. The expectation is read from the v0.2 provider at runtime, never from
    // a hard-coded device: invariant 16 says the spatial layer composes provider facts, so every
    // mount place it shows must be one `get mount` also reports, and the root mount must be
    // among them.
    let Some(boundary) = boundary_mount() else {
        eprintln!("skipped: this host reports no mount below `/` to discover");
        return;
    };
    let run = ono("home; enter storage; enter mounts; near --all | to json");
    let neighbors = rows(&run, "near --all");
    assert!(
        !neighbors.is_empty(),
        "spec §44.3: the mounts place lists the host's mounts, got {:?}",
        run.stdout()
    );
    let shown = neighbors.iter().map(rendered).collect::<String>();
    for target in ["/", boundary.target.as_str()] {
        assert!(
            shown.contains(target),
            "spec §7.4/§44.3: the mounts of the STORAGE domain include {target:?}, got {shown}"
        );
    }
    let known: Vec<String> = mounts().into_iter().map(|mount| mount.target).collect();
    for neighbor in &neighbors {
        let Some(target) = neighbor
            .get("target")
            .or_else(|| neighbor.get("path"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        assert!(
            known.iter().any(|mount| mount == target),
            "spec §2 invariant 16: every mount place comes from the mount provider, and \
             {target:?} is not among {known:?}"
        );
    }
}

#[test]
fn should_list_the_filesystems_the_providers_report_when_walking_into_the_storage_domain() {
    // §7.4: `STORAGE` MUST provide access to filesystems as well as mounts, and they are
    // different places built from the different v0.2 objects (`get filesystem`, `get mount`).
    let filesystems = ono("get filesystem | to json");
    filesystems.assert_success();
    let known: Vec<String> = serde_yaml_ng::from_str::<Value>(filesystems.stdout().trim())
        .expect("`get filesystem | to json` prints JSON")
        .as_sequence()
        .expect("`to json` prints an array")
        .iter()
        .filter_map(|row| Some(row.get("target")?.as_str()?.to_owned()))
        .collect();
    assert!(
        !known.is_empty(),
        "the v0.2 filesystem provider answers on this host"
    );

    let run = ono("home; enter storage; enter filesystems; near --all | to json");
    let neighbors = rows(&run, "near --all");
    assert!(
        !neighbors.is_empty(),
        "spec §7.4: the filesystems place lists the host's filesystems, got {:?}",
        run.stdout()
    );
    let shown = neighbors.iter().map(rendered).collect::<String>();
    assert!(
        known.iter().any(|target| shown.contains(target)),
        "spec §7.4/invariant 16: the filesystem places are the ones the provider reports \
         ({known:?}), got {shown}"
    );
}

#[test]
fn should_show_the_source_device_and_filesystem_when_the_place_is_a_mount_boundary() {
    // §15.3: crossing a mount boundary MUST be discoverable, and the boundary the spec draws
    // carries the local path, the filesystem and the source. The expected values come from
    // `get mount` at runtime, so the test names no device of the developer's machine.
    let Some(mount) = boundary_mount() else {
        eprintln!("skipped: this host reports no mount below `/` to enter");
        return;
    };
    let run = ono(&format!("enter {}; look --json", mount.target));
    let (_, _, view) = split(&run, "enter <mount>; look --json", 0, 0);
    let shown = rendered(&view);
    assert!(
        carries(&view, "filesystem", &mount.filesystem),
        "spec §15.3: the boundary names the filesystem the provider reports ({}), got {shown}",
        mount.filesystem
    );
    assert!(
        carries(&view, "source", &mount.source),
        "spec §15.3: the boundary names the source the provider reports ({}), got {shown}",
        mount.source
    );
    assert!(
        shown.contains(&mount.target),
        "spec §15.3: the boundary names the local path {}, got {shown}",
        mount.target
    );
}

#[test]
fn should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory() {
    // §44.3 asks for the traversal itself: enter the mount and traverse into the mounted
    // directory. §3.2 fixes what that must leave behind — "Crossing a scope boundary MUST be
    // observable in the navigation trail and prompt/HUD" — and invariant 18 repeats it for mount
    // boundaries. So the step from `/` into the mount is a trail step that names the crossing.
    let Some(mount) = boundary_mount() else {
        eprintln!("skipped: this host reports no mount below `/` to traverse into");
        return;
    };
    let run = ono(&format!("enter /; enter {}; trail --json", mount.target));
    assert_spatial_command(&run, "trail --json");
    let document: Value = serde_yaml_ng::from_str(run.stdout().trim()).unwrap_or_else(|error| {
        panic!(
            "spec §29.1: `trail --json` prints the trail as JSON, got {:?} ({error}); stderr: {:?}",
            run.stdout(),
            run.stderr()
        )
    });
    let steps = document
        .as_sequence()
        .cloned()
        .or_else(|| document.get("steps").and_then(Value::as_sequence).cloned())
        .unwrap_or_else(|| {
            panic!(
                "spec §6.7/§20.1: `trail --json` returns the navigation steps, got {:?}",
                run.stdout()
            )
        });
    let crossing = steps
        .iter()
        .find(|step| rendered(step).contains(&mount.target))
        .unwrap_or_else(|| {
            panic!(
                "spec §44.3: the trail records the traversal into {}, got {:?}",
                mount.target,
                run.stdout()
            )
        });
    let shown = rendered(crossing).to_lowercase();
    assert!(
        shown.contains("boundary")
            || shown.contains("scope")
            || shown.contains("crossing")
            || shown.contains(&mount.filesystem.to_lowercase()),
        "spec §3.2/§15.3/invariant 18: crossing the mount boundary into {} is visible in the \
         trail, got {shown}",
        mount.target
    );
}

#[test]
fn should_refuse_a_path_that_does_not_exist_with_a_structured_error() {
    // §40: a spatial operation that cannot resolve its target emits `spatial.not_found`. An
    // empty place would be the wrong answer twice over — it would claim a location that does not
    // exist, and it would hide the failure from a script (§29.3).
    let directory = scratch();
    let home = canonical(directory.path());
    let missing = home.join("no-such-directory");

    let refused = ono_in(&home, &format!("enter {}", missing.display()));
    assert_spatial_error(&refused, "enter <missing path>", "spatial.not_found");

    let after = ono_in(
        &home,
        &format!(
            "enter {}; enter {}; look --json; pwd",
            home.display(),
            missing.display()
        ),
    );
    let (_, tail, view) = split(&after, "enter <missing path>; look --json", 0, 1);
    assert_eq!(
        tail[0],
        home.display().to_string(),
        "spec §40: a refused `enter` moves neither the place nor the working directory"
    );
    assert!(
        rendered(&place(&view)).contains(&home.display().to_string()),
        "spec §40: the place after the refusal is the one the run was in, not an empty place, got \
         {}",
        rendered(&place(&view))
    );
}

#[test]
fn should_summarize_a_large_directory_instead_of_enumerating_it() {
    // §15.4: "The spatial renderer MUST NOT enumerate huge directories by default. It SHOULD
    // cluster or summarize when entry counts exceed the view budget", and §3.6 gives the
    // structured form of that summary — a `hidden_count` beside the groups. Invariant 9 is the
    // same rule from the other side: the horizon is bounded.
    let directory = scratch();
    let home = canonical(directory.path());
    let crowded = home.join("many");
    std::fs::create_dir(&crowded).expect("create the crowded directory");
    for index in 0..400 {
        std::fs::write(crowded.join(format!("entry-{index:04}")), b"x")
            .expect("create a directory entry");
    }

    let run = ono_in(&home, &format!("enter {}; look --json", crowded.display()));
    let (_, _, view) = split(&run, "enter <large directory>; look --json", 0, 0);
    let shown = rendered(&view);
    let listed = (0..400)
        .filter(|index| shown.contains(&format!("entry-{index:04}")))
        .count();
    assert!(
        listed < 400,
        "spec §15.4: a directory of 400 entries is not enumerated by default, got all {listed} of \
         them in {shown}"
    );
    let lowered = shown.to_lowercase();
    assert!(
        lowered.contains("hidden") || lowered.contains("cluster") || shown.contains("400"),
        "spec §15.4/§3.6: what is not shown is summarised — a cluster or a hidden count — rather \
         than silently dropped, got {shown}"
    );
}
