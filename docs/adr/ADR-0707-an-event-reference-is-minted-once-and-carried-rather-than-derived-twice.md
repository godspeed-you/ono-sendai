# ADR-0707: An event reference is minted once and carried rather than derived twice

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.6, §12.2, §16.2, §20.4, §34, §35.1; ADR-0660, ADR-0704
- Decided by: agent (autonomous)

## Context

§11.6 makes a rendered reference a MUST and requires it to work afterwards:

> Rendered events MUST expose stable references usable in subsequent commands
>
> ```text
> inspect event @e42
> at event @e42
> why event @e42
> ```

Two implementations of that reference existed. `ono_temporal_query::search::EventReferences` mints
the shortest prefix of the digest that names one event inside a session (ADR-0660), which is what
`resolve` accepts and what §20.4's completion offers. `ono.temporal-event/1` had no field for it,
so `ono_temporal_render` derived an eight-digit prefix of its own (ADR-0704, consequence 1, which
recorded the defect and asked the contract to grow the field).

Both resolve to the same event, and they are different strings. A reader who saw `@e4f3a2c1` on a
row and `@e4f` in a completion menu was looking at two names for one thing, and ADR-0704 said so.

## Decision

**1. `ono.temporal-event/1` declares a nullable `reference`.** It is the short form *this session*
minted, written without the `@` a renderer adds.

**2. It is null on a persisted event and on any record not produced for a session.** A reference is
one session's word for an event: it is the shortest prefix that was unambiguous among the events
*that session had shown*, and a later session showing more events shortens differently. Storing one
would freeze a word whose meaning is a session's history. Null is therefore the normal state of the
field in the ledger, and `event_record` — which takes no session — always writes null.

**3. The timeline producer fills it.** `Timeline::with_references(&mut EventReferences)` mints one
per event in presentation order and `Timeline::to_record` writes them through
`value::event_record_with_reference`. So the string a row prints is, by construction, the string
`EventReferences::resolve` accepts, which is what §11.6 requires of `at event`, `inspect event` and
`why event`.

**4. The renderer's own derivation survives only as the fallback.** A record carrying no
`reference` still gets ADR-0704's eight-digit prefix, which resolves through the same prefix rule
and raises `temporal.ambiguous_event` (§34) when it names two events.

## Consequences

- A timeline the CLI produced for a session shows one spelling of each reference, and it is the one
  completion offers.
- `Timeline` gained `references`, a map that is empty for a timeline produced outside a session.
  `with_references` is the only way to fill it, so producing a timeline never needs a session.
- Every other producer of `ono.temporal-event/1` keeps calling `event_record` and keeps writing
  null, which is correct rather than merely compatible.
- Minting in presentation order means the shortest references go to the events read first.

## Alternatives considered

- **Make the renderer the only minter.** Rejected: the renderer sees one window at a time and has
  no session state, so it cannot keep a reference stable across two renderings (§11.6 says stable).
- **Persist the reference with the event.** Rejected: it is not a property of the event. The digest
  is; the abbreviation is a property of what a session has shown.
- **Widen the derived prefix until collisions are impossible.** Rejected: it fixes nothing —
  the two spellings still differ — and spends columns §39.3 has none of.
