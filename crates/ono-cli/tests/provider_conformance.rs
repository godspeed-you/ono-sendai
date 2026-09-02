//! The provider conformance suite of spec §35.3, generated from `docs/spec/providers/*.yaml`
//! and `docs/spec/schemas/*.v1.yaml` by `cargo xtask conformance`.
//!
//! Do not edit by hand: your changes will be overwritten and the gate will fail. What a provider
//! advertises is declared in the registry; this file is that declaration turned into questions
//! the running providers have to answer.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

mod conformance_harness;

use conformance_harness as harness;

#[rustfmt::skip]
#[tokio::test]
async fn should_register_exactly_the_providers_the_declarations_name() {
    harness::assert_registry(&[
        ("container-engine", &["container", "image"]),
        ("linux.netlink", &["interface"]),
        ("linux.netlink", &["route"]),
        ("linux.netlink", &["neighbor"]),
        ("linux.sock-diag", &["socket", "connection"]),
        ("linux.packages", &["package"]),
        ("linux.packages.rpm", &["package"]),
        ("linux.procfs", &["process", "signal"]),
        ("linux.fs", &["file", "dir"]),
        ("linux.nss", &["user", "group"]),
        ("ono.session", &["env"]),
        ("ono.shell", &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"]),
        ("linux.mountinfo", &["mount", "filesystem"]),
        ("linux.sysfs", &["device"]),
        ("linux.resolver", &["dns"]),
        ("ono.probe", &["port"]),
        ("systemd", &["service"]),
        ("systemd-journal", &["journal", "log"]),
        ("systemd-logind", &["session"]),
    ]).await;
}

/// Containers and images, from `GET /containers/json`, `GET /containers/{id}/json` and `GET /images/json` over the runtime's Unix socket — never from `docker ps` (spec §23, §31.57).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_container_engine_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "container-engine",
        targets: &["container", "image"],
        capabilities: &[
            harness::CapabilityClaim { id: "container.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "image.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "container.manage", risk: "mutate", elevation: "conditional" },
        ],
        schemas: &["ono.container/1", "ono.image/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_container_1_the_way_container_engine_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "container-engine",
        targets: &["container", "image"],
        schema: "ono.container/1",
        identity: &["id"],
        default_view: &["id", "name", "image", "state", "created"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "image", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "image_id", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<created|running|paused|restarting|removing|exited|dead|stopping|stopped|configured|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "created", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "labels", ty: "map", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_image_1_the_way_container_engine_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "container-engine",
        targets: &["container", "image"],
        schema: "ono.image/1",
        identity: &["id"],
        default_view: &["reference", "id", "size", "created"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "reference", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "tags", ty: "list<string>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "size", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "created", ty: "timestamp", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_container_within_its_contract_when_container_engine_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "container-engine",
        targets: &["container", "image"],
        target: "container",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.container/1", "ono.image/1"],
        identity_strategy: Some("stable"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_image_within_its_contract_when_container_engine_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "container-engine",
        targets: &["container", "image"],
        target: "image",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.container/1", "ono.image/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// Network interfaces, from rtnetlink (RTM_GETLINK / RTM_GETADDR); configured through RTM_NEWLINK / RTM_DELLINK / RTM_NEWADDR / RTM_DELADDR, which need CAP_NET_ADMIN.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_netlink_serving_interface_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.netlink",
        targets: &["interface"],
        capabilities: &[
            harness::CapabilityClaim { id: "interface.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "interface.set", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.interface/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_interface_1_the_way_linux_netlink_serving_interface_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.netlink",
        targets: &["interface"],
        schema: "ono.interface/1",
        identity: &["index"],
        default_view: &["name", "state", "addresses", "mtu", "mac"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "index", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "mac", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<up|down|unknown|dormant|testing|lower-layer-down|not-present>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "mtu", ty: "int", required: true, nullable: false, unit: Some("bytes") },
            harness::FieldContract { name: "addresses", ty: "list<ipnetwork>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "rx_bytes", ty: "bytesize", required: false, nullable: true, unit: Some("bytes") },
            harness::FieldContract { name: "tx_bytes", ty: "bytesize", required: false, nullable: true, unit: Some("bytes") },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_interface_within_its_contract_when_linux_netlink_serving_interface_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.netlink",
        targets: &["interface"],
        target: "interface",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.interface/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// Routes, from rtnetlink (RTM_GETROUTE); changed through RTM_NEWROUTE / RTM_DELROUTE, which need CAP_NET_ADMIN.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_netlink_serving_route_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.netlink",
        targets: &["route"],
        capabilities: &[
            harness::CapabilityClaim { id: "route.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "route.set", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.route/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_route_1_the_way_linux_netlink_serving_route_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.netlink",
        targets: &["route"],
        schema: "ono.route/1",
        identity: &["table", "family", "destination", "gateway", "interface"],
        default_view: &["destination", "gateway", "interface", "metric", "protocol"],
        fields: &[
            harness::FieldContract { name: "destination", ty: "ipnetwork", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "gateway", ty: "ip", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "interface", ty: "ref<ono.interface/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "source", ty: "ip", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "family", ty: "enum<inet|inet6>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "type", ty: "enum<unicast|local|broadcast|multicast|anycast|blackhole|unreachable|prohibit|throw|other>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "scope", ty: "enum<universe|site|link|host|nowhere>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "protocol", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "metric", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "table", ty: "string", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_route_within_its_contract_when_linux_netlink_serving_route_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.netlink",
        targets: &["route"],
        target: "route",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.route/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Neighbors, from rtnetlink (RTM_GETNEIGH).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_netlink_serving_neighbor_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.netlink",
        targets: &["neighbor"],
        capabilities: &[
            harness::CapabilityClaim { id: "neighbor.list", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.neighbor/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_neighbor_1_the_way_linux_netlink_serving_neighbor_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.netlink",
        targets: &["neighbor"],
        schema: "ono.neighbor/1",
        identity: &["address", "interface"],
        default_view: &["address", "mac", "interface", "state"],
        fields: &[
            harness::FieldContract { name: "address", ty: "ip", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "mac", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "interface", ty: "ref<ono.interface/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "family", ty: "enum<inet|inet6>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state", ty: "enum<incomplete|reachable|stale|delay|probe|failed|permanent|noarp|none>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "router", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "updated", ty: "timestamp", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_neighbor_within_its_contract_when_linux_netlink_serving_neighbor_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.netlink",
        targets: &["neighbor"],
        target: "neighbor",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.neighbor/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Sockets and connections, from sock_diag; endpoints are ono.endpoint/1 records. A socket is closed through SOCK_DESTROY, which needs CAP_NET_ADMIN.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_sock_diag_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.sock-diag",
        targets: &["socket", "connection"],
        capabilities: &[
            harness::CapabilityClaim { id: "socket.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "connection.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "socket.close", risk: "destructive", elevation: "required" },
        ],
        schemas: &["ono.socket/1", "ono.endpoint/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_socket_1_the_way_linux_sock_diag_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.sock-diag",
        targets: &["socket", "connection"],
        schema: "ono.socket/1",
        identity: &["inode"],
        default_view: &["protocol", "local", "remote", "state", "process"],
        fields: &[
            harness::FieldContract { name: "protocol", ty: "enum<tcp|udp|unix|raw|sctp|dccp|packet|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "family", ty: "enum<inet|inet6|unix|packet|netlink|other>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "local", ty: "record<ono.endpoint/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "remote", ty: "record<ono.endpoint/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<established|syn-sent|syn-recv|fin-wait-1|fin-wait-2|time-wait|close|close-wait|last-ack|listen|closing|unknown>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "process", ty: "ref<ono.process/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "user", ty: "ref<ono.user/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "inode", ty: "int", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_endpoint_1_the_way_linux_sock_diag_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.sock-diag",
        targets: &["socket", "connection"],
        schema: "ono.endpoint/1",
        identity: &[],
        default_view: &["address", "port"],
        fields: &[
            harness::FieldContract { name: "address", ty: "ip", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "port", ty: "port", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "path", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "host", ty: "string", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_socket_within_its_contract_when_linux_sock_diag_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.sock-diag",
        targets: &["socket", "connection"],
        target: "socket",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.socket/1", "ono.endpoint/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_connection_within_its_contract_when_linux_sock_diag_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.sock-diag",
        targets: &["socket", "connection"],
        target: "connection",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.socket/1", "ono.endpoint/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Packages, from dpkg and apt. A listing that is not in the declared machine format is a provider defect (E0403), never a source of invented records.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_packages_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.packages",
        targets: &["package"],
        capabilities: &[
            harness::CapabilityClaim { id: "package.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "package.search", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "package.manage", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.package/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_package_1_the_way_linux_packages_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.packages",
        targets: &["package"],
        schema: "ono.package/1",
        identity: &["provider", "name"],
        default_view: &["name", "version", "installed", "description"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "version", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "installed", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "description", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "provider", ty: "string", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_package_within_its_contract_when_linux_packages_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.packages",
        targets: &["package"],
        target: "package",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.package/1"],
        identity_strategy: None,
    }).await;
}

/// Packages, from the rpm database — `rpm -qa --queryformat` for what is installed, and dnf, yum or zypper for what the repositories carry and for changes. The records name `rpm` as their provider on both Red Hat and SUSE, because that is the one database both families keep.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_packages_rpm_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.packages.rpm",
        targets: &["package"],
        capabilities: &[
            harness::CapabilityClaim { id: "package.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "package.search", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "package.manage", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.package/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_package_1_the_way_linux_packages_rpm_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.packages.rpm",
        targets: &["package"],
        schema: "ono.package/1",
        identity: &["provider", "name"],
        default_view: &["name", "version", "installed", "description"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "version", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "installed", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "description", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "provider", ty: "string", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_package_within_its_contract_when_linux_packages_rpm_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.packages.rpm",
        targets: &["package"],
        target: "package",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.package/1"],
        identity_strategy: None,
    }).await;
}

/// Processes, from /proc. Identity is (pid, start time), never pid alone (ADR-0015 T13).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_procfs_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.procfs",
        targets: &["process", "signal"],
        capabilities: &[
            harness::CapabilityClaim { id: "process.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "process.inspect", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "process.signal", risk: "mutate", elevation: "conditional" },
            harness::CapabilityClaim { id: "process.set", risk: "mutate", elevation: "conditional" },
        ],
        schemas: &["ono.process/1", "ono.process-detail/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_process_1_the_way_linux_procfs_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.procfs",
        targets: &["process", "signal"],
        schema: "ono.process/1",
        identity: &["pid", "started"],
        default_view: &["pid", "name", "cpu", "memory", "user"],
        fields: &[
            harness::FieldContract { name: "pid", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "ppid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "command", ty: "list<string>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "executable", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "user", ty: "ref<ono.user/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "group", ty: "ref<ono.group/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<running|sleeping|disk-sleep|stopped|tracing-stop|zombie|dead|idle|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "cpu", ty: "float", required: false, nullable: true, unit: Some("percent") },
            harness::FieldContract { name: "cpu_window", ty: "duration", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "memory", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "virtual_mem", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "threads", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "started", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "cwd", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "service", ty: "ref<ono.service/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "container", ty: "ref<ono.container/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pid_namespace", ty: "int", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_process_detail_1_the_way_linux_procfs_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.procfs",
        targets: &["process", "signal"],
        schema: "ono.process-detail/1",
        identity: &["pid", "started"],
        default_view: &["pid", "name", "parent", "user", "cpu", "memory", "started", "service", "cgroup", "open_files", "sockets"],
        fields: &[
            harness::FieldContract { name: "pid", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "parent", ty: "ref<ono.process/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "command", ty: "list<string>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "executable", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "user", ty: "ref<ono.user/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "group", ty: "ref<ono.group/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<running|sleeping|disk-sleep|stopped|tracing-stop|zombie|dead|idle|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "cpu", ty: "float", required: false, nullable: true, unit: Some("percent") },
            harness::FieldContract { name: "cpu_window", ty: "duration", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "memory", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "virtual_mem", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "threads", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "started", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "cwd", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "service", ty: "ref<ono.service/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "container", ty: "ref<ono.container/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pid_namespace", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "cgroup", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "open_files", ty: "list<path>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "sockets", ty: "list<int>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_process_within_its_contract_when_linux_procfs_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.procfs",
        targets: &["process", "signal"],
        target: "process",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.process/1", "ono.process-detail/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_signal_within_its_contract_when_linux_procfs_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.procfs",
        targets: &["process", "signal"],
        target: "signal",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.process/1", "ono.process-detail/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Files and directories, from the filesystem itself.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_fs_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.fs",
        targets: &["file", "dir"],
        capabilities: &[
            harness::CapabilityClaim { id: "file.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.find", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.read", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.write", risk: "mutate", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.copy", risk: "mutate", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.move", risk: "mutate", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.remove", risk: "destructive", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.set", risk: "mutate", elevation: "conditional" },
            harness::CapabilityClaim { id: "file.open", risk: "mutate", elevation: "none" },
            harness::CapabilityClaim { id: "file.watch", risk: "observe", elevation: "conditional" },
            harness::CapabilityClaim { id: "dir.list", risk: "read", elevation: "conditional" },
        ],
        schemas: &["ono.file/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_file_1_the_way_linux_fs_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.fs",
        targets: &["file", "dir"],
        schema: "ono.file/1",
        identity: &["device", "inode"],
        default_view: &["name", "kind", "size", "modified", "owner"],
        fields: &[
            harness::FieldContract { name: "path", ty: "path", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kind", ty: "enum<file|dir|symlink|socket|fifo|device|other>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "size", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "owner", ty: "ref<ono.user/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "group", ty: "ref<ono.group/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "mode", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "modified", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "accessed", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "created", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "inode", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "device", ty: "ref<ono.device/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "target", ty: "path", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_file_within_its_contract_when_linux_fs_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.fs",
        targets: &["file", "dir"],
        target: "file",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.file/1"],
        identity_strategy: Some("stable"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_dir_within_its_contract_when_linux_fs_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.fs",
        targets: &["file", "dir"],
        target: "dir",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.file/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// Users and groups, through NSS — so LDAP and sssd answer too, not only /etc/passwd.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_nss_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.nss",
        targets: &["user", "group"],
        capabilities: &[
            harness::CapabilityClaim { id: "user.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "group.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "user.manage", risk: "mutate", elevation: "required" },
            harness::CapabilityClaim { id: "group.manage", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.user/1", "ono.group/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_user_1_the_way_linux_nss_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.nss",
        targets: &["user", "group"],
        schema: "ono.user/1",
        identity: &["uid"],
        default_view: &["uid", "name", "home", "shell"],
        fields: &[
            harness::FieldContract { name: "uid", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "primary_group", ty: "ref<ono.group/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "home", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "shell", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "gecos", ty: "string", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_group_1_the_way_linux_nss_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.nss",
        targets: &["user", "group"],
        schema: "ono.group/1",
        identity: &["gid"],
        default_view: &["gid", "name", "members"],
        fields: &[
            harness::FieldContract { name: "gid", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "members", ty: "list<string>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_user_within_its_contract_when_linux_nss_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.nss",
        targets: &["user", "group"],
        target: "user",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.user/1", "ono.group/1"],
        identity_strategy: Some("stable"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_group_within_its_contract_when_linux_nss_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.nss",
        targets: &["user", "group"],
        target: "group",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.user/1", "ono.group/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// The session's own environment. The shell is the source; no kernel interface is asked.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_ono_session_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "ono.session",
        targets: &["env"],
        capabilities: &[
            harness::CapabilityClaim { id: "env.read", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.env-var/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_env_var_1_the_way_ono_session_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.session",
        targets: &["env"],
        schema: "ono.env-var/1",
        identity: &["name"],
        default_view: &["name", "value", "exported"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "value", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "exported", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "source", ty: "enum<inherited|config|invocation|shell>", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_env_within_its_contract_when_ono_session_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.session",
        targets: &["env"],
        target: "env",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.env-var/1"],
        identity_strategy: None,
    }).await;
}

/// The shell's own tables, published by the session before each pipeline runs: the job table of spec §18.4, the links of spec §21 with the hosts they reach and the hosts the configured sources list (ADR-0090, ADR-0103), and the KUANG/11 packages of spec §31.8 — the plugin home overlaid with the runtime instances this session started (ADR-0107) — and the pinned host keys of spec §21.5, which are a decision this shell recorded rather than something a provider found on a machine (ADR-0355), and the client keys this machine authorizes to reach its listening agent (v0.4.1 §9.2, ADR-0468).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_ono_shell_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        capabilities: &[
            harness::CapabilityClaim { id: "job.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "link.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "host.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "host.trust", risk: "mutate", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.search", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.inspect", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.remove", risk: "destructive", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.unload", risk: "mutate", elevation: "none" },
            harness::CapabilityClaim { id: "plugin.set", risk: "mutate", elevation: "none" },
            harness::CapabilityClaim { id: "capability.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "capability.revoke", risk: "mutate", elevation: "none" },
            harness::CapabilityClaim { id: "audit.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "assistant.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "model.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "finding.list", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_job_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.job/1",
        identity: &["id"],
        default_view: &["id", "state", "kind", "command", "started"],
        fields: &[
            harness::FieldContract { name: "id", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kind", ty: "enum<external|native>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state", ty: "enum<running|stopped|done|failed|cancelled>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "command", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "current", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "process_group", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pids", ty: "list<int>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "started", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "exit_status", ty: "int", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_link_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.link/1",
        identity: &["name"],
        default_view: &["name", "host", "transport", "mode", "state", "targets"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "host", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "transport", ty: "enum<ssh|local|tcp>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "mode", ty: "enum<agent|agentless>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state", ty: "enum<defined|connected|closed>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "targets", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "protocol", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "providers", ty: "list<string>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "transport_fingerprint", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "transport_trust", ty: "enum<pinned|newly_pinned|unauthenticated>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "authenticated", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "authorized", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "runtime_user", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "runtime_uid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "runtime_elevated", ty: "bool", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_host_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.host/1",
        identity: &["name"],
        default_view: &["name", "address", "source", "link", "transport"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "address", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "port", ty: "port", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "user", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "link", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "transport", ty: "enum<ssh|local>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_host_key_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.host-key/1",
        identity: &["host"],
        default_view: &["host", "algorithm", "fingerprint"],
        fields: &[
            harness::FieldContract { name: "host", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "algorithm", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "fingerprint", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "path", ty: "path", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_client_key_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.client-key/1",
        identity: &["fingerprint"],
        default_view: &["fingerprint", "label", "observe", "actions"],
        fields: &[
            harness::FieldContract { name: "fingerprint", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "label", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "observe", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "actions", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "path", ty: "path", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_plugin_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.plugin/1",
        identity: &["id", "version"],
        default_view: &["id", "version", "state", "trust", "jobs", "memory"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "version", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "publisher", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state", ty: "enum<installed|enabled|loaded|active|degraded|quarantined>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "trust", ty: "enum<signed|verified|local|unknown|untrusted>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "isolation", ty: "enum<core-built-in|trusted-native|isolated-component|remote-service>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "execution_tier", ty: "enum<native-confined|native-isolated|wasm|remote-service|declarative|core-built-in>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "roles", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "enabled", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "active_version", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "integrity", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kuang_api", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "jobs", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "memory", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state_usage", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "degraded_reason", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "quarantine_reason", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "installed_at", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "loaded_at", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "restart_count", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "last_error", ty: "error", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_plugin_package_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.plugin-package/1",
        identity: &["id", "version", "source"],
        default_view: &["name", "version", "publisher", "signature", "source", "installed"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "version", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "publisher", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "summary", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "license", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kuang_api", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "platforms", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "roles", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "contributions", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "requested_capabilities", ty: "list<map>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "network", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "integrity", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "signature", ty: "enum<valid|invalid|absent|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "trust", ty: "enum<verified|signed|local|unknown|untrusted>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "installed", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "size", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "published_at", ty: "timestamp", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_plugin_inspection_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.plugin-inspection/1",
        identity: &["plugin"],
        default_view: &["plugin", "origin", "memory_current", "open_streams", "restart_count", "last_error"],
        fields: &[
            harness::FieldContract { name: "plugin", ty: "ref<ono.plugin/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "manifest", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "origin", ty: "enum<core|plugin|remote-provider>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "contributions", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "capability_grants", ty: "list<record<ono.capability-grant/1>>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "capability_requests", ty: "list<map>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "verification", ty: "record<ono.verification-result/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "runtime", ty: "record<ono.plugin-runtime/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "memory_current", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "memory_limit", ty: "bytesize", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "cpu_time", ty: "duration", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "host_calls", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "open_streams", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "queued_events", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "dropped_events", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "last_error", ty: "error", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "restart_count", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "network_destinations", ty: "list<map>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state_usage", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state_quota", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "jobs", ty: "list<map>", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_capability_grant_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.capability-grant/1",
        identity: &["id"],
        default_view: &["plugin", "capability", "scope", "duration", "decision", "expires_at"],
        fields: &[
            harness::FieldContract { name: "id", ty: "uuid", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "plugin", ty: "ref<ono.plugin/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "capability", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "class", ty: "enum<required|optional|runtime-requested>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "decision", ty: "enum<allow|deny|ask>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "scope", ty: "map", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "enforcement", ty: "enum<broker|advisory|none>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "duration", ty: "enum<once|command|view|session|link-session|always>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "granted_at", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "expires_at", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "max_uses", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "uses", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "actions", ty: "list<string>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "selector", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "condition", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "source", ty: "enum<system-policy|user-policy|session|prompt|default>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "link", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "purpose", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "revoked_at", ty: "timestamp", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_plugin_audit_event_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.plugin-audit-event/1",
        identity: &["id"],
        default_view: &["at", "plugin", "capability", "action", "target", "result"],
        fields: &[
            harness::FieldContract { name: "id", ty: "uuid", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "plugin", ty: "ref<ono.plugin/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "invocation", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "capability", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "scope", ty: "any", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "enforcement", ty: "enum<broker|advisory>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "action", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "target", ty: "any", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "at", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "result", ty: "enum<success|denied|failed>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "user_confirmation", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "lease", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "link", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "error", ty: "error", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_assistant_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.assistant/1",
        identity: &["id"],
        default_view: &["id", "plugin", "state", "model", "autonomy", "tools"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "plugin", ty: "ref<ono.plugin/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "state", ty: "enum<loaded|ready|busy|degraded>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "model_policy", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "model", ty: "ref<ono.model-provider/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "capabilities", ty: "list<record<ono.capability-grant/1>>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "context_policy", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "autonomy", ty: "enum<L0|L1|L2|L3|L4>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "autonomy_declared", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "tools", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "memory", ty: "enum<turn|conversation|session|persistent>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "conversation", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "turns", ty: "int", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_model_provider_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.model-provider/1",
        identity: &["id"],
        default_view: &["name", "kind", "location", "context_window", "tools", "data_policy"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kind", ty: "enum<local|remote>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "location", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "endpoint", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "context_window", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "tools", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "structured_output", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "streaming", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "data_policy", ty: "enum<local-only|external-ok|redacted-only>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "allowed_classes", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "transformed_classes", ty: "map", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "denied_classes", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "available", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "unavailable_reason", ty: "string", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_finding_1_the_way_ono_shell_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        schema: "ono.finding/1",
        identity: &["id"],
        default_view: &["severity", "subject", "title", "confidence", "source"],
        fields: &[
            harness::FieldContract { name: "id", ty: "uuid", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "subject", ty: "any", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "severity", ty: "enum<info|low|medium|high|critical>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "confidence", ty: "float", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "title", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "summary", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "evidence", ty: "list<record<ono.evidence/1>>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "recommendations", ty: "list<record<ono.recommendation/1>>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "created_at", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "expires_at", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "tags", ty: "map", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_job_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "job",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_link_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "link",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_host_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "host",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_host_key_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "host-key",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_client_key_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "client-key",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_plugin_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "plugin",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_capability_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "capability",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_audit_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "audit",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_assistant_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "assistant",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_model_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "model",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_finding_within_its_contract_when_ono_shell_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.shell",
        targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"],
        target: "finding",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.job/1", "ono.link/1", "ono.host/1", "ono.host-key/1", "ono.client-key/1", "ono.plugin/1", "ono.plugin-package/1", "ono.plugin-inspection/1", "ono.capability-grant/1", "ono.plugin-audit-event/1", "ono.assistant/1", "ono.model-provider/1", "ono.finding/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Mounts and filesystems, from /proc/self/mountinfo and statvfs; mounting, unmounting and remounting through mount(2) and umount2(2) (ADR-0098).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_mountinfo_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.mountinfo",
        targets: &["mount", "filesystem"],
        capabilities: &[
            harness::CapabilityClaim { id: "mount.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "filesystem.list", risk: "read", elevation: "conditional" },
            harness::CapabilityClaim { id: "mount.manage", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.mount/1", "ono.filesystem/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_mount_1_the_way_linux_mountinfo_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.mountinfo",
        targets: &["mount", "filesystem"],
        schema: "ono.mount/1",
        identity: &["target"],
        default_view: &["target", "source", "filesystem", "read_only"],
        fields: &[
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "target", ty: "path", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "filesystem", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "options", ty: "list<string>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "read_only", ty: "bool", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "device", ty: "ref<ono.device/1>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "peer_group", ty: "int", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_filesystem_1_the_way_linux_mountinfo_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.mountinfo",
        targets: &["mount", "filesystem"],
        schema: "ono.filesystem/1",
        identity: &["uuid", "source"],
        default_view: &["source", "type", "size", "used", "available", "target"],
        fields: &[
            harness::FieldContract { name: "source", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "type", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "uuid", ty: "uuid", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "label", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "target", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "size", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "used", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "available", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "read_only", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "device", ty: "ref<ono.device/1>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_mount_within_its_contract_when_linux_mountinfo_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.mountinfo",
        targets: &["mount", "filesystem"],
        target: "mount",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.mount/1", "ono.filesystem/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_filesystem_within_its_contract_when_linux_mountinfo_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.mountinfo",
        targets: &["mount", "filesystem"],
        target: "filesystem",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.mount/1", "ono.filesystem/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

/// Block and character devices, from the nodes under /dev and their sysfs entries (ADR-0097).
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_sysfs_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.sysfs",
        targets: &["device"],
        capabilities: &[
            harness::CapabilityClaim { id: "device.list", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.device/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_device_1_the_way_linux_sysfs_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.sysfs",
        targets: &["device"],
        schema: "ono.device/1",
        identity: &["path"],
        default_view: &["path", "kind", "major", "minor", "size", "subsystem"],
        fields: &[
            harness::FieldContract { name: "path", ty: "path", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "kind", ty: "enum<block|char>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "major", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "minor", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "size", ty: "bytesize", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "subsystem", ty: "string", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_device_within_its_contract_when_linux_sysfs_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.sysfs",
        targets: &["device"],
        target: "device",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.device/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// Names and addresses, through the C library's resolver (getaddrinfo / getnameinfo) — NSS, so /etc/hosts, DNS, mDNS and LDAP all answer, exactly as they do for every other program.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_linux_resolver_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "linux.resolver",
        targets: &["dns"],
        capabilities: &[
            harness::CapabilityClaim { id: "dns.resolve", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.dns-record/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_dns_record_1_the_way_linux_resolver_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "linux.resolver",
        targets: &["dns"],
        schema: "ono.dns-record/1",
        identity: &["name", "type", "address"],
        default_view: &["name", "type", "address"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "type", ty: "enum<A|AAAA|PTR>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "address", ty: "ip", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_dns_within_its_contract_when_linux_resolver_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "linux.resolver",
        targets: &["dns"],
        target: "dns",
        exercise: harness::Exercise::SelectorRequired,
        schemas: &["ono.dns-record/1"],
        identity_strategy: None,
    }).await;
}

/// Reachability probes: the shell itself connects to a host and port and reports what it found, with timing. A refused or silent port is the answer, not an error.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_ono_probe_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "ono.probe",
        targets: &["port"],
        capabilities: &[
            harness::CapabilityClaim { id: "port.probe", risk: "observe", elevation: "none" },
        ],
        schemas: &["ono.probe-result/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_probe_result_1_the_way_ono_probe_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "ono.probe",
        targets: &["port"],
        schema: "ono.probe-result/1",
        identity: &[],
        default_view: &["host", "port", "protocol", "reachable", "duration", "error"],
        fields: &[
            harness::FieldContract { name: "host", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "port", ty: "port", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "protocol", ty: "enum<tcp|udp|icmp|ono>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "reachable", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "duration", ty: "duration", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "error", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "transport", ty: "enum<ssh|local|tcp>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "protocol_version", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "agent", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "providers", ty: "list<string>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_port_within_its_contract_when_ono_probe_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "ono.probe",
        targets: &["port"],
        target: "port",
        exercise: harness::Exercise::SelectorRequired,
        schemas: &["ono.probe-result/1"],
        identity_strategy: None,
    }).await;
}

/// Services, from org.freedesktop.systemd1 over D-Bus.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_systemd_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "systemd",
        targets: &["service"],
        capabilities: &[
            harness::CapabilityClaim { id: "service.list", risk: "read", elevation: "none" },
            harness::CapabilityClaim { id: "service.manage", risk: "mutate", elevation: "required" },
        ],
        schemas: &["ono.service/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_service_1_the_way_systemd_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "systemd",
        targets: &["service"],
        schema: "ono.service/1",
        identity: &["provider", "name"],
        default_view: &["name", "state", "substate", "enabled", "description"],
        fields: &[
            harness::FieldContract { name: "name", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "description", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<active|reloading|inactive|failed|activating|deactivating|unknown>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "substate", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "enabled", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "since", ty: "timestamp", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "provider", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "unit_file", ty: "path", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "dependencies", ty: "list<string>", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_service_within_its_contract_when_systemd_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "systemd",
        targets: &["service"],
        target: "service",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.service/1"],
        identity_strategy: Some("stable"),
    }).await;
}

/// The journal and the log, from `journalctl --output=json` through the decoder of the systemd adapter pack (ADR-0085). Registered even where no `journalctl` is on PATH or no journal files exist: that is `provider.unavailable` with the reason, not an empty journal.
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_systemd_journal_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "systemd-journal",
        targets: &["journal", "log"],
        capabilities: &[
            harness::CapabilityClaim { id: "log.read", risk: "read", elevation: "conditional" },
        ],
        schemas: &["ono.journal-event/1", "ono.log-record/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_journal_event_1_the_way_systemd_journal_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "systemd-journal",
        targets: &["journal", "log"],
        schema: "ono.journal-event/1",
        identity: &["cursor"],
        default_view: &["timestamp", "priority", "identifier", "message"],
        fields: &[
            harness::FieldContract { name: "timestamp", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "priority", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "message", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "identifier", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "unit", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "uid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "boot_id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "host", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "cursor", ty: "string", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_log_record_1_the_way_systemd_journal_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "systemd-journal",
        targets: &["journal", "log"],
        schema: "ono.log-record/1",
        identity: &["cursor"],
        default_view: &["timestamp", "level", "unit", "message"],
        fields: &[
            harness::FieldContract { name: "timestamp", ty: "timestamp", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "level", ty: "enum<debug|info|notice|warning|error|crit|alert|emerg>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "priority", ty: "int", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "message", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "identifier", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "unit", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "pid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "uid", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "boot_id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "host", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "cursor", ty: "string", required: true, nullable: false, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_journal_within_its_contract_when_systemd_journal_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "systemd-journal",
        targets: &["journal", "log"],
        target: "journal",
        exercise: harness::Exercise::Unbounded,
        schemas: &["ono.journal-event/1", "ono.log-record/1"],
        identity_strategy: None,
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_log_within_its_contract_when_systemd_journal_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "systemd-journal",
        targets: &["journal", "log"],
        target: "log",
        exercise: harness::Exercise::Unbounded,
        schemas: &["ono.journal-event/1", "ono.log-record/1"],
        identity_strategy: None,
    }).await;
}

/// Login sessions, from org.freedesktop.login1 over D-Bus (ADR-0100). Registered even when no login manager answers: that is `provider.unavailable` with the reason, not "no sessions".
#[rustfmt::skip]
#[tokio::test]
async fn should_advertise_exactly_what_systemd_logind_declares() {
    harness::assert_surface(&harness::Surface {
        provider: "systemd-logind",
        targets: &["session"],
        capabilities: &[
            harness::CapabilityClaim { id: "session.list", risk: "read", elevation: "none" },
        ],
        schemas: &["ono.session/1"],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_shape_ono_session_1_the_way_systemd_logind_declares_it() {
    harness::assert_schema_contract(&harness::SchemaContract {
        provider: "systemd-logind",
        targets: &["session"],
        schema: "ono.session/1",
        identity: &["id"],
        default_view: &["id", "user", "seat", "tty", "type", "state", "since"],
        fields: &[
            harness::FieldContract { name: "id", ty: "string", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "user", ty: "ref<ono.user/1>", required: true, nullable: false, unit: None },
            harness::FieldContract { name: "seat", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "tty", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "display", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "type", ty: "enum<tty|x11|wayland|mir|web|unspecified>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "class", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "state", ty: "enum<online|active|closing|unknown>", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "remote", ty: "bool", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "remote_host", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "service", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "leader", ty: "int", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "scope", ty: "string", required: false, nullable: true, unit: None },
            harness::FieldContract { name: "since", ty: "timestamp", required: false, nullable: true, unit: None },
        ],
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_answer_for_session_within_its_contract_when_systemd_logind_is_asked() {
    harness::assert_target_conforms(&harness::TargetCase {
        provider: "systemd-logind",
        targets: &["session"],
        target: "session",
        exercise: harness::Exercise::Enumerable,
        schemas: &["ono.session/1"],
        identity_strategy: Some("lifetime"),
    }).await;
}

#[rustfmt::skip]
#[tokio::test]
async fn should_account_for_every_capability_the_declarations_name() {
    harness::assert_accounts(&[
        harness::Account { provider: "container-engine", targets: &["container", "image"], capability: "container.list", risk: "read", through: harness::Through::Snapshot("container") },
        harness::Account { provider: "container-engine", targets: &["container", "image"], capability: "image.list", risk: "read", through: harness::Through::Snapshot("image") },
        harness::Account { provider: "container-engine", targets: &["container", "image"], capability: "container.manage", risk: "mutate", through: harness::Through::Command(&["ono.container.remove", "ono.container.restart", "ono.container.set", "ono.container.start", "ono.container.stop"]) },
        harness::Account { provider: "linux.netlink", targets: &["interface"], capability: "interface.list", risk: "read", through: harness::Through::Snapshot("interface") },
        harness::Account { provider: "linux.netlink", targets: &["interface"], capability: "interface.set", risk: "mutate", through: harness::Through::Command(&["ono.interface.add", "ono.interface.remove", "ono.interface.set", "ono.interface.start", "ono.interface.stop"]) },
        harness::Account { provider: "linux.netlink", targets: &["route"], capability: "route.list", risk: "read", through: harness::Through::Snapshot("route") },
        harness::Account { provider: "linux.netlink", targets: &["route"], capability: "route.set", risk: "mutate", through: harness::Through::Command(&["ono.route.add", "ono.route.remove", "ono.route.set"]) },
        harness::Account { provider: "linux.netlink", targets: &["neighbor"], capability: "neighbor.list", risk: "read", through: harness::Through::Snapshot("neighbor") },
        harness::Account { provider: "linux.sock-diag", targets: &["socket", "connection"], capability: "socket.list", risk: "read", through: harness::Through::Snapshot("socket") },
        harness::Account { provider: "linux.sock-diag", targets: &["socket", "connection"], capability: "connection.list", risk: "read", through: harness::Through::Snapshot("connection") },
        harness::Account { provider: "linux.sock-diag", targets: &["socket", "connection"], capability: "socket.close", risk: "destructive", through: harness::Through::Command(&["ono.socket.stop"]) },
        harness::Account { provider: "linux.packages", targets: &["package"], capability: "package.list", risk: "read", through: harness::Through::Snapshot("package") },
        harness::Account { provider: "linux.packages", targets: &["package"], capability: "package.search", risk: "read", through: harness::Through::Snapshot("package") },
        harness::Account { provider: "linux.packages", targets: &["package"], capability: "package.manage", risk: "mutate", through: harness::Through::Command(&["ono.package.add", "ono.package.remove", "ono.package.set"]) },
        harness::Account { provider: "linux.packages.rpm", targets: &["package"], capability: "package.list", risk: "read", through: harness::Through::Snapshot("package") },
        harness::Account { provider: "linux.packages.rpm", targets: &["package"], capability: "package.search", risk: "read", through: harness::Through::Snapshot("package") },
        harness::Account { provider: "linux.packages.rpm", targets: &["package"], capability: "package.manage", risk: "mutate", through: harness::Through::Command(&["ono.package.add", "ono.package.remove", "ono.package.set"]) },
        harness::Account { provider: "linux.procfs", targets: &["process", "signal"], capability: "process.list", risk: "read", through: harness::Through::Snapshot("process") },
        harness::Account { provider: "linux.procfs", targets: &["process", "signal"], capability: "process.inspect", risk: "read", through: harness::Through::Snapshot("process") },
        harness::Account { provider: "linux.procfs", targets: &["process", "signal"], capability: "process.signal", risk: "mutate", through: harness::Through::Command(&["ono.process.kill", "ono.process.stop", "ono.signal.send"]) },
        harness::Account { provider: "linux.procfs", targets: &["process", "signal"], capability: "process.set", risk: "mutate", through: harness::Through::Command(&["ono.process.set"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.list", risk: "read", through: harness::Through::Snapshot("file") },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.find", risk: "read", through: harness::Through::Snapshot("file") },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.read", risk: "read", through: harness::Through::Snapshot("file") },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.write", risk: "mutate", through: harness::Through::Command(&["ono.file.write"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.copy", risk: "mutate", through: harness::Through::Command(&["ono.file.copy"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.move", risk: "mutate", through: harness::Through::Command(&["ono.file.move"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.remove", risk: "destructive", through: harness::Through::Command(&["ono.dir.remove", "ono.file.remove"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.set", risk: "mutate", through: harness::Through::Command(&["ono.dir.set", "ono.file.set"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.open", risk: "mutate", through: harness::Through::Command(&["ono.file.open"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "file.watch", risk: "observe", through: harness::Through::Command(&["ono.file.tail", "ono.file.watch"]) },
        harness::Account { provider: "linux.fs", targets: &["file", "dir"], capability: "dir.list", risk: "read", through: harness::Through::Snapshot("dir") },
        harness::Account { provider: "linux.nss", targets: &["user", "group"], capability: "user.list", risk: "read", through: harness::Through::Snapshot("user") },
        harness::Account { provider: "linux.nss", targets: &["user", "group"], capability: "group.list", risk: "read", through: harness::Through::Snapshot("group") },
        harness::Account { provider: "linux.nss", targets: &["user", "group"], capability: "user.manage", risk: "mutate", through: harness::Through::Command(&["ono.user.add", "ono.user.remove", "ono.user.set"]) },
        harness::Account { provider: "linux.nss", targets: &["user", "group"], capability: "group.manage", risk: "mutate", through: harness::Through::Command(&["ono.group.add", "ono.group.remove", "ono.group.set"]) },
        harness::Account { provider: "ono.session", targets: &["env"], capability: "env.read", risk: "read", through: harness::Through::Snapshot("env") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "job.list", risk: "read", through: harness::Through::Snapshot("job") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "link.list", risk: "read", through: harness::Through::Snapshot("link") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "host.list", risk: "read", through: harness::Through::Snapshot("host") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "host.trust", risk: "mutate", through: harness::Through::Command(&["ono.client-key.add", "ono.client-key.remove", "ono.client-key.set", "ono.host-key.add", "ono.host-key.remove", "ono.host-key.set"]) },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.list", risk: "read", through: harness::Through::Snapshot("plugin") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.search", risk: "read", through: harness::Through::Snapshot("plugin") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.inspect", risk: "read", through: harness::Through::Snapshot("plugin") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.remove", risk: "destructive", through: harness::Through::Command(&["ono.plugin.remove"]) },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.unload", risk: "mutate", through: harness::Through::Command(&["ono.plugin.unload"]) },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "plugin.set", risk: "mutate", through: harness::Through::Command(&["ono.plugin.set"]) },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "capability.list", risk: "read", through: harness::Through::Snapshot("capability") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "capability.revoke", risk: "mutate", through: harness::Through::Command(&["ono.capability.revoke"]) },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "audit.list", risk: "read", through: harness::Through::Snapshot("audit") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "assistant.list", risk: "read", through: harness::Through::Snapshot("assistant") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "model.list", risk: "read", through: harness::Through::Snapshot("model") },
        harness::Account { provider: "ono.shell", targets: &["job", "link", "host", "host-key", "client-key", "plugin", "capability", "audit", "assistant", "model", "finding"], capability: "finding.list", risk: "read", through: harness::Through::Snapshot("finding") },
        harness::Account { provider: "linux.mountinfo", targets: &["mount", "filesystem"], capability: "mount.list", risk: "read", through: harness::Through::Snapshot("mount") },
        harness::Account { provider: "linux.mountinfo", targets: &["mount", "filesystem"], capability: "filesystem.list", risk: "read", through: harness::Through::Snapshot("filesystem") },
        harness::Account { provider: "linux.mountinfo", targets: &["mount", "filesystem"], capability: "mount.manage", risk: "mutate", through: harness::Through::Command(&["ono.filesystem.mount", "ono.filesystem.unmount", "ono.mount.add", "ono.mount.remove", "ono.mount.set", "ono.mount.start", "ono.mount.stop"]) },
        harness::Account { provider: "linux.sysfs", targets: &["device"], capability: "device.list", risk: "read", through: harness::Through::Snapshot("device") },
        harness::Account { provider: "linux.resolver", targets: &["dns"], capability: "dns.resolve", risk: "read", through: harness::Through::Snapshot("dns") },
        harness::Account { provider: "ono.probe", targets: &["port"], capability: "port.probe", risk: "observe", through: harness::Through::Command(&["ono.port.test"]) },
        harness::Account { provider: "systemd", targets: &["service"], capability: "service.list", risk: "read", through: harness::Through::Snapshot("service") },
        harness::Account { provider: "systemd", targets: &["service"], capability: "service.manage", risk: "mutate", through: harness::Through::Command(&["ono.service.restart", "ono.service.set", "ono.service.start", "ono.service.stop"]) },
        harness::Account { provider: "systemd-journal", targets: &["journal", "log"], capability: "log.read", risk: "read", through: harness::Through::Snapshot("log") },
        harness::Account { provider: "systemd-logind", targets: &["session"], capability: "session.list", risk: "read", through: harness::Through::Snapshot("session") },
    ]).await;
}
