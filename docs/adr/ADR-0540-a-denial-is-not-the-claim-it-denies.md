# ADR-0540: A denial is not the claim it denies

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §51.1, §5.3, §15.2, §17.3; supplements ADR-0447 and ADR-0536
- Decided by: agent (autonomous)

## Context

`xtask::terminology` reports a phrase from a term's `overstates` list wherever it appears in a
user-facing document, without exception. That was ADR-0447's decision and it is right for the
case it was written for: a document that says "the plugin runs sandboxed" in one paragraph and
states the boundary in another has still said the false thing, and §17.3 asks for a qualifier that
is *immediate*.

It is wrong for one shape, and the shape is the one the specification itself uses. §5.3, verbatim:

> A native KUANG/11 plugin running under the same Unix account is also **not treated as fully
> isolated** from that account …

`fully isolated` is on the `isolated` term's list. Writing §5.3's sentence into `SECURITY.md` — a
document whose whole purpose is to say plainly what is not protected — turned the gate red for
saying exactly the true thing. §51.1 is explicit that this is not what the check is for: *"The
goal is not to ban these words. The goal is to ensure they refer to a defined contract."*

## Decision

A match whose **immediately preceding word** is a negation — `not`, `never`, `isn't`, `aren't` —
is a denial and is not reported. Everything else is unchanged.

Immediately preceding, and nothing looser. A denial in another sentence does not excuse a claim in
this one: "A native plugin is not a toy. It is fully isolated from your user account" still fails,
and a test asserts that it does. The tightness is deliberate — the same tightness §17.3 asks of a
qualifier — and it is what keeps the exemption from becoming a way past the rule.

This is narrower than the mention exemption ADR-0465 introduced for backticks and quotation marks.
A mention is the document naming a phrase; a denial is the document making the opposite claim.
Both are cases where the phrase is present and the claim is not, and both needed saying because
the honest documents in this repository use both shapes.

## Consequences

- `SECURITY.md` can state §5.3's out-of-scope attacker and §15.2's native trust statement in the
  specification's own words rather than paraphrasing around a gate rule.
- The rule now has three exemptions — word boundaries, mentions, denials — and each exists because
  a true sentence was reported. None of them widens what a claim is.
- A document could evade the rule by writing "not" before a claim it means. That is the same
  exposure the backtick exemption already carries, and the same answer applies: the gate refuses
  the accident, and a reviewer refuses the lie.
- Encoded by `xtask/tests/terminology.rs::should_not_read_a_denial_as_the_claim_it_denies`, which
  asserts both halves.

## Alternatives considered

- **Reword `SECURITY.md` to avoid the phrase.** Rejected: the phrase is §5.3's, and a security
  page that has to write around its own specification's wording is a page whose fidelity is
  decided by a regular expression.
- **Let a document-wide disclaimer excuse a claim.** Rejected in ADR-0536 and rejected again here.
  It would have solved this case and would also have excused the case ADR-0447 was written for.
- **Drop `fully isolated` from the `isolated` term's list.** Rejected: it is §51.1's own example
  of a phrase requiring review, and the claim form of it is exactly what §65.5 forbids.
