# ADR-0783: A printed event reference is unique in the ledger, not in the session

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.6, §32.3, §34, §49; ADR-0620, ADR-0660
- Decided by: agent (autonomous)

## Context

v0.5 §11.6 is one sentence long and it is a promise about the *next* command:

> "Rendered events MUST expose stable references usable in subsequent commands"

with `inspect event @e42`, `at event @e42` and `why event @e42` given as the three uses. The
reference is a prefix of the event's content digest (ADR-0620, ADR-0660), and ADR-0660 decided how
long that prefix is: the shortest that names one event **among the references this session has
already issued**, floored at two hex digits.

That floor was also the answer in practice, because the session table starts empty. A shell that
renders a timeline of four events issues `@e8f`, `@eb0`, `@eb6`, `@e87` — two digits each — and
those are correct only in the session that printed them. The command the reader types next is a
different process with an empty session table, so `EventReferences::resolve` falls through to
`LedgerRead::event`, which resolves the prefix against **the whole ledger**. As soon as two
retained events share those two digits, the reader is told:

```text
ono: Ono-Sendai-E1306 temporal.ambiguous_event that reference names 2 events
  name one of @e34bbc6fb2fb524ca7493ac78, @e34e54e55404f2d59cda13878
```

for a reference Ono itself had just printed. `crates/ono-cli/tests/timeline.rs::should_carry_a_
rendered_reference_into_inspect_at_and_why` reads a reference off one shell's rendering and hands
it to another, and that is what it caught. The test is right; the product was wrong. A session
table can keep a reference *stable*, but it cannot make one *usable*, because usability is decided
in a process that never saw the table.

A second, sharper form of the same problem showed up while reproducing it. The ledger is
append-only and alive: a recording shell appends its own `coverage.started` / `coverage.ended`
events as it starts and exits, so the very shell that printed a row adds events *after* printing
it. Measured on the reproduction below, forty printed references resolved in later shells and one
had been taken back that way — by an event that did not exist when the row was drawn.

## Decision

**A reference Ono prints is the shortest prefix of the event's identity that names at most one
event in the ledger it was printed from, plus one hex digit of headroom, and it never collides
with a reference this session has already shown.** The floor stays at `MIN_REFERENCE_DIGITS`.

