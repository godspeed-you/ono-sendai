//! The canonical object schemas of spec §28.
//!
//! These are the contract the Linux providers of phase C implement against, and the reason the
//! field names and their nullability are asserted field by field in the crate's tests: a
//! provider that quietly renames `virtual_mem` breaks every script that reads it, and only a
//! test that spells the contract out catches that.
//!
//! Where spec §28 leaves a detail open — the exact members of an enumeration, the identity of a
//! type it gives no identity line for, the default view of a type spec §27.3 does not
//! exemplify — the choice is made here and stated in the schema's documentation.

use std::sync::{Arc, OnceLock};

use crate::{FieldDef, FieldType, Schema, SchemaId, SchemaRegistry, Unit};

/// Every schema the shell ships with, by id (spec §28).
///
/// ```
/// use ono_value::{SchemaId, builtin_schemas};
/// let process = builtin_schemas()
///     .get(&SchemaId::new("ono.process", 1))
///     .expect("the process schema ships with the shell");
/// assert_eq!(process.name(), "Process");
/// ```
#[must_use]
pub fn builtin_schemas() -> &'static SchemaRegistry {
    static REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();
    // A schema that fails to build would leave the registry short of that entry, which the
    // per-schema tests in `tests/builtin_schemas.rs` fail on immediately.
    REGISTRY.get_or_init(|| build_registry().unwrap_or_default())
}

/// The `ActionResult` schema of spec §11.5 and §28.8.
#[must_use]
pub fn action_result_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.action-result", 1))
            .unwrap_or_else(|| {
                // Only reachable if the definition below stopped building, which the crate's
                // schema tests fail on. An empty schema keeps the shell running rather than
                // taking the process down over a contract bug.
                Arc::new(Schema::empty(
                    SchemaId::new("ono.action-result", 1),
                    "ActionResult",
                ))
            })
    }))
}

fn build_registry() -> Result<SchemaRegistry, crate::ErrorValue> {
    let mut registry = SchemaRegistry::new();
    for schema in [
        process()?,
        file()?,
        service()?,
        socket()?,
        interface()?,
        mount()?,
        user()?,
        group()?,
        route()?,
        neighbor()?,
        action_result_definition()?,
    ] {
        registry.register(schema)?;
    }
    Ok(registry)
}

fn user_ref() -> FieldType {
    FieldType::Ref(SchemaId::new("ono.user", 1))
}

fn group_ref() -> FieldType {
    FieldType::Ref(SchemaId::new("ono.group", 1))
}

fn device_ref() -> FieldType {
    FieldType::Ref(SchemaId::new("ono.device", 1))
}

