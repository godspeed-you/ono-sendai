//! The exact relations the bridge composes from provider facts.
//!
//! Spec v0.4 §3.5 (every field an edge carries), §11.2 (the relationship graph), §11.3 (the
//! canonical parent), §11.4 (explainability), §11.5 (confidence), §12–§18 (the relations each
//! space must expose), §32 (what a relationship provider declares) and §42.3 (relation
//! integrity: no dangling internal ids).
//!
//! §2.16 is the rule every case here holds the bridge to: it composes provider data. Every edge
//! below exists because a record said so, and carries that record's provenance.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{NOW, bridge, index, record, socket_with};
use ono_spatial_core::{Confidence, SpatialId, SpatialType};
use ono_spatial_index::{ProviderBridge, SpatialIndex};
use ono_value::Value;

/// Registers `records` and returns the index and the bridge that read them.
fn absorb(records: Vec<ono_value::RecordValue>) -> (SpatialIndex, ProviderBridge) {
    let mut index = index();
    let mut bridge = bridge();
    let outcome = bridge.absorb(&mut index, &records, NOW);
    assert!(
        outcome.refused().is_empty(),
        "the fixture records must all become places: {:?}",
        outcome.refused()
    );
    (index, bridge)
}

/// The ids `id` is joined to by the relation `relation`, in index order.
fn along(index: &SpatialIndex, id: &SpatialId, relation: &str) -> Vec<SpatialId> {
    index
        .get(id)
        .expect("the object is in the index")
        .edges()
        .iter()
        .filter(|edge| edge.relation().as_str() == relation)
        .filter_map(|edge| edge.other_end(id).cloned())
        .collect()
}

fn process(pid: i64, name: &str, extra: &[(&str, Value)]) -> ono_value::RecordValue {
    let mut fields = vec![
        ("pid", Value::Int(i128::from(pid))),
        ("name", Value::string(name)),
        ("state", Value::string("running")),
        ("started", Value::string("2026-08-10T06:12:00Z")),
        ("pid_namespace", Value::Int(4_026_531_836)),
    ];
    fields.extend(extra.iter().cloned());
    record("ono.process/1", &fields)
}

fn user(uid: i64, name: &str) -> ono_value::RecordValue {
    record(
        "ono.user/1",
        &[
            ("uid", Value::Int(i128::from(uid))),
            ("name", Value::string(name)),
        ],
    )
}

fn service(name: &str) -> ono_value::RecordValue {
    record(
        "ono.service/1",
        &[
            ("provider", Value::string("systemd")),
            ("name", Value::string(name)),
            ("state", Value::string("active")),
        ],
    )
}

fn file(path: &str, kind: &str, inode: i64, extra: &[(&str, Value)]) -> ono_value::RecordValue {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut fields = vec![
        (
            "path",
            Value::Path(std::sync::Arc::from(std::path::Path::new(path))),
        ),
        ("name", Value::string(name)),
        ("kind", Value::string(kind)),
        ("device", Value::Int(2049)),
        ("inode", Value::Int(i128::from(inode))),
    ];
    fields.extend(extra.iter().cloned());
    record("ono.file/1", &fields)
}

#[test]
fn should_link_a_process_to_the_parent_that_forked_it() {
    // §11.2's first edge: process --parent-of--> process.
    let (index, bridge) = absorb(vec![
        process(1, "systemd", &[]),
        process(1842, "nginx", &[("ppid", Value::Int(1))]),
    ]);
    let child = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the child is a place")
        .clone();
    let parent = bridge
        .resolve(SpatialType::Process, "1")
        .expect("the parent is a place")
        .clone();
    assert_eq!(along(&index, &child, "process.parent_of"), vec![parent]);
}

#[test]
fn should_link_a_socket_to_the_process_that_owns_it() {
    // §12's `sockets` exit and §14.3's `owner process` group are the two ends of one edge.
    let mut socket = socket_with(9_001, Some("listen"), None);
    socket = common::with(socket, "process", Value::Int(1842));
    let (index, bridge) = absorb(vec![process(1842, "nginx", &[]), socket]);

    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    let listener = bridge
        .resolve(SpatialType::Listener, "9001")
        .expect("the listener is a place")
        .clone();
    assert_eq!(
        along(&index, &process_id, "process.owns_socket"),
        vec![listener]
    );
}

