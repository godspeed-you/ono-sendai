//! The netlink constants this crate uses, transcribed from the Linux uapi headers.
//!
//! They are written out rather than pulled from a binding crate for one reason: the numbers are
//! a stable kernel ABI, and a table with the header name beside each value is easier to check
//! against `include/uapi/linux/` than a generated one. Nothing here is a guess; anything the
//! kernel could change is read from the message rather than assumed.

/// `NLMSG_ALIGNTO` and `RTA_ALIGNTO`, both four.
pub(crate) const ALIGN: usize = 4;

/// The size of `struct nlmsghdr`.
pub(crate) const NLMSG_HEADER: usize = 16;

/// The size of `struct rtattr`.
pub(crate) const ATTR_HEADER: usize = 4;

// `linux/netlink.h`: standard message types.
pub(crate) const NLMSG_NOOP: u16 = 1;
pub(crate) const NLMSG_ERROR: u16 = 2;
pub(crate) const NLMSG_DONE: u16 = 3;
pub(crate) const NLMSG_OVERRUN: u16 = 4;

// `linux/netlink.h`: header flags.
pub(crate) const NLM_F_REQUEST: u16 = 0x0001;
/// `NLM_F_ROOT | NLM_F_MATCH`, the pair the kernel reads as "dump everything matching".
pub(crate) const NLM_F_DUMP: u16 = 0x0300;
/// Ask for an `NLMSG_ERROR` acknowledgement even on success, so a write has a definite answer.
pub(crate) const NLM_F_ACK: u16 = 0x0004;
/// Replace the object if it exists (`ip route replace`).
pub(crate) const NLM_F_REPLACE: u16 = 0x0100;
/// Refuse to touch an object that already exists (`ip route add`).
pub(crate) const NLM_F_EXCL: u16 = 0x0200;
/// Create the object if it does not exist.
pub(crate) const NLM_F_CREATE: u16 = 0x0400;

// `linux/rtnetlink.h`: the message types this crate asks for.
/// The rtnetlink multicast groups a subscription binds, as `rtnetlink(7)` numbers them.
///
/// They are the legacy bitmask form, which is what `bind(2)` takes; every group below is within
/// the 32 the mask can name, so no `NETLINK_ADD_MEMBERSHIP` is needed (ADR-0235).
pub(crate) const RTMGRP_LINK: u32 = 0x0001;
/// IPv4 address additions and removals.
pub(crate) const RTMGRP_IPV4_IFADDR: u32 = 0x0010;
/// IPv4 route additions and removals.
pub(crate) const RTMGRP_IPV4_ROUTE: u32 = 0x0040;
/// IPv6 address additions and removals.
pub(crate) const RTMGRP_IPV6_IFADDR: u32 = 0x0100;
/// IPv6 route additions and removals.
pub(crate) const RTMGRP_IPV6_ROUTE: u32 = 0x0400;

pub(crate) const RTM_NEWLINK: u16 = 16;
pub(crate) const RTM_DELLINK: u16 = 17;
pub(crate) const RTM_GETLINK: u16 = 18;
pub(crate) const RTM_NEWADDR: u16 = 20;
pub(crate) const RTM_DELADDR: u16 = 21;
pub(crate) const RTM_GETADDR: u16 = 22;
pub(crate) const RTM_NEWROUTE: u16 = 24;
pub(crate) const RTM_DELROUTE: u16 = 25;
pub(crate) const RTM_GETROUTE: u16 = 26;
pub(crate) const RTM_NEWNEIGH: u16 = 28;
pub(crate) const RTM_GETNEIGH: u16 = 30;

/// `linux/sock_diag.h`: the one request and reply type of the diag protocol.
pub(crate) const SOCK_DIAG_BY_FAMILY: u16 = 20;
pub(crate) const SOCK_DESTROY: u16 = 21;

// `linux/if_link.h`: the `IFLA_*` attributes read here.
pub(crate) const IFLA_ADDRESS: u16 = 1;
pub(crate) const IFLA_IFNAME: u16 = 3;
pub(crate) const IFLA_MTU: u16 = 4;
pub(crate) const IFLA_STATS: u16 = 7;
pub(crate) const IFLA_OPERSTATE: u16 = 16;
pub(crate) const IFLA_LINKINFO: u16 = 18;
pub(crate) const IFLA_STATS64: u16 = 23;
/// `IFLA_INFO_KIND`, nested inside `IFLA_LINKINFO`.
pub(crate) const IFLA_INFO_KIND: u16 = 1;