/// Spec §28.1.
fn process() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.process", 1), "Process")
        .doc("A running process, as spec §28.1 defines it.")
        .field(
            FieldDef::new("pid", FieldType::Int)
                .required()
                .with_doc("Process id."),
        )
        .field(
            FieldDef::new("ppid", FieldType::Int)
                .nullable()
                .with_doc("Parent process id."),
        )
        .field(
            FieldDef::new("name", FieldType::String)
                .required()
                .with_doc("Executable name."),
        )
        .field(
            FieldDef::new("command", FieldType::list(FieldType::String))
                .nullable()
                .with_doc("Argument vector, unsplit and unquoted."),
        )
        .field(
            FieldDef::new("executable", FieldType::Path)
                .nullable()
                .with_doc("Resolved binary."),
        )
        .field(
            FieldDef::new("user", user_ref())
                .nullable()
                .with_doc("Owning user."),
        )
        .field(
            FieldDef::new("group", group_ref())
                .nullable()
                .with_doc("Owning group."),
        )
        .field(
            FieldDef::new("state", process_state())
                .required()
                .with_doc("Scheduler state."),
        )
        .field(
            FieldDef::new("cpu", FieldType::Float)
                .nullable()
                .with_unit(Unit::Percent)
                .with_doc("Percent of one logical CPU, unless a provider documents otherwise."),
        )
        .field(
            FieldDef::new("memory", FieldType::ByteSize)
                .nullable()
                .with_doc("Resident set size."),
        )
        .field(
            FieldDef::new("virtual_mem", FieldType::ByteSize)
                .nullable()
                .with_doc("Virtual address space size."),
        )
        .field(
            FieldDef::new("threads", FieldType::Int)
                .nullable()
                .with_doc("Thread count."),
        )
        .field(
            FieldDef::new("started", FieldType::Timestamp)
                .nullable()
                .with_doc("Start time, which spec §23.1 makes part of the identity."),
        )
        .field(
            FieldDef::new("cwd", FieldType::Path)
                .nullable()
                .with_doc("Working directory."),
        )
        .field(
            FieldDef::new("service", FieldType::Ref(SchemaId::new("ono.service", 1)))
                .nullable()
                .with_doc("Owning service, where one claims the process."),
        )
        .field(
            FieldDef::new(
                "container",
                FieldType::Ref(SchemaId::new("ono.container", 1)),
            )
            .nullable()
            .with_doc("Owning container, where one claims the process."),
        )
        .identity(["pid", "started"])
        .default_view(["pid", "name", "cpu", "memory", "user"])
        .build()
}

/// The scheduler states a Linux process can be in, plus `unknown` for a provider that cannot
/// tell. Spec §28.1 declares the field an enumeration without listing its members.
fn process_state() -> FieldType {
    FieldType::enumeration(&[
        "running",
        "sleeping",
        "waiting",
        "stopped",
        "tracing-stopped",
        "zombie",
        "dead",
        "idle",
        "unknown",
    ])
}

/// Spec §28.2.
fn file() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.file", 1), "File")
        .doc("A filesystem entry, as spec §28.2 defines it.")
        .field(
            FieldDef::new("path", FieldType::Path)
                .required()
                .with_doc("Path this entry was reached by."),
        )
        .field(
            FieldDef::new("name", FieldType::String)
                .required()
                .with_doc("Final path component."),
        )
        .field(
            FieldDef::new(
                "kind",
                FieldType::enumeration(&[
                    "file", "dir", "symlink", "socket", "fifo", "device", "other",
                ]),
            )
            .required()
            .with_doc("What kind of entry this is."),
        )
        .field(
            FieldDef::new("size", FieldType::ByteSize)
                .nullable()
                .with_doc("Size in bytes."),
        )
        .field(
            FieldDef::new("owner", user_ref())
                .nullable()
                .with_doc("Owning user."),
        )
        .field(
            FieldDef::new("group", group_ref())
                .nullable()
                .with_doc("Owning group."),
        )
        .field(
            FieldDef::new("mode", FieldType::Int)
                .nullable()
                .with_doc("Permission bits, as the octal mode the kernel reports."),
        )
        .field(
            FieldDef::new("modified", FieldType::Timestamp)
                .nullable()
                .with_doc("Last content change."),
        )
        .field(
            FieldDef::new("accessed", FieldType::Timestamp)
                .nullable()
                .with_doc("Last access."),
        )
        .field(
            FieldDef::new("created", FieldType::Timestamp)
                .nullable()
                .with_doc("Creation time, where the filesystem records one."),
        )
        .field(
            FieldDef::new("inode", FieldType::Int)
                .nullable()
                .with_doc("Inode number."),
        )
        .field(
            FieldDef::new("device", device_ref())
                .nullable()
                .with_doc("Device the entry lives on."),
        )
        .field(
            FieldDef::new("target", FieldType::Path)
                .nullable()
                .with_doc("Symlink target."),
        )
        .identity(["device", "inode"])
        .default_view(["name", "kind", "size", "modified", "owner"])
        .build()
}

