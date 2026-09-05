//! The object contracts these providers answer against.
//!
//! Each schema is `docs/contracts/schemas/<name>.v1.yaml` transcribed field for field, in the order
//! the contract lists the fields, with the contract's own documentation on each one. The
//! registry is the public API surface (AGENTS.md §6): the shape is decided there and implemented
//! here, never the other way round.

use std::sync::{Arc, OnceLock};

use ono_value::{FieldDef, FieldType, Schema, SchemaBuilder, SchemaId, Unit};

/// `ono.interface/1` — a network interface with its addresses, state and counters (spec §28.5).
///
/// The identity is the kernel interface index, not the name: a name can be changed while the
/// interface stays the same thing, and an identity that moves is not an identity.
#[must_use]
pub fn interface_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        build(
            Schema::builder(SchemaId::new("ono.interface", 1), "Interface")
                .doc("A network interface with its addresses, operational state and counters.")
                .field(
                    FieldDef::new("name", FieldType::String)
                        .required()
                        .with_doc(
                            "The interface name. Renameable, so a reference but not the identity.",
                        ),
                )
                .field(
                    FieldDef::new("index", FieldType::Int)
                        .required()
                        .with_doc("The kernel interface index."),
                )
                .field(FieldDef::new("mac", FieldType::String).nullable().with_doc(
                    "The link-layer address; null for interfaces that have none, such as \
                     loopback or tun.",
                ))
                .field(
                    FieldDef::new(
                        "state",
                        FieldType::enumeration(&[
                            "up",
                            "down",
                            "unknown",
                            "dormant",
                            "testing",
                            "lower-layer-down",
                            "not-present",
                        ]),
                    )
                    .required()
                    .with_doc(
                        "The operational state as the kernel reports it, not the administrative \
                         flag; the flag is the `netlink.admin_up` extension.",
                    ),
                )
                .field(
                    FieldDef::new("mtu", FieldType::Int)
                        .required()
                        .with_unit(Unit::Bytes)
                        .with_doc("The maximum transmission unit in bytes."),
                )
                .field(
                    FieldDef::new("addresses", FieldType::list(FieldType::IpNetwork))
                        .required()
                        .with_doc(
                            "Configured addresses with their prefix lengths; an empty list when \
                             none are configured.",
                        ),
                )
                .field(
                    FieldDef::new("rx_bytes", FieldType::ByteSize)
                        .nullable()
                        .with_unit(Unit::Bytes)
                        .with_doc(
                            "Bytes received since the counter was last reset; null when the \
                             provider has no counters.",
                        ),
                )
                .field(
                    FieldDef::new("tx_bytes", FieldType::ByteSize)
                        .nullable()
                        .with_unit(Unit::Bytes)
                        .with_doc("Bytes transmitted since the counter was last reset."),
                )
                .identity(["index"])
                .default_view(["name", "state", "addresses", "mtu", "mac"]),
        )
    }))
}

/// `ono.route/1` — a routing table entry.
#[must_use]
pub fn route_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        build(
            Schema::builder(SchemaId::new("ono.route", 1), "Route")
                .doc("A routing table entry.")
                .field(FieldDef::new("destination", FieldType::IpNetwork).nullable().with_doc(
                    "The destination prefix. Null is the default route — a real answer, not an \
                     unknown one.",
                ))
                .field(
                    FieldDef::new("gateway", FieldType::Ip)
                        .nullable()
                        .with_doc("The next hop; null for a directly connected route."),
                )
                .field(
                    FieldDef::new("interface", FieldType::Ref(SchemaId::new("ono.interface", 1)))
                        .nullable()
                        .with_doc(
                            "The outgoing interface, by name where the kernel named it and by \
                             index otherwise; null for routes that name none, such as blackholes.",
                        ),
                )
                .field(
                    FieldDef::new("source", FieldType::Ip)
                        .nullable()
                        .with_doc("The preferred source address for traffic taking this route."),
                )
                .field(
                    FieldDef::new("family", FieldType::enumeration(&["inet", "inet6"]))
                        .required()
                        .with_doc("The address family of the route."),
                )
                .field(
                    FieldDef::new(
                        "type",
                        FieldType::enumeration(&[
                            "unicast",
                            "local",
                            "broadcast",
                            "multicast",
                            "anycast",
                            "blackhole",
                            "unreachable",
                            "prohibit",
                            "throw",
                            "other",
                        ]),
                    )
                    .nullable()
                    .with_doc("The route type; null when the provider does not distinguish types."),
                )
                .field(
                    FieldDef::new(
                        "scope",
                        FieldType::enumeration(&["universe", "site", "link", "host", "nowhere"]),
                    )
                    .nullable()
                    .with_doc("The scope of the destination."),
                )
                .field(FieldDef::new("protocol", FieldType::String).nullable().with_doc(
                    "What installed the route — `kernel`, `static`, `dhcp`, `ra`, a routing \
                     daemon's name. A string because the set is the system's registry of route \
                     origins, not Ono's.",
                ))
                .field(FieldDef::new("metric", FieldType::Int).nullable().with_doc(
                    "The route priority; lower wins. Null when the provider supplies no metric.",
                ))
                .field(FieldDef::new("table", FieldType::String).nullable().with_doc(
                    "The routing table the entry belongs to, by name where one exists, otherwise \
                     its number.",
                ))
                .identity(["table", "family", "destination", "gateway", "interface"])
                .default_view(["destination", "gateway", "interface", "metric", "protocol"]),
        )
    }))
}

