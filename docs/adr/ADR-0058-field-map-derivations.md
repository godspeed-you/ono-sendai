# ADR-0058: Field-map derivations and invocation literals

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.8, §1.11, §1.14, §1.33, §1.44, §1.45; ADR-0055, ADR-0057
- Decided by: agent (autonomous)

## Context

ADR-0055's field map covered util-linux: one decoded field becomes one canonical field, with
a unit, a split or a literal translation. `ip -j` does not fit that: an address is two
decoded fields (`local`, `prefixlen`); the records of `ip address` are the entries of each
interface's `addr_info`; `ip neigh` reports a state as a one-element list and no family at
all; `ip route` reports no family either, and `default` instead of a network. Writing these
adapters in Rust would have made the first pack after util-linux a code pack, against
v0.3 §1.45's "simple adapters SHOULD be possible without writing a full plugin runtime
component".

## Decision

The contract grows six small, generic derivations, each written down in
`docs/contracts/adapters/schema.yaml` and each recorded in provenance:

1. `decoder.children: <key>` — the records are the entries of `<key>` inside each element of
   the list; the element is their `$parent`. An element without entries contributes nothing.
2. `field.template: "{local}/{prefixlen}"` — filled from the decoded record (`from` empty) or
   from each object in the list at `from`; a missing placeholder is an error, never an empty
   string, because a half-filled address is a fabricated one. Exactness `normalized`.
3. `field.first: true` — the first element of a list (`state: ["REACHABLE"]`). `normalized`.
4. `field.infer: ip-family` — `inet`/`inet6` from an address or network. Exactness
   `inferred`, which is the only way a field may be inferred (v0.3 §1.8).
5. `adapter.literals: {family: inet6}` — canonical fields the invocation itself states; set
   on every record, never decoded, exactness `exact` because the user typed `-6`.
6. `match.flags.require: [-6]` — flags that must all be present for the invocation to match,
   which is how `ip -6 route` selects the `ip-route6` adapter while `ip route` selects
   `ip-route`. A required flag is consumed by the match; it passes through only if `allow`
   also lists it.

Two consequences for the ip pack specifically: `ip link` runs `ip -j address` so the
canonical `Interface` gets its `addresses` (the schema requires them and `ip -j link` cannot
report them); and `default` maps to `0.0.0.0/0` or `::/0` per adapter, since a null
destination would read as unknown where the tool said "everything".

References (`ref<ono.interface/1>`) are coerced from the name the tool prints, which is
exactly how the netlink providers carry them (ADR-0012).

## Consequences

- The ip pack is YAML, fixtures and an acceptance case — no Rust — which is the property
  ADR-0055 was written for. `ss` (tier C) will still be code.
- Tests: `ono-adapter/tests/decode.rs`
  (`should_derive_records_from_children_with_templates_literals_and_inference`),
  `negotiation.rs` (`should_require_a_flag_when_the_contract_says_so_and_pin_the_family`),
  `contracts.rs` (a template without placeholders is refused), the conformance harness over
  `docs/contracts/adapters/fixtures/iproute2/`, `ono-cli/tests/adapters.rs`, acceptance case `076`.

## Alternatives considered

- A per-adapter Rust decoder for `ip` — rejected as above.
- Deriving the family of a route from the destination — rejected: `default` has no address,
  and the invocation already states the family.
