//! Decoding `RTM_GETNEIGH` into `ono.neighbor/1`.

use ono_value::Value;

use crate::decoded::Decoded;
use crate::interface::{InterfaceNames, short, unexpected, unreadable};
use crate::route::{family_name, unsupported_family};
use crate::schema::neighbor_schema;
use crate::wire::{self, Frame};
use crate::{NETLINK_PROVIDER, sys};

/// Where a neighbour record says it came from.
pub(crate) const SOURCE: &str = "NETLINK_ROUTE RTM_GETNEIGH";

/// Decodes an `RTM_GETNEIGH` dump into neighbour records, naming interfaces through `names`.
///
/// An entry the kernel has not resolved keeps its `mac` null. That is the distinction spec §10.5
/// exists for: the neighbour is known to exist and its hardware address is not known, which is
/// not the same as it having none.
///
/// ```
/// use ono_provider_netlink::{InterfaceNames, decode_neighbors};
/// let decoded = decode_neighbors(&[], &InterfaceNames::default());
/// assert!(decoded.records().is_empty());
/// ```
#[must_use]
pub fn decode_neighbors(bytes: &[u8], names: &InterfaceNames) -> Decoded {
    let mut decoded = Decoded::new();
    let schema = neighbor_schema();

    for frame in wire::frames(bytes) {
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
            sys::RTM_NEWNEIGH => {}
            other => {
                decoded.fail(unexpected(other, "a neighbour"));
                continue;
            }
        }
        if message.payload.len() < sys::NDMSG {
            decoded.fail(short("ndmsg", message.payload.len(), sys::NDMSG));
            continue;
        }

        let family = wire::u8_at(message.payload, 0).unwrap_or(0);
        let Some(family_name) = family_name(family) else {
            decoded.fail(unsupported_family(family));
            continue;
        };
        let index = wire::i32_at(message.payload, 4).unwrap_or(0);
        let state = wire::u16_at(message.payload, 8).unwrap_or(0);
        let flags = wire::u8_at(message.payload, 10).unwrap_or(0);
        let attributes = message.payload.get(sys::NDMSG..).unwrap_or(&[]);

        let address = wire::attribute(attributes, sys::NDA_DST)
            .and_then(|payload| wire::address(payload, family))
            .map_or_else(|| unreadable("NDA_DST", index), Value::Ip);
        let mac = wire::attribute(attributes, sys::NDA_LLADDR)
            .and_then(wire::hardware_address)
            .map_or(Value::Null, |mac| Value::String(mac.into()));
        let interface = match u32::try_from(index)
            .ok()
            .map(|index| names.reference(index))
        {
            Some(Value::Null) | None => unreadable("a neighbour's interface index", index),
            Some(reference) => reference,
        };
        // `NTF_ROUTER` is set by neighbour discovery only; on IPv4 the bit means nothing, and
        // reporting `false` there would answer a question ARP never asked.
        let router = if family == sys::AF_INET6 {
            Value::Bool(flags & sys::NTF_ROUTER != 0)
        } else {
            Value::Null
        };

        decoded.record(
            &schema,
            SOURCE,
            NETLINK_PROVIDER,
            vec![
                ("address", address),
                ("mac", mac),
                ("interface", interface),
                ("family", Value::string(family_name)),
                ("state", Value::string(sys::neighbour_state(state))),
                ("router", router),
                // The kernel's `NDA_CACHEINFO` counts clock ticks since an event, not an instant;
                // turning that into a timestamp needs a boot time this provider does not read, so
                // the field stays null rather than becoming an approximation (spec §35.3).
                ("updated", Value::Null),
            ],
            Vec::new(),
        );
    }
    decoded
}