/// `ono.neighbor/1` — an ARP or NDP neighbour table entry.
#[must_use]
pub fn neighbor_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        build(
            Schema::builder(SchemaId::new("ono.neighbor", 1), "Neighbor")
                .doc("An ARP or NDP neighbour table entry.")
                .field(
                    FieldDef::new("address", FieldType::Ip)
                        .required()
                        .with_doc("The neighbour's network address."),
                )
                .field(FieldDef::new("mac", FieldType::String).nullable().with_doc(
                    "The resolved link-layer address; null for an incomplete or failed entry. \
                     Null here means \"not resolved\", which is what spec §10.5 requires it to \
                     mean.",
                ))
                .field(
                    FieldDef::new("interface", FieldType::Ref(SchemaId::new("ono.interface", 1)))
                        .required()
                        .with_doc(
                            "The interface the neighbour was seen on; part of the identity, since \
                             addresses repeat.",
                        ),
                )
                .field(
                    FieldDef::new("family", FieldType::enumeration(&["inet", "inet6"]))
                        .required()
                        .with_doc("The address family."),
                )
                .field(
                    FieldDef::new(
                        "state",
                        FieldType::enumeration(&[
                            "incomplete",
                            "reachable",
                            "stale",
                            "delay",
                            "probe",
                            "failed",
                            "permanent",
                            "noarp",
                            "none",
                        ]),
                    )
                    .required()
                    .with_doc("The neighbour cache state as the kernel reports it."),
                )
                .field(
                    FieldDef::new("router", FieldType::Bool)
                        .nullable()
                        .with_doc(
                            "Whether the neighbour advertises itself as a router; null outside NDP.",
                        ),
                )
                .field(
                    FieldDef::new("updated", FieldType::Timestamp)
                        .nullable()
                        .with_doc(
                            "When the entry was last confirmed; null when the provider keeps no \
                             timestamp.",
                        ),
                )
                .identity(["address", "interface"])
                .default_view(["address", "mac", "interface", "state"]),
        )
    }))
}

