//! A DNS client of its own, for `resolve dns --server` (ADR-0240).
//!
//! The system resolver answers what *this machine* believes: `resolv.conf`, `/etc/hosts`, mDNS,
//! LDAP, whatever NSS is configured with. That is the right answer to "what does this host
//! resolve `x` to", and it is the wrong answer to "what does *that* nameserver say" — the
//! question an administrator asks when a name is wrong on one resolver and right on another.
//! `getaddrinfo(3)` cannot be pointed at a server, so asking one needs a client, and this is it.
//!
//! It speaks only as much of RFC 1035 as `ono.dns-record/1` can carry: a question for `A`,
//! `AAAA` or `PTR`, and the answers of those three types. Everything it reads is bounded and
//! every length comes from the message rather than from an assumption, because a nameserver is
//! something on the network and a parser of what the network says is where a shell gets broken.

use std::io::{Read as _, Write as _};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::resolver::RecordType;

/// The largest reply this client will read.
///
/// A UDP answer is bounded by the EDNS buffer we advertise (we advertise none, so 512 by RFC
/// 1035); a TCP answer is bounded by its own length prefix, and 64 KiB is the most that prefix
/// can name.
const MAX_MESSAGE: usize = 65_535;

/// How long one exchange with one nameserver may take.
///
/// Shorter than the system resolver's total budget on purpose: the caller named the server, so
/// there is no second one to fall back to and nothing to be gained by waiting longer (spec §34).
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(5);

/// A compression pointer chain longer than this is a message trying to make the parser loop.
const MAX_POINTER_HOPS: usize = 64;

/// One answer, in the terms `ono.dns-record/1` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answer {
    /// The owner name of the record, without its trailing dot.
    pub(crate) name: String,
    /// The record's type.
    pub(crate) kind: RecordType,
    /// The address an `A`/`AAAA` carries.
    pub(crate) address: Option<IpAddr>,
    /// The name a `PTR` carries, without its trailing dot.
    pub(crate) target: Option<String>,
}

/// Asks `server` for `question` of `kind`, over UDP and then TCP if the reply was truncated.
///
/// # Errors
///
/// - `io.not_found` when the nameserver answered `NXDOMAIN`: the name does not exist, which is
///   an answer and not a failure of the server.
/// - `provider.unavailable`, retryable, when the server did not answer in time or answered
///   something that is not a DNS message.
/// - `provider.unavailable` naming the RCODE for every other refusal, because a `SERVFAIL` from
///   the server the user named is that server's answer and must not look like an empty result.
pub(crate) fn ask(
    server: IpAddr,
    port: u16,
    question: &str,
    kind: RecordType,
) -> Result<Vec<Answer>, ErrorValue> {
    let id = query_id();
    let request = encode_query(id, question, kind)?;
    let address = SocketAddr::new(server, port);

    let reply = over_udp(address, &request)?;
    let (truncated, answers) = decode_reply(&reply, id, question, kind, server)?;
    if !truncated {
        return Ok(answers);
    }
    // The answer did not fit in a datagram. RFC 1035 §4.2.2: ask again over TCP, where the
    // length prefix bounds a reply of any size.
    let reply = over_tcp(address, &request)?;
    let (_, answers) = decode_reply(&reply, id, question, kind, server)?;
    Ok(answers)
}

/// One exchange over UDP.
fn over_udp(server: SocketAddr, request: &[u8]) -> Result<Vec<u8>, ErrorValue> {
    let bind: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0".parse().map_err(|_| unreachable(server))?
    } else {
        "[::]:0".parse().map_err(|_| unreachable(server))?
    };
    let socket = UdpSocket::bind(bind).map_err(|error| transport(server, "bind", &error))?;
    socket
        .set_read_timeout(Some(EXCHANGE_TIMEOUT))
        .map_err(|error| transport(server, "set a read timeout on", &error))?;
    socket
        .send_to(request, server)
        .map_err(|error| transport(server, "send to", &error))?;

    let mut buffer = vec![0u8; 512];
    loop {
        let (read, from) = socket
            .recv_from(&mut buffer)
            .map_err(|error| transport(server, "read from", &error))?;
        // A datagram from somewhere else is not this exchange's reply. Reading on is what keeps
        // an off-path answer from being taken for the server's.
        if from.ip() == server.ip() {
            buffer.truncate(read);
            return Ok(buffer);
        }
    }
}

