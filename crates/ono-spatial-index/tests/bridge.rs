//! Which place a provider record *is*, and when two records are one place.
//!
//! Spec v0.4 §7 (the six domains), §14.3/§14.4 (listener versus connection), §15.4/§15.5
//! (directory versus file), §7.4/§7.7/§18 (block device versus device), §42.1 (identity) and
//! §50's gate for Phase S2: "provider objects can be reconciled into one graph without duplicate
//! identity for known-equal objects".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{bridge, index, record, socket_with};
use std::collections::BTreeSet;

use ono_spatial_core::SpatialType;
use ono_spatial_index::{bridge::reference_key, spatial_type_of};
use ono_value::Value;

#[test]
fn should_place_a_listening_socket_as_a_listener_and_a_peered_one_as_a_connection() {
    // §14.3 and §14.4 make these two different places with different exits, and `ono.socket/1`
    // is the one schema that carries both.
    let listener = socket_with(9_001, Some("listen"), None);
    let connection = socket_with(9_002, Some("established"), Some("10.0.0.5"));
    let datagram = socket_with(9_003, None, None);

    assert_eq!(spatial_type_of(&listener), Some(SpatialType::Listener));
    assert_eq!(spatial_type_of(&connection), Some(SpatialType::Connection));
    assert_eq!(
        spatial_type_of(&datagram),
        Some(SpatialType::Listener),
        "a bound socket with no peer is a place traffic arrives at, which is what §14.3 calls a \
         listener"
    );
}

#[test]
fn should_place_a_directory_as_a_directory_and_every_other_entry_as_a_file() {
    let directory = record(
        "ono.file/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/etc/nginx"))),
            ),
            ("name", Value::string("nginx")),
            ("kind", Value::string("dir")),
            ("device", Value::Int(2049)),
            ("inode", Value::Int(17)),
        ],
    );
    let file = record(
        "ono.file/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new(
                    "/etc/nginx/nginx.conf",
                ))),
            ),
            ("name", Value::string("nginx.conf")),
            ("kind", Value::string("file")),
            ("device", Value::Int(2049)),
            ("inode", Value::Int(18)),
        ],
    );

    assert_eq!(spatial_type_of(&directory), Some(SpatialType::Directory));
    assert_eq!(spatial_type_of(&file), Some(SpatialType::File));
}

#[test]
fn should_place_a_block_device_in_storage_and_a_character_device_in_devices() {
    // §7.4 puts "volumes/devices where known" in STORAGE and §18 makes a block device the thing
    // that backs a filesystem; §7.7 keeps the rest of the kernel's devices in DEVICES.
    let disk = record(
        "ono.device/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/dev/sda2"))),
            ),
            ("name", Value::string("sda2")),
            ("kind", Value::string("block")),
            ("major", Value::Int(8)),
            ("minor", Value::Int(2)),
        ],
    );
    let tty = record(
        "ono.device/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/dev/tty0"))),
            ),
            ("name", Value::string("tty0")),
            ("kind", Value::string("char")),
            ("major", Value::Int(4)),
            ("minor", Value::Int(0)),
        ],
    );

    assert_eq!(spatial_type_of(&disk), Some(SpatialType::BlockDevice));
    assert_eq!(spatial_type_of(&tty), Some(SpatialType::Device));
}

#[test]
fn should_leave_a_value_that_no_domain_holds_without_a_place() {
    // §7 gives a package, an environment variable and a log record no domain. They are values in
    // the typed shell; making them places would be inventing geography.
    for schema in ["ono.package/1", "ono.env-var/1", "ono.log-record/1"] {
        let value = record(schema, &[]);
        assert_eq!(
            spatial_type_of(&value),
            None,
            "{schema} names no canonical domain, so it is not a place"
        );
    }
}

