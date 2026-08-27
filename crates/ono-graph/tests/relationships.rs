//! The relationships spec §22.2 requires, each against a fixture rather than the machine the
//! tests run on.

mod common;

use std::sync::Arc;

use common::{
    FixtureProvider, ProcFixture, TableResolver, edges, endpoint, file, filesystem, group,
    interface, make_readable, mount, neighbor, node, owned_process, process, registry, route,
    service, socket, trace_with, user,
};
use ono_core::ErrorCode;
use ono_graph::{
    Confidence, FileHolders, InterfaceSockets, MountDevices, MountFilesystems, MountUsers,
    OpenFiles, ProcessSockets, ProcessTree, ProcessUsers, RemoteHosts, RouteInterfaces,
    ServiceProcesses, SocketOwners, TraceOptions, UserGroups, UserProcesses,
};
use ono_provider_api::Provider;
use ono_value::Value;

/// One hop, so a test about one relationship sees only that relationship.
fn one_hop() -> TraceOptions {
    TraceOptions::new().depth(1)
}

#[tokio::test]
async fn should_link_a_process_to_its_parent_and_to_its_children() {
    let processes: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            process(4381, Some(1), "cargo"),
            process(4419, Some(4381), "rustc"),
            process(4420, Some(4419), "rustc"),
            process(4421, Some(4419), "rustc"),
        ],
    ))];
    let registry = registry(processes);
    let subject = process(4419, Some(4381), "rustc");

    let graph = trace_with(
        vec![Arc::new(ProcessTree::new(Arc::clone(&registry)))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        [
            "parent -> process/4381 cargo",
            "child -> process/4420 rustc",
            "child -> process/4421 rustc",
        ],
        "the process tree is parent first, then children in pid order"
    );
    assert!(
        graph
            .edges()
            .iter()
            .all(|edge| edge.confidence() == Confidence::Exact),
        "a parent link read from the kernel is exact at observation time (spec §22.2)"
    );
}

