# ADR-0537: A refusal names its boundary in the sentence, and a census says which

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §54.1, §54.2, §53.2, §53.3, §6.2, §9.1, §10.4, §21.3, §24.2, §24.3
- Decided by: agent (autonomous)

## Context

ADR-0473 is titled *"A refusal says which boundary decided"* and it delivered the remote half:
`peer_fingerprint`, `requested_capability` and `denied_because`, as structured metadata. ADR-0444
did the same for the plugin controls, H5 for the resource budgets, H3 for the listening ceilings.
Four phases, four families, four passing proofs.

Reading all four as a user reads them showed two things none of the four could have seen alone.

**The metadata is not rendered.** `ono_cli::report::Reporter::error` prints the code, the message,
the `details` list and the help. Every other metadata key — `denied_because` included, the field
ADR-0473 designated as the answer to "which boundary decided" — is reachable only by catching the
error and selecting the field. §54.2 says the explanation "MUST appear in normal structured
errors" and that a user "must not need `RUST_LOG=debug`" to have it; a field the default rendering
path never prints fails that in spirit even though no log level is involved. §54.1's own examples
are *sentences*, and that is the level the requirement is written at.

Two refusals were failing it in the sentence:

- `remote.capability_denied` said *"this client is authorized, but not for `service.manage`"*.
  §54.1's example is *"remote client sha256:… is authenticated but not authorized for
  service.restart"* — the peer named, and the word "authenticated" present so the reader knows
  which of the two boundaries let them through and which stopped them.
- The retained-history notice said *"result history kept 4 of 50 values because its retention
  budget was reached"*. §54.1's example is *"because the 16 MiB history budget was reached"*.
  Four ceilings can stop a retention, fixed by four different settings, and the sentence was
  identical whichever one did — a test already proved that a byte ceiling and an item ceiling
  produced the same words.

**Nothing held the property across the families.** Each phase proved its own refusal. A hardening
error added later, or one declared and never raised, would say nothing about its boundary and no
gate would notice.

## Decision

**The sentence names the boundary, not only the metadata.**

- `PeerAuthorization::denial` now writes the subject of its own sentence. With an authorization
  context it is `remote client <fingerprint>`, and the three capability denials read *"… is
  authenticated but not authorized for `x`"*, *"… is authenticated but not authorized to
  observe"* and *"… is authenticated, and `y` needs a capability this agent cannot name"*. §53.3
  permits the fingerprint in full: it is public identity material, and it is the exact string the
  operator types into `add client-key` to fix the refusal.
- `Retained` records which ceiling stopped it, and the notice names that ceiling, its configured
  figure and the setting that moves it. `Ceiling::written` is the one place a ceiling is spelled,
  so the refusal in `Exceeded::into_error` and the notice in `ono-history` cannot disagree about
  how a figure reads.
- `remote.unauthenticated` and `remote.authorization_store_invalid` gained `denied_because`, and
  the store errors gained `store_path` and `store_line`. §53.2 forbids matching on message text
  for policy, which only works if the machine-readable field is there.

**`docs/spec/hardening/refusals.yaml` is the census.** One row per hardening refusal, naming the
§6.1 boundary that decided, the §6.2 crate that constructs it, the metadata keys it carries and a
fragment of the sentence it renders. `covers` declares the scope as code prefixes — the blocks
v0.4.1 added, plus §22.3's finiteness refusal, which §54.1 uses as its third example — so the
census cannot be narrowed by deleting a row: an error inside a covered block with no row fails the
gate.

`cargo xtask spec-check` checks four things: every covered error has a row; every row names an
error the registries declare; `decided_by` is a crate that exists; and every key in `explains` is
attached somewhere in that crate's own sources. The last is a textual reading of
`with_metadata("key", …)`, which is crude and answers the question that matters — a row cannot
claim a field nobody sets.

**Two rows are deliberately not errors.**

- `resource.materialization_limit` (E1103) is declared by §21.4 and constructed nowhere. The
  finite-input refusal is `stream.unbounded_operation` and the two budget ceilings are their own
  codes, so nothing is left for it to mean. It carries `raised: false` rather than being omitted:
  a declared code nobody raises is a contract with no behaviour behind it, and leaving it out of
  the census would hide that.
- The history notice has `error: null`. §21.3 gives a caller two lawful responses to a reached
  budget, and history takes the eviction branch because §24.2 makes it a cache. It is in the
  census because §54.1's fourth example *is* this notice, and a census of "which boundary decided"
  that omitted the refusal a user meets most often would have a hole in it.

**The rendered sentence is asserted where it is produced, not in the gate.** A static check cannot
read a `format!` and know what it prints. `says` records the fragment for a reader; the proofs are
`crates/ono-cli/tests/resource_limits.rs::should_name_the_deciding_boundary_in_every_hardening_refusal`,
`crates/ono-protocol/tests/authorization.rs::should_say_which_policy_refused_an_authenticated_client`,
`crates/ono-kuang-supervisor/tests/confinement.rs::should_name_the_control_that_could_not_be_installed_in_the_structured_error`
and case `200`, which runs all four through the real binary with `RUST_LOG=error` set, to say out
loud that none of it depends on a log level.

## Consequences

- A user reading a refusal on stderr now learns which boundary decided without catching the error.
- A new hardening error cannot be added silently: it lands inside a covered block and the gate
  asks which boundary decided it.
- `boundary` is a §6.1 id validated for shape rather than membership, because §6.1's inventory —
  `docs/spec/hardening/security_boundaries.yaml` — is owed by issue #118 and does not exist. When
  it lands, this field becomes the join key and the check becomes a membership test. That is the
  one place this ADR knowingly leaves a check weaker than it should be.
- The `explains` check is textual, so it proves a key is attached *somewhere* in the owning crate
  rather than on that specific error. A row could name a key another error in the same crate sets.
  Narrowing it would mean reading the construction expression, which is a parser; the tests above
  cover the specific error and this covers the class.
- E1103's deadness is now written down where a reader will find it, rather than discoverable only
  by grepping for a variant with no constructor.

## Alternatives considered

- **Render the metadata map in `Reporter::error`.** Rejected for this increment: it changes what
  every error in the product prints, which is a presentation decision with its own tests and its
  own §53.3 questions about what may be shown. Putting the boundary in the sentence is what §54.1
  asks for and touches only the refusals that were failing it.
- **Add `boundary:` to every row of `docs/spec/errors.yaml`.** Rejected: the errors registry is
  the stable taxonomy of spec §43 and is read by the whole product; a hardening-specific field on
  a hundred and thirty unrelated rows would be noise, and §52.1 already puts hardening policy data
  under `docs/spec/hardening/`.
- **Wait for issue #118's boundary inventory and key off it.** Rejected under AGENTS.md §7: the
  census is useful now, the shape check is honest about what it does not verify, and the join is
  a later one-line change.
- **Keep the history notice generic and let the user run `inspect limits`.** Rejected: §54.1's
  example names the budget, and asking the reader to go and look up which of four ceilings applied
  is exactly the second step §54.2 exists to remove.
