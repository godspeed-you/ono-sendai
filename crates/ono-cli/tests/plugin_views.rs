//! A package's view with the full lens (spec §31.27, §31.28; ADR-0572): the host draws the tree
//! the package submits, forwards the keys, and gives the terminal back; redirected output gets
//! the declared fallback.
//!
//! The interactive test drives a real pseudo-terminal, because the effect is on the screen and
//! nowhere else, and asserts on the bytes the terminal receives (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::{Scratch, scratch};

mod support;
use support::{ono_with_plugins, read_until};

/// Starts `ono` interactively on a pseudo-terminal, with the package directory of `home`.
fn interactive_shell_with_plugins(home: &Scratch) -> ono_process::PtySession {
    let mut executor = ono_process::Executor::detached();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", home.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("config").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            home.path().join("state").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_PATH",
            home.path().join("plugins").display().to_string(),
        )
        .current_dir(home.path());
    executor
        .run_pty(&command, ono_process::WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available")
}

const ALTERNATE_SCREEN: &str = "\x1b[?1049h";
const MAIN_SCREEN: &str = "\x1b[?1049l";

/// The example package, installed under `<scratch>/plugins`, requesting `ui.view`.
fn plugin_home(home: &Scratch) {
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"format: kuang-package/1
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
capabilities:
  optional:
    - ui.view
network:
  outbound: none
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = home.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
}

#[test]
fn should_emit_the_declared_fallback_when_output_is_redirected() {
    let home = scratch();
    plugin_home(&home);
    let run = ono_with_plugins(
        &home,
        "grant capability ui.view --plugin dev.example.echo | count; load plugin dev.example.echo; echo:browse --count 2",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("item 1") && run.stdout().contains("item 2"),
        "the items as a stream, deterministic (spec §31.28): {}",
        run.stdout()
    );
    assert!(
        !run.stdout().contains(ALTERNATE_SCREEN),
        "nothing switches screens on a redirected output: {:?}",
        run.stdout()
    );
}

#[test]
fn should_draw_the_packages_tree_forward_the_keys_and_give_the_terminal_back() {
    let home = scratch();
    plugin_home(&home);
    let mut shell = interactive_shell_with_plugins(&home);
    let _ = read_until(&mut shell, "local://", Duration::from_secs(10));

    shell
        .write_all(
            b"grant capability ui.view --plugin dev.example.echo | count; load plugin dev.example.echo\n",
        )
        .expect("input");
    let loaded = read_until(
        &mut shell,
        "loaded dev.example.echo",
        Duration::from_secs(20),
    );
    assert!(
        loaded.contains("loaded dev.example.echo"),
        "the package loads with `ui.view`; saw:\n{loaded:?}"
    );

    shell.write_all(b"echo:browse --count 3\n").expect("input");
    let drawn = read_until(&mut shell, "1/3", Duration::from_secs(20));
    assert!(
        drawn.contains(ALTERNATE_SCREEN),
        "the view is drawn on the alternate screen; the terminal saw:\n{drawn:?}"
    );
    assert!(
        drawn.contains("item 1") && drawn.contains("item 3"),
        "the package's table is on the screen; the terminal saw:\n{drawn:?}"
    );

    // Down arrow: the package moves its cursor and submits a new tree; the status line follows.
    shell.write_all(b"\x1b[B").expect("input");
    let moved = read_until(&mut shell, "2/3", Duration::from_secs(20));
    assert!(
        moved.contains("2/3"),
        "the key reached the package and the new tree was drawn; saw:\n{moved:?}"
    );

    shell.write_all(b"q").expect("input");
    let closed = read_until(&mut shell, "selected: item 2", Duration::from_secs(20));
    let back = closed
        .find(MAIN_SCREEN)
        .unwrap_or_else(|| panic!("the terminal comes back to the main screen; saw:\n{closed:?}"));
    assert!(
        closed[back..].contains("selected: item 2"),
        "what the package emitted after the view is printed on the main screen; saw:\n{:?}",
        &closed[back..]
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}
