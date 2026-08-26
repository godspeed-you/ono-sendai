//! Decoding `RTM_GETLINK` and `RTM_GETADDR` into `ono.interface/1` (spec §23.2, §28.5).

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, IpNetwork, Value};

use crate::decoded::Decoded;
use crate::schema::interface_schema;
use crate::wire::{self, Frame};
use crate::{NETLINK_PROVIDER, sys};

/// Where an interface record says it came from.
pub(crate) const SOURCE: &str = "NETLINK_ROUTE RTM_GETLINK+RTM_GETADDR";

/// The interface names the kernel gave for each index.
///
/// Routes and neighbours name their interface by index; a person names it `eth0`. This is the
/// only join between the two, and it is built from the same `RTM_GETLINK` dump the interface
/// provider reads, so no separate lookup can disagree with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterfaceNames {
    by_index: BTreeMap<u32, Arc<str>>,
}

impl InterfaceNames {
    /// The names in an `RTM_GETLINK` dump.
    #[must_use]
    pub fn from_links(links: &[u8]) -> Self {
        let mut names = Self::default();
        for frame in wire::frames(links) {
            let Frame::Message(message) = frame else {
                continue;
            };
            if message.kind != sys::RTM_NEWLINK {
                continue;
            }
            let Some(index) = wire::i32_at(message.payload, 4) else {
                continue;
            };
            let attributes = message.payload.get(sys::IFINFOMSG..).unwrap_or(&[]);
            if let Some(name) = wire::attribute(attributes, sys::IFLA_IFNAME).and_then(wire::text)
                && let Ok(index) = u32::try_from(index)
            {
                names.by_index.insert(index, name.into());
            }
        }
        names
    }

    /// The name of the interface with this index, if the dump carried one.
    #[must_use]
    pub fn name(&self, index: u32) -> Option<&str> {
        self.by_index.get(&index).map(|name| &**name)
    }

    /// How many interfaces were named.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    /// Whether no interface was named.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }

    /// A reference to an interface, as the value the `interface` field of a route or a neighbour
    /// carries: its name where the kernel named it, its index where it did not, and `Value::Null`
    /// for a message that names no interface at all.
    #[must_use]
    pub fn reference(&self, index: u32) -> Value {
        if index == 0 {
            return Value::Null;
        }
        match self.name(index) {
            Some(name) => Value::String(name.into()),
            None => Value::Int(i128::from(index)),
        }
    }
}

impl FromIterator<(u32, String)> for InterfaceNames {
    fn from_iter<I: IntoIterator<Item = (u32, String)>>(entries: I) -> Self {
        Self {
            by_index: entries
                .into_iter()
                .map(|(index, name)| (index, Arc::from(name.as_str())))
                .collect(),
        }
    }
}