/// Spec §28.3.
fn service() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.service", 1), "Service")
        .doc("A managed service, as spec §28.3 defines it.")
        .field(
            FieldDef::new("name", FieldType::String)
                .required()
                .with_doc("Service name."),
        )
        .field(
            FieldDef::new("description", FieldType::String)
                .nullable()
                .with_doc("Human description."),
        )
        .field(
            FieldDef::new(
                "state",
                FieldType::enumeration(&[
                    "active",
                    "activating",
                    "deactivating",
                    "inactive",
                    "failed",
                    "reloading",
                    "unknown",
                ]),
            )
            .required()
            .with_doc("High-level state."),
        )
        .field(
            FieldDef::new("substate", FieldType::String)
                .nullable()
                .with_doc("Provider-specific sub-state."),
        )
        .field(
            FieldDef::new("pid", FieldType::Int)
                .nullable()
                .with_doc("Main process id."),
        )
        .field(
            FieldDef::new("enabled", FieldType::Bool)
                .nullable()
                .with_doc("Whether it starts at boot."),
        )
        .field(
            FieldDef::new("since", FieldType::Timestamp)
                .nullable()
                .with_doc("When the state was entered."),
        )
        .field(
            FieldDef::new("provider", FieldType::String)
                .required()
                .with_doc("Service manager that reported it."),
        )
        .field(
            FieldDef::new("unit_file", FieldType::Path)
                .nullable()
                .with_doc("Backing unit file."),
        )
        .identity(["provider", "name"])
        .default_view(["name", "state", "substate", "enabled", "pid"])
        .build()
}

/// Spec §28.4. `local` and `remote` are endpoint maps of `address` and `port`; spec §47 lists no
/// endpoint schema, so they stay maps rather than inventing an object type for two fields.
fn socket() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.socket", 1), "Socket")
        .doc("A socket, as spec §28.4 defines it.")
        .field(
            FieldDef::new(
                "protocol",
                FieldType::enumeration(&["tcp", "udp", "unix", "raw", "other"]),
            )
            .required()
            .with_doc("Transport protocol."),
        )
        .field(
            FieldDef::new(
                "family",
                FieldType::enumeration(&["inet", "inet6", "unix", "netlink", "packet", "other"]),
            )
            .required()
            .with_doc("Address family."),
        )
        .field(
            FieldDef::new("local", FieldType::Map)
                .nullable()
                .with_doc("Local endpoint: address and port."),
        )
        .field(
            FieldDef::new("remote", FieldType::Map)
                .nullable()
                .with_doc("Remote endpoint: address and port."),
        )
        .field(
            FieldDef::new(
                "state",
                FieldType::enumeration(&[
                    "established",
                    "syn-sent",
                    "syn-recv",
                    "fin-wait1",
                    "fin-wait2",
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
            .with_doc("Connection state, where the protocol has one."),
        )
        .field(
            FieldDef::new("process", FieldType::Ref(SchemaId::new("ono.process", 1)))
                .nullable()
                .with_doc("Process holding the socket."),
        )
        .field(
            FieldDef::new("user", user_ref())
                .nullable()
                .with_doc("Owning user."),
        )
        .field(
            FieldDef::new("inode", FieldType::Int)
                .nullable()
                .with_doc("Socket inode, the kernel's identity for it."),
        )
        .identity(["inode"])
        .default_view(["protocol", "local", "remote", "state", "process"])
        .build()
}

/// Spec §28.5. Spec §28.5 gives no identity line; the interface name is what a user addresses an
/// interface by on Linux, so that is the identity here.
fn interface() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.interface", 1), "Interface")
        .doc("A network interface, as spec §28.5 defines it.")
        .field(
            FieldDef::new("name", FieldType::String)
                .required()
                .with_doc("Interface name."),
        )
        .field(
            FieldDef::new("index", FieldType::Int)
                .required()
                .with_doc("Kernel interface index."),
        )
        .field(
            FieldDef::new("mac", FieldType::String)
                .nullable()
                .with_doc("Hardware address."),
        )
        .field(
            FieldDef::new(
                "state",
                FieldType::enumeration(&[
                    "up",
                    "down",
                    "testing",
                    "dormant",
                    "not-present",
                    "lower-layer-down",
                    "unknown",
                ]),
            )
            .required()
            .with_doc("Operational state."),
        )
        .field(
            FieldDef::new("mtu", FieldType::Int)
                .required()
                .with_doc("Maximum transmission unit."),
        )
        .field(
            FieldDef::new("addresses", FieldType::list(FieldType::IpNetwork))
                .required()
                .with_doc("Configured addresses with their prefixes."),
        )
        .field(
            FieldDef::new("rx_bytes", FieldType::ByteSize)
                .nullable()
                .with_doc("Bytes received."),
        )
        .field(
            FieldDef::new("tx_bytes", FieldType::ByteSize)
                .nullable()
                .with_doc("Bytes transmitted."),
        )
        .identity(["name"])
        .default_view(["name", "state", "mtu", "addresses"])
        .build()
}

/// Spec §28.6. Spec §28.6 gives no identity line; a mount point carries at most one mount, so the
/// target is the identity.
fn mount() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.mount", 1), "Mount")
        .doc("A mounted filesystem, as spec §28.6 defines it.")
        .field(
            FieldDef::new("source", FieldType::String)
                .required()
                .with_doc("What is mounted."),
        )
        .field(
            FieldDef::new("target", FieldType::Path)
                .required()
                .with_doc("Where it is mounted."),
        )
        .field(
            FieldDef::new("filesystem", FieldType::String)
                .required()
                .with_doc("Filesystem type."),
        )
        .field(
            FieldDef::new("options", FieldType::list(FieldType::String))
                .required()
                .with_doc("Mount options, one per element."),
        )
        .field(
            FieldDef::new("read_only", FieldType::Bool)
                .required()
                .with_doc("Whether the mount is read-only."),
        )
        .field(
            FieldDef::new("device", device_ref())
                .nullable()
                .with_doc("Backing device."),
        )
        .identity(["target"])
        .default_view(["target", "source", "filesystem", "options"])
        .build()
}

