# ADR-0016: Value model details the specification leaves open

- Status: accepted
- Date: 2026-08-26
- Spec refs: §10.2, §10.4, §10.5, §12.2, §25, §25.1, §27.3, §28, §43, §46
- Decided by: agent (autonomous)

## Context

Spec §25 introduces its `Value` enum as an "illustrative representation" and §28 says of the
canonical schemas that "names and fields can evolve". Building the model forced a set of choices
the spec does not fix, each of which other crates will now depend on. They are recorded here so
a later agent finds them enumerated rather than having to infer them from the code.

## Decision

### 1. A record holds its schema, not only its id

Spec §25.1 sketches `schema: SchemaId`. A `RecordValue` holds `Arc<Schema>` instead. Field access
by name is on the hot path of every `where` and `select`, and resolving an id through a registry
on each access would make the common operation the expensive one. The id remains available as
`record.schema_id()`, so nothing that wanted an id has lost one. §25.1 is labelled illustrative,
so this is a gap filled rather than a rule departed from.

### 2. The three absences of §10.5 are four outcomes in the type system

`FieldAccess` is `Absent | Unknown | Known(Value) | Failed(Arc<ErrorValue>)`. Spec §10.5 names
three things that must never be conflated; the fourth is `Known`, which is what the other three
are not. Making this an enum rather than a convention means a provider cannot collapse "I was not
allowed to read this process's memory" into "this process uses no memory" by accident — only
deliberately, by writing the collapse out.

There is no separate "unset" state: a required field left unset is stored as `Null` and reported
by `validate` as `provider.schema_violation`, so the mistake surfaces as a contract violation
rather than as a fifth kind of nothing.

### 3. A field holding an `Error` satisfies any declared type

A `Value::Error` in a typed field is valid. Spec §10.5 requires a failed access to stay visible
and distinguishable, and the only alternatives are to drop it — which fabricates data, forbidden
by §35.3 — or to call the whole record invalid, which would push providers back to filling in
zeros to keep their records legal. An `extra` key that shadows a declared field *is* a violation,
because that is a genuine mistake with no legitimate reading.

### 4. `PartialEq` on `Value` is structural; semantic comparison is a method

`Int(1) != Float(1.0)` under `==`; `compare_to` is where unit conversion and numeric coercion
live. Two operations that look alike would otherwise be silently different, and round-trip tests
over a codec would pass while losing type identity.

### 5. `Decimal` is implemented, without a dependency

Spec §10.2 marks `Decimal` optional. It is implemented as an `i128` mantissa with a scale:
exact addition, subtraction and multiplication; division truncated at ten fractional digits.
Money and exact ratios appear in provider data often enough to matter, the implementation is
small, and a dependency for it would be larger than the code.

### 6. JSON is a tagged codec

Natural JSON where JSON is faithful — null, bool, string, list, map, in-range integers. A
single-key `$`-tagged object for everything JSON cannot carry: every semantic scalar, records,
errors, out-of-`i64` integers and non-finite floats. Bytes and non-UTF-8 paths become hex, so an
undecodable byte survives the round trip, which spec §12.2 requires ("never lose undecodable
bytes").

`from_json` takes the schema registry, because rebuilding a record needs its field order.

Known and accepted ambiguity: a foreign JSON object that happens to have exactly one key named
`$bytesize` decodes as a byte size. The alternative — a wrapper on every value — would make
`to json` output that no other tool wants to read, which defeats the purpose of having it.

### 7. `resolve.ambiguous` covers a duplicate schema registration

Registering two different schemas under one id is `resolve.ambiguous` (E0103). Spec §43 has a
`conflict` *kind* but no `conflict.*` *code*, and the taxonomy is closed and additive (ADR-0006);
inventing a code here would be the first crack in that. The situation genuinely is one id
resolving to two things.

### 8. Details of §28 that §28 does not give

Enum member lists (process, service, socket, interface and neighbour states; socket protocol and
family; file kind) are chosen and documented in each schema's doc string.

Identities where §28 gives none: `Interface[name]`, `Mount[target]`, `Socket[inode]`,
`Route[destination]`, `Neighbor[address, interface]`, `ActionResult[target, operation]`.
`Process` uses the `[pid, started]` of §28.1, which also answers the PID-reuse threat of
ADR-0015 T13.

`Group`, `Route` and `Neighbor` have no fields in §28 at all; they are derived from the targets
§8.1 lists and what §23.2 says those providers must answer. `Endpoint` stays a `Map` and a
permission mode stays an octal `Int`, because inventing a schema for either would freeze a shape
the spec never proposed.

### 9. Default-view membership lives on the schema, not on the field

`Schema::default_view()` is the single list; `is_default_view_column(name)` answers the per-field
question. Duplicating the flag onto each field would give two places for it to drift apart, and
spec §27.3 states it as a list on the schema.

## Consequences

Easy: providers get a contract that says exactly what each field means and what its absence
means; the codec loses nothing; the hot path does not consult a registry.

Hard: the `$`-tag ambiguity in item 6 is real and cannot be removed without making `to json`
output that other tools reject. It is documented rather than hidden.

Must be revisited when phase D generates schemas from `docs/contracts/schemas/*.yaml` rather than
declaring them in Rust: the registry becomes derived, and the field lists asserted by
`crates/ono-value/tests/builtin_schemas.rs` become the check that the generation is faithful.

Encoded by: `crates/ono-value/tests/` — 106 integration tests, one per rule above where the rule
is observable.
