//! The write paths: routes, interfaces and sockets changed through the same netlink families
//! the read paths use (spec §23.2, §28.5; ADR-0088).
//!
//! Every operation here builds one request, sends it, and reports the kernel's acknowledgement
//! as the outcome of that one target. Nothing is checked in advance and nothing is faked: a
//! caller without `CAP_NET_ADMIN` is refused by the kernel with `EPERM` before it looks at the
//! payload, and that refusal — `io.permission_denied` — is the honest answer, not a guess made
//! here about what the kernel would say.

use std::net::IpAddr;

use ono_core::ErrorCode;
use ono_provider_api::{Action, ActionOutcome};
use ono_value::{ErrorValue, IpNetwork, Value};

use crate::interface::InterfaceNames;
use crate::socket::SocketProtocol;
use crate::sys;
use crate::transport::{NetlinkSocket, link_request};
use crate::wire;

// --- routes ---------------------------------------------------------------------------------

/// What a route request names: the identity of a listed route, or what the user wrote.
#[derive(Debug, Default)]
struct RouteSpec {
    destination: Option<IpNetwork>,
    gateway: Option<IpAddr>,
    interface: Option<String>,
    metric: Option<u32>,
    table: Option<String>,
    family: Option<u8>,
}

impl RouteSpec {
    /// The route an action is about.
    ///
    /// A route that arrived through the pipeline carries its full identity — `table`, `family`,
    /// `destination`, `gateway`, `interface` (`route.v1.yaml`) — and the options say what to
    /// change. A route the user named carries the selector as `destination` and the rest as
    /// options.
    fn of(action: &Action) -> Result<Self, ErrorValue> {
        let mut spec = Self::default();
        let identity = action.target().values();
        if identity.len() == 5 {
            spec.table = text(identity.first());
            spec.family = match text(identity.get(1)).as_deref() {
                Some("inet") => Some(sys::AF_INET),
                Some("inet6") => Some(sys::AF_INET6),
                _ => None,
            };
            spec.destination = network(identity.get(2))?;
            spec.gateway = address(identity.get(3))?;
            spec.interface = text(identity.get(4));
        } else if let Some(value) = identity.first().or_else(|| action.argument("destination")) {
            spec.destination = network(Some(value))?;
        }
        if let Some(value) = action.argument("gateway") {
            spec.gateway = address(Some(value))?;
        }
        if let Some(value) = action.argument("interface") {
            spec.interface = text(Some(value));
        }
        if let Some(value) = action.argument("metric") {
            spec.metric = Some(number(value, "metric")?);
        }
        if let Some(value) = action.argument("table") {
            spec.table = text(Some(value));
        }
        if spec.destination.is_none() && spec.gateway.is_none() {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "a route needs a destination prefix, or `--gateway` for the default route",
            )
            .with_help("as in `add route 10.0.0.0/8 --gateway 192.168.1.1`"));
        }
        Ok(spec)
    }

    fn family(&self) -> u8 {
        self.family
            .or_else(|| {
                self.destination
                    .map(|network| wire::family_of(network.address()))
            })
            .or_else(|| self.gateway.map(wire::family_of))
            .unwrap_or(sys::AF_INET)
    }

    fn table_id(&self) -> Result<u32, ErrorValue> {
        match self.table.as_deref() {
            None | Some("main") => Ok(u32::from(sys::RT_TABLE_MAIN)),
            Some("local") => Ok(255),
            Some("default") => Ok(253),
            Some("unspec") => Ok(0),
            Some(other) => other.parse::<u32>().map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{other}` is not a routing table: main, local, default or a number"),
                )
            }),
        }
    }

    /// The `rtmsg` and attributes of this route, for `kind`.
    fn message(&self, kind: u16, socket: &NetlinkSocket) -> Result<Vec<u8>, ErrorValue> {
        let table = self.table_id()?;
        let deleting = kind == sys::RTM_DELROUTE;
        let mut message = vec![0u8; sys::RTMSG];
        message[0] = self.family();
        message[1] = self.destination.map_or(0, IpNetwork::prefix_len);
        message[4] = u8::try_from(table).unwrap_or(0);
        // A deletion matches whatever is there; a creation says what it creates.
        message[5] = if deleting { 0 } else { sys::RTPROT_BOOT };
        message[6] = if deleting {
            255 // RT_SCOPE_NOWHERE: any scope
        } else if self.gateway.is_some() {
            sys::RT_SCOPE_UNIVERSE
        } else {
            sys::RT_SCOPE_LINK
        };
        message[7] = if deleting { 0 } else { sys::RTN_UNICAST };

        if let Some(destination) = self.destination {
            wire::push_attribute(
                &mut message,
                sys::RTA_DST,
                &wire::address_bytes(destination.address()),
            );
        }
        if let Some(gateway) = self.gateway {
            wire::push_attribute(
                &mut message,
                sys::RTA_GATEWAY,
                &wire::address_bytes(gateway),
            );
        }
        if let Some(interface) = &self.interface {
            let index = interface_index(socket, interface)?;
            wire::push_attribute(&mut message, sys::RTA_OIF, &index.to_ne_bytes());
        }
        if let Some(metric) = self.metric {
            wire::push_attribute(&mut message, sys::RTA_PRIORITY, &metric.to_ne_bytes());
        }
        if table > u32::from(u8::MAX) {
            wire::push_attribute(&mut message, sys::RTA_TABLE, &table.to_ne_bytes());
        }
        Ok(message)
    }

    fn describe(&self) -> String {
        let destination = self
            .destination
            .map_or_else(|| "default".to_owned(), |network| network.to_string());
        match &self.gateway {
            Some(gateway) => format!("{destination} via {gateway}"),
            None => destination,
        }
    }
}

/// `add`, `set` and `remove` over `RTM_NEWROUTE` / `RTM_DELROUTE`.
pub(crate) fn route(action: &Action) -> Result<ActionOutcome, ErrorValue> {
    let (kind, flags, verb) = match action.operation() {
        "add" => (
            sys::RTM_NEWROUTE,
            sys::NLM_F_CREATE | sys::NLM_F_EXCL,
            "add",
        ),
        "set" => (
            sys::RTM_NEWROUTE,
            sys::NLM_F_CREATE | sys::NLM_F_REPLACE,
            "replace",
        ),
        "remove" => (sys::RTM_DELROUTE, 0, "remove"),
        other => return Err(unsupported("route", other)),
    };
    let spec = match RouteSpec::of(action) {
        Ok(spec) => spec,
        Err(error) => return Ok(ActionOutcome::failed(action, error)),
    };
    if action.is_dry_run() {
        return Ok(ActionOutcome::skipped(
            action,
            format!("would {verb} the route {}", spec.describe()),
        ));
    }
    let socket = NetlinkSocket::open_route()?;
    let message = match spec.message(kind, &socket) {
        Ok(message) => message,
        Err(error) => return Ok(ActionOutcome::failed(action, error)),
    };
    Ok(match socket.request(kind, flags, &message) {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err(error) => ActionOutcome::failed(action, refused(error, "NETLINK_ROUTE")),
    })
}

// --- interfaces -----------------------------------------------------------------------------

/// `set`, `start`, `stop`, `add` and `remove` over `RTM_NEWLINK`, `RTM_DELLINK`, `RTM_NEWADDR`
/// and `RTM_DELADDR`.
pub(crate) fn interface(action: &Action) -> Result<ActionOutcome, ErrorValue> {
    let operation = action.operation();
    if !matches!(operation, "set" | "start" | "stop" | "add" | "remove") {
        return Err(unsupported("interface", operation));
    }
    let socket = NetlinkSocket::open_route()?;

    // The identity is the kernel index (`interface.v1.yaml`); a name the user wrote and nothing
    // resolved is the one the kernel is asked about — or, for a creation, the one it is given.
    let named = match action.target().values().first() {
        Some(Value::Int(index)) => Named::Index(u32::try_from(*index).unwrap_or(0)),
        Some(Value::String(name)) => Named::Name(name.to_string()),
        _ => match action.argument("name") {
            Some(Value::String(name)) => Named::Name(name.to_string()),
            _ => {
                return Ok(ActionOutcome::failed(
                    action,
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "an interface is named by its name or its index",
                    ),
                ));
            }
        },
    };

    let outcome = match operation {
        "add" => match (action.argument("kind"), action.argument("address")) {
            (Some(Value::String(kind)), _) => {
                let name = match &named {
                    Named::Name(name) => name.clone(),
                    Named::Index(index) => {
                        return Ok(ActionOutcome::failed(
                            action,
                            ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "a new `{kind}` interface needs a name, not the index {index}"
                                ),
                            ),
                        ));
                    }
                };
                if action.is_dry_run() {
                    return Ok(ActionOutcome::skipped(
                        action,
                        format!("would create the {kind} interface {name}"),
                    ));
                }
                let mut message = vec![0u8; sys::IFINFOMSG];
                wire::push_attribute(&mut message, sys::IFLA_IFNAME, name.as_bytes());
                wire::push_nested(&mut message, sys::IFLA_LINKINFO, |info| {
                    wire::push_attribute(info, sys::IFLA_INFO_KIND, kind.as_bytes());
                });
                socket.request(
                    sys::RTM_NEWLINK,
                    sys::NLM_F_CREATE | sys::NLM_F_EXCL,
                    &message,
                )
            }
            (_, Some(value)) => {
                let network = match network(Some(value))? {
                    Some(network) => network,
                    None => return Ok(missing_argument(action, "address")),
                };
                let index = match resolve_index(&socket, &named) {
                    Ok(index) => index,
                    Err(error) => return Ok(ActionOutcome::failed(action, error)),
                };
                if action.is_dry_run() {
                    return Ok(ActionOutcome::skipped(
                        action,
                        format!("would add {network} to {}", named.describe()),
                    ));
                }
                socket.request(
                    sys::RTM_NEWADDR,
                    sys::NLM_F_CREATE | sys::NLM_F_EXCL,
                    &address_message(index, network),
                )
            }
            _ => {
                return Ok(ActionOutcome::failed(
                    action,
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`add interface` creates a virtual interface (`--kind`) or adds an \
                         address (`--address`); neither was given",
                    ),
                ));
            }
        },
        "remove" => {
            let index = match resolve_index(&socket, &named) {
                Ok(index) => index,
                Err(error) => return Ok(ActionOutcome::failed(action, error)),
            };
            match action.argument("address") {
                Some(value) => {
                    let network = match network(Some(value))? {
                        Some(network) => network,
                        None => return Ok(missing_argument(action, "address")),
                    };
                    if action.is_dry_run() {
                        return Ok(ActionOutcome::skipped(
                            action,
                            format!("would remove {network} from {}", named.describe()),
                        ));
                    }
                    socket.request(sys::RTM_DELADDR, 0, &address_message(index, network))
                }
                None => {
                    if action.is_dry_run() {
                        return Ok(ActionOutcome::skipped(
                            action,
                            format!("would delete the interface {}", named.describe()),
                        ));
                    }
                    socket.request(sys::RTM_DELLINK, 0, &link_message(index, 0, 0, &[]))
                }
            }
        }
        _ => {
            // `set`, `start`, `stop`: the administrative state and the MTU of a link that exists.
            let index = match resolve_index(&socket, &named) {
                Ok(index) => index,
                Err(error) => return Ok(ActionOutcome::failed(action, error)),
            };
            let up = match operation {
                "start" => Some(true),
                "stop" => Some(false),
                _ => match action.argument("up") {
                    Some(Value::Bool(up)) => Some(*up),
                    _ => None,
                },
            };
            let mtu = match action.argument("mtu") {
                Some(value) => Some(number(value, "mtu")?),
                None => None,
            };
            if up.is_none() && mtu.is_none() {
                return Ok(ActionOutcome::failed(
                    action,
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`set interface` changes `--mtu` or `--up`; neither was given",
                    ),
                ));
            }
            if action.is_dry_run() {
                let mut changes = Vec::new();
                if let Some(up) = up {
                    changes.push(if up { "bring it up" } else { "bring it down" }.to_owned());
                }
                if let Some(mtu) = mtu {
                    changes.push(format!("set the MTU to {mtu}"));
                }
                return Ok(ActionOutcome::skipped(
                    action,
                    format!("would {} on {}", changes.join(" and "), named.describe()),
                ));
            }
            let (flags, change) = match up {
                Some(true) => (sys::IFF_UP, sys::IFF_UP),
                Some(false) => (0, sys::IFF_UP),
                None => (0, 0),
            };
            let mut attributes = Vec::new();
            if let Some(mtu) = mtu {
                wire::push_attribute(&mut attributes, sys::IFLA_MTU, &mtu.to_ne_bytes());
            }
            socket.request(
                sys::RTM_NEWLINK,
                0,
                &link_message(index, flags, change, &attributes),
            )
        }
    };
    Ok(match outcome {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err(error) => ActionOutcome::failed(action, refused(error, "NETLINK_ROUTE")),
    })
}

/// How the user named an interface.
#[derive(Debug)]
enum Named {
    Index(u32),
    Name(String),
}

impl Named {
    fn describe(&self) -> String {
        match self {
            Named::Index(index) => format!("interface #{index}"),
            Named::Name(name) => name.clone(),
        }
    }
}

/// The kernel index of the named interface, through the link dump.
fn resolve_index(socket: &NetlinkSocket, named: &Named) -> Result<u32, ErrorValue> {
    match named {
        Named::Index(index) => Ok(*index),
        Named::Name(name) => interface_index(socket, name),
    }
}

fn interface_index(socket: &NetlinkSocket, name: &str) -> Result<u32, ErrorValue> {
    let names = InterfaceNames::from_links(&socket.dump(sys::RTM_GETLINK, &link_request())?);
    names.index_of(name).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("no interface is named `{name}`"),
        )
        .with_help("`get interface` lists what is there")
    })
}

/// An `ifinfomsg` for `index` with the given flag change, followed by `attributes`.
fn link_message(index: u32, flags: u32, change: u32, attributes: &[u8]) -> Vec<u8> {
    let mut message = vec![0u8; sys::IFINFOMSG];
    message[4..8].copy_from_slice(&index.to_ne_bytes());
    message[8..12].copy_from_slice(&flags.to_ne_bytes());
    message[12..16].copy_from_slice(&change.to_ne_bytes());
    message.extend_from_slice(attributes);
    message
}

/// An `ifaddrmsg` for one address on `index`, with the address as both local and peer.
fn address_message(index: u32, network: IpNetwork) -> Vec<u8> {
    let address = network.address();
    let mut message = vec![0u8; sys::IFADDRMSG];
    message[0] = wire::family_of(address);
    message[1] = network.prefix_len();
    message[3] = if address.is_loopback() {
        254 // RT_SCOPE_HOST
    } else {
        sys::RT_SCOPE_UNIVERSE
    };
    message[4..8].copy_from_slice(&index.to_ne_bytes());
    let bytes = wire::address_bytes(address);
    wire::push_attribute(&mut message, sys::IFA_LOCAL, &bytes);
    wire::push_attribute(&mut message, sys::IFA_ADDRESS, &bytes);
    message
}

// --- sockets --------------------------------------------------------------------------------

/// `stop socket` over `SOCK_DESTROY`.
///
/// The socket is looked up again by its inode so the request carries the addresses and ports
/// the kernel matches on; a socket that is no longer there is `io.not_found`, and a Unix socket
/// is refused, because `sock_diag` can only destroy inet sockets.
pub(crate) fn socket(action: &Action) -> Result<ActionOutcome, ErrorValue> {
    if action.operation() != "stop" {
        return Err(unsupported("socket", action.operation()));
    }
    let Some(Value::Int(inode)) = action.target().values().first() else {
        return Ok(ActionOutcome::failed(
            action,
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "a socket is identified by its inode, and this one has none the kernel can \
                 be asked about",
            )
            .with_help("a socket in time-wait has already been released"),
        ));
    };
    let inode = *inode;
    let decoded = crate::provider::read_sockets(None, false)?;
    let Some(record) = decoded
        .records()
        .iter()
        .find(|record| record.get("inode") == Some(&Value::Int(inode)))
    else {
        return Ok(ActionOutcome::failed(
            action,
            ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("no socket has the inode {inode} any more"),
            ),
        ));
    };
    let protocol = match record.get("protocol") {
        Some(Value::String(text)) if &**text == "tcp" => SocketProtocol::Tcp,
        Some(Value::String(text)) if &**text == "udp" => SocketProtocol::Udp,
        other => {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!(
                        "sock_diag destroys inet sockets only, and this one is {}",
                        other.map_or_else(
                            || "of no known protocol".to_owned(),
                            |value| { ono_value::canonical_text(value).unwrap_or_default() }
                        )
                    ),
                ),
            ));
        }
    };
    let family = match record.get("family") {
        Some(Value::String(text)) if &**text == "inet6" => sys::AF_INET6,
        _ => sys::AF_INET,
    };
    let (local_address, local_port) = endpoint_of(record.get("local"));
    let (remote_address, remote_port) = endpoint_of(record.get("remote"));
    let label = format!(
        "{}/{}:{}",
        protocol.as_str(),
        local_address.map_or_else(String::new, |address| address.to_string()),
        local_port
    );
    if action.is_dry_run() {
        return Ok(ActionOutcome::skipped(
            action,
            format!("would destroy the socket {label}"),
        ));
    }

    let mut request = Vec::with_capacity(56);
    request.push(family);
    request.push(protocol.number());
    request.push(0); // idiag_ext
    request.push(0); // padding
    request.extend_from_slice(&u32::MAX.to_ne_bytes()); // every state
    request.extend_from_slice(&local_port.to_be_bytes());
    request.extend_from_slice(&remote_port.to_be_bytes());
    request.extend_from_slice(&padded_address(local_address));
    request.extend_from_slice(&padded_address(remote_address));
    request.extend_from_slice(&0u32.to_ne_bytes()); // idiag_if: any interface
    request.extend_from_slice(&u32::MAX.to_ne_bytes()); // INET_DIAG_NOCOOKIE
    request.extend_from_slice(&u32::MAX.to_ne_bytes());

    let socket = NetlinkSocket::open_diag()?;
    Ok(match socket.request(sys::SOCK_DESTROY, 0, &request) {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err(error) => ActionOutcome::failed(action, refused(error, "NETLINK_SOCK_DIAG")),
    })
}

/// The address and port of one end of a socket record.
fn endpoint_of(value: Option<&Value>) -> (Option<IpAddr>, u16) {
    match value {
        Some(Value::Record(endpoint)) => (
            match endpoint.get("address") {
                Some(Value::Ip(address)) => Some(*address),
                _ => None,
            },
            match endpoint.get("port") {
                Some(Value::Port(port)) => *port,
                _ => 0,
            },
        ),
        _ => (None, 0),
    }
}

/// An address in the sixteen bytes `inet_diag_sockid` reserves for it.
fn padded_address(address: Option<IpAddr>) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    if let Some(address) = address {
        let raw = wire::address_bytes(address);
        bytes
            .iter_mut()
            .zip(raw.iter())
            .for_each(|(slot, byte)| *slot = *byte);
    }
    bytes
}

// --- shared ---------------------------------------------------------------------------------

/// The kernel's refusal, with the family that refused named in the help.
fn refused(error: ErrorValue, family: &str) -> ErrorValue {
    if error.code() == ErrorCode::IoPermissionDenied {
        return error.with_help(format!(
            "{family} needs CAP_NET_ADMIN for this; `explain` reports the privilege a command \
             needs before it runs"
        ));
    }
    error
}

fn unsupported(target: &str, operation: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnsupported,
        format!(
            "{} has no operation `{operation}` on a {target}",
            crate::NETLINK_PROVIDER
        ),
    )
}

fn missing_argument(action: &Action, name: &str) -> ActionOutcome {
    ActionOutcome::failed(
        action,
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--{name}` needs an address with a prefix length, as in 192.168.1.5/24"),
        ),
    )
}

fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    }
}

fn network(value: Option<&Value>) -> Result<Option<IpNetwork>, ErrorValue> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::IpNetwork(network)) => Ok(Some(*network)),
        Some(Value::Ip(address)) => Ok(Some(IpNetwork::new(
            *address,
            if address.is_ipv4() { 32 } else { 128 },
        )?)),
        Some(Value::String(text)) => IpNetwork::parse(text).map(Some),
        Some(other) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "a prefix is written as address/length, not as a {}",
                other.type_name()
            ),
        )),
    }
}

fn address(value: Option<&Value>) -> Result<Option<IpAddr>, ErrorValue> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Ip(address)) => Ok(Some(*address)),
        Some(Value::String(text)) => text.parse::<IpAddr>().map(Some).map_err(|_| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{text}` is not an IP address"),
            )
        }),
        Some(other) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("an address is an ip, not a {}", other.type_name()),
        )),
    }
}

fn number(value: &Value, what: &str) -> Result<u32, ErrorValue> {
    match value {
        Value::Int(number) => u32::try_from(*number).map_err(|_| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "{number} does not fit a {what}: it must be between 0 and {}",
                    u32::MAX
                ),
            )
        }),
        other => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--{what}` is a number, not a {}", other.type_name()),
        )),
    }
}