/// Spec §28.7.
fn user() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.user", 1), "User")
        .doc("A system user, as spec §28.7 defines it.")
        .field(
            FieldDef::new("uid", FieldType::Int)
                .required()
                .with_doc("User id."),
        )
        .field(
            FieldDef::new("name", FieldType::String)
                .nullable()
                .with_doc("Login name."),
        )
        .field(
            FieldDef::new("primary_group", group_ref())
                .nullable()
                .with_doc("Primary group."),
        )
        .field(
            FieldDef::new("home", FieldType::Path)
                .nullable()
                .with_doc("Home directory."),
        )
        .field(
            FieldDef::new("shell", FieldType::Path)
                .nullable()
                .with_doc("Login shell."),
        )
        .field(
            FieldDef::new("gecos", FieldType::String)
                .nullable()
                .with_doc("GECOS field."),
        )
        .identity(["uid"])
        .default_view(["uid", "name", "home", "shell"])
        .build()
}

/// Spec §28 references `GroupRef` from three schemas without defining the object it refers to,
/// and spec §8.1 lists `group` as a target. This is the shape those references resolve to.
fn group() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.group", 1), "Group")
        .doc("A system group, referenced by spec §28.1, §28.2 and §28.7 as `GroupRef`.")
        .field(
            FieldDef::new("gid", FieldType::Int)
                .required()
                .with_doc("Group id."),
        )
        .field(
            FieldDef::new("name", FieldType::String)
                .nullable()
                .with_doc("Group name."),
        )
        .field(
            FieldDef::new("members", FieldType::list(FieldType::String))
                .nullable()
                .with_doc("Names of the members, where the provider can enumerate them."),
        )
        .identity(["gid"])
        .default_view(["gid", "name"])
        .build()
}