#[test]
fn should_resolve_one_process_seen_through_two_schemas_to_one_place() {
    // §50's gate for Phase S2, and §42.1's identity test across providers rather than across
    // observations: `get process` answers with `ono.process/1` and `inspect process` with
    // `ono.process-detail/1`, and they describe the same process.
    let listed = record(
        "ono.process/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            ("pid_namespace", Value::Int(4_026_531_836)),
        ],
    );
    let inspected = record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            ("pid_namespace", Value::Int(4_026_531_836)),
            (
                "cgroup",
                Value::Path(std::sync::Arc::from(std::path::Path::new(
                    "/system.slice/nginx.service",
                ))),
            ),
        ],
    );

    let mut index = index();
    let mut bridge = bridge();
    let first = bridge.absorb(&mut index, &[listed], common::NOW);
    let second = bridge.absorb(&mut index, &[inspected], common::NOW);
    assert!(first.refused().is_empty(), "{:?}", first.refused());
    assert!(second.refused().is_empty(), "{:?}", second.refused());

    let listed_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the listed process is a place")
        .clone();
    assert!(
        first.added().contains(&listed_id),
        "the first observation put the process in the index"
    );
    assert!(
        second.reconciled().contains(&listed_id),
        "the detail record is the same process, not a second one"
    );
    assert_eq!(
        index.of_type(SpatialType::Process).len(),
        1,
        "one process, one place — whatever schema carried it"
    );
}

#[test]
fn should_resolve_one_disk_seen_through_the_kernel_and_through_an_adapter_to_one_place() {
    // §37.1: "Typed canonical adapter objects may enter spatial index after identity
    // reconciliation." `linux.sysfs` answers with `ono.device/1` and the util-linux `lsblk`
    // adapter with `ono.block-device/1`; both are identified by the device node, so both are
    // /dev/sda2.
    let native = record(
        "ono.device/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/dev/sda2"))),
            ),
            ("name", Value::string("sda2")),
            ("kind", Value::string("block")),
            ("major", Value::Int(8)),
            ("minor", Value::Int(2)),
        ],
    );
    let adapted = record(
        "ono.block-device/1",
        &[
            (
                "path",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/dev/sda2"))),
            ),
            ("name", Value::string("sda2")),
            ("type", Value::string("part")),
            ("size", Value::Int(512)),
            (
                "mountpoints",
                Value::list([Value::Path(std::sync::Arc::from(std::path::Path::new("/")))]),
            ),
            ("read_only", Value::Bool(false)),
            ("removable", Value::Bool(false)),
            ("device_number", Value::string("8:2")),
        ],
    );

    let mut index = index();
    let mut bridge = bridge();
    let first = bridge.absorb(&mut index, &[native], common::NOW);
    let second = bridge.absorb(&mut index, &[adapted], common::NOW);

    assert_eq!(first.added().len(), 1, "{:?}", first.refused());
    assert_eq!(
        second.reconciled(),
        first.added(),
        "one disk, seen twice, is one place — not one per schema: {:?}",
        second.refused()
    );
    assert_eq!(index.of_type(SpatialType::BlockDevice).len(), 1);
}

#[test]
fn should_count_a_value_without_a_place_rather_than_refusing_the_whole_batch() {
    let mut index = index();
    let mut bridge = bridge();
    let absorbed = bridge.absorb(
        &mut index,
        &[
            record(
                "ono.service/1",
                &[
                    ("provider", Value::string("systemd")),
                    ("name", Value::string("nginx.service")),
                    ("state", Value::string("active")),
                ],
            ),
            record("ono.package/1", &[]),
        ],
        common::NOW,
    );

    assert_eq!(absorbed.added().len(), 1);
    assert_eq!(absorbed.unplaced(), ["ono.package/1"]);
    assert!(absorbed.refused().is_empty());
}

