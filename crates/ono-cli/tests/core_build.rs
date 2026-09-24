//! The core build: the object shell without its enhancements (#127, ADR-0910, ADR-0911).
//!
//! These run against the binary this test build produced, so they exist only when that binary is
//! the core one — `cargo test -p ono-cli --no-default-features --features core`. What they pin
//! down is the contract of a compiled-out tier: its commands refuse with `resolve.not_in_build`
//! and the status of a command that cannot be executed, its providers answer through the same
//! `provider.unavailable` a host without the system gives, and nothing that is absent is
//! advertised.

#![cfg(not(feature = "spatial"))]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::{Run, Scratch, Shell};

mod support;

/// A shell that reads and writes nothing outside `home`.
fn core(home: &Scratch, args: &[&str]) -> Run {
    Shell::new()
        .args(args.iter().copied())
        .env("HOME", home.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("xdg").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            home.path().join("state").display().to_string(),
        )
        .env(
            "ONO_CONFIG_DIR",
            home.path().join("ono").display().to_string(),
        )
        .env("NO_COLOR", "1")
        .env_remove("ONO_CONFIG")
        .timeout(Duration::from_secs(60))
        .run()
}

fn script(home: &Scratch, source: &str) -> Run {
    core(home, &["-c", source])
}

#[test]
fn should_name_the_core_profile_and_what_it_leaves_out_when_asked_for_its_version() {
    let home = ono_testkit::scratch();
    let run = core(&home, &["--version"]);
    run.assert_success();
    let lines: Vec<&str> = run.stdout().lines().collect();
    assert_eq!(
        lines.first().copied(),
        Some(format!("ono {}", env!("CARGO_PKG_VERSION")).as_str()),
        "the first line is the one every build prints, so a script reading it reads the same thing"
    );
    let second = lines.get(1).copied().unwrap_or_default();
    assert!(second.starts_with("build: core"), "got {lines:?}");
    for tier in [
        "adapter",
        "change",
        "container",
        "graph",
        "kuang",
        "remote",
        "spatial",
        "systemd",
        "temporal",
    ] {
        assert!(
            second.contains(tier),
            "`{tier}` is named as left out: {second}"
        );
    }
}

#[test]
fn should_refuse_every_compiled_out_command_by_its_tier_and_not_as_a_missing_name() {
    let home = ono_testkit::scratch();
    let cases = [
        ("look", "spatial"),
        ("map", "spatial"),
        ("find place nginx", "spatial"),
        ("enter /tmp", "spatial"),
        ("help here", "spatial"),
        ("trace process 1", "graph"),
        ("get process | where pid == 1 | trace process", "graph"),
        ("get plugin", "KUANG/11"),
        ("load plugin dev.example.tool", "KUANG/11"),
        ("echo:emit", "KUANG/11"),
        ("link host example", "remote"),
        ("get link", "remote"),
        ("get host", "remote"),
        ("at -1h", "temporal"),
        ("timeline", "temporal"),
        ("get process --at -1h", "temporal"),
        ("plan remove file /tmp/nothing-here", "change"),
        ("get recovery", "change"),
        ("adapt ls", "adapters"),
    ];
    for (source, tier) in cases {
        let run = script(&home, source);
        assert_eq!(
            run.status().code(),
            126,
            "`{source}` is found and cannot be executed here: {}",
            run.output()
        );
        let stderr = run.stderr();
        assert!(
            stderr.contains("Ono-Sendai-E0104"),
            "`{source}` refuses with resolve.not_in_build: {stderr}"
        );
        assert!(
            stderr.contains(tier) && stderr.contains("does not include"),
            "`{source}` names its tier ({tier}) and says the build lacks it: {stderr}"
        );
    }
}

#[test]
fn should_refuse_the_remote_flags_as_not_in_this_build() {
    let home = ono_testkit::scratch();
    for flag in [&["--agent"][..], &["--print-peer-key"][..]] {
        let run = core(&home, flag);
        assert_eq!(run.status().code(), 126, "{flag:?}: {}", run.output());
        assert!(
            run.stderr().contains("Ono-Sendai-E0104"),
            "{flag:?}: {}",
            run.stderr()
        );
    }
    let help = core(&home, &["--help"]);
    help.assert_success();
    assert!(
        !help.stdout().contains("--print-peer-key"),
        "the usage text does not offer a flag this build refuses: {}",
        help.stdout()
    );
}