/// One exchange over TCP, whose two-byte length prefix bounds the reply.
fn over_tcp(server: SocketAddr, request: &[u8]) -> Result<Vec<u8>, ErrorValue> {
    let mut stream = TcpStream::connect_timeout(&server, EXCHANGE_TIMEOUT)
        .map_err(|error| transport(server, "connect to", &error))?;
    stream
        .set_read_timeout(Some(EXCHANGE_TIMEOUT))
        .map_err(|error| transport(server, "set a read timeout on", &error))?;
    let length = u16::try_from(request.len()).map_err(|_| unreachable(server))?;
    stream
        .write_all(&length.to_be_bytes())
        .and_then(|()| stream.write_all(request))
        .map_err(|error| transport(server, "send to", &error))?;

    let mut prefix = [0u8; 2];
    stream
        .read_exact(&mut prefix)
        .map_err(|error| transport(server, "read from", &error))?;
    let expected = usize::from(u16::from_be_bytes(prefix)).min(MAX_MESSAGE);
    let mut reply = vec![0u8; expected];
    stream
        .read_exact(&mut reply)
        .map_err(|error| transport(server, "read from", &error))?;
    Ok(reply)
}

/// Builds the query message: a header with one question and recursion desired.
fn encode_query(id: u16, question: &str, kind: RecordType) -> Result<Vec<u8>, ErrorValue> {
    let mut message = Vec::with_capacity(64);
    message.extend_from_slice(&id.to_be_bytes());
    // QR=0 (query), OPCODE=0 (standard), RD=1 (recursion desired).
    message.extend_from_slice(&0x0100_u16.to_be_bytes());
    message.extend_from_slice(&1_u16.to_be_bytes()); // QDCOUNT
    message.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // AN, NS, AR counts
    encode_name(question, &mut message)?;
    message.extend_from_slice(&type_code(kind).to_be_bytes());
    message.extend_from_slice(&1_u16.to_be_bytes()); // QCLASS = IN
    Ok(message)
}

