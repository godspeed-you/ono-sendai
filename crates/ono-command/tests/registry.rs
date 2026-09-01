//! The command registry answers exactly what `docs/spec/commands/` declares (spec §27).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use ono_command::{ArgumentMode, Origin, Privilege, Stability};
use ono_parser::ArgMode;
use ono_value::SchemaId;

mod support;
use support::registry;

fn contract_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/commands")
}

/// The entries the contract files declare on disk, counted independently of the crate.
fn declared_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut files = 0;
    for entry in fs::read_dir(contract_directory()).expect("the contract directory must exist") {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_none_or(|extension| extension != "yaml") {
            continue;
        }
        files += 1;
        let text = fs::read_to_string(&path).expect("a readable contract file");
        for line in text.lines() {
            if let Some(id) = line.strip_prefix("  - id: ") {
                ids.insert(id.trim().to_owned());
            }
        }
    }
    assert!(files > 0, "no contract files were found on disk");
    ids
}

#[test]
fn should_load_every_command_the_contract_files_declare() {
    let declared = declared_ids();
    let loaded: BTreeSet<String> = registry()
        .commands()
        .iter()
        .map(|command| command.id().to_owned())
        .collect();

    assert_eq!(
        loaded, declared,
        "the embedded registry must contain exactly the ids `docs/spec/commands/` declares"
    );
    assert_eq!(
        registry().len(),
        declared.len(),
        "every declared command is loaded exactly once"
    );
}

#[test]
fn should_find_a_command_by_its_id() {
    let command = registry()
        .get("ono.process.get")
        .expect("`ono.process.get` is declared by docs/spec/commands/process.yaml");

    assert_eq!(command.verb(), "get");
    assert_eq!(command.target(), Some("process"));
    assert_eq!(command.summary(), "Enumerate or resolve processes.");
    assert_eq!(command.stability(), Stability::Stable);
    assert_eq!(command.argument_mode(), ArgumentMode::Words);
    assert_eq!(command.provider_capability(), Some("process.list"));
    assert_eq!(command.privilege(), Privilege::None);
    assert!(command.is_streaming());
    assert_eq!(command.output().text(), "stream<ono.process/1>");
    assert!(
        command
            .selectors()
            .iter()
            .any(|selector| selector.name() == "pid")
    );
    assert!(
        command
            .options()
            .iter()
            .any(|option| option.name() == "tree")
    );
    assert!(command.examples().contains(&"get process".to_owned()));
}

#[test]
fn should_find_a_command_by_verb_and_target() {
    let command = registry()
        .find("kill", Some("process"))
        .expect("`kill process` is declared");
    assert_eq!(command.id(), "ono.process.kill");

    let targetless = registry()
        .find("where", None)
        .expect("`where` is declared with no target");
    assert_eq!(targetless.id(), "ono.data.where");

    assert!(
        registry().find("get", Some("nonesuch")).is_none(),
        "an undeclared target must not resolve"
    );
}

#[test]
fn should_list_every_command_of_one_verb() {
    let ids: BTreeSet<&str> = registry()
        .by_verb("get")
        .into_iter()
        .map(|command| command.id())
        .collect();

    assert!(ids.contains("ono.process.get"));
    assert!(ids.contains("ono.job.get"));
    assert!(ids.contains("ono.command.get"));
    assert!(
        !ids.contains("ono.process.kill"),
        "a command of another verb must not be listed"
    );
    assert!(
        registry().by_verb("nonesuch").is_empty(),
        "an unknown verb lists nothing"
    );
}

#[test]
fn should_list_every_command_of_one_target() {
    let ids: BTreeSet<&str> = registry()
        .by_target("process")
        .into_iter()
        .map(|command| command.id())
        .collect();

    assert!(ids.contains("ono.process.get"));
    assert!(ids.contains("ono.process.kill"));
    assert!(ids.contains("ono.process.watch"));
    assert!(
        !ids.contains("ono.job.get"),
        "a command of another target must not be listed"
    );
}