1. **Uniqueness is the ledger's question, so the ledger answers it.** `LedgerRead` gains
   `shortest_unique_prefixes(&[EventId], minimum) -> Vec<usize>`: the inverse of `LedgerRead::event`,
   which already resolves a prefix and already knows which prefixes it would call ambiguous. It is
   batched, because a timeline mints one reference per rendered row (up to §11.4's 500) and asks
   once for all of them, and `minimum` is the shortest length the caller would ever print, below
   which a store need not distinguish. The default implementation answers with the whole identity —
   always unambiguous, never short — so no foreign implementation becomes wrong by not overriding it.
2. **The persistent store answers with two index seeks per identity.** `event_id` is the table's
   primary key, so everything sharing a prefix with an identity is contiguous with it in identity
   order: the nearest retained identity below and the nearest above bound the answer, and the
   shortest distinguishing prefix is one character past the longer of the two shared prefixes
   (`distinguishing_length`). No query per lengthening step, and two statements prepared once for
   the whole rendering. The session ledger answers the same question in one pass over its events,
   bucketed by the `minimum` characters every answer starts with.
3. **One digit of headroom** (`REFERENCE_HEADROOM`), because of the live-ledger case above. A
   prefix that named one event the instant it was printed can be taken back by an event nobody had
   seen yet, and the reader then meets E1306 for a reference that was correct on screen. The digit
   costs one character and divides that chance by sixteen.
4. **The session table keeps its job, unchanged.** `reference` returns the same reference for the
   same event for the whole session; a minted prefix is lengthened again if it collides with one
   already shown; `resolve` still asks the session table first and the ledger second, in that
   order. What changed is only how long the prefix is when it is first minted.
5. **A reference cannot be minted without a ledger.** `EventReferences::reference`,
   `EventReferences::reference_all` and `Timeline::with_references` all take the `&dyn LedgerRead`
   the events were read from. The signature is the enforcement: there is no way left to mint a
   reference against nothing and print it.

## Consequences

- **What a reference looks like.** Short where the ledger is small — `@e42f` for a handful of
  events — and longer where it is not. Against the §49 million-event fixture, 500 rendered rows
  took between six and nine hex digits, 6.7 on average: `@e3f7a91`. §11.6's `@e42` shape survives;
  its exact digit count does not, and correctness is the reason.
- **What it costs.** `reference_all` over 500 events against the §49 fixture (1 000 000 events,
  1.9 GB): **4.3–6.8 ms p95** across four release runs on a loaded machine. The `temporal.timeline_15m`
  query itself is 6.6 ms p95 in `docs/contracts/hardening/performance_baseline.json`, so a rendered
  15-minute timeline stays around 12 ms against §32.3's 100 ms p95 budget. The benchmark measures
  `timeline::timeline` and does not mint references, so the two figures are reported separately
  rather than one being read off the other.
- **A reference can still be taken back by history that arrives later.** Headroom makes it
  unlikely, not impossible, and nothing short of writing an allocation table into the ledger could
  make it impossible — which §32.3's budget and §4.7's read-only historical context both forbid.
  The failure mode is the loud one: `temporal.ambiguous_event` naming both candidates, never a
  silent resolution to the wrong event.
- **`LedgerRead` grew a method.** It is a storage-shaped question (which prefixes are ambiguous
  here?), not a presentation one, and it sits beside the `event` lookup that already asks the
  mirror image of it. The presentation rules — the two-digit floor and the headroom digit — stay in
  `ono-temporal-query`, where §11.6 lives.
- **Encoded by:** `crates/ono-temporal-query/tests/search.rs::should_mint_a_reference_a_later_
  session_resolves_when_another_event_shares_its_prefix`, `::should_mint_references_a_later_session_
  resolves_for_every_row_of_a_rendering`, `::should_keep_a_digit_in_hand_so_an_event_recorded_after_
  the_row_cannot_take_it_back`, `::should_issue_the_same_reference_whether_a_row_is_minted_alone_or_
  in_a_rendering`, and `crates/ono-temporal-ledger/tests/query.rs::should_shorten_an_identity_only_
  as_far_as_the_store_can_still_tell_it_apart`, `::should_spell_a_lonely_identity_at_the_shortest_
  length_a_caller_would_print`. The end-to-end promise stays where it was:
  `crates/ono-cli/tests/timeline.rs::should_carry_a_rendered_reference_into_inspect_at_and_why`.
- ADR-0660's clause 1 (allocation) is replaced by this one. Everything else it decided — the
  prefix scheme itself, the session table, resolution order, nothing stored in the ledger — stands.

## Alternatives considered

- **Probe the ledger per event per digit** — mint two digits, ask `LedgerRead::event`, lengthen on
  `temporal.ambiguous_event`, repeat. Correct, and it needs no new trait method; it also costs four
  or five round trips per rendered row against a large ledger, and uses a refusal as control flow.
- **Print the whole identity.** Always unambiguous, and unusable at a prompt: 25 characters per
  row is exactly what §11.6's short form exists to avoid. It survives only as the answer a store
  gives when it cannot answer the question at all.
- **Pick the length from the ledger's size** — enough digits that a collision is improbable for
  `retention().events`. No query, no trait method, and no guarantee: probable is not the same as
  "names exactly one event", and the reader who loses the coin flip cannot tell why.
- **Allocate short names in the ledger.** Globally unique and stable for good, at the price of a
  write on the path of drawing a timeline. Rejected for the same reasons ADR-0660 rejected it.
- **Headroom instead of a ledger lookup.** Two or three extra digits over the session-local rule
  would have made most collisions go away without any new query. It answers the wrong question: a
  reference that is *probably* unique is what the defect already was, one order of magnitude down.
