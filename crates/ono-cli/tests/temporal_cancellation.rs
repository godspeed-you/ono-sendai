//! Cancelling a long historical query, and what the ledger looks like afterwards (v0.5 §32.6).
//!
//! §32.6 is two sentences and both are observable from outside: "Long historical queries MUST be
//! cancellable using normal Ono cancellation semantics" and "Ctrl-C MUST not corrupt the ledger
//! or leave locks held."
//!
//! The query is long because of the work it asks for, never because a test made it wait: a
//! session standing at a historical coordinate reads its retained events once per event, once per
//! event again, which on the ledger [`seed`] leaves behind is about twenty seconds against the
//! four the interrupt waits. Nothing here asserts a duration as a threshold
//! (issue #21, ADR-0459) — what is asserted is *what stopped*: the answer was never printed, the
//! shell came back with the interrupt status, and the ledger the interrupted session held open is
//! still readable, writable and destroyable by another shell.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::{Scratch, scratch};

/// A query evaluated at a historical coordinate that takes far longer than any interrupt this
/// suite sends: the retained events, read once for every event, once for every event.
///
/// It has to be *historical* work rather than merely slow work, or the test would be about
/// cancellation in general and §32.6 is about cancelling a historical query. Every stage here
/// reads the ledger at the session's coordinate: `find event` is §20.3's search over retained
/// events, and §4.8 lets a pure transform over reconstructed values run in past context where an
/// external program is refused. Nothing in the line touches the live machine.
///
/// It is long by arithmetic rather than by luck. `find event` over an *n*-event ledger, run once
/// per event, run again once per event, is *n*² queries — with the fifty-odd events
/// [`seed`] leaves behind that is about twenty seconds on this machine, against the four the
/// interrupt waits. A smaller ledger would make the test flaky and a larger one slow; the seed is
/// what fixes it.
///
/// It used to be `find file / | select path | count`, which stopped being a historical query when
/// the filesystem provider learned to answer at the coordinate: §14.5 gives no support for
/// historical path structure, so it now refuses in milliseconds and there is nothing left to
/// cancel (ADR-0653, ADR-0780).
///
/// `| to json` matters — it is what makes the *answer* a document on stdout, so "the query never
/// finished" is a fact about what was printed rather than a guess about timing.
const LONG_HISTORICAL_QUERY: &str =
    "find event | each { find event | each { find event | count } | count } | count | to json";

/// What the interactive shell is given: the same query, behind a statement that says out loud
/// that the line is running.
///
/// A terminal test has to know *when* the query started, and the line editor is no witness — it
/// echoes what is typed long before the shell acts on it, and on a loaded machine it can still be
/// reading a line seconds later. An interrupt sent then is an editing key rather than a signal.
/// So the shell says it itself, in a marker only a *running* shell can produce: `under-way-$?` is
/// typed and `under-way-0` comes back, so the echo of the line being typed can never be mistaken
/// for the line being executed (the trick `signals.rs` uses). §4.8 puts `present` in front of it,
/// because `echo` alone is refused in the past.
const INTERACTIVE_QUERY: &str = "present echo under-way-$? ; find event | each { find event | \
     each { find event | count } | count } | count | to json";

/// The marker [`INTERACTIVE_QUERY`] prints when the shell has begun executing it.
const RUNNING: &str = "under-way-0";

/// How long a session must have been alive before `at -2s` names an instant it has evidence for.
///
/// §10.7's in-memory session ledger begins when the session begins, so a shell that has just
/// started has no past to enter (§12.3 makes that a refusal rather than a pretence). Waiting is
/// the precondition of the test, not the thing being measured.
const SETTLE: Duration = Duration::from_secs(4);

/// The canonical store of §31.1 inside `home`, as [`support::recording_shell`] roots it.
fn store(home: &Scratch) -> PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

/// How many Ono mutations [`seed`] makes.
///
/// Each one is §17.2's four-stage lifecycle, so twelve mutations leave about fifty events — the
/// size at which [`LONG_HISTORICAL_QUERY`]'s *n*² reads take twenty seconds rather than one.
const SEEDED_MUTATIONS: usize = 12;