#[test]
fn should_find_a_place_again_through_the_key_another_record_names_it_by() {
    // A socket names its owner by pid, a route names its interface by name, a process names its
    // service by unit name. None of those is the object's identity, and all of them must lead
    // back to the same place.
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(
        &mut index,
        &[
            record(
                "ono.process/1",
                &[
                    ("pid", Value::Int(1842)),
                    ("name", Value::string("nginx")),
                    ("state", Value::string("running")),
                    ("started", Value::string("2026-08-10T06:12:00Z")),
                ],
            ),
            record(
                "ono.interface/1",
                &[
                    ("name", Value::string("eth0")),
                    ("index", Value::Int(2)),
                    ("state", Value::string("up")),
                    ("mtu", Value::Int(1500)),
                    ("addresses", Value::list([])),
                ],
            ),
        ],
        common::NOW,
    );

    assert!(bridge.resolve(SpatialType::Process, "1842").is_some());
    assert!(bridge.resolve(SpatialType::Interface, "eth0").is_some());
    assert_eq!(
        bridge.resolve(SpatialType::Interface, "eth0"),
        bridge.resolve(SpatialType::Interface, "2"),
        "a route names an interface by name where the kernel resolved one and by index where it \
         did not; both are the same interface"
    );
    assert!(
        bridge.resolve(SpatialType::Process, "4419").is_none(),
        "a reference to something nobody observed is no edge at all, never a dangling one (§42.3)"
    );
}

#[test]
fn should_find_a_socket_through_a_reference_that_names_the_general_type() {
    // §41.2's relation table says `process.owns_socket` runs to a `Socket`; §14.3 makes the
    // place a `Listener`. Resolving the general name must reach the specialised place.
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(
        &mut index,
        &[socket_with(9_001, Some("listen"), None)],
        common::NOW,
    );

    assert!(
        bridge.resolve(SpatialType::Socket, "9001").is_some(),
        "a listener is a socket"
    );
}

#[test]
fn should_read_a_reference_whether_it_arrives_as_a_record_or_as_the_bare_key() {
    let user = record(
        "ono.user/1",
        &[("uid", Value::Int(1000)), ("name", Value::string("ada"))],
    );
    assert_eq!(
        reference_key(&Value::Record(std::sync::Arc::new(user)), SpatialType::User).as_deref(),
        Some("1000"),
        "a `ref<ono.user/1>` is a record carrying the uid"
    );
    assert_eq!(
        reference_key(&Value::string("nginx.service"), SpatialType::Service).as_deref(),
        Some("nginx.service"),
        "`ono.process/1`'s `service` is the unit name the provider resolved"
    );
    assert_eq!(
        reference_key(&Value::Null, SpatialType::Service),
        None,
        "null is a reference to nothing, never a reference to the empty name"
    );
}

#[test]
fn should_place_every_object_type_the_canonical_geography_serves() {
    // §7 and §41.1: a collection space that declares a member type and a schema is a promise
    // that records of that schema become places of that type. This holds the bridge's table to
    // the geography, so a space cannot quietly become one nothing can fill.
    let served: BTreeSet<SpatialType> = ono_spatial_core::spaces()
        .iter()
        .filter(|space| space.is_served() && space.schema.is_some())
        .filter_map(|space| space.member_type)
        .collect();

    let placed: BTreeSet<SpatialType> = [
        ("ono.process/1", &[("pid", Value::Int(1))][..]),
        ("ono.service/1", &[]),
        ("ono.job/1", &[]),
        ("ono.container/1", &[]),
        ("ono.socket/1", &[("state", Value::string("listen"))]),
        ("ono.socket/1", &[("state", Value::string("established"))]),
        ("ono.interface/1", &[]),
        ("ono.interface-address/1", &[]),
        ("ono.route/1", &[]),
        ("ono.neighbor/1", &[]),
        ("ono.namespace/1", &[]),
        ("ono.filesystem/1", &[]),
        ("ono.mount/1", &[]),
        ("ono.device/1", &[("kind", Value::string("block"))]),
        ("ono.device/1", &[("kind", Value::string("char"))]),
        ("ono.file/1", &[("kind", Value::string("dir"))]),
        ("ono.file/1", &[("kind", Value::string("file"))]),
        ("ono.user/1", &[]),
        ("ono.group/1", &[]),
        ("ono.session/1", &[]),
        ("ono.host/1", &[]),
        ("ono.cgroup/1", &[]),
    ]
    .into_iter()
    .filter_map(|(schema, fields)| spatial_type_of(&record(schema, fields)))
    .collect();

    let missing: Vec<&SpatialType> = served.difference(&placed).collect();
    assert!(
        missing.is_empty(),
        "every member type a served space declares must be a place some record projects to; \
         nothing projects to {missing:?}"
    );
}
