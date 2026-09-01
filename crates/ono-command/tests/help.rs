//! Help is generated from the registry, never hand-written (spec §15.2), and is complete for
//! every command the shell advertises (spec §50).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use ono_command::HelpPage;
use ono_core::ErrorCode;

mod support;
use support::registry;

fn help(topic: &str) -> HelpPage {
    ono_command::help(registry(), None, topic)
        .unwrap_or_else(|error| panic!("`help {topic}` must produce a page: {}", error.message()))
}

#[test]
fn should_render_every_section_spec_15_2_requires_for_a_command() {
    let page = help("get process");
    let text = page.render();

    for required in [
        "get process",
        "Enumerate or resolve processes.",
        "pid",
        "int",
        "Resolve one process by id",
        "tree",
        "bool",
        "stream<ono.process/1>",
        "process.list",
        "stable",
        "get process | where cpu > 20",
    ] {
        assert!(
            text.contains(required),
            "`help get process` must mention `{required}`, rendered:\n{text}"
        );
    }
}

#[test]
fn should_offer_the_same_page_as_structured_data() {
    let page = help("get process");
    let value = page.to_value();
    let map = value.as_map().expect("a help page is a map of fields");

    assert_eq!(
        map.get("id").and_then(|id| id.as_str().ok()),
        Some("ono.process.get")
    );
    assert_eq!(
        map.get("output").and_then(|output| output.as_str().ok()),
        Some("stream<ono.process/1>")
    );
    assert!(
        map.get("examples").is_some(),
        "a script must be able to read the examples without parsing the rendering"
    );
}

#[test]
fn should_generate_complete_help_for_every_command_in_the_registry() {
    for command in registry().commands() {
        let page = ono_command::help(registry(), None, command.id()).unwrap_or_else(|error| {
            panic!("`{}` has no help page: {}", command.id(), error.message())
        });
        let text = page.render();

        assert!(
            !command.summary().trim().is_empty(),
            "`{}` renders help with no summary; spec §50 makes complete help a release requirement",
            command.id()
        );
        assert!(
            text.contains(command.summary()),
            "`{}` renders help without its summary",
            command.id()
        );
        assert!(
            !command.examples().is_empty() && text.contains(&command.examples()[0]),
            "`{}` renders help with no example; spec §50 requires executable examples",
            command.id()
        );
        for parameter in command.selectors().iter().chain(command.options()) {
            assert!(
                !parameter.doc().trim().is_empty(),
                "`{}` declares `{}` without documentation",
                command.id(),
                parameter.name()
            );
            assert!(
                text.contains(parameter.name()) && text.contains(parameter.doc()),
                "`{}` renders help without documenting `{}`",
                command.id(),
                parameter.name()
            );
        }
        assert!(
            text.contains(command.input().text()) && text.contains(command.output().text()),
            "`{}` renders help without its input and output types",
            command.id()
        );
    }
}

#[test]
fn should_generate_help_for_a_verb() {
    let page = help("get");
    let text = page.render();

    assert!(text.contains("Obtain current objects or state."));
    assert!(text.contains("get process"));
    assert!(text.contains("get service"));
    assert!(
        !text.contains("kill process"),
        "a verb page lists that verb's commands only"
    );
}

#[test]
fn should_generate_help_for_a_target() {
    let page = help("process");
    let text = page.render();

    assert!(
        text.contains("ono.process/1"),
        "a target page names its schema"
    );
    assert!(text.contains("get process"));
    assert!(text.contains("kill process"));
}

#[test]
fn should_generate_help_for_a_topic() {
    let verbs = help("verbs").render();
    assert!(verbs.contains("get") && verbs.contains("watch") && verbs.contains("stop"));

    let targets = help("targets").render();
    assert!(targets.contains("process") && targets.contains("socket"));

    let overview = help("").render();
    assert!(
        overview.contains("help") && overview.contains("verbs"),
        "the bare `help` topic must point at the other topics, rendered:\n{overview}"
    );
}

#[test]
fn should_suggest_a_near_miss_for_an_unknown_topic() {
    let error = ono_command::help(registry(), None, "prcoess")
        .expect_err("`prcoess` names nothing in the registry");

    assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    assert!(
        error.help().unwrap_or_default().contains("process"),
        "spec §15.4: an unknown name suggests the near miss, help was: {:?}",
        error.help()
    );
}
