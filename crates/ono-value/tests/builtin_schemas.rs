//! The canonical object schemas of spec §28, asserted field by field so drift is caught.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::sync::Arc;

use ono_value::{FieldType, Schema, SchemaId, Unit, builtin_schemas};

fn schema(id: &str) -> Arc<Schema> {
    let id: SchemaId = id.parse().expect("the test names a well-formed schema id");
    builtin_schemas()
        .get(&id)
        .unwrap_or_else(|| panic!("{id} must be a built-in schema"))
}

/// The declared names of a schema's identity or default-view list.
fn names(list: &[Arc<str>]) -> Vec<&str> {
    list.iter().map(|name| &**name).collect()
}

/// `(name, nullable)` for every field, in declaration order.
fn shape(schema: &Schema) -> Vec<(&str, bool)> {
    schema
        .fields()
        .iter()
        .map(|field| (field.name(), field.is_nullable()))
        .collect()
}

#[test]
fn should_register_every_canonical_schema() {
    let ids: Vec<String> = builtin_schemas()
        .ids()
        .map(std::string::ToString::to_string)
        .collect();

    for expected in [
        "ono.action-result/1",
        "ono.file/1",
        "ono.group/1",
        "ono.interface/1",
        "ono.mount/1",
        "ono.neighbor/1",
        "ono.process/1",
        "ono.route/1",
        "ono.service/1",
        "ono.socket/1",
        "ono.user/1",
    ] {
        assert!(
            ids.iter().any(|id| id == expected),
            "{expected} must be a built-in schema, registry holds {ids:?}"
        );
    }
}

#[test]
fn should_define_the_process_schema_exactly_as_the_spec_does() {
    let process = schema("ono.process/1");

    assert_eq!(
        shape(&process),
        vec![
            ("pid", false),
            ("ppid", true),
            ("name", false),
            ("command", true),
            ("executable", true),
            ("user", true),
            ("group", true),
            ("state", false),
            ("cpu", true),
            // ADR-0232: a share of a CPU means nothing without the window it is a share over,
            // so `ono.process/1` states the window beside the number.
            ("cpu_window", true),
            ("memory", true),
            ("virtual_mem", true),
            ("threads", true),
            ("started", true),
            ("cwd", true),
            ("service", true),
            ("container", true),
            // v0.4 §10.2 makes the pid namespace part of a process's spatial identity: without
            // it a container's pid 1 and the host's pid 1 reduce to one identity (ADR-0134).
            ("pid_namespace", true),
        ]
    );
    assert_eq!(names(process.identity()), ["pid", "started"]);
    assert_eq!(
        names(process.default_view()),
        ["pid", "name", "cpu", "memory", "user"]
    );
    assert_eq!(process.field("pid").unwrap().ty(), &FieldType::Int);
    assert!(process.field("pid").unwrap().is_required());
    assert_eq!(process.field("memory").unwrap().ty(), &FieldType::ByteSize);
    assert_eq!(process.field("cpu").unwrap().unit(), Some(Unit::Percent));
    assert_eq!(
        process.field("command").unwrap().ty(),
        &FieldType::list(FieldType::String)
    );
    assert_eq!(
        process.field("user").unwrap().ty(),
        &FieldType::Ref(SchemaId::new("ono.user", 1))
    );
}

#[test]
fn should_define_the_file_schema_exactly_as_the_spec_does() {
    let file = schema("ono.file/1");

    assert_eq!(
        shape(&file),
        vec![
            ("path", false),
            ("name", false),
            ("kind", false),
            ("size", true),
            ("owner", true),
            ("group", true),
            ("mode", true),
            ("modified", true),
            ("accessed", true),
            ("created", true),
            ("inode", true),
            ("device", true),
            ("target", true),
        ]
    );
    assert_eq!(
        names(file.identity()),
        ["device", "inode"],
        "spec §28.2: path is a reference, not always identity"
    );
    assert_eq!(
        file.field("kind").unwrap().ty(),
        &FieldType::enumeration(&[
            "file", "dir", "symlink", "socket", "fifo", "device", "other"
        ])
    );
    assert_eq!(file.field("path").unwrap().ty(), &FieldType::Path);
}

#[test]
fn should_define_the_service_schema_exactly_as_the_spec_does() {
    let service = schema("ono.service/1");

    assert_eq!(
        shape(&service),
        vec![
            ("name", false),
            ("description", true),
            ("state", false),
            ("substate", true),
            ("pid", true),
            ("enabled", true),
            ("since", true),
            ("provider", false),
            ("unit_file", true),
            // ADR-0239: the units the service manager says this one requires. v0.4 §13 puts
            // dependencies among a service place's groups, and nothing could fill them while
            // the fact lived only on the bus.
            ("dependencies", true),
        ]
    );
    assert_eq!(names(service.identity()), ["provider", "name"]);
}