// `linux/if_addr.h`.
pub(crate) const IFA_ADDRESS: u16 = 1;
pub(crate) const IFA_LOCAL: u16 = 2;

// `linux/rtnetlink.h`: the `RTA_*` attributes read here.
pub(crate) const RTA_DST: u16 = 1;
pub(crate) const RTA_OIF: u16 = 4;
pub(crate) const RTA_GATEWAY: u16 = 5;
pub(crate) const RTA_PRIORITY: u16 = 6;
pub(crate) const RTA_PREFSRC: u16 = 7;
pub(crate) const RTA_TABLE: u16 = 15;

// `linux/neighbour.h`.
pub(crate) const NDA_DST: u16 = 1;
pub(crate) const NDA_LLADDR: u16 = 2;
/// `NTF_ROUTER`: the neighbour advertises itself as a router.
pub(crate) const NTF_ROUTER: u8 = 0x80;

// `linux/unix_diag.h`.
pub(crate) const UNIX_DIAG_NAME: u16 = 0;
pub(crate) const UNIX_DIAG_PEER: u16 = 2;
/// `UDIAG_SHOW_NAME | UDIAG_SHOW_PEER`, the two facts this crate reports about a Unix socket.
pub(crate) const UDIAG_SHOW: u32 = 0x0001 | 0x0004;

// `sys/socket.h`.
pub(crate) const AF_UNIX: u8 = 1;
pub(crate) const AF_INET: u8 = 2;
pub(crate) const AF_INET6: u8 = 10;

// `netinet/in.h`.
pub(crate) const IPPROTO_TCP: u8 = 6;
pub(crate) const IPPROTO_UDP: u8 = 17;

/// The size of `struct ifinfomsg`.
pub(crate) const RT_TABLE_MAIN: u8 = 254;
pub(crate) const RTPROT_BOOT: u8 = 3;
pub(crate) const RT_SCOPE_UNIVERSE: u8 = 0;
pub(crate) const RT_SCOPE_LINK: u8 = 253;
pub(crate) const RTN_UNICAST: u8 = 1;

pub(crate) const IFINFOMSG: usize = 16;
/// The size of `struct ifaddrmsg`.
pub(crate) const IFADDRMSG: usize = 8;
/// The size of `struct rtmsg`.
pub(crate) const RTMSG: usize = 12;
/// The size of `struct ndmsg`.
pub(crate) const NDMSG: usize = 12;
/// The size of `struct inet_diag_msg`, header and sockid together.
pub(crate) const INET_DIAG_MSG: usize = 72;
/// The size of `struct unix_diag_msg`.
pub(crate) const UNIX_DIAG_MSG: usize = 16;

/// `IF_OPER_*`, the operational states of RFC 2863 as `linux/if.h` numbers them.
pub(crate) fn operational_state(code: u8) -> &'static str {
    match code {
        1 => "not-present",
        2 => "down",
        3 => "lower-layer-down",
        4 => "testing",
        5 => "dormant",
        6 => "up",
        _ => "unknown",
    }
}

/// `IFF_*` from `linux/if.h`, in ascending bit order so a flag list renders the same every time.
pub(crate) const INTERFACE_FLAGS: [(u32, &str); 19] = [
    (0x0000_0001, "up"),
    (0x0000_0002, "broadcast"),
    (0x0000_0004, "debug"),
    (0x0000_0008, "loopback"),
    (0x0000_0010, "point-to-point"),
    (0x0000_0020, "no-trailers"),
    (0x0000_0040, "running"),
    (0x0000_0080, "no-arp"),
    (0x0000_0100, "promiscuous"),
    (0x0000_0200, "all-multicast"),
    (0x0000_0400, "master"),
    (0x0000_0800, "slave"),
    (0x0000_1000, "multicast"),
    (0x0000_2000, "port-select"),
    (0x0000_4000, "auto-media"),
    (0x0000_8000, "dynamic"),
    (0x0001_0000, "lower-up"),
    (0x0002_0000, "dormant"),
    (0x0004_0000, "echo"),
];