/// Decodes an `RTM_GETLINK` dump and an `RTM_GETADDR` dump into interface records.
///
/// The two dumps are read together because an interface without its addresses is not the object
/// spec §28.5 describes. An interface the address dump says nothing about gets an *empty* list,
/// not an unknown one: the kernel enumerated its addresses and there were none.
///
/// ```
/// use ono_provider_netlink::decode_interfaces;
/// // A dump this decoder cannot read produces no records and says so, rather than reporting
/// // that the machine has no interfaces.
/// let decoded = decode_interfaces(&[0xff, 0xff], &[]);
/// assert!(decoded.records().is_empty());
/// assert!(!decoded.errors().is_empty());
/// ```
#[must_use]
pub fn decode_interfaces(links: &[u8], addresses: &[u8]) -> Decoded {
    let mut decoded = Decoded::new();
    let by_index = decode_addresses(addresses, &mut decoded);
    let schema = interface_schema();

    for frame in wire::frames(links) {
        let message = match frame {
            Frame::Message(message) => message,
            Frame::Malformed(error) => {
                decoded.fail(error);
                break;
            }
        };
        match message.kind {
            sys::NLMSG_DONE => break,
            sys::NLMSG_ERROR => {
                decoded.fail(wire::error_message(message.payload));
                continue;
            }
            kind if wire::control(kind) => continue,
            sys::RTM_NEWLINK => {}
            other => {
                decoded.fail(unexpected(other, "an interface"));
                continue;
            }
        }

        if message.payload.len() < sys::IFINFOMSG {
            decoded.fail(short("ifinfomsg", message.payload.len(), sys::IFINFOMSG));
            continue;
        }
        let index = wire::i32_at(message.payload, 4).unwrap_or(0);
        let flags = wire::u32_at(message.payload, 8).unwrap_or(0);
        let attributes = message.payload.get(sys::IFINFOMSG..).unwrap_or(&[]);

        let name = wire::attribute(attributes, sys::IFLA_IFNAME)
            .and_then(wire::text)
            .map_or_else(
                || unreadable("IFLA_IFNAME", index),
                |name| Value::String(name.into()),
            );
        let mtu = wire::attribute_u32(attributes, sys::IFLA_MTU).map_or_else(
            || unreadable("IFLA_MTU", index),
            |mtu| Value::Int(i128::from(mtu)),
        );
        let mac = wire::attribute(attributes, sys::IFLA_ADDRESS)
            .and_then(wire::hardware_address)
            .map_or(Value::Null, |mac| Value::String(mac.into()));
        let state = wire::attribute(attributes, sys::IFLA_OPERSTATE)
            .and_then(|payload| wire::u8_at(payload, 0))
            .map_or("unknown", sys::operational_state);
        let configured = u32::try_from(index)
            .ok()
            .and_then(|index| by_index.get(&index))
            .cloned()
            .unwrap_or_default();
        let stats = link_statistics(attributes);

        decoded.record(
            &schema,
            SOURCE,
            NETLINK_PROVIDER,
            vec![
                ("name", name),
                ("index", Value::Int(i128::from(index))),
                ("mac", mac),
                ("state", Value::string(state)),
                ("mtu", mtu),
                ("addresses", Value::list(configured)),
                (
                    "rx_bytes",
                    stats.map_or(Value::Null, |stats| {
                        Value::ByteSize(ByteSize::from_bytes(u128::from(stats.rx_bytes)))
                    }),
                ),
                (
                    "tx_bytes",
                    stats.map_or(Value::Null, |stats| {
                        Value::ByteSize(ByteSize::from_bytes(u128::from(stats.tx_bytes)))
                    }),
                ),
            ],
            extensions(attributes, flags, stats),
        );
    }
    decoded
}

/// The addresses of each interface index, in the order the kernel reported them.
fn decode_addresses(addresses: &[u8], decoded: &mut Decoded) -> BTreeMap<u32, Vec<Value>> {
    let mut by_index: BTreeMap<u32, Vec<Value>> = BTreeMap::new();
    for frame in wire::frames(addresses) {
        let message = match frame {
            Frame::Message(message) => message,
            Frame::Malformed(error) => {
                decoded.fail(error);
                break;
            }
        };
        match message.kind {
            sys::NLMSG_DONE => break,
            sys::NLMSG_ERROR => {
                decoded.fail(wire::error_message(message.payload));
                continue;
            }
            kind if wire::control(kind) => continue,
            sys::RTM_NEWADDR => {}
            other => {
                decoded.fail(unexpected(other, "an address"));
                continue;
            }
        }
        if message.payload.len() < sys::IFADDRMSG {
            decoded.fail(short("ifaddrmsg", message.payload.len(), sys::IFADDRMSG));
            continue;
        }
        let family = wire::u8_at(message.payload, 0).unwrap_or(0);
        let prefix_len = wire::u8_at(message.payload, 1).unwrap_or(0);
        let index = wire::u32_at(message.payload, 4).unwrap_or(0);
        let attributes = message.payload.get(sys::IFADDRMSG..).unwrap_or(&[]);

        // `IFA_LOCAL` is the address configured on this machine; `IFA_ADDRESS` is the peer on a
        // point-to-point link, and reporting that as ours would be wrong rather than merely
        // imprecise.
        let Some(address) = wire::attribute(attributes, sys::IFA_LOCAL)
            .or_else(|| wire::attribute(attributes, sys::IFA_ADDRESS))
            .and_then(|payload| wire::address(payload, family))
        else {
            continue;
        };
        match IpNetwork::new(address, prefix_len) {
            Ok(network) => by_index
                .entry(index)
                .or_default()
                .push(Value::IpNetwork(network)),
            Err(error) => decoded.fail(error),
        }
    }
    by_index
}

