# ADR-0240: Asking one nameserver is a different question

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §9.1 (`resolve dns` yields `Stream<DnsRecord>`), §16.5, §34 (latency is a
  product property), §35.3, §43; supersedes the `--server` paragraph of ADR-0087
- Decided by: agent (autonomous)

## Context

`network.yaml` has always declared `resolve dns --server <ip>`, "query this resolver instead of
the system's", and ADR-0087 §1 refused it:

> `--server` is refused: the system resolver answers from `resolv.conf` and NSS, and asking a
> particular server needs a DNS client this build does not include.

The refusal was accurate about the cause and left the option as help text for behaviour that did
not exist — the family ADR-0233 exists to prevent. And the option is not a convenience. "What
does this host resolve `x` to" and "what does *that* nameserver say about `x`" are different
questions with different answers, and the second is the one an administrator asks when a name is
wrong on one resolver and right on another. `getaddrinfo(3)` can only answer the first.

## Decision

**`ono-provider-net` has a DNS client**, `nameserver.rs`, and `--server` uses it. It speaks
exactly as much of RFC 1035 as `ono.dns-record/1` can carry — a question for `A`, `AAAA` or
`PTR`, and answers of those three types — over UDP, retrying over TCP when the reply comes back
truncated. An address query becomes a `PTR` question in `in-addr.arpa` or `ip6.arpa`; a name
query without `--type` asks for both address families, as the system resolver does.

**Every length in a reply comes from the message, and every read is bounded.** A nameserver is
something on the network: name compression pointers are followed with a hop limit, a pointer past
the end of the message is a parse failure, a datagram from another address is not this exchange's
reply, and a reply whose id is not the question's is refused. A parser of what the network says
is where a shell gets broken, so the tests include a self-referential name and a mismatched id.

**A refusal from the named server is that server's answer.** `NXDOMAIN` is `io.not_found` —
the server states the name does not exist — `SERVFAIL` is a retryable `provider.unavailable`, and
every other RCODE is named in the message. None of them is an empty result: `--server` has no
second server to fall back to, so silence would be a lie (§35.3).

**`--port` is added beside it.** A local stub resolver rarely listens on 53 — systemd-resolved's
is on 5353, a test zone is wherever it was started — and `dig -p` exists for the same reason.
Without `--server` it is refused, because it would say nothing.

The system resolver stays the default and is unchanged. Nothing about `resolve dns example.com`
moves: it still goes through NSS, so `/etc/hosts`, mDNS and LDAP still take part.

## Consequences

- `resolve dns example.com --server 9.9.9.9` answers that server's records; the provenance says
  `9.9.9.9:53` rather than `getaddrinfo(3)`, so `inspect` shows which of the two answered.
- A name that resolves through `/etc/hosts` and not in DNS answers over the system resolver and
  `NXDOMAIN` over `--server`. That is the difference the option exists to show, not a bug.
- No TTL and no authority section are exposed, because `ono.dns-record/1` carries neither. The
  client reads the TTL off the wire and discards it; promising it would be a schema change with
  no question behind it yet.
- The acceptance suite proves the option surface and the refusals (`124-dns-named-server`), and
  not the positive path: the container runs with `--network=none` and ships no nameserver to
  point at. The positive path is proved by
  `should_answer_from_the_nameserver_that_was_named_rather_than_from_the_system_resolver` and
  `should_answer_a_reverse_question_from_the_named_nameserver`, which start a real UDP
  nameserver on a port the kernel chooses and answer a name that exists in no zone anywhere —
  so an answer can only have come from it.
- Encoded by the five tests in `crates/ono-provider-net/src/nameserver.rs` and the four in
  `crates/ono-cli/tests/network_missing.rs`.

## Alternatives considered

- **Take a resolver crate.** It would bring a full RFC-1035 implementation, an async runtime
  integration and a dependency surface far larger than three record types need, for a client
  whose whole job is to ask one server one question.
- **Shell out to `dig`.** v0.3's adapter road exists for exactly this and it is the wrong tool
  here: an adapter parses a program's output, and `dig`'s output is human text that changes
  between versions — spec §50's "never parse unstable human-readable output" (AGENTS.md §6).
- **Keep the refusal and remove the option from the contract.** Honest, and it deletes a
  capability the spec's own command table asks for rather than delivering it.
