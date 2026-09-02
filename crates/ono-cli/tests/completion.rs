//! Completion at the real prompt (spec §15.1, §34; ADR-0252).
//!
//! `ono_command::complete` answers from the contracts; a *value* — the users on this machine, the
//! services of this host — can only come from a provider, and `ValueCompleter` is the seam the
//! contracts left for it. These tests drive a real pseudo-terminal, because completion only
//! happens at one, and assert what appears on the screen.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, WindowSize};
use ono_testkit::{Scratch, scratch};

mod support;
use support::{interactive_shell_in, read_until};

/// Reads from the terminal until `needle` appears or `budget` runs out.
#[test]
fn should_offer_this_machines_users_when_completing_a_user_selector() {
    // Spec §15.1: completion is provider-aware. `ValueCompleter` has been the seam for it since
    // phase D — "the users on this machine, the services of this host" — and the shell installed
    // nothing in it, so `get user <TAB>` could only ever answer from registry metadata, which
    // knows no users. `root` exists on every Linux host, so the fixture is the machine (ADR-0252).
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    shell.write_all(b"get user ro\t").expect("input");
    let seen = read_until(&mut shell, "get user root", Duration::from_secs(10));
    assert!(
        seen.contains("get user root"),
        "spec §15.1: `get user ro<TAB>` completes from the accounts this machine has; saw:\n{seen}"
    );

    shell.write_all(b"\x03").expect("abandon the line");
    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_answer_a_completion_that_no_provider_can_serve_without_waiting_for_one() {
    // The budget of spec §34 is a promise about the *keystroke*, not about the provider: a
    // target nothing can answer must come back as fast as one that is answered instantly, or a
    // container runtime that is not running would be felt on every Tab. Ten completions of a
    // target with no provider on this host, well inside ten times the 50 ms budget.
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    let started = Instant::now();
    for _ in 0..10 {
        shell.write_all(b"get container zz\t").expect("input");
        let _ = read_until(&mut shell, "\u{200b}", Duration::from_millis(120));
        shell.write_all(b"\x03").expect("abandon the line");
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "ten completions took {elapsed:?}; a provider that cannot answer must not be waited for \
         (spec §34, ADR-0252)"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

// --- a contributed command completes before its package is loaded (spec §31.68) ---------------

/// An installed package that declares one command and has never been started.
fn declaring_package(directory: &Scratch) {
    directory.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
network:
  outbound: none
contributions:
  commands: [contributions/commands.yaml]
"#,
    );
    directory.write(
        "plugins/dev.example.echo/contributions/commands.yaml",
        r#"
commands:
  - id: dev.example.echo.command.emit
    verb: get
    target: echo-item
    summary: Emit a counted stream of integers.
    output: stream<int>
    argument_mode: expression
    capabilities: []
    examples:
      - get echo-item --count 3
"#,
    );
}

#[test]
fn should_complete_a_contributed_target_before_its_package_is_loaded() {
    // Spec §31.68: `installed manifest -> registry placeholders`. The package's runtime is not
    // even copied into the fixture — nothing about it may have to run for the shell to know the
    // command exists.
    let directory = scratch();
    declaring_package(&directory);
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", directory.path().display().to_string())
        .env(
            "ONO_PLUGIN_PATH",
            directory.path().join("plugins").display().to_string(),
        )
        .current_dir(directory.path());
    let mut shell = executor
        .run_pty(&command, WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available");
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    shell.write_all(b"get echo-i\t").expect("input");
    let seen = read_until(&mut shell, "get echo-item", Duration::from_secs(10));
    assert!(
        seen.contains("get echo-item"),
        "spec §31.68: a declared contribution completes like any other command; saw:\n{seen}"
    );

    shell.write_all(b"\x03").expect("abandon the line");
    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

// --- v0.4.1 §36.2: the completion budgets are the shell's, and they are enforced ---------------

#[test]
fn should_read_its_budgets_from_the_limits_catalogue() {
    // ADR-0456 recorded the gap this closes: `limits.completion_soft_ms` and
    // `limits.completion_hard_ms` were declared, range-checked and read by nobody, while the
    // completion path carried a 40 ms constant of its own. §52.2 allows one home for a number.
    let completer = ono_cli::complete::ProviderValues::new(Vec::new());
    let (soft, hard) = completer.budgets();

    assert_eq!(
        soft,
        std::time::Duration::from_millis(declared("limits.completion_soft_ms")),
        "v0.4.1 §36.2 and Appendix A fix the soft budget, and the catalogue is where it is written"
    );
    assert_eq!(
        hard,
        std::time::Duration::from_millis(declared("limits.completion_hard_ms")),
        "and the hard one"
    );
    assert!(
        soft < hard,
        "§36.2 states the soft budget as the point at which completion *may* answer and the hard \
         one as the point at which it must stop, so the second cannot come first"
    );
}

#[test]
fn should_stop_discovery_at_the_hard_budget_and_answer_what_it_has() {
    // §36.2: "At the hard budget it MUST stop additional discovery work and return what it has."
    //
    // Asserted as an outcome rather than with a stopwatch: given a budget no provider read can
    // fit inside, the answer is what the completer already had — which, on a cold cache, is
    // nothing at all. A completer that ignored the budget would answer with this machine's
    // accounts, because they are there to be read.
    let registry = ono_command::CommandRegistry::embedded().expect("the embedded contracts parse");
    let command = registry
        .commands()
        .iter()
        .find(|command| command.id() == "ono.user.get")
        .expect("`get user` is a stable command");
    let parameter = command
        .selectors()
        .first()
        .expect("`get user` takes a selector");

    let impatient = ono_cli::complete::ProviderValues::new(Vec::new())
        .budgeted(Duration::from_nanos(1), Duration::from_nanos(1));
    let within_a_nanosecond =
        ono_command::ValueCompleter::complete(&impatient, command, parameter, "");
    assert!(
        within_a_nanosecond.is_empty(),
        "a budget of one nanosecond admits no discovery, so completion answers with what it had — \
         nothing — rather than waiting for the provider. Got {within_a_nanosecond:?}"
    );

    // And with the budgets the catalogue declares, the same question is answered from the machine
    // — so what the nanosecond budget stopped was real work rather than a missing provider.
    let patient = ono_cli::complete::ProviderValues::new(Vec::new());
    let mut offered = ono_command::ValueCompleter::complete(&patient, command, parameter, "");
    for _ in 0..20 {
        if !offered.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        offered = ono_command::ValueCompleter::complete(&patient, command, parameter, "");
    }
    assert!(
        !offered.is_empty(),
        "within its own budget, and with the cache the first read filled, completion offers this \
         machine's accounts (§15.1, ADR-0252)"
    );
}

/// The default the settings catalogue declares for a `limits.*` key, in its base unit.
fn declared(key: &str) -> u64 {
    let spec = ono_cli::settings::spec(key)
        .unwrap_or_else(|| panic!("v0.4.1 §55.1 declares `{key}` in the settings catalogue"));
    match spec.default_value() {
        ono_value::Value::Int(number) => u64::try_from(number).expect("a budget is not negative"),
        other => panic!("`{key}` is a duration in milliseconds, got {other:?}"),
    }
}