#[test]
fn should_answer_a_compiled_out_provider_as_unavailable_exactly_as_a_host_without_it() {
    let home = ono_testkit::scratch();
    for (source, target, provider) in [
        ("get service", "service", "systemd"),
        ("get journal", "journal", "systemd-journal"),
        ("get container", "container", "container-engine"),
    ] {
        let run = script(&home, source);
        assert_eq!(run.status().code(), 1, "`{source}`: {}", run.output());
        let stderr = run.stderr();
        assert!(
            stderr.contains("Ono-Sendai-E0401"),
            "`{source}` refuses with provider.unavailable: {stderr}"
        );
        assert!(
            stderr.contains(&format!("`{target}` cannot be answered here — {provider}:")),
            "`{source}` is the registry's own refusal, naming the provider: {stderr}"
        );
    }
}

#[test]
fn should_keep_a_compiled_out_provider_s_mutations_bound_and_refused_as_unavailable() {
    let home = ono_testkit::scratch();
    let run = script(&home, "start service nothing-by-this-name");
    assert_ne!(run.status().code(), 0, "{}", run.output());
    assert!(
        run.output().contains("Ono-Sendai-E0401"),
        "`start service` meets the missing provider, not a missing command: {}",
        run.output()
    );
}

#[test]
fn should_not_advertise_a_compiled_out_command_in_help_or_the_registry() {
    let home = ono_testkit::scratch();
    let registry = script(
        &home,
        "get command | where verb == \"map\" or verb == \"trace\" or verb == \"link\" or \
         target == \"plugin\" or target == \"place\" | count | to json",
    );
    registry.assert_success();
    assert_eq!(
        registry.stdout().trim(),
        "[0]",
        "the registry carries no command of a compiled-out tier: {}",
        registry.output()
    );
    let kept = script(
        &home,
        "get command | where target == \"service\" | count | to json",
    );
    kept.assert_success();
    assert_ne!(
        kept.stdout().trim(),
        "[0]",
        "a compiled-out provider keeps its commands: {}",
        kept.output()
    );

    let help = script(&home, "help");
    help.assert_success();
    for absent in [
        "find place",
        "trace process",
        "link host",
        "load plugin",
        "timeline",
    ] {
        assert!(
            !help.stdout().contains(absent),
            "`help` does not list `{absent}`: {}",
            help.stdout()
        );
    }
    let topic = script(&home, "help map");
    assert_eq!(topic.status().code(), 126, "{}", topic.output());
    assert!(
        topic.stderr().contains("Ono-Sendai-E0104"),
        "{}",
        topic.stderr()
    );
}

#[test]
fn should_run_the_object_shell_itself_in_the_core_build() {
    let home = ono_testkit::scratch();
    let run = script(
        &home,
        "get process | take 1 | count | to json\n\
         get interface | where name == \"lo\" | count | to json\n\
         raw echo raw-still-runs\n\
         echo '[{\"n\": 1}, {\"n\": 2}, {\"n\": 3}]' | from json | where n > 1 | count | to json",
    );
    run.assert_success();
    let out = run.stdout();
    assert!(out.contains("raw-still-runs"), "{out}");
    assert!(
        out.contains("[1]"),
        "a process and the loopback interface answer: {out}"
    );
    assert!(
        out.contains("[2]"),
        "records flow through a typed pipeline: {out}"
    );
}

