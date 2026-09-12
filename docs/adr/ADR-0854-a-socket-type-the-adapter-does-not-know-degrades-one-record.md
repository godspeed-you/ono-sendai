# ADR-0854: A socket type the `ss` adapter does not know degrades one record

- Status: accepted
- Date: 2026-09-12
- Spec refs: v0.6.1 §7, §29, §32; v0.2 §10.4, §10.5, §28.4; v0.3 §1.9, §1.10; ADR-0063, ADR-0096
- Decided by: agent (autonomous)

## Context

Issue #131: bare `ss` lists every socket type the host has. `ss_text_v6` recognised the Netid
column from a closed table, and one line of a type outside it — an ICMPv6 ping socket, `icmp6` —
rejected the whole output as `adapter.decode_failed` (ADR-0063 point 1). iproute2 6.x prints
further netids the table lacks (`icmp`, `mptcp`, `tipc`, `v_str`, `v_dgr`, `xdp`), and future
versions will print more. v0.6.1 §7 asks that unknown external data degrade representation
locally and never destroy otherwise valid records.

The issue expects `icmp`/`icmp6` to decode with their own protocol, and an unknown netid to decode
with *that netid* as `protocol` and a null `family`. `ono.socket/1` declares `protocol` as a closed
`enum` and `family` as a required `enum`, and both are load-bearing: the bare-word comparison
`where protocol == tcp` works because `protocol` is an enum (ADR-0096), value completion and
enum-ordered sorting read the declared variants, and a value outside the variants fails schema
coercion — which would reject the output again, one step later.

## Decision

1. `icmp` and `icmp6` join the netid table and the `protocol` vocabulary as protocols of their
   own. `icmp6` takes family `inet6` from the netid, because its `*` address carries no `:` for
   the endpoint inference to see.
2. A first word that is neither a state nor a known netid is read as a netid the adapter does
   not know. The line decodes with protocol `unknown` — the vocabulary's word for a socket type
   it cannot name, already used for `nl` — and a null family, unless an endpoint address shows
   IPv6 the same way it does on every other line. The netid ss printed is kept as `netid` in the
   adapter's extension map, so nothing ss said is lost.
3. A line whose second word is not a state ss prints still fails as `adapter.decode_failed`, so
   output that is not a socket listing is refused as before (the `garbage` fixture).
4. `ono.socket/1` changes in place: `protocol` gains `icmp` and `icmp6`; `family` becomes
   nullable. Both are additive for readers — every record valid before is valid now, and the
   only records that can carry the new values are records that previously made the whole output
   fail — so the schema keeps its version under spec v0.2 §10.4's intent. The netlink provider's
   copy of the schema follows the contract, and the generated conformance suite is regenerated.

## Consequences

- `ss | where protocol == "tcp"` yields the tcp sockets whatever else the host has open.
- A reader of `family` must now expect null. `classify_change` in `ono-value` would call both
  widenings breaking (it knows no enum widening, and treats a nullable widening as breaking); no
  gate applies it to the contracts, and the in-place precedent is ADR-0554's change to the same
  schema.
- Tests: `crates/ono-adapter/tests/decode.rs::should_keep_every_other_socket_when_ss_lists_a_socket_type_the_adapter_does_not_know`
  and the fixture `docs/contracts/adapters/fixtures/iproute2/ss/unknown-netid` under the
  conformance harness.
- ADR-0063 point 1 stands for lines without a state word; its "neither a socket type nor a state"
  refusal is replaced by this ADR for the first column.

## Alternatives considered

- The netid itself as `protocol`, as the issue suggests — needs `protocol` to be an open string,
  which ends bare-word comparison (`where protocol == tcp` would name an unknown field), value
  completion and enum ordering for every socket reader: a compatibility break a patch release
  must not make (v0.6.1 §32).
- An "open enum" in the schema language — declared variants plus any other string — is a new
  schema-language feature across the parser, the generator, spec-check and coercion; not a
  bounded fix.
- Family `other` instead of null — `other` claims the family is outside the list, which for
  `mptcp` (inet) would be false; an unknown is null (spec v0.2 §10.5).
- A new version `ono.socket/2` — every provider, command, spatial space and test that names the
  schema would move for two additive widenings.