/// `ono.socket/1` — a socket with its endpoints, owner and state (spec §28.4).
#[must_use]
pub fn socket_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        build(
            Schema::builder(SchemaId::new("ono.socket", 1), "Socket")
                .doc("A socket with its endpoints, owner and state.")
                .field(
                    FieldDef::new(
                        "protocol",
                        FieldType::enumeration(&[
                            "tcp", "udp", "unix", "raw", "sctp", "dccp", "packet", "unknown",
                        ]),
                    )
                    .required()
                    .with_doc("The transport protocol."),
                )
                .field(
                    FieldDef::new(
                        "family",
                        FieldType::enumeration(&[
                            "inet", "inet6", "unix", "packet", "netlink", "other",
                        ]),
                    )
                    .required()
                    .with_doc("The address family."),
                )
                .field(
                    FieldDef::new("local", FieldType::Record(SchemaId::new("ono.endpoint", 1)))
                        .nullable()
                        .with_doc("The local endpoint; null for socket kinds that have none."),
                )
                .field(
                    FieldDef::new(
                        "remote",
                        FieldType::Record(SchemaId::new("ono.endpoint", 1)),
                    )
                    .nullable()
                    .with_doc("The peer endpoint; null for listening and connectionless sockets."),
                )
                .field(
                    FieldDef::new(
                        "state",
                        FieldType::enumeration(&[
                            "established",
                            "syn-sent",
                            "syn-recv",
                            "fin-wait-1",
                            "fin-wait-2",
                            "time-wait",
                            "close",
                            "close-wait",
                            "last-ack",
                            "listen",
                            "closing",
                            "unknown",
                        ]),
                    )
                    .nullable()
                    .with_doc(
                        "The connection state; null for protocols that have none, such as UDP.",
                    ),
                )
                .field(
                    FieldDef::new("process", FieldType::Ref(SchemaId::new("ono.process", 1)))
                        .nullable()
                        .with_doc(
                            "The owning process, as an identity map of `pid` and `name`. Null \
                             unless `--process` was given, because finding the owner means \
                             scanning every `/proc/<pid>/fd` on the machine, and null again when \
                             that scan saw every process and none of them held this socket. \
                             Where the scan was refused a process, the field carries that \
                             refusal as an `io.permission_denied` error rather than a null: an \
                             owner this reader may not see is denied, not absent (v0.4 §35.2, \
                             §2.17).",
                        ),
                )
                .field(
                    FieldDef::new("user", FieldType::Ref(SchemaId::new("ono.user", 1)))
                        .nullable()
                        .with_doc("The owning user, by numeric uid."),
                )
                .field(FieldDef::new("inode", FieldType::Int).nullable().with_doc(
                    "The socket inode; the identity field, null when the kernel supplies \
                             none, as it does for a socket in `time-wait`.",
                ))
                .identity(["inode"])
                // A `time-wait` socket has no inode, and it is still a connection the kernel is
                // reporting. The tuple every network stack names a connection by joins the
                // identity for exactly those sockets (ADR-0554).
                .identity_fallback(["protocol", "local", "remote"])
                .default_view(["protocol", "local", "remote", "state", "process"]),
        )
    }))
}

/// `ono.endpoint/1` — one end of a socket.
///
/// A structural sub-record rather than an addressable object, so it declares no identity.
#[must_use]
pub fn endpoint_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        build(
            Schema::builder(SchemaId::new("ono.endpoint", 1), "Endpoint")
                .doc(
                    "One end of a socket: an address and port, or a filesystem path for a Unix \
                     socket.",
                )
                .field(
                    FieldDef::new("address", FieldType::Ip)
                        .nullable()
                        .with_doc("The IP address; null for a Unix-domain or unbound endpoint."),
                )
                .field(
                    FieldDef::new("port", FieldType::Port)
                        .nullable()
                        .with_doc("The transport port; null where the protocol has none."),
                )
                .field(
                    FieldDef::new("path", FieldType::Path)
                        .nullable()
                        .with_doc(
                            "The filesystem path of a Unix-domain socket, with a leading `@` for \
                             an abstract name; null for every other family.",
                        ),
                )
                .field(FieldDef::new("host", FieldType::String).nullable().with_doc(
                    "The reverse-resolved host name. Null unless resolution was requested and \
                     succeeded; spec §22.2 classes it as derived, so it is never substituted for \
                     `address`.",
                ))
                .default_view(["address", "port"]),
        )
    }))
}

/// Every schema this crate produces.
#[must_use]
pub fn schemas() -> Vec<Arc<Schema>> {
    vec![
        interface_schema(),
        route_schema(),
        neighbor_schema(),
        socket_schema(),
        endpoint_schema(),
    ]
}

/// Finishes a schema definition.
///
/// [`SchemaBuilder::build`] fails only when a definition declares a field twice, or names a
/// field in its identity or default view that it does not declare. Every definition in this file
/// is a literal, so that is a compile-time property of the text above rather than anything a
/// caller can cause, and `tests/schema_contract.rs` asserts each one field by field. AGENTS.md
/// §16 allows `expect` in exactly this position: a provably unreachable state with a comment
/// saying why.
#[expect(
    clippy::expect_used,
    reason = "the schema definitions above are literals; a failure here is a bug in this file \
              that tests/schema_contract.rs fails on first"
)]
fn build(builder: SchemaBuilder) -> Arc<Schema> {
    Arc::new(
        builder
            .build()
            .expect("a schema definition in this crate is well formed"),
    )
}