/// `IFF_UP`: the administrative flag, which is not the operational state.
pub(crate) const IFF_UP: u32 = 0x0000_0001;
/// `IFF_RUNNING`.
pub(crate) const IFF_RUNNING: u32 = 0x0000_0040;

/// `RTN_*` from `linux/rtnetlink.h`, mapped onto the enumeration `ono.route/1` declares.
pub(crate) fn route_type(code: u8) -> &'static str {
    match code {
        1 => "unicast",
        2 => "local",
        3 => "broadcast",
        4 => "anycast",
        5 => "multicast",
        6 => "blackhole",
        7 => "unreachable",
        8 => "prohibit",
        9 => "throw",
        _ => "other",
    }
}

/// `RT_SCOPE_*`. A scope the kernel numbers between the named ones is a site-local convention
/// this provider cannot name, so it reports it as unknown rather than rounding it to a neighbour.
pub(crate) fn route_scope(code: u8) -> Option<&'static str> {
    match code {
        0 => Some("universe"),
        200 => Some("site"),
        253 => Some("link"),
        254 => Some("host"),
        255 => Some("nowhere"),
        _ => None,
    }
}

/// `RTPROT_*`. The set is the system's registry of route origins rather than Ono's, so an
/// unknown value is reported as its number instead of being collapsed into "other".
pub(crate) fn route_protocol(code: u8) -> Option<&'static str> {
    match code {
        0 => Some("unspec"),
        1 => Some("redirect"),
        2 => Some("kernel"),
        3 => Some("boot"),
        4 => Some("static"),
        8 => Some("gated"),
        9 => Some("ra"),
        10 => Some("mrt"),
        11 => Some("zebra"),
        12 => Some("bird"),
        13 => Some("dnrouted"),
        14 => Some("xorp"),
        15 => Some("ntk"),
        16 => Some("dhcp"),
        17 => Some("mrouted"),
        18 => Some("keepalived"),
        42 => Some("babel"),
        99 => Some("openr"),
        186 => Some("bgp"),
        187 => Some("isis"),
        188 => Some("ospf"),
        189 => Some("rip"),
        192 => Some("eigrp"),
        _ => None,
    }
}

/// `RT_TABLE_*`, and otherwise the table's number as its name.
pub(crate) fn route_table(id: u32) -> String {
    match id {
        0 => "unspec".to_owned(),
        252 => "compat".to_owned(),
        253 => "default".to_owned(),
        254 => "main".to_owned(),
        255 => "local".to_owned(),
        other => other.to_string(),
    }
}

/// `NUD_*` from `linux/neighbour.h`, tested from the most specific state down so that a mask
/// carrying more than one bit still resolves to one name.
pub(crate) fn neighbour_state(bits: u16) -> &'static str {
    for (bit, name) in [
        (0x80u16, "permanent"),
        (0x40, "noarp"),
        (0x20, "failed"),
        (0x10, "probe"),
        (0x08, "delay"),
        (0x04, "stale"),
        (0x02, "reachable"),
        (0x01, "incomplete"),
    ] {
        if bits & bit != 0 {
            return name;
        }
    }
    "none"
}

/// The TCP states `linux/tcp_states.h` numbers, which `sock_diag` reports in `idiag_state`.
pub(crate) fn tcp_state(code: u8) -> &'static str {
    match code {
        1 => "established",
        2 => "syn-sent",
        3 => "syn-recv",
        4 => "fin-wait-1",
        5 => "fin-wait-2",
        6 => "time-wait",
        7 => "close",
        8 => "close-wait",
        9 => "last-ack",
        10 => "listen",
        11 => "closing",
        _ => "unknown",
    }
}

/// `SOCK_STREAM`, `SOCK_DGRAM` and `SOCK_SEQPACKET`, the three a Unix socket can be.
pub(crate) fn unix_socket_type(code: u8) -> &'static str {
    match code {
        1 => "stream",
        2 => "dgram",
        5 => "seqpacket",
        _ => "unknown",
    }
}