#[test]
fn should_filter_the_listing_by_stability() {
    let stable = registry().with_stability(Stability::Stable);
    let planned = registry().with_stability(Stability::Planned);

    assert!(
        stable
            .iter()
            .all(|command| command.stability() == Stability::Stable)
    );
    assert!(
        planned
            .iter()
            .all(|command| command.stability() == Stability::Planned)
    );
    assert!(
        planned.iter().all(|command| command.validation_required()),
        "ADR-0012: every `planned` entry carries `validation_required: true`"
    );
    assert_eq!(
        stable.len() + planned.len() + registry().with_stability(Stability::Experimental).len(),
        registry().len(),
        "every command has exactly one stability level"
    );
}

#[test]
fn should_declare_the_argument_mode_the_parser_implements() {
    for command in registry().commands() {
        let parser_mode = ArgMode::for_head(command.verb());
        let declared = match command.argument_mode() {
            ArgumentMode::Words => ArgMode::Words,
            ArgumentMode::Expression => ArgMode::Expression,
        };
        assert_eq!(
            declared,
            parser_mode,
            "`{}` declares {:?} but the parser reads `{}` as {parser_mode:?}; help and completion \
             would describe a language the parser does not implement",
            command.id(),
            command.argument_mode(),
            command.verb(),
        );
    }
}

#[test]
fn should_declare_every_schema_reference_as_a_schema_id() {
    for command in registry().commands() {
        for io in [command.input(), command.output()] {
            for reference in io.schema_references() {
                let parsed: SchemaId = reference.parse().unwrap_or_else(|_| {
                    panic!("`{reference}` in `{}` is not a schema id", command.id())
                });
                assert!(
                    parsed.name().starts_with("ono."),
                    "`{reference}` in `{}` must live in the reserved `ono.*` namespace (spec §31.5)",
                    command.id()
                );
                assert!(parsed.version() >= 1, "a schema id carries a major version");
            }
        }
    }
}

#[test]
fn should_carry_the_verb_and_target_registries() {
    let verb = registry().verb("get").expect("`get` is a registered verb");
    assert_eq!(verb.semantics(), "Obtain current objects or state.");
    assert!(!verb.is_mutating());

    let target = registry()
        .target("process")
        .expect("`process` is a registered target");
    assert_eq!(target.schema(), Some("ono.process/1"));
    assert_eq!(target.category(), "system");

    let capability = registry()
        .capability("process.list")
        .expect("`process.list` is a registered provider capability");
    assert_eq!(
        capability.summary(),
        "Enumerate processes and their metadata."
    );
}

#[test]
fn should_name_every_capability_and_verb_a_command_refers_to() {
    for command in registry().commands() {
        assert!(
            registry().verb(command.verb()).is_some(),
            "`{}` names verb `{}`, which `docs/spec/verbs.yaml` does not declare",
            command.id(),
            command.verb()
        );
        if let Some(target) = command.target() {
            assert!(
                registry().target(target).is_some(),
                "`{}` names target `{target}`, which `docs/spec/targets.yaml` does not declare",
                command.id()
            );
        }
        if let Some(capability) = command.provider_capability() {
            assert!(
                registry().capability(capability).is_some(),
                "`{}` names capability `{capability}`, which `docs/spec/capabilities.yaml` does \
                 not declare",
                command.id()
            );
        }
    }
}

#[test]
fn should_list_the_targets_a_verb_can_be_applied_to() {
    let targets = registry().targets_for_verb("get");
    assert!(targets.contains(&"process"));
    assert!(targets.contains(&"service"));
    assert!(targets.contains(&"file"));
    assert!(
        targets.windows(2).all(|pair| pair[0] < pair[1]),
        "targets are listed in a stable order"
    );
}

// --- origin (spec §31.64, ADR-0281) -----------------------------------------------------------

#[test]
fn should_attribute_every_embedded_command_to_the_core() {
    for command in registry().commands() {
        assert_eq!(
            command.origin(),
            &Origin::Core,
            "spec §31.64: `{}` ships with Ono, so its registry entry reads `core`",
            command.id()
        );
        assert_eq!(command.origin().to_string(), "core");
    }
}

#[test]
fn should_write_a_package_origin_as_the_package_and_its_version() {
    let origin = Origin::plugin("dev.example.echo", "0.1.0");
    assert_eq!(origin.to_string(), "plugin(dev.example.echo, 0.1.0)");
    assert_eq!(origin.package(), Some("dev.example.echo"));
    assert_eq!(origin.version(), Some("0.1.0"));
    assert_eq!(Origin::Core.package(), None);
}