#[tokio::test]
async fn should_link_a_process_to_the_files_it_has_open_saying_which_it_reads_and_writes() {
    let proc = ProcFixture::new();
    proc.process(921)
        .fd(3, "/etc/nginx/nginx.conf", 0)
        .fd(4, "/var/log/nginx/access.log", 1);
    let files: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        vec![
            file("/etc/nginx/nginx.conf", "file", 2049, 17),
            file("/var/log/nginx/access.log", "file", 2049, 18),
        ],
    ))];
    let registry = registry(files);
    let subject = process(921, Some(1), "nginx");

    let graph = trace_with(
        vec![Arc::new(
            OpenFiles::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        [
            "reads -> /etc/nginx/nginx.conf",
            "writes -> /var/log/nginx/access.log",
        ],
        "the open mode of the descriptor decides the relation, and it is read from the kernel"
    );
    assert!(graph.failures().is_empty(), "nothing failed to be read");
}

/// Whether the suite is running with a uid that mode bits cannot restrain.
fn running_as_root() -> bool {
    // `id -u` rather than a libc call: this crate forbids `unsafe` and the answer is only used to
    // decide whether a permission scenario is reachable at all.
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

#[tokio::test]
async fn should_report_an_error_when_the_file_descriptors_cannot_be_read() {
    // The general contract, and it holds for every user: a source that cannot be read becomes a
    // failure attributed to the object, never a missing edge. A relationship you were not allowed
    // to see is not a relationship that does not exist.
    let proc = ProcFixture::new();
    proc.process(921).fds_not_a_directory();
    let registry = registry(vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        Vec::new(),
    ))]);
    let subject = process(921, Some(1), "nginx");

    let graph = trace_with(
        vec![Arc::new(
            OpenFiles::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;

    assert!(
        graph.edges().is_empty(),
        "a relationship that could not be read is not asserted"
    );
    let failure = graph
        .failures()
        .first()
        .expect("a source that cannot be read is reported, not silently dropped");
    assert_eq!(
        failure.subject(),
        node(&subject).id(),
        "the failure names the object it concerns"
    );
}

#[tokio::test]
async fn should_report_an_error_when_a_process_hides_its_file_descriptors() {
    // The realistic form of the same thing: another user's process. Mode bits do not restrain
    // root, so under root the scenario is unreachable and the general test above is what covers
    // the contract. `ono-provider-linux` skips its two permission tests the same way.
    if running_as_root() {
        return;
    }
    let proc = ProcFixture::new();
    proc.process(921).unreadable_fds();
    let registry = registry(vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        Vec::new(),
    ))]);
    let subject = process(921, Some(1), "nginx");

    let graph = trace_with(
        vec![Arc::new(
            OpenFiles::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;
    make_readable(proc.root());

    assert!(
        graph.edges().is_empty(),
        "a relationship that could not be read is not asserted"
    );
    let failure = graph
        .failures()
        .first()
        .expect("a relationship this user may not see is reported, not silently dropped");
    assert_eq!(failure.error().code(), ErrorCode::IoPermissionDenied);
    assert_eq!(
        failure.subject(),
        node(&subject).id(),
        "the failure names the object it concerns"
    );
}

#[tokio::test]
async fn should_link_a_process_to_the_socket_it_holds_by_inode() {
    let proc = ProcFixture::new();
    proc.process(921).fd(5, "socket:[8323]", 2);
    let sockets: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.socket",
        &["socket"],
        vec![socket(
            8323,
            "tcp",
            endpoint(Some("0.0.0.0"), Some(443)),
            endpoint(None, None),
            "listen",
        )],
    ))];
    let registry = registry(sockets);
    let subject = process(921, Some(1), "nginx");

    let graph = trace_with(
        vec![Arc::new(
            ProcessSockets::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(edges(&graph), ["listens -> tcp/:443"]);
    let edge = graph.edges().first().expect("the socket edge");
    assert_eq!(edge.confidence(), Confidence::Exact);
    assert_eq!(
        edge.metadata().get("inode"),
        Some(&ono_value::Value::Int(8323)),
        "the edge carries the inode it was read from"
    );
}

#[tokio::test]
async fn should_link_a_socket_back_to_the_process_that_owns_it() {
    let proc = ProcFixture::new();
    proc.process(921).fd(5, "socket:[8323]", 2);
    proc.process(4419).fd(3, "/tmp/other", 0);
    let processes: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            process(921, Some(1), "nginx"),
            process(4419, Some(1), "rustc"),
        ],
    ))];
    let registry = registry(processes);
    let subject = socket(
        8323,
        "tcp",
        endpoint(Some("0.0.0.0"), Some(443)),
        endpoint(None, None),
        "listen",
    );

    let graph = trace_with(
        vec![Arc::new(
            SocketOwners::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(edges(&graph), ["owner -> process/921 nginx"]);
}

#[tokio::test]
async fn should_link_a_service_to_its_main_process_and_to_its_cgroup_members() {
    let proc = ProcFixture::new();
    proc.process(921).cgroup("0::/system.slice/nginx.service");
    proc.process(922).cgroup("0::/system.slice/nginx.service");
    proc.process(4419).cgroup("0::/user.slice/user-1000.slice");
    let processes: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            process(921, Some(1), "nginx"),
            process(922, Some(921), "nginx"),
            process(4419, Some(1), "rustc"),
        ],
    ))];
    let registry = registry(processes);
    let subject = service("nginx.service", Some(921));

    let graph = trace_with(
        vec![Arc::new(
            ServiceProcesses::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["owns -> process/921 nginx", "contains -> process/922 nginx"],
        "the main process is owned; the rest of the cgroup is contained"
    );
}

#[tokio::test]
async fn should_link_a_mount_to_the_device_backing_it() {
    let devices: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        vec![file("/dev/sda1", "device", 5, 421)],
    ))];
    let registry = registry(devices);
    let subject = mount("/dev/sda1", "/mnt/data", "ext4");

    let graph = trace_with(
        vec![Arc::new(MountDevices::new(Arc::clone(&registry)))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(edges(&graph), ["backed-by -> /dev/sda1"]);
}

#[tokio::test]
async fn should_not_invent_a_device_for_a_filesystem_that_has_none() {
    let registry = registry(vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        Vec::new(),
    ))]);
    let subject = mount("tmpfs", "/run", "tmpfs");

    let graph = trace_with(
        vec![Arc::new(MountDevices::new(Arc::clone(&registry)))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert!(
        graph.edges().is_empty() && graph.failures().is_empty(),
        "a tmpfs has no backing device: that is absence, not a failed read"
    );
}

#[tokio::test]
async fn should_mark_a_reverse_resolved_host_as_inferred_and_keep_its_evidence() {
    let resolver = Arc::new(TableResolver::new().with("10.4.2.11", "db.internal"));
    let subject = socket(
        9001,
        "tcp",
        endpoint(Some("10.4.2.9"), Some(52344)),
        endpoint(Some("10.4.2.11"), Some(5432)),
        "established",
    );

    let graph = trace_with(
        vec![Arc::new(RemoteHosts::new(resolver))],
        node(&subject),
        one_hop(),
    )
    .await;

    let edge = graph
        .edges()
        .first()
        .expect("the resolved host is an edge of the graph");
    assert_eq!(edge.relation(), "resolves-to");
    assert_eq!(
        edge.confidence(),
        Confidence::Inferred,
        "a name from a resolver is derived, never an observation (spec §22.2)"
    );
    let evidence = edge
        .metadata()
        .get("inferred_from")
        .and_then(|value| ono_value::canonical_text(value).ok())
        .expect("an inferred edge names what it was inferred from (spec §31.25)");
    assert!(
        evidence.contains("10.4.2.11") && evidence.contains("fixture.resolver"),
        "the evidence names the address and the resolver that answered: {evidence}"
    );
}

#[tokio::test]
async fn should_keep_an_inferred_edge_inferred_when_an_exact_edge_joins_the_same_pair() {
    let proc = ProcFixture::new();
    proc.process(921).fd(5, "socket:[9001]", 2);
    let subject = socket(
        9001,
        "tcp",
        endpoint(Some("10.4.2.9"), Some(52344)),
        endpoint(Some("10.4.2.11"), Some(5432)),
        "established",
    );
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(FixtureProvider::new(
            "fixture.process",
            &["process"],
            vec![process(921, Some(1), "nginx")],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.socket",
            &["socket"],
            vec![subject.clone()],
        )),
    ];
    let registry = registry(providers);

    let graph = trace_with(
        vec![
            Arc::new(SocketOwners::new(Arc::clone(&registry)).rooted(proc.root())),
            Arc::new(RemoteHosts::new(Arc::new(
                TableResolver::new().with("10.4.2.11", "db.internal"),
            ))),
        ],
        node(&subject),
        one_hop(),
    )
    .await;

    let confidences: Vec<(&str, Confidence)> = graph
        .edges()
        .iter()
        .map(|edge| (edge.relation(), edge.confidence()))
        .collect();
    assert_eq!(
        confidences,
        [
            ("owner", Confidence::Exact),
            ("resolves-to", Confidence::Inferred)
        ],
        "an exact edge beside an inferred one promotes nothing"
    );
}

