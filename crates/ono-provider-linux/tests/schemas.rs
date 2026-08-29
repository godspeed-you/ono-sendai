//! The provider conformance suite of spec §35.3: the contracts these providers advertise are the
//! ones `docs/spec/schemas/*.v1.yaml` fixes, and every record they emit satisfies the contract it
//! claims.

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
use ono_value::{Schema, SchemaId, Value};

/// One field as `docs/spec/schemas/*.v1.yaml` declares it: name, type, required, nullable.
type FieldSpec = (&'static str, &'static str, bool, bool);

fn assert_contract(
    id: &SchemaId,
    identity: &[&str],
    default_view: &[&str],
    fields: &[FieldSpec],
) -> Arc<Schema> {
    let schema = schemas::require(id).expect("the crate carries the schema it advertises");
    let declared: Vec<(String, String, bool, bool)> = schema
        .fields()
        .iter()
        .map(|field| {
            (
                field.name().to_owned(),
                field.ty().name(),
                field.is_required(),
                field.is_nullable(),
            )
        })
        .collect();
    let wanted: Vec<(String, String, bool, bool)> = fields
        .iter()
        .map(|(name, ty, required, nullable)| {
            ((*name).to_owned(), (*ty).to_owned(), *required, *nullable)
        })
        .collect();
    assert_eq!(
        declared, wanted,
        "{id} must declare exactly the fields, types and nullability of its contract"
    );
    let declared_identity: Vec<&str> = schema.identity().iter().map(|name| &**name).collect();
    assert_eq!(declared_identity, identity, "{id} identity");
    let declared_view: Vec<&str> = schema.default_view().iter().map(|name| &**name).collect();
    assert_eq!(declared_view, default_view, "{id} default view");
    schema
}

#[test]
fn should_declare_the_process_contract_exactly_as_the_registry_fixes_it() {
    assert_contract(
        &schemas::process_id(),
        &["pid", "started"],
        &["pid", "name", "cpu", "memory", "user"],
        &[
            ("pid", "int", true, false),
            ("ppid", "int", false, true),
            ("name", "string", true, false),
            ("command", "list<string>", false, true),
            ("executable", "path", false, true),
            ("user", "ref<ono.user/1>", false, true),
            ("group", "ref<ono.group/1>", false, true),
            (
                "state",
                "enum<running|sleeping|disk-sleep|stopped|tracing-stop|zombie|dead|idle|unknown>",
                true,
                false,
            ),
            ("cpu", "float", false, true),
            ("cpu_window", "duration", false, true),
            ("memory", "bytesize", false, true),
            ("virtual_mem", "bytesize", false, true),
            ("threads", "int", false, true),
            ("started", "timestamp", false, true),
            ("cwd", "path", false, true),
            ("service", "ref<ono.service/1>", false, true),
            ("container", "ref<ono.container/1>", false, true),
            ("pid_namespace", "int", false, true),
        ],
    );
}

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

#[test]
fn should_declare_the_file_contract_exactly_as_the_registry_fixes_it() {
    assert_contract(
        &schemas::file_id(),
        &["device", "inode"],
        &["name", "kind", "size", "modified", "owner"],
        &[
            ("path", "path", true, false),
            ("name", "string", true, false),
            (
                "kind",
                "enum<file|dir|symlink|socket|fifo|device|other>",
                true,
                false,
            ),
            ("size", "bytesize", false, true),
            ("owner", "ref<ono.user/1>", false, true),
            ("group", "ref<ono.group/1>", false, true),
            ("mode", "string", false, true),
            ("modified", "timestamp", false, true),
            ("accessed", "timestamp", false, true),
            ("created", "timestamp", false, true),
            ("inode", "int", false, true),
            ("device", "ref<ono.device/1>", false, true),
            ("target", "path", false, true),
        ],
    );
}

#[test]
fn should_declare_the_identity_contracts_exactly_as_the_registry_fixes_them() {
    assert_contract(
        &schemas::user_id(),
        &["uid"],
        &["uid", "name", "home", "shell"],
        &[
            ("uid", "int", true, false),
            ("name", "string", false, true),
            ("primary_group", "ref<ono.group/1>", false, true),
            ("home", "path", false, true),
            ("shell", "path", false, true),
            ("gecos", "string", false, true),
        ],
    );
    assert_contract(
        &schemas::group_id(),
        &["gid"],
        &["gid", "name", "members"],
        &[
            ("gid", "int", true, false),
            ("name", "string", false, true),
            ("members", "list<string>", false, true),
        ],
    );
    assert_contract(
        &schemas::env_var_id(),
        &["name"],
        &["name", "value", "exported"],
        &[
            ("name", "string", true, false),
            ("value", "string", true, false),
            ("exported", "bool", true, false),
            (
                "source",
                "enum<inherited|config|invocation|shell>",
                true,
                false,
            ),
        ],
    );
}

#[test]
fn should_declare_the_storage_contracts_exactly_as_the_registry_fixes_them() {
    assert_contract(
        &schemas::mount_id(),
        &["target"],
        &["target", "source", "filesystem", "read_only"],
        &[
            ("source", "string", true, false),
            ("target", "path", true, false),
            ("filesystem", "string", true, false),
            ("options", "list<string>", true, false),
            ("read_only", "bool", true, false),
            ("device", "ref<ono.device/1>", false, true),
            // ADR-0236: `mountinfo(5)`'s `shared:N`, which is what makes two mounts peers.
            ("peer_group", "int", false, true),
        ],
    );
    assert_contract(
        &schemas::filesystem_id(),
        &["uuid", "source"],
        &["source", "type", "size", "used", "available", "target"],
        &[
            ("source", "string", true, false),
            ("type", "string", true, false),
            ("uuid", "uuid", false, true),
            ("label", "string", false, true),
            ("target", "path", false, true),
            ("size", "bytesize", false, true),
            ("used", "bytesize", false, true),
            ("available", "bytesize", false, true),
            ("read_only", "bool", false, true),
            ("device", "ref<ono.device/1>", false, true),
        ],
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