/// Spec §47 lists `route.v1.yaml` and spec §8.1 lists `route` as a target, but spec §28 defines
/// no fields for it. This is the shape the netlink provider of spec §23.2 reports.
fn route() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.route", 1), "Route")
        .doc("A routing table entry, implied by spec §8.1 and spec §47.")
        .field(
            FieldDef::new("destination", FieldType::IpNetwork)
                .required()
                .with_doc("Destination prefix."),
        )
        .field(
            FieldDef::new("gateway", FieldType::Ip)
                .nullable()
                .with_doc("Next hop."),
        )
        .field(
            FieldDef::new("interface", FieldType::String)
                .nullable()
                .with_doc("Outgoing interface name."),
        )
        .field(
            FieldDef::new("source", FieldType::Ip)
                .nullable()
                .with_doc("Preferred source address."),
        )
        .field(
            FieldDef::new("protocol", FieldType::String)
                .nullable()
                .with_doc("Routing protocol that installed it."),
        )
        .field(
            FieldDef::new("scope", FieldType::String)
                .nullable()
                .with_doc("Route scope."),
        )
        .field(
            FieldDef::new("metric", FieldType::Int)
                .nullable()
                .with_doc("Route metric."),
        )
        .identity(["destination"])
        .default_view(["destination", "gateway", "interface", "metric"])
        .build()
}

/// Spec §8.1 lists `neighbor` as a target and spec §23.2 makes netlink its source; spec §28
/// defines no fields for it. This is the shape that provider reports.
fn neighbor() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.neighbor", 1), "Neighbor")
        .doc("A neighbour table entry, implied by spec §8.1 and spec §23.2.")
        .field(
            FieldDef::new("address", FieldType::Ip)
                .required()
                .with_doc("Neighbour address."),
        )
        .field(
            FieldDef::new("mac", FieldType::String)
                .nullable()
                .with_doc("Link-layer address."),
        )
        .field(
            FieldDef::new("interface", FieldType::String)
                .required()
                .with_doc("Interface the entry belongs to."),
        )
        .field(
            FieldDef::new(
                "state",
                FieldType::enumeration(&[
                    "reachable",
                    "stale",
                    "delay",
                    "probe",
                    "failed",
                    "incomplete",
                    "permanent",
                    "none",
                ]),
            )
            .nullable()
            .with_doc("Neighbour state."),
        )
        .identity(["address", "interface"])
        .default_view(["address", "mac", "interface", "state"])
        .build()
}

/// Spec §11.5 and §28.8.
fn action_result_definition() -> Result<Schema, crate::ErrorValue> {
    Schema::builder(SchemaId::new("ono.action-result", 1), "ActionResult")
        .doc("The acknowledgement a mutating command returns, as spec §11.5 defines it.")
        .field(
            FieldDef::new("target", FieldType::Any)
                .required()
                .with_doc("What the action was performed on."),
        )
        .field(
            FieldDef::new("operation", FieldType::String)
                .required()
                .with_doc("What was attempted."),
        )
        .field(
            FieldDef::new(
                "status",
                FieldType::enumeration(&["success", "skipped", "failed"]),
            )
            .required()
            .with_doc("How it ended."),
        )
        .field(
            FieldDef::new("changed", FieldType::Bool)
                .required()
                .with_doc("Whether anything actually changed."),
        )
        .field(
            FieldDef::new("message", FieldType::String)
                .nullable()
                .with_doc("Human explanation."),
        )
        .field(
            FieldDef::new("error", FieldType::Error)
                .nullable()
                .with_doc("The structured failure, when there was one."),
        )
        .field(
            FieldDef::new("duration", FieldType::Duration)
                .required()
                .with_doc("How long the action took."),
        )
        .identity(["target", "operation"])
        .default_view(["target", "operation", "status", "changed", "duration"])
        .build()
}