#[tokio::test]
async fn should_link_a_user_to_the_processes_running_as_it_by_uid_not_by_name() {
    let processes: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            owned_process(812, "postgres", 999, "postgres"),
            owned_process(813, "postgres", 999, "postgres"),
            // Same name, other uid: a lookalike account, not this user.
            owned_process(900, "postgres", 1001, "postgres"),
            owned_process(1, "systemd", 0, "root"),
        ],
    ))];
    let registry = registry(processes);
    let subject = user(999, "postgres", 999);

    let graph = trace_with(
        vec![Arc::new(UserProcesses::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        [
            "runs -> process/812 postgres",
            "runs -> process/813 postgres"
        ],
        "spec §23.6: a process belongs to a user by uid, in pid order"
    );
    assert!(
        graph
            .edges()
            .iter()
            .all(|edge| edge.confidence() == Confidence::Exact),
        "the kernel reports a process's owner; nothing is inferred"
    );
}

#[tokio::test]
async fn should_link_a_user_to_its_primary_group_and_to_the_groups_that_list_it() {
    let groups: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.group",
        &["group"],
        vec![
            group(0, "root", &[]),
            group(27, "sudo", &["alice", "bob"]),
            group(1000, "alice", &[]),
            group(44, "video", &["bob"]),
        ],
    ))];
    let registry = registry(groups);
    let subject = user(1000, "alice", 1000);

    let graph = trace_with(
        vec![Arc::new(UserGroups::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["member-of -> sudo", "primary-group -> alice"],
        "the primary group comes from the account, the others from the group's own member \
         list, in gid order; a group that does not list the user is not related to it"
    );
}

#[tokio::test]
async fn should_link_a_mount_to_the_filesystem_at_the_same_mount_point() {
    let filesystems: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.filesystem",
        &["filesystem"],
        vec![
            filesystem(
                "/dev/sda2",
                "/",
                "ext4",
                "5d6c1406-0b18-4cb7-8f0b-2a6aec04847e",
            ),
            filesystem(
                "/dev/sdb1",
                "/srv/data",
                "xfs",
                "0f7c2b1e-9a3d-4e55-8c21-1d2e3f4a5b6c",
            ),
        ],
    ))];
    let registry = registry(filesystems);
    let subject = mount("/dev/sdb1", "/srv/data", "xfs");

    let graph = trace_with(
        vec![Arc::new(MountFilesystems::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["filesystem -> filesystem/0f7c2b1e-9a3d-4e55-8c21-1d2e3f4a5b6c"],
        "the filesystem is the one at the mount's own target, not the root's"
    );
}

#[tokio::test]
async fn should_link_a_mount_to_the_processes_rooted_or_working_on_it() {
    let proc = ProcFixture::new();
    proc.process(700)
        .link("root", "/")
        .link("cwd", "/srv/data/pg");
    proc.process(701)
        .link("root", "/")
        .link("cwd", "/home/alice");
    // The mount a path lies on is its longest-prefix mount, so `/srv/data` is not `/srv`.
    proc.process(702).link("root", "/").link("cwd", "/srv");
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(FixtureProvider::new(
            "fixture.mount",
            &["mount"],
            vec![
                mount("/dev/sda2", "/", "ext4"),
                mount("/dev/sdb1", "/srv/data", "xfs"),
                mount("/dev/sdc1", "/srv", "ext4"),
                mount("/dev/sdd1", "/home", "ext4"),
            ],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.process",
            &["process"],
            vec![
                process(700, Some(1), "postgres"),
                process(701, Some(1), "bash"),
                process(702, Some(1), "nginx"),
            ],
        )),
    ];
    let registry = registry(providers);
    let subject = mount("/dev/sdb1", "/srv/data", "xfs");

    let graph = trace_with(
        vec![Arc::new(MountUsers::new(registry).rooted(proc.root()))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["cwd -> process/700 postgres"],
        "only the process whose working directory lies on this mount uses it; the roots are \
         on `/`, and `/srv` is another mount"
    );
    let edge = &graph.edges()[0];
    assert_eq!(edge.confidence(), Confidence::Exact);
    assert_eq!(
        edge.metadata().get("path"),
        Some(&ono_value::Value::Path(Arc::from(std::path::Path::new(
            "/srv/data/pg"
        )))),
        "the edge names the path it was read from"
    );
}

#[tokio::test]
async fn should_link_a_route_to_its_interface_and_to_the_neighbour_that_is_its_gateway() {
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(FixtureProvider::new(
            "fixture.interface",
            &["interface"],
            vec![
                interface(1, "lo", &["127.0.0.1/8"]),
                interface(2, "eth0", &["192.168.1.20/24"]),
            ],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.neighbor",
            &["neighbor"],
            vec![
                neighbor("192.168.1.1", "aa:bb:cc:dd:ee:01", "eth0"),
                // The same address seen on another interface is another neighbour.
                neighbor("192.168.1.1", "aa:bb:cc:dd:ee:02", "wlan0"),
            ],
        )),
    ];
    let registry = registry(providers);
    let subject = route("main", "0.0.0.0/0", Some("192.168.1.1"), "eth0");

    let graph = trace_with(
        vec![Arc::new(RouteInterfaces::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["via -> eth0", "gateway -> neighbor/192.168.1.1"],
        "the route leaves through its interface and via the neighbour on that interface"
    );
    let gateway = graph
        .nodes()
        .iter()
        .find(|node| node.kind().to_string() == "ono.neighbor/1")
        .expect("the gateway neighbour node");
    assert_eq!(
        gateway.text("mac").as_deref(),
        Some("aa:bb:cc:dd:ee:01"),
        "the neighbour is the one on the route's interface, not a lookalike elsewhere"
    );
}

#[tokio::test]
async fn should_not_invent_a_gateway_neighbour_the_kernel_has_not_resolved() {
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(FixtureProvider::new(
            "fixture.interface",
            &["interface"],
            vec![interface(2, "eth0", &["192.168.1.20/24"])],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.neighbor",
            &["neighbor"],
            vec![],
        )),
    ];
    let registry = registry(providers);
    let subject = route("main", "0.0.0.0/0", Some("192.168.1.1"), "eth0");

    let graph = trace_with(
        vec![Arc::new(RouteInterfaces::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["via -> eth0"],
        "spec §22.4: an unresolved gateway is absence, not a made-up neighbour"
    );
    assert!(graph.failures().is_empty(), "absence is not a failed read");
}

#[tokio::test]
async fn should_link_an_interface_to_sockets_bound_to_its_addresses_and_mark_wildcards_inferred() {
    let sockets: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.socket",
        &["socket"],
        vec![
            socket(
                11,
                "tcp",
                endpoint(Some("127.0.0.1"), Some(631)),
                Value::Null,
                "listen",
            ),
            socket(
                12,
                "tcp",
                endpoint(Some("0.0.0.0"), Some(22)),
                Value::Null,
                "listen",
            ),
            socket(
                13,
                "tcp",
                endpoint(Some("192.168.1.20"), Some(443)),
                Value::Null,
                "listen",
            ),
        ],
    ))];
    let registry = registry(sockets);
    let subject = interface(1, "lo", &["127.0.0.1/8", "::1/128"]);

    let graph = trace_with(
        vec![Arc::new(InterfaceSockets::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["bound -> tcp/127.0.0.1:631", "bound -> tcp/:22"],
        "the loopback listener and the wildcard listener are bound to `lo`; the one on another \
         interface's address is not"
    );
    let confidences: Vec<Confidence> = graph.edges().iter().map(|e| e.confidence()).collect();
    assert_eq!(
        confidences,
        [Confidence::Exact, Confidence::Inferred],
        "spec §22.2: a socket on the interface's own address is observed; a wildcard binding is \
         inferred and says so"
    );
}

#[tokio::test]
async fn should_link_a_file_to_the_processes_holding_it_open() {
    let proc = ProcFixture::new();
    proc.process(300).fd(0, "/srv/data/held.txt", 0);
    proc.process(301).fd(5, "/srv/data/other.txt", 1);
    proc.process(302).fd(7, "/srv/data/held.txt", 1);
    let processes: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            process(300, Some(1), "sleep"),
            process(301, Some(1), "cat"),
            process(302, Some(1), "tee"),
        ],
    ))];
    let registry = registry(processes);
    let subject = file("/srv/data/held.txt", "file", 2049, 41);

    let graph = trace_with(
        vec![Arc::new(FileHolders::new(registry).rooted(proc.root()))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(
        edges(&graph),
        ["holder -> process/300 sleep", "holder -> process/302 tee"],
        "spec §22.3: the holders are the processes whose descriptor tables name the file, in \
         pid order"
    );
    let access: Vec<Option<String>> = graph
        .edges()
        .iter()
        .map(|edge| {
            edge.metadata()
                .get("access")
                .and_then(|value| value.as_str().ok().map(str::to_owned))
        })
        .collect();
    assert_eq!(
        access,
        [Some("read".to_owned()), Some("write".to_owned())],
        "each edge says how the holder opened the file"
    );
}

#[tokio::test]
async fn should_link_a_process_to_the_user_it_runs_as() {
    let users: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.user",
        &["user"],
        vec![user(0, "root", 0), user(999, "postgres", 999)],
    ))];
    let registry = registry(users);
    let subject = owned_process(812, "postgres", 999, "postgres");

    let graph = trace_with(
        vec![Arc::new(ProcessUsers::new(registry))],
        node(&subject),
        one_hop(),
    )
    .await;

    assert_eq!(edges(&graph), ["runs-as -> postgres"]);
    assert_eq!(graph.edges()[0].confidence(), Confidence::Exact);
}
