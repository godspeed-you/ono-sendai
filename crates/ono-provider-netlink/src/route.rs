//! Decoding `RTM_GETROUTE` into `ono.route/1`.

use ono_core::ErrorCode;
use ono_value::{ErrorValue, IpNetwork, Value};

use crate::decoded::Decoded;
use crate::interface::{InterfaceNames, short, unexpected};
use crate::schema::route_schema;
use crate::wire::{self, Frame};
use crate::{NETLINK_PROVIDER, sys};

/// Where a route record says it came from.
pub(crate) const SOURCE: &str = "NETLINK_ROUTE RTM_GETROUTE";

/// Decodes an `RTM_GETROUTE` dump into route records, naming interfaces through `names`.
///
/// A route of a family this provider cannot describe — MPLS, bridge, anything but IPv4 and IPv6
/// — is reported as an error rather than dropped: `ono.route/1` declares `family` required, and
/// a silently shorter answer is the failure mode spec §35.3 exists to prevent.
///
/// ```
/// use ono_provider_netlink::{InterfaceNames, decode_routes};
/// let decoded = decode_routes(&[], &InterfaceNames::default());
/// assert!(decoded.records().is_empty());
/// assert!(decoded.errors().is_empty(), "an empty dump is not a failure");
/// ```
#[must_use]
pub fn decode_routes(bytes: &[u8], names: &InterfaceNames) -> Decoded {
    let mut decoded = Decoded::new();
    let schema = route_schema();

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
            sys::RTM_NEWROUTE => {}
            other => {
                decoded.fail(unexpected(other, "a route"));
                continue;
            }
        }
        if message.payload.len() < sys::RTMSG {
            decoded.fail(short("rtmsg", message.payload.len(), sys::RTMSG));
            continue;
        }

        let family = wire::u8_at(message.payload, 0).unwrap_or(0);
        let Some(family_name) = family_name(family) else {
            decoded.fail(unsupported_family(family));
            continue;
        };
        let prefix_len = wire::u8_at(message.payload, 1).unwrap_or(0);
        let header_table = wire::u8_at(message.payload, 4).unwrap_or(0);
        let protocol = wire::u8_at(message.payload, 5).unwrap_or(0);
        let scope = wire::u8_at(message.payload, 6).unwrap_or(0);
        let kind = wire::u8_at(message.payload, 7).unwrap_or(0);
        let attributes = message.payload.get(sys::RTMSG..).unwrap_or(&[]);

        let destination = match wire::attribute(attributes, sys::RTA_DST)
            .and_then(|payload| wire::address(payload, family))
        {
            // A route with no destination attribute is the default route, and null is the answer
            // rather than the absence of one.
            None => Value::Null,
            Some(address) => match IpNetwork::new(address, prefix_len) {
                Ok(network) => Value::IpNetwork(network),
                Err(error) => error.into_value(),
            },
        };
        let gateway = wire::attribute(attributes, sys::RTA_GATEWAY)
            .and_then(|payload| wire::address(payload, family))
            .map_or(Value::Null, Value::Ip);
        let source = wire::attribute(attributes, sys::RTA_PREFSRC)
            .and_then(|payload| wire::address(payload, family))
            .map_or(Value::Null, Value::Ip);
        let interface = wire::attribute_u32(attributes, sys::RTA_OIF)
            .map_or(Value::Null, |index| names.reference(index));
        let metric = wire::attribute_u32(attributes, sys::RTA_PRIORITY)
            .map_or(Value::Null, |metric| Value::Int(i128::from(metric)));
        // `RTA_TABLE` carries the table id for tables the single header byte cannot hold, and is
        // authoritative wherever the kernel sends it.
        let table = wire::attribute_u32(attributes, sys::RTA_TABLE)
            .unwrap_or_else(|| u32::from(header_table));

        decoded.record(
            &schema,
            SOURCE,
            NETLINK_PROVIDER,
            vec![
                ("destination", destination),
                ("gateway", gateway),
                ("interface", interface),
                ("source", source),
                ("family", Value::string(family_name)),
                ("type", Value::string(sys::route_type(kind))),
                (
                    "scope",
                    sys::route_scope(scope).map_or(Value::Null, Value::string),
                ),
                (
                    "protocol",
                    Value::string(
                        &sys::route_protocol(protocol)
                            .map_or_else(|| protocol.to_string(), ToOwned::to_owned),
                    ),
                ),
                ("metric", metric),
                ("table", Value::string(&sys::route_table(table))),
            ],
            Vec::new(),
        );
    }
    decoded
}

/// The name `ono.route/1` and `ono.neighbor/1` give an address family.
pub(crate) fn family_name(family: u8) -> Option<&'static str> {
    match family {
        sys::AF_INET => Some("inet"),
        sys::AF_INET6 => Some("inet6"),
        _ => None,
    }
}

/// A family neither schema can name.
pub(crate) fn unsupported_family(family: u8) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnsupported,
        format!("this provider describes IPv4 and IPv6; address family {family} is neither"),
    )
    .with_help(
        "the entry exists and was skipped deliberately. It is reported so that a short answer is \
         never mistaken for a complete one.",
    )
}
