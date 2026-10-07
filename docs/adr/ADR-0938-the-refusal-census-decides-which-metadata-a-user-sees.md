# ADR-0938: The refusal census decides which metadata a user sees

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §54.1 ("A refusal should tell the user which boundary made the decision"),
  §54.2 ("Important refusal explanations MUST appear in normal structured errors. Users must not
  need `RUST_LOG=debug` to understand why a security policy denied them"), §53.2, §21.4, §16.2;
  ADR-0211, ADR-0015 T1, ADR-0537, ADR-0571
- Decided by: agent (autonomous)

## Context

Four hardening phases attached the fields that name a refusal's deciding boundary to the error
value — `stage`, `limit`, `consumed`, `setting` for the materialization budget, `peer_fingerprint`
and `denied_because` for remote authorization, `control` and `execution_tier` for confinement.
`docs/contracts/hardening/refusals.yaml` declares them per error as `explains`, and spec-check
already holds each declared key to the crate that decides the refusal (ADR-0537). But
`Reporter::error`, the one path every terminal and every redirected run prints an error through,
showed only the message, `details` (ADR-0211) and the help (issue #180). The fields reached a
script that caught the error and no person who read it.

`ono_render::Layout::render_error` with `Detail::Full` renders metadata, and has no production
caller. It renders *all* of it, unordered by intent, together with the whole cause chain and the
target — the full form `inspect @error` is for. What a refusal needs at the prompt is the subset
the census names, in the census's order.

## Decision

1. `Reporter::error` shows, for an error whose census row lists `explains` keys, each of those
   keys the error carries, as `  key: value` lines after the message and its `details`, before the
   help. The order is the census's. Keys the error does not carry are skipped; a `null` value
   shows as `null` (spec §10.5).
2. The census is the only list. `ono-cli` embeds `refusals.yaml` with `include_str!` and parses it
   on the first reported error, as `ono-command` does `language.yaml` (ADR-0571): it is read on an
   error path, never at startup. No key list is copied into Rust.
3. A value is its canonical text, sanitised like every other line the reporter writes
   (ADR-0015 T1), folded to one line and bounded to 160 characters; the whole value stays on the
   error for a script.
4. The same lines appear at a terminal and with stderr redirected: presentation decides colour,
   never content.
5. `render_error` stays what `inspect @error` would use; it is not the reporter's renderer,
   because it cannot select by census without becoming a second census.

## Consequences

- `crates/ono-cli/tests/refusal_explanations.rs` proves the item-budget refusal shows `stage`,
  `ceiling`, `limit`, `consumed`, `setting` in that order, redirected and at a terminal, and that
  an error outside the census gains nothing.
- A remote refusal does not benefit yet: the agent's `Reject` frame carries a code and a sentence
  only (`crates/ono-protocol/src/link.rs`), so `peer_fingerprint` and `store_present` of
  `remote.unauthorized` never reach the client's error value and cannot be rendered there.
  Carrying the explaining metadata across the wire is a protocol contract change of its own.
- Acceptance case 190 reads the whole refusal instead of its first two lines, and asserts the
  fields.
- A census row that adds a key is shown as soon as the code attaches it; spec-check already
  refuses a declared key no source in `decided_by` attaches (ADR-0537). It checks the owning
  crate, not the exact construction site of that one error — a key attached for one refusal of a
  crate satisfies the row of another. Narrowing that is a follow-up, not part of this decision.

## Alternatives considered

- Reuse `render_error(Detail::Full)` — rejected: every metadata key and the cause chain at the
  prompt, which is the "screen of detail" §16.2's terse default exists against.
- Show every metadata key — rejected for the same reason, and because metadata includes machine
  detail (errnos, provider ids) the census deliberately does not list.
- Transcode the census at build time — rejected: an error path does not need it, and a parse on
  first use costs nothing at startup.