#[test]
fn should_link_a_service_to_the_process_it_controls_and_the_user_that_owns_it() {
    // §13's `processes` group and §17's ownership relationships, both from one process record.
    let (index, bridge) = absorb(vec![
        service("nginx.service"),
        user(33, "www-data"),
        process(
            1842,
            "nginx",
            &[
                ("service", Value::string("nginx.service")),
                (
                    "user",
                    Value::Record(std::sync::Arc::new(user(33, "www-data"))),
                ),
            ],
        ),
    ]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();

    assert_eq!(
        along(&index, &process_id, "service.controls_process"),
        vec![
            bridge
                .resolve(SpatialType::Service, "nginx.service")
                .expect("the service is a place")
                .clone()
        ]
    );
    assert_eq!(
        along(&index, &process_id, "process.run_by_user"),
        vec![
            bridge
                .resolve(SpatialType::User, "33")
                .expect("the user is a place")
                .clone()
        ]
    );
}

#[test]
fn should_link_a_process_to_the_files_it_holds_open() {
    // §12's `files` exit, from the descriptor table `ono.process-detail/1` reports.
    let detail = record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            (
                "open_files",
                Value::list([Value::Path(std::sync::Arc::from(std::path::Path::new(
                    "/etc/nginx/nginx.conf",
                )))]),
            ),
        ],
    );
    let (index, bridge) = absorb(vec![file("/etc/nginx/nginx.conf", "file", 18, &[]), detail]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    assert_eq!(
        along(&index, &process_id, "process.opened_file"),
        vec![
            bridge
                .resolve(SpatialType::File, "/etc/nginx/nginx.conf")
                .expect("the file is a place")
                .clone()
        ]
    );
}

#[test]
fn should_link_a_process_to_the_cgroup_it_is_in_even_though_no_provider_serves_cgroups() {
    // §16.3: cgroups are spatially navigable through the hierarchy the kernel reports. No
    // provider answers `get cgroup`; the path in `/proc/<pid>/cgroup` is the fact, and the place
    // is composed from it (§2.16).
    let detail = record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            (
                "cgroup",
                Value::Path(std::sync::Arc::from(std::path::Path::new(
                    "/system.slice/nginx.service",
                ))),
            ),
        ],
    );
    let (index, bridge) = absorb(vec![detail]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    let cgroup = along(&index, &process_id, "process.member_of_cgroup");
    assert_eq!(cgroup.len(), 1, "the process is in exactly one cgroup");
    let entry = index.get(&cgroup[0]).expect("the cgroup is a place");
    assert_eq!(entry.object().object_type(), SpatialType::Cgroup);
    assert_eq!(entry.object().display_name(), "/system.slice/nginx.service");
    assert_eq!(
        entry.object().provenance().provider(),
        "test",
        "a composed place keeps the provenance of the record that named it (§2.16)"
    );
}

#[test]
fn should_link_a_process_to_the_pid_namespace_it_runs_in() {
    // §16.2: entering a namespace must show the boundary, which needs the namespace to be a
    // place. `pid_namespace` is the fact the process record carries (ADR-0134).
    let (index, bridge) = absorb(vec![process(1, "systemd", &[])]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1")
        .expect("the process is a place")
        .clone();
    let namespaces = along(&index, &process_id, "process.in_namespace");
    assert_eq!(namespaces.len(), 1);
    assert_eq!(
        index
            .get(&namespaces[0])
            .expect("the namespace is a place")
            .object()
            .object_type(),
        SpatialType::Namespace
    );
}

#[test]
fn should_link_a_container_to_the_process_its_cgroup_holds_without_claiming_to_have_observed_it() {
    // §11.5: the kernel does not report container membership; the cgroup path names the runtime
    // id, which is strong evidence and not an observation. The edge says so, and names what it
    // was derived from (§11.4).
    let container = record(
        "ono.container/1",
        &[
            (
                "id",
                Value::string("9f2c1b3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9"),
            ),
            ("name", Value::string("payments-api")),
            ("state", Value::string("running")),
        ],
    );
    let detail = record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(4419)),
            ("name", Value::string("python")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            (
                "cgroup",
                Value::Path(std::sync::Arc::from(std::path::Path::new(
                    "/system.slice/docker-9f2c1b3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9.scope",
                ))),
            ),
        ],
    );
    let (index, bridge) = absorb(vec![container, detail]);
    let process_id = bridge
        .resolve(SpatialType::Process, "4419")
        .expect("the process is a place")
        .clone();
    let edge = index
        .get(&process_id)
        .expect("the process is in the index")
        .edges()
        .iter()
        .find(|edge| edge.relation().as_str() == "container.contains_process")
        .expect("the cgroup path names the container that holds the process")
        .clone();

    assert_eq!(edge.confidence(), Confidence::Strong);
    assert!(
        edge.attributes().get("evidence").is_some(),
        "§11.4: a derived edge names what it was derived from, got {:?}",
        edge.attributes()
    );
    assert!(edge.honours_declaration());
}