/// Fills `home`'s ledger with the events [`LONG_HISTORICAL_QUERY`] is long because of.
///
/// Real mutations against real processes this test owns, so what the ledger holds afterwards is
/// what §17.1 says a session records about itself — not a fixture written behind the shell's back.
fn seed(home: &Scratch) {
    let mut victims: Vec<std::process::Child> = (0..SEEDED_MUTATIONS)
        .map(|_| support::fixture_process())
        .collect();
    let script = victims
        .iter()
        .map(|victim| format!("stop process {}", victim.id()))
        .collect::<Vec<_>>()
        .join("\n");
    let run = support::recording_shell(home, &script);
    for victim in &mut victims {
        let _ = victim.kill();
        let _ = victim.wait();
    }
    assert!(
        run.status().is_success(),
        "the seeded ledger is the precondition of every test here; the mutations said {:?}",
        run.output()
    );
}

/// A recording shell running the long historical query, as an ordinary child this test can signal.
///
/// [`support::recording_shell`] runs to completion, and a query that has to be interrupted needs
/// a handle instead. The environment is the same one, so the store it opens is the same store.
fn long_historical_query(home: &Scratch) -> Child {
    let root = home.path().display().to_string();
    std::process::Command::new(ono_testkit::ono_binary())
        .args([
            "-c",
            &format!(
                "sleep {}\nat -2s\n{LONG_HISTORICAL_QUERY}",
                SETTLE.as_secs()
            ),
        ])
        .env("NO_COLOR", "1")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("ONO_TEMPORAL_RECORDING_ENABLED", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the ono binary must be built before an integration test runs it")
}

/// Sends SIGINT to `pid`, the way a terminal does when a person presses Ctrl-C.
fn interrupt(pid: u32) {
    let delivered = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .expect("`kill` is available on every test host");
    assert!(delivered.success(), "the interrupt was delivered to {pid}");
}

/// Waits up to `budget` for `child` to end, killing and reaping it if it does not.
///
/// Nothing is left running whatever the assertion says: issue #22 is this repository's record of
/// what a test that walks away from a child it started costs.
fn ended_within(child: &mut Child, budget: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("cannot wait for the interrupted query: {error}"),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// Starts `ono` interactively on a pseudo-terminal, with `home` as its whole world.
///
/// Recording is on, because the events that make the query long were written by [`seed`] in an
/// earlier shell and only a session that opens the same store can read them back (§56.6).
fn interactive_shell(home: &Scratch) -> PtySession {
    let root = home.path().display().to_string();
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("ONO_TEMPORAL_RECORDING_ENABLED", "true")
        .current_dir(home.path());
    executor
        .run_pty(&command, WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available")
}

#[test]
fn should_return_to_the_prompt_with_the_interrupt_status_when_a_long_historical_query_is_cancelled()
{
    // §32.6: "Long historical queries MUST be cancellable using normal Ono cancellation
    // semantics." The normal semantics are the ones every other command already has (spec §18.1,
    // §23.3): Ctrl-C at the terminal ends the command, the status is 128 + SIGINT, and the shell
    // keeps taking commands. `present echo` is how the status is read back without leaving the
    // historical context the query was evaluated in (§4.8).
    let home = scratch();
    seed(&home);
    let mut shell = interactive_shell(&home);
    support::read_until(&mut shell, ">", Duration::from_secs(20));

    // §10.7's session ledger has to reach back before `at` has anywhere to stand.
    std::thread::sleep(SETTLE);
    shell
        .write_all(b"at -2s\n")
        .expect("the terminal accepts a command");
    let entered = support::read_until(&mut shell, "[PAST", Duration::from_secs(20));
    assert!(
        entered.contains("[PAST"),
        "v0.5 §4.2, §4.6: the session must be standing in the past before a historical query can \
         be the thing that is cancelled; saw:\n{entered}"
    );

    shell
        .write_all(format!("{INTERACTIVE_QUERY}\n").as_bytes())
        .expect("the terminal accepts the query");
    // The interrupt has to reach a *running* query, and the marker is the shell saying that it
    // is running one.
    let started = support::read_until(&mut shell, RUNNING, Duration::from_secs(60));
    assert!(
        started.contains(RUNNING),
        "the shell must have begun executing the line before it can be asked to abandon it; \
         saw:\n{started}"
    );
    // Long enough that the reconstruction behind the marker is well under way.
    std::thread::sleep(Duration::from_secs(2));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");

    shell
        .write_all(b"present echo alive-$?\n")
        .expect("the terminal accepts the follow-up");
    let seen = support::read_until(&mut shell, "alive-130", Duration::from_secs(30));
    assert!(
        seen.contains("alive-130"),
        "v0.5 §32.6: the historical query is cancelled with 128 + SIGINT and the shell keeps \
         taking commands; saw:\n{seen}"
    );

    // And the session is still usable, which a shell still grinding through a reconstruction it
    // was told to abandon would not be: it returns to the present and answers again.
    shell
        .write_all(b"now\necho after-$?\n")
        .expect("the terminal accepts another command");
    let after = support::read_until(&mut shell, "after-0", Duration::from_secs(30));
    assert!(
        after.contains("after-0"),
        "v0.5 §4.3, §32.6: `now` is available from anywhere, including from a session whose \
         historical query was interrupted; saw:\n{after}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_leave_the_ledger_unlocked_and_intact_when_a_long_historical_query_is_interrupted() {
    // §32.6: "Ctrl-C MUST not corrupt the ledger or leave locks held." Three observations, each
    // one a thing another shell can do: read the store while the interrupted session holds it
    // open, read it again once the interrupt has landed, and destroy it (§30.8) — which is the
    // one operation a held lock or a corrupt store would defeat.
    let home = scratch();
    seed(&home);
    let mut child = long_historical_query(&home);
    let pid = child.id();

    // The child sleeps, enters the past and starts reconstructing; by now the store is open.
    std::thread::sleep(SETTLE + Duration::from_secs(4));
    assert!(
        store(&home).is_file(),
        "the interrupted session must be holding a real store at `{}` for this test to say \
         anything about locks",
        store(&home).display()
    );

    let during = support::recording_shell(&home, "get recorder | to json");
    during.assert_success();
    let holding = support::single_result(&during);
    assert_eq!(
        support::text(&holding, "health"),
        "healthy",
        "v0.5 §32.6: a running historical query holds no lock that stops another shell reading \
         the same ledger, got {holding:?}"
    );

    interrupt(pid);
    let status = ended_within(&mut child, Duration::from_secs(20)).unwrap_or_else(|| {
        panic!("v0.5 §32.6: a long historical query must end when it is interrupted, and this one did not")
    });
    assert!(
        !status.success(),
        "v0.5 §32.6: an interrupted query does not report success, got {status:?}"
    );

    let printed = child
        .wait_with_output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    assert!(
        !printed
            .lines()
            .any(|line| line.trim_start().starts_with('[')),
        "v0.5 §32.6: the query was cancelled, so its answer was never produced; saw:\n{printed}"
    );

    // The ledger survives the interrupt as a ledger: it opens, it answers, and it is not corrupt.
    let after = support::recording_shell(&home, "timeline --since 24h | to json");
    after.assert_success();
    assert!(
        !after.output().contains("temporal.store_corrupt"),
        "v0.5 §32.6: an interrupted query leaves the ledger intact, got {:?}",
        after.output()
    );

    // And it can still be written to and destroyed, which is what a lock left held would prevent.
    let removed = support::recording_shell(&home, "remove temporal-history --confirm | to json");
    removed.assert_success();
    let result = support::single_result(&removed);
    assert_eq!(
        support::text(&result, "status"),
        "success",
        "v0.5 §30.8, §32.6: the store the interrupted session held is destroyable afterwards, \
         got {result:?}"
    );
    assert!(
        !store(&home).exists(),
        "v0.5 §30.8: `{}` is gone once the removal reported success",
        store(&home).display()
    );
}