#[test]
fn should_define_the_socket_schema_exactly_as_the_spec_does() {
    let socket = schema("ono.socket/1");

    assert_eq!(
        shape(&socket),
        vec![
            ("protocol", false),
            ("family", false),
            ("local", true),
            ("remote", true),
            ("state", true),
            ("process", true),
            ("user", true),
            ("inode", true),
        ]
    );
    assert_eq!(names(socket.identity()), ["inode"]);
}

#[test]
fn should_define_the_interface_schema_exactly_as_the_spec_does() {
    let interface = schema("ono.interface/1");

    assert_eq!(
        shape(&interface),
        vec![
            ("name", false),
            ("index", false),
            ("mac", true),
            ("state", false),
            ("mtu", false),
            ("addresses", false),
            ("rx_bytes", true),
            ("tx_bytes", true),
        ]
    );
    assert_eq!(
        interface.field("addresses").unwrap().ty(),
        &FieldType::list(FieldType::IpNetwork)
    );
}

#[test]
fn should_define_the_mount_schema_exactly_as_the_spec_does() {
    let mount = schema("ono.mount/1");

    assert_eq!(
        shape(&mount),
        vec![
            ("source", false),
            ("target", false),
            ("filesystem", false),
            ("options", false),
            ("read_only", false),
            ("device", true),
            // ADR-0236: `mountinfo(5)`'s `shared:N`. §28.6 names no propagation field, and
            // storage.yaml promises `trace mount` shows propagation peers; the group is the
            // fact both mounts state, so it is the mount's and not the trace's.
            ("peer_group", true),
        ]
    );
}

#[test]
fn should_define_the_user_schema_exactly_as_the_spec_does() {
    let user = schema("ono.user/1");

    assert_eq!(
        shape(&user),
        vec![
            ("uid", false),
            ("name", true),
            ("primary_group", true),
            ("home", true),
            ("shell", true),
            ("gecos", true),
        ]
    );
    assert_eq!(names(user.identity()), ["uid"]);
}

#[test]
fn should_define_the_action_result_schema_as_the_pipeline_contract_of_spec_11_5() {
    let action = schema("ono.action-result/1");

    assert_eq!(
        shape(&action),
        vec![
            ("target", false),
            ("operation", false),
            ("status", false),
            ("changed", false),
            ("message", true),
            ("error", true),
            ("duration", false),
        ]
    );
    assert_eq!(
        action.field("status").unwrap().ty(),
        &FieldType::enumeration(&["success", "skipped", "failed"])
    );
    assert_eq!(action.field("duration").unwrap().ty(), &FieldType::Duration);
    assert_eq!(action.field("error").unwrap().ty(), &FieldType::Error);
}

#[test]
fn should_define_a_group_schema_for_the_group_references_the_spec_uses() {
    let group = schema("ono.group/1");

    assert_eq!(
        shape(&group),
        vec![("gid", false), ("name", true), ("members", true)]
    );
    assert_eq!(names(group.identity()), ["gid"]);
}

#[test]
fn should_define_route_and_neighbor_schemas_for_the_network_targets() {
    let route = schema("ono.route/1");
    assert_eq!(
        shape(&route),
        vec![
            ("destination", true),
            ("gateway", true),
            ("interface", true),
            ("source", true),
            ("family", false),
            ("type", true),
            ("scope", true),
            ("protocol", true),
            ("metric", true),
            ("table", true),
        ]
    );
    assert_eq!(
        route.field("destination").unwrap().ty(),
        &FieldType::IpNetwork
    );

    let neighbor = schema("ono.neighbor/1");
    assert_eq!(
        shape(&neighbor),
        vec![
            ("address", false),
            ("mac", true),
            ("interface", false),
            ("family", false),
            ("state", false),
            ("router", true),
            ("updated", true),
        ]
    );
}

#[test]
fn should_list_only_declared_fields_in_every_default_view() {
    for id in builtin_schemas().ids() {
        let schema = builtin_schemas()
            .get(id)
            .expect("the registry lists only what it holds");
        for column in schema.default_view() {
            assert!(
                schema.field(column).is_some(),
                "{id} lists `{column}` in its default view but does not declare it"
            );
        }
        for key in schema.identity() {
            assert!(
                schema.field(key).is_some(),
                "{id} names `{key}` as identity but does not declare it"
            );
        }
    }
}