#[test]
fn should_link_a_connection_to_the_endpoint_at_its_far_end() {
    // §14.4's `remote endpoint` group, and §42.3's "explicit unresolved endpoint object": the
    // far end is a place even where Ono cannot say what host it is.
    let mut socket = socket_with(9_100, Some("established"), Some("10.0.0.5"));
    socket = common::with(socket, "process", Value::Int(1842));
    let (index, bridge) = absorb(vec![socket]);
    let connection = bridge
        .resolve(SpatialType::Connection, "9100")
        .expect("the connection is a place")
        .clone();
    let peers = along(&index, &connection, "socket.connected_to");
    assert_eq!(peers.len(), 1);
    assert_eq!(
        index
            .get(&peers[0])
            .expect("the endpoint is a place")
            .object()
            .object_type(),
        SpatialType::Endpoint
    );
}

#[test]
fn should_lead_from_a_process_to_the_endpoint_its_connection_reaches() {
    // §11.2 puts `process.connects_to` in the graph, and the socket record states both halves:
    // the process that holds the socket and the peer the socket is connected to. The process is
    // therefore joined to the same endpoint place its connection leads to (ADR-0972).
    let owner = process(1842, "curl", &[]);
    let mut socket = socket_with(9_100, Some("established"), Some("10.0.0.5"));
    socket = common::with(socket, "process", Value::Int(1842));
    let (index, bridge) = absorb(vec![owner, socket]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    let connection = bridge
        .resolve(SpatialType::Connection, "9100")
        .expect("the connection is a place")
        .clone();

    let reached = along(&index, &process_id, "process.connects_to");
    assert_eq!(
        reached,
        along(&index, &connection, "socket.connected_to"),
        "the process reaches the endpoint its connection is connected to, and no other"
    );
    assert_eq!(reached.len(), 1);
    let edge = index
        .get(&process_id)
        .expect("the process is in the index")
        .edges()
        .iter()
        .find(|edge| edge.relation().as_str() == "process.connects_to")
        .expect("the edge is recorded on the process")
        .clone();
    assert_eq!(edge.source(), &process_id, "the process is the source");
    assert_eq!(edge.confidence(), Confidence::Exact);
    assert!(edge.honours_declaration());
}

#[test]
fn should_join_a_process_to_its_endpoint_when_the_process_is_observed_after_the_socket() {
    // §42.3: no dangling ids. A socket listing read before the process listing names a pid that
    // is no place yet; the assertion waits for the batch that brings the process.
    let mut socket = socket_with(9_100, Some("established"), Some("10.0.0.5"));
    socket = common::with(socket, "process", Value::Int(1842));
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &[socket], NOW);
    assert!(
        bridge.resolve(SpatialType::Process, "1842").is_none(),
        "the socket alone does not make its owner a place"
    );

    bridge.absorb(&mut index, &[process(1842, "curl", &[])], NOW);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    assert_eq!(along(&index, &process_id, "process.connects_to").len(), 1);
}

