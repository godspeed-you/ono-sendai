# ADR-0089: Null propagates through a schema-known field, and a port compares to an integer

- Status: accepted
- Date: 2026-08-27
- Spec refs: §10.3, §10.5, §10.6, §11.4, §22.3, §28.4, §41.2; ADR-0014, ADR-0031
- Decided by: agent (autonomous)

## Context

`socket.v1.yaml` types `local` and `remote` as nullable `ono.endpoint/1` records — null "for
socket kinds that have none" and "for listening and connectionless sockets" — and the contract's
own examples reach through them: `where local.address not in [127.0.0.1, ::1]` (spec §41.2),
`where remote.address == 10.4.2.11 | stop socket` (`network.yaml`). Two things kept those from
working:

1. `get socket | where local.port == 43067` failed every row whose `local` was null with
   `E0201 cannot read field port on a value of type null`. ADR-0014 gives `?.` for a field the
   schema does not declare; it says nothing about a field the schema *does* declare whose
   value is unknown. The evaluator read `local.port` as two separate accesses — `local` on the
   record (null), then `port` on null — and the second had no way of knowing the null was an
   unknown rather than a value with no fields.
2. Once that was fixed, the predicate matched nothing: `local.port` is a `port` and `43067` is
   an `int`, and `Value::compare_to` called the pair incompatible. Spec §10.6 says a port "may
   parse from integer context", and no example anywhere writes a port literal any other way.

## Decision

### 1. A null record-typed field is an unknown record all the way down

`Value::follow` tracks whether the value it holds came from a record field the schema declares
with a type that has fields — `Record`, `Ref`, `Map`, `Any` — and whose value is unknown
(`FieldAccess::Unknown`). While that is so and the value is null, every further step yields
null without error: `local.port` on a socket with no local endpoint is unknown, and a predicate
over an unknown does not match (ADR-0014). Two things still refuse a required step, exactly as
ADR-0014 wrote them: a null of a field whose type has no fields (`owner.name` where `owner` is
a nullable string is a type error whatever its value), and a null that was never a field — a
`$variable` nobody bound, `@` with no current value. `?.` keeps its meaning: it is for a field
the schema does not declare. `crates/ono-value/tests/null_semantics.rs` stands unchanged.

The expression evaluator follows a chain `a.b.c` as one path from its receiver rather than as
nested single steps, so the tracking is possible; `Expr::Path` at the head of a chain names the
current record's field. A `?.` step anywhere in the chain is honoured where it stands.

### 2. A port and an integer compare as numbers

`Value::compare_to` orders `Port(a)` against `Int(b)` as `i128` — the same shape ADR-0031 gave
paths and strings. `where local.port == 443`, `where local.port > 1024` and `sort local.port`
mean what they say. A port literal (`443port`) remains unspecified by spec §10.6 and is not
added.

## Consequences

- `crates/ono-cli/tests/options_and_selectors_missing.rs::should_resolve_an_endpoint_field_in_a_predicate_when_filtering_sockets_by_local_port`
  is green; every socket example in the contracts that reaches into an endpoint evaluates.
- `docs/reference` needs no change: no contract changed, only what two existing contracts
  already promised now holds.
- A record field whose value is an *error* still propagates the error (spec §10.5: "could not
  be read" is not "unknown"); nothing here touches that.

## Alternatives considered

- **Require `local?.port`.** Rejected: the contract's own examples write `local.address`, and
  making every socket predicate spell the optional access would make the examples wrong.
- **Give unnamed Unix sockets an all-null endpoint record instead of a null `local`.** Rejected:
  it fixes one target and leaves `remote` — null on every listening socket by contract — broken.
- **Coerce integer literals to ports at pre-flight.** Rejected: the comparison is the general
  rule; pre-flight cannot see a `$port` variable.