/// Writes a domain name as length-prefixed labels, ending with the root label.
fn encode_name(name: &str, into: &mut Vec<u8>) -> Result<(), ErrorValue> {
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() {
            continue;
        }
        let length = u8::try_from(label.len())
            .ok()
            .filter(|length| *length <= 63)
            .ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{label}` is longer than the 63 bytes a DNS label may be"),
                )
            })?;
        into.push(length);
        into.extend_from_slice(label.as_bytes());
    }
    into.push(0);
    Ok(())
}

/// Reads the reply, answering whether it was truncated and what it carried.
fn decode_reply(
    message: &[u8],
    id: u16,
    question: &str,
    kind: RecordType,
    server: IpAddr,
) -> Result<(bool, Vec<Answer>), ErrorValue> {
    let header = message
        .get(..12)
        .ok_or_else(|| malformed(server, "header"))?;
    if u16::from_be_bytes([header[0], header[1]]) != id {
        return Err(malformed(server, "reply to another query"));
    }
    let flags = u16::from_be_bytes([header[2], header[3]]);
    let truncated = flags & 0x0200 != 0;
    match flags & 0x000f {
        0 => {}
        3 => {
            return Err(ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("{server} answered NXDOMAIN for `{question}`"),
            )
            .with_help("the nameserver states that the name does not exist"));
        }
        code => {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("{server} answered {} for `{question}`", rcode_name(code)),
            )
            .with_retryable(code == 2));
        }
    }
    let questions = u16::from_be_bytes([header[4], header[5]]);
    let answers = u16::from_be_bytes([header[6], header[7]]);

    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(message, at, server)?;
        at = at
            .checked_add(4)
            .filter(|end| *end <= message.len())
            .ok_or_else(|| malformed(server, "question"))?;
    }

    let mut found = Vec::new();
    for _ in 0..answers {
        let (owner, next) = read_name(message, at, server)?;
        at = next;
        let fields = message
            .get(at..at + 10)
            .ok_or_else(|| malformed(server, "answer header"))?;
        let record_type = u16::from_be_bytes([fields[0], fields[1]]);
        let class = u16::from_be_bytes([fields[2], fields[3]]);
        let length = usize::from(u16::from_be_bytes([fields[8], fields[9]]));
        at += 10;
        let data = message
            .get(at..at + length)
            .ok_or_else(|| malformed(server, "answer data"))?;
        at += length;
        // Class 1 is IN. Anything else is not an internet record, whatever its type says.
        if class != 1 || record_type != type_code(kind) {
            continue;
        }
        match kind {
            RecordType::A if length == 4 => found.push(Answer {
                name: owner,
                kind,
                address: Some(IpAddr::V4(Ipv4Addr::new(
                    data[0], data[1], data[2], data[3],
                ))),
                target: None,
            }),
            RecordType::Aaaa if length == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(data);
                found.push(Answer {
                    name: owner,
                    kind,
                    address: Some(IpAddr::V6(Ipv6Addr::from(octets))),
                    target: None,
                });
            }
            RecordType::Ptr => {
                let (target, _) = read_name(message, at - length, server)?;
                found.push(Answer {
                    name: owner,
                    kind,
                    address: None,
                    target: Some(target),
                });
            }
            // A record whose type says one thing and whose length says another is not a record
            // this client will guess at.
            _ => {}
        }
    }
    Ok((truncated, found))
}

/// Reads a name at `at`, following compression pointers, and answers where the *encoded* name
/// ended in the message.
fn read_name(message: &[u8], at: usize, server: IpAddr) -> Result<(String, usize), ErrorValue> {
    let mut name = String::new();
    let mut cursor = at;
    let mut after: Option<usize> = None;
    let mut hops = 0usize;
    loop {
        let length = *message
            .get(cursor)
            .ok_or_else(|| malformed(server, "name"))?;
        if length & 0xc0 == 0xc0 {
            let second = *message
                .get(cursor + 1)
                .ok_or_else(|| malformed(server, "name pointer"))?;
            let target = usize::from(u16::from_be_bytes([length & 0x3f, second]));
            after.get_or_insert(cursor + 2);
            hops += 1;
            if hops > MAX_POINTER_HOPS || target >= message.len() {
                return Err(malformed(server, "name pointer"));
            }
            cursor = target;
            continue;
        }
        if length == 0 {
            return Ok((name, after.unwrap_or(cursor + 1)));
        }
        let label = message
            .get(cursor + 1..cursor + 1 + usize::from(length))
            .ok_or_else(|| malformed(server, "name label"))?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(label));
        cursor += 1 + usize::from(length);
    }
}

/// Skips over a name, answering where it ended.
fn skip_name(message: &[u8], at: usize, server: IpAddr) -> Result<usize, ErrorValue> {
    read_name(message, at, server).map(|(_, end)| end)
}

const fn type_code(kind: RecordType) -> u16 {
    match kind {
        RecordType::A => 1,
        RecordType::Ptr => 12,
        RecordType::Aaaa => 28,
    }
}

const fn rcode_name(code: u16) -> &'static str {
    match code {
        1 => "FORMERR",
        2 => "SERVFAIL",
        4 => "NOTIMP",
        5 => "REFUSED",
        _ => "an RCODE it does not name",
    }
}

/// The reverse name of an address: `4.3.2.1.in-addr.arpa` or the nibble form under `ip6.arpa`.
pub(crate) fn reverse_name(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            format!(
                "{}.{}.{}.{}.in-addr.arpa",
                octets[3], octets[2], octets[1], octets[0]
            )
        }
        IpAddr::V6(v6) => {
            let mut name = String::with_capacity(72);
            for octet in v6.octets().iter().rev() {
                name.push_str(&format!("{:x}.{:x}.", octet & 0x0f, octet >> 4));
            }
            name.push_str("ip6.arpa");
            name
        }
    }
}

/// A query id, distinct enough that a stale reply to an earlier query is not taken for this one.
fn query_id() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU16 = AtomicU16::new(0);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos() as u16);
    NEXT.fetch_add(1, Ordering::Relaxed).wrapping_add(seed)
}

fn transport(server: SocketAddr, what: &str, error: &std::io::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        format!("could not {what} the nameserver {server}: {error}"),
    )
    .with_help("`--server` names one nameserver, and there is no other to fall back to")
    .with_retryable(true)
}

fn unreachable(server: SocketAddr) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        format!("the nameserver {server} could not be addressed"),
    )
}

fn malformed(server: IpAddr, part: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        format!("{server} answered something that is not a DNS message: its {part} does not parse"),
    )
    .with_retryable(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_write_a_question_the_way_rfc_1035_spells_it() {
        let message = encode_query(0x1234, "example.com", RecordType::A).expect("a question");

        assert_eq!(&message[..2], &[0x12, 0x34], "the id is the caller's");
        assert_eq!(&message[2..4], &[0x01, 0x00], "recursion is desired");
        assert_eq!(&message[4..6], &[0x00, 0x01], "one question");
        assert_eq!(
            &message[12..],
            &[
                7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1
            ],
            "the name is length-prefixed labels ending in the root, then QTYPE A and QCLASS IN"
        );
    }

    #[test]
    fn should_read_an_address_out_of_an_answer_that_uses_a_compression_pointer() {
        // The owner name of the answer is `0xc00c` — a pointer back to the question's name,
        // which is what every real nameserver sends and what a parser that assumes literal
        // names gets wrong.
        let mut message = encode_query(0x1234, "example.com", RecordType::A).expect("a question");
        message[6] = 0;
        message[7] = 1; // ANCOUNT = 1
        message.extend_from_slice(&[0xc0, 0x0c]); // NAME: pointer to offset 12
        message.extend_from_slice(&[0, 1, 0, 1]); // TYPE A, CLASS IN
        message.extend_from_slice(&[0, 0, 0, 60]); // TTL
        message.extend_from_slice(&[0, 4, 93, 184, 216, 34]); // RDLENGTH and the address

        let (truncated, answers) = decode_reply(
            &message,
            0x1234,
            "example.com",
            RecordType::A,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .expect("a reply");

        assert!(!truncated);
        assert_eq!(
            answers,
            vec![Answer {
                name: "example.com".to_owned(),
                kind: RecordType::A,
                address: Some(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))),
                target: None,
            }]
        );
    }

    #[test]
    fn should_report_a_name_that_does_not_exist_as_not_found_rather_than_as_nothing() {
        let mut message = encode_query(1, "nope.example", RecordType::A).expect("a question");
        message[3] |= 3; // RCODE = NXDOMAIN

        let error = decode_reply(
            &message,
            1,
            "nope.example",
            RecordType::A,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .expect_err("NXDOMAIN is an answer, and the answer is that there is none");

        assert_eq!(error.code(), ErrorCode::IoNotFound);
        assert!(error.message().contains("NXDOMAIN"));
    }

    #[test]
    fn should_refuse_a_reply_whose_id_is_not_the_questions() {
        let message = encode_query(7, "example.com", RecordType::A).expect("a question");

        let error = decode_reply(
            &message,
            8,
            "example.com",
            RecordType::A,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .expect_err("a reply to another query is not this query's answer");

        assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    }

    #[test]
    fn should_not_loop_on_a_name_that_points_at_itself() {
        // A message crafted so the parser would follow a pointer chain for ever. Bounded reading
        // is the difference between a hostile nameserver and a hung shell.
        let mut message = encode_query(1, "a", RecordType::A).expect("a question");
        message[6] = 0;
        message[7] = 1;
        let start = message.len();
        message.push(0xc0);
        message.push(u8::try_from(start).expect("the fixture is short"));

        let error = decode_reply(
            &message,
            1,
            "a",
            RecordType::A,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .expect_err("a self-referential name is not a name");

        assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    }

    #[test]
    fn should_spell_the_reverse_name_of_an_address_as_the_arpa_zones_do() {
        assert_eq!(
            reverse_name(IpAddr::V4(Ipv4Addr::new(10, 4, 2, 11))),
            "11.2.4.10.in-addr.arpa"
        );
        assert_eq!(
            reverse_name(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa"
        );
    }
}