#[test]
fn should_claim_no_endpoint_for_a_process_when_the_socket_names_no_owner() {
    // An unprivileged listing shows other users' sockets without their owner. The connection
    // still leads to its peer; nobody is said to hold it.
    let owner = process(1842, "curl", &[]);
    let socket = socket_with(9_100, Some("established"), Some("10.0.0.5"));
    let (index, bridge) = absorb(vec![owner, socket]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    assert!(along(&index, &process_id, "process.connects_to").is_empty());
}

#[test]
fn should_link_storage_from_the_device_through_the_filesystem_to_the_mount_and_the_directory() {
    // §15.2's storage hierarchy, end to end: DEVICES -> FILESYSTEMS -> MOUNTS -> DIRECTORY ROOTS.
    let device = record(
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
    let filesystem = record(
        "ono.filesystem/1",
        &[
            ("source", Value::string("/dev/sda2")),
            ("type", Value::string("ext4")),
            (
                "target",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/data"))),
            ),
        ],
    );
    let mount = record(
        "ono.mount/1",
        &[
            ("source", Value::string("/dev/sda2")),
            (
                "target",
                Value::Path(std::sync::Arc::from(std::path::Path::new("/data"))),
            ),
            ("filesystem", Value::string("ext4")),
            ("options", Value::list([Value::string("rw")])),
            ("read_only", Value::Bool(false)),
        ],
    );
    let directory = file("/data", "dir", 2, &[]);
    let (index, bridge) = absorb(vec![device, filesystem, mount, directory]);

    let fs = bridge
        .resolve(SpatialType::Filesystem, "/dev/sda2")
        .expect("the filesystem is a place")
        .clone();
    let mount_id = bridge
        .resolve(SpatialType::Mount, "/data")
        .expect("the mount is a place")
        .clone();
    let disk = bridge
        .resolve(SpatialType::BlockDevice, "/dev/sda2")
        .expect("the disk is a place")
        .clone();
    let dir = bridge
        .resolve(SpatialType::Directory, "/data")
        .expect("the directory is a place")
        .clone();

    assert_eq!(
        along(&index, &fs, "filesystem.mounted_at"),
        vec![mount_id.clone()]
    );
    assert_eq!(along(&index, &mount_id, "mount.backs_directory"), vec![dir]);
    assert_eq!(along(&index, &fs, "device.backs_filesystem"), vec![disk]);
}

#[test]
fn should_link_the_network_from_the_route_and_the_address_to_the_interface() {
    let interface = record(
        "ono.interface/1",
        &[
            ("name", Value::string("eth0")),
            ("index", Value::Int(2)),
            ("state", Value::string("up")),
            ("mtu", Value::Int(1500)),
            ("addresses", Value::list([])),
        ],
    );
    let route = record(
        "ono.route/1",
        &[
            ("family", Value::string("inet")),
            ("interface", Value::string("eth0")),
            ("table", Value::string("main")),
        ],
    );
    let address = record(
        "ono.interface-address/1",
        &[
            ("interface", Value::string("eth0")),
            ("index", Value::Int(2)),
            ("family", Value::string("inet")),
            (
                "address",
                Value::IpNetwork(
                    ono_value::IpNetwork::new("10.0.0.1".parse().expect("a fixture address"), 24)
                        .expect("a fixture network"),
                ),
            ),
        ],
    );
    let (index, bridge) = absorb(vec![interface, route, address]);
    let eth0 = bridge
        .resolve(SpatialType::Interface, "eth0")
        .expect("the interface is a place")
        .clone();

    assert_eq!(along(&index, &eth0, "route.via_interface").len(), 1);
    assert_eq!(along(&index, &eth0, "interface.has_address").len(), 1);
}

#[test]
fn should_link_a_user_to_the_group_it_belongs_to() {
    let group = record(
        "ono.group/1",
        &[("gid", Value::Int(33)), ("name", Value::string("www-data"))],
    );
    let member = record(
        "ono.user/1",
        &[
            ("uid", Value::Int(33)),
            ("name", Value::string("www-data")),
            (
                "primary_group",
                Value::Record(std::sync::Arc::new(record(
                    "ono.group/1",
                    &[("gid", Value::Int(33)), ("name", Value::string("www-data"))],
                ))),
            ),
        ],
    );
    let (index, bridge) = absorb(vec![group, member]);
    let user_id = bridge
        .resolve(SpatialType::User, "33")
        .expect("the user is a place")
        .clone();
    assert_eq!(along(&index, &user_id, "user.member_of_group").len(), 1);
}

#[test]
fn should_carry_every_field_an_edge_must_expose_for_inspection() {
    // §3.5 and §11.4: an edge that cannot be explained is one nobody should trust.
    let (index, bridge) = absorb(vec![
        process(1, "systemd", &[]),
        process(1842, "nginx", &[("ppid", Value::Int(1))]),
    ]);
    let child = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the child is a place")
        .clone();
    let edge = index
        .get(&child)
        .expect("the child is in the index")
        .edges()
        .first()
        .expect("the child has an edge")
        .clone();

    assert_eq!(edge.relation().as_str(), "process.parent_of");
    assert_eq!(edge.confidence(), Confidence::Exact);
    assert_eq!(edge.provenance().provider(), "test");
    assert_eq!(edge.observed_at(), NOW);
    assert!(edge.other_end(&child).is_some());
    assert!(!edge.edge_id().as_str().is_empty());
    assert!(edge.honours_declaration());
}

#[test]
fn should_never_assert_an_edge_to_an_object_the_index_does_not_hold() {
    // §42.3: dangling internal ids are invalid. A process whose parent nobody observed has no
    // parent edge — which is different from having a parent edge that points nowhere.
    let (index, bridge) = absorb(vec![process(
        1842,
        "nginx",
        &[
            ("ppid", Value::Int(1)),
            ("service", Value::string("nginx.service")),
        ],
    )]);
    let child = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the child is a place")
        .clone();
    assert!(along(&index, &child, "process.parent_of").is_empty());
    assert!(along(&index, &child, "service.controls_process").is_empty());
    for edge in index.get(&child).expect("the child is indexed").edges() {
        assert!(
            index.contains(edge.source()) && index.contains(edge.target()),
            "both ends of every edge are places the index holds: {edge:?}"
        );
    }
}

#[test]
fn should_link_a_reference_whose_target_arrives_after_it() {
    // Discovery is not ordered: sockets can be listed before processes. An edge that could not
    // be made yet must be made when the far end appears, not lost.
    let mut index = index();
    let mut bridge = bridge();
    let mut socket = socket_with(9_001, Some("listen"), None);
    socket = common::with(socket, "process", Value::Int(1842));
    bridge.absorb(&mut index, &[socket], NOW);
    bridge.absorb(&mut index, &[process(1842, "nginx", &[])], NOW);

    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    assert_eq!(
        along(&index, &process_id, "process.owns_socket").len(),
        1,
        "the socket was seen first; the edge arrives when the process does"
    );
}

#[test]
fn should_send_up_from_a_process_to_the_service_that_controls_it() {
    // §11.3: one canonical parent for `up`, deterministic, chosen by the rule and not by the
    // order edges arrived in.
    let (index, bridge) = absorb(vec![
        service("nginx.service"),
        process(
            1842,
            "nginx",
            &[("service", Value::string("nginx.service"))],
        ),
    ]);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    let parent = index
        .canonical_parent(&process_id)
        .expect("a process always has somewhere to go up to");
    assert_eq!(
        parent.parent(),
        bridge
            .resolve(SpatialType::Service, "nginx.service")
            .expect("the service is a place")
    );
}

#[test]
fn should_send_up_from_a_file_to_the_directory_that_contains_it() {
    // §15.1: "Ono MUST preserve canonical Unix filesystem paths and directory semantics." The
    // path tree is hierarchy, not a relationship (§3.4), so `up` follows it directly.
    let (index, bridge) = absorb(vec![
        file("/etc/nginx", "dir", 17, &[]),
        file("/etc/nginx/nginx.conf", "file", 18, &[]),
    ]);
    let conf = bridge
        .resolve(SpatialType::File, "/etc/nginx/nginx.conf")
        .expect("the file is a place")
        .clone();
    let parent = index
        .canonical_parent(&conf)
        .expect("a file inside an observed directory has a parent");
    assert_eq!(
        parent.parent(),
        bridge
            .resolve(SpatialType::Directory, "/etc/nginx")
            .expect("the directory is a place")
    );
}

#[test]
fn should_show_one_container_when_a_process_record_names_it_and_its_cgroup_carries_the_id() {
    // A process record may name its container outright, and its detail view also carries the
    // control group whose path holds the same id. That is one fact stated twice, and the
    // process has one container to show — never the same place counted as two (§42.1,
    // ADR-0983).
    let id = "4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c";
    let container = common::record(
        "ono.container/1",
        &[
            ("id", Value::string(id)),
            ("name", Value::string("web-1")),
            ("state", Value::string("running")),
        ],
    );
    let process = common::record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(441)),
            ("name", Value::string("nginx")),
            ("started", Value::string("1000")),
            ("container", Value::string(id)),
            (
                "cgroup",
                Value::string(&format!("/system.slice/docker-{id}.scope")),
            ),
        ],
    );
    let mut index = common::index();
    let mut bridge = common::bridge();
    bridge.absorb(&mut index, &[container, process.clone()], common::NOW);

    let place = bridge.project(&process).expect("a place");
    let containers: Vec<_> = index
        .relation_summary(place.spatial_id(), usize::MAX, common::NOW)
        .into_iter()
        .find(|group| group.label() == "container")
        .map(|group| group.members().to_vec())
        .unwrap_or_default();
    assert_eq!(containers.len(), 1, "one container, got {containers:?}");
}
