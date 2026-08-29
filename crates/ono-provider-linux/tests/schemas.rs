//! What these providers *do*, held against the contracts they advertise.
//!
//! The shape of each contract — its fields, types, nullability, units, identity and default view
//! — is stated by the generated suite of spec §35.3
//! (`crates/ono-cli/tests/provider_conformance.rs`, from `docs/spec/schemas/*.v1.yaml`). What is
//! left here is what a declaration cannot express: the meaning of a field, and that every record
//! a provider emits against a fixed `/proc` tree satisfies the contract it claims.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::sync::Arc;

use common::{ProcFixture, StatFields, drain, records};
use ono_provider_api::{Provider, ProviderRegistry, Query};
use ono_provider_linux::{
    EnvBinding, EnvProvider, FileProvider, IdentityProvider, ProcessProvider, StorageProvider,
    schemas,
};
use ono_value::Value;

#[test]
fn should_declare_the_cpu_field_as_a_percentage() {
    let schema = schemas::require(&schemas::process_id()).expect("the process schema");
    let cpu = schema.field("cpu").expect("the cpu field");
    assert_eq!(
        cpu.unit().map(ono_value::Unit::as_str),
        Some("percent"),
        "the unit is part of the contract: changing it changes the meaning (spec §10.4)"
    );
    assert!(
        cpu.doc().is_some_and(|doc| doc.contains("cpu_window")),
        "the field documents that it is a share over a window, and which field names that \
         window: the same number means one thing over half a second and another over a week \
         (ADR-0232)"
    );
    let window = schema.field("cpu_window").expect("the cpu_window field");
    assert_eq!(
        window.ty().name(),
        "duration",
        "the window `cpu` is measured over is a duration, so a reader can compare two of them"
    );
}

/// Checks every record of a stream against the schema it claims, both directly and through the
/// crate's registry — the drift check spec §36.5 asks `spec-check` to make.
async fn assert_every_record_validates(provider: &dyn Provider, query: &Query) {
    let stream = provider
        .snapshot(query)
        .unwrap_or_else(|error| panic!("{} could not answer: {}", provider.id(), error));
    let collected = drain(stream).await;
    let records = records(&collected);
    assert!(
        !records.is_empty(),
        "{} produced nothing for `{}`, so the conformance case proved nothing",
        provider.id(),
        query.target_name()
    );
    for record in records {
        record.validate().unwrap_or_else(|error| {
            panic!(
                "{} emitted a record outside {}: {error}",
                provider.id(),
                record.schema_id()
            )
        });
        schemas::registry()
            .validate(&record)
            .unwrap_or_else(|error| panic!("{} drifted from the registry: {error}", provider.id()));
        assert!(
            provider
                .schemas()
                .iter()
                .any(|schema| schema.id() == record.schema_id()),
            "{} emitted {} without advertising it",
            provider.id(),
            record.schema_id()
        );
        assert_eq!(
            record.provenance().schema(),
            record.schema_id(),
            "provenance names the contract the record claims"
        );
        assert!(
            record.provenance().observed().is_some(),
            "every observation says when it was made (spec §25.2)"
        );
    }
}

#[tokio::test]
async fn should_emit_only_records_that_satisfy_the_contract_they_claim() {
    let fixture = ProcFixture::new();
    fixture
        .process(101)
        .stat("full", StatFields::default())
        .status(0, 0)
        .cmdline(&["full", "--flag"])
        .exe("/usr/bin/full")
        .cwd("/")
        .cgroup("0::/system.slice/full.service\n");
    // A second process with nothing readable but its stat line, which is the everyday case for
    // somebody else's process.
    fixture.process(102).stat("bare", StatFields::default());

    assert_every_record_validates(
        &ProcessProvider::rooted(fixture.root()),
        &Query::target("process"),
    )
    .await;
    assert_every_record_validates(&ProcessProvider::new(), &Query::target("process").limit(20))
        .await;

    let scratch = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(scratch.path().join("a.txt"), "x").expect("a file");
    std::fs::create_dir(scratch.path().join("sub")).expect("a directory");
    std::os::unix::fs::symlink("a.txt", scratch.path().join("link")).expect("a symlink");
    assert_every_record_validates(
        &FileProvider::new(),
        &Query::target("dir")
            .with(ono_provider_api::Selector::field(
                "path",
                Value::Path(Arc::from(scratch.path())),
            ))
            .option("recursive", Value::Bool(true)),
    )
    .await;

    assert_every_record_validates(&IdentityProvider::new(), &Query::target("user")).await;
    assert_every_record_validates(&IdentityProvider::new(), &Query::target("group")).await;
    assert_every_record_validates(
        &EnvProvider::new([EnvBinding::inherited("PATH", "/usr/bin")]),
        &Query::target("env"),
    )
    .await;
    assert_every_record_validates(&StorageProvider::new(), &Query::target("mount")).await;
    assert_every_record_validates(&StorageProvider::new(), &Query::target("filesystem")).await;
}

#[test]
fn should_register_a_provider_for_every_target_the_command_registry_names() {
    let mut registry = ProviderRegistry::new();
    ono_provider_linux::register(&mut registry, [EnvBinding::inherited("PATH", "/usr/bin")]);

    for target in [
        "process",
        "file",
        "dir",
        "user",
        "group",
        "env",
        "mount",
        "filesystem",
    ] {
        registry
            .provider_for(target)
            .unwrap_or_else(|error| panic!("nothing answers `{target}`: {error}"));
    }
    assert!(
        registry.provider_for("nonesuch").is_err(),
        "a target nothing claims must be reported, not answered with nothing"
    );
}

#[test]
fn should_offer_every_schema_it_produces_through_the_registry() {
    let mut registry = ProviderRegistry::new();
    ono_provider_linux::register(&mut registry, []);
    let ids: Vec<String> = registry
        .schemas()
        .iter()
        .map(|schema| schema.id().to_string())
        .collect();
    for wanted in [
        "ono.process/1",
        "ono.file/1",
        "ono.user/1",
        "ono.group/1",
        "ono.env-var/1",
        "ono.mount/1",
        "ono.filesystem/1",
    ] {
        assert!(
            ids.contains(&wanted.to_owned()),
            "`inspect` cannot show a schema the registry never sees: {wanted}"
        );
    }
}