#[test]
fn should_neither_offer_nor_accept_a_setting_of_a_compiled_out_tier() {
    let home = ono_testkit::scratch();
    let listed = script(&home, "get config | select key | to json");
    listed.assert_success();
    for prefix in [
        "\"spatial.",
        "\"temporal.",
        "\"change.",
        "\"recovery.",
        "\"limits.remote_",
    ] {
        assert!(
            !listed.stdout().contains(prefix),
            "`get config` offers no {prefix}… key: {}",
            listed.stdout()
        );
    }
    assert!(
        listed.stdout().contains("\"render.table.max_rows\""),
        "the settings the build reads are still listed: {}",
        listed.stdout()
    );
    let refused = script(&home, "set config spatial.enabled = false");
    assert_eq!(refused.status().code(), 126, "{}", refused.output());
    assert!(
        refused.stderr().contains("Ono-Sendai-E0104"),
        "{}",
        refused.stderr()
    );
}

#[test]
fn should_open_an_interactive_session_without_reaching_for_a_compiled_out_tier() {
    // The full build draws the spatial horizon (`look`) before the first prompt. The core build
    // has no spatial tier, so a session must start at a clean prompt rather than with a refusal
    // it never asked for (ADR-0911).
    let home = ono_testkit::scratch();
    let mut shell = support::interactive_shell_in(&home);
    let seen = support::read_until(&mut shell, "> ", Duration::from_secs(10));
    shell.write_all(b"exit\n").expect("input");
    assert!(
        seen.contains("> "),
        "the session reaches its prompt: {seen:?}"
    );
    assert!(
        !seen.contains("Ono-Sendai-E"),
        "nothing is refused before the user typed anything: {seen:?}"
    );
}

#[test]
fn should_resolve_the_function_namespace_as_the_full_build_does() {
    // `fn:` is the shell's own namespace (ADR-0011), not a package's: the core build answers it
    // exactly as the full build, never as a compiled-out KUANG/11 command (#127, ADR-0911).
    let home = ono_testkit::scratch();
    let undefined = script(&home, "fn:nonesuch");
    assert_eq!(undefined.status().code(), 127, "{}", undefined.output());
    assert!(
        undefined.stderr().contains("Ono-Sendai-E0101"),
        "an undefined function is not found, not compiled out: {}",
        undefined.stderr()
    );
    let defined = script(&home, "fn greet() { echo hello-from-fn }; fn:greet");
    defined.assert_success();
    assert!(
        defined.stdout().contains("hello-from-fn"),
        "{}",
        defined.output()
    );
    let background = script(&home, "fn greet() { echo hello-from-fn }; fn:greet &; jobs");
    assert!(
        !background.output().contains("Ono-Sendai-E0104"),
        "a defined function runs in the background too: {}",
        background.output()
    );
}

#[test]
fn should_neither_show_nor_complete_an_option_of_a_compiled_out_tier() {
    // `--at` is the temporal tier's and `--profile` on `get config` the change tier's; the
    // commands stay in the core build, their options do not (#127, ADR-0911 §5, ADR-0923).
    let home = ono_testkit::scratch();
    let help = script(&home, "help get process");
    help.assert_success();
    assert!(help.stdout().contains("--tree"), "{}", help.stdout());
    assert!(
        !help.stdout().contains("--at"),
        "`help get process` does not offer `--at`: {}",
        help.stdout()
    );
    let config = script(&home, "help get config");
    config.assert_success();
    assert!(
        !config.stdout().contains("--profile"),
        "{}",
        config.stdout()
    );

    let registry = ono_cli::eval::native::registry().expect("the core registry");
    let line = "get process --";
    let offered: Vec<String> = ono_command::complete(
        registry,
        &ono_command::StageContext::from_line(line, line.len()),
        None,
    )
    .iter()
    .map(|candidate| candidate.text().to_owned())
    .collect();
    assert!(offered.iter().any(|text| text == "--tree"), "{offered:?}");
    assert!(
        !offered.iter().any(|text| text == "--at"),
        "completion does not offer `--at`: {offered:?}"
    );

    for source in ["get process --at -1h", "get config --profile"] {
        let run = script(&home, source);
        assert_eq!(run.status().code(), 126, "`{source}`: {}", run.output());
        assert!(
            run.stderr().contains("Ono-Sendai-E0104"),
            "`{source}` refuses as not in this build: {}",
            run.stderr()
        );
    }
}