/// The eight counters every kernel reports, from `IFLA_STATS64` or from the 32-bit `IFLA_STATS`.
#[derive(Debug, Clone, Copy)]
struct Statistics {
    rx_packets: u64,
    tx_packets: u64,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_errors: u64,
    tx_errors: u64,
    rx_dropped: u64,
    tx_dropped: u64,
}

fn link_statistics(attributes: &[u8]) -> Option<Statistics> {
    if let Some(payload) = wire::attribute(attributes, sys::IFLA_STATS64) {
        let read = |index: usize| wire::u64_at(payload, index * 8);
        return Some(Statistics {
            rx_packets: read(0)?,
            tx_packets: read(1)?,
            rx_bytes: read(2)?,
            tx_bytes: read(3)?,
            rx_errors: read(4)?,
            tx_errors: read(5)?,
            rx_dropped: read(6)?,
            tx_dropped: read(7)?,
        });
    }
    let payload = wire::attribute(attributes, sys::IFLA_STATS)?;
    let read = |index: usize| wire::u32_at(payload, index * 4).map(u64::from);
    Some(Statistics {
        rx_packets: read(0)?,
        tx_packets: read(1)?,
        rx_bytes: read(2)?,
        tx_bytes: read(3)?,
        rx_errors: read(4)?,
        tx_errors: read(5)?,
        rx_dropped: read(6)?,
        tx_dropped: read(7)?,
    })
}

/// The facts netlink carries that `ono.interface/1` does not declare (spec §10.4).
fn extensions(
    attributes: &[u8],
    flags: u32,
    stats: Option<Statistics>,
) -> Vec<(&'static str, Value)> {
    let kind = wire::attribute(attributes, sys::IFLA_LINKINFO)
        .and_then(|payload| wire::attribute(payload, sys::IFLA_INFO_KIND))
        .and_then(wire::text)
        .map_or(Value::Null, |kind| Value::String(kind.into()));

    let named: Vec<Value> = sys::INTERFACE_FLAGS
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .map(|(_, name)| Value::string(name))
        .collect();

    let counter =
        |value: Option<u64>| value.map_or(Value::Null, |value| Value::Int(i128::from(value)));

    vec![
        ("netlink.kind", kind),
        ("netlink.admin_up", Value::Bool(flags & sys::IFF_UP != 0)),
        (
            "netlink.running",
            Value::Bool(flags & sys::IFF_RUNNING != 0),
        ),
        ("netlink.flags", Value::list(named)),
        ("netlink.rx_packets", counter(stats.map(|s| s.rx_packets))),
        ("netlink.tx_packets", counter(stats.map(|s| s.tx_packets))),
        ("netlink.rx_errors", counter(stats.map(|s| s.rx_errors))),
        ("netlink.tx_errors", counter(stats.map(|s| s.tx_errors))),
        ("netlink.rx_dropped", counter(stats.map(|s| s.rx_dropped))),
        ("netlink.tx_dropped", counter(stats.map(|s| s.tx_dropped))),
    ]
}

/// A field the kernel did not send: unreadable, which spec §10.5 keeps apart from unknown.
pub(crate) fn unreadable(attribute: &str, index: i32) -> Value {
    ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        format!("the kernel sent no {attribute} for interface index {index}"),
    )
    .into_value()
}

/// A netlink message shorter than the fixed struct it claims to be.
pub(crate) fn short(structure: &str, got: usize, wanted: usize) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("a netlink message carries {got} bytes where a {structure} needs {wanted}"),
    )
}

/// A message of a type the decoder was not asked to read.
pub(crate) fn unexpected(kind: u16, wanted: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("netlink message type {kind} is not {wanted}"),
    )
    .with_help("the reply did not answer the request; the objects it did answer are still shown")
}
