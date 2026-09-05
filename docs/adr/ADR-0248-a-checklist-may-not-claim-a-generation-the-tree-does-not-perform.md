# ADR-0248: A checklist may not claim a generation the tree does not perform

- Status: superseded by ADR-0331 (in part: the decision not to build the generator)
- Date: 2026-08-29
- Spec refs: §36.2 (documentation derived from the registries), §47; `docs/ACCEPTANCE.md` §3,
  §4.1 D, §4.5, §4.7.4; ADR-0018, ADR-0137
- Decided by: agent (autonomous)

## Context

`docs/ACCEPTANCE.md` §4.1 D said "docs and provider conformance tests are generated from them",
and §4.7.4 said the spatial conformance suite is "generated from `docs/contracts/providers/*.yaml` the
way the v0.2 conformance suites are". Neither was true. `xtask/src/reference.rs` generates
`docs/reference/` and nothing else; `crates/ono-spatial-index/tests/conformance.rs` reads no YAML
at all, and neither do the v0.2 conformance suites the sentence appealed to. What both sentences
described accurately was a *drift check*: the provider claims live in `docs/contracts/providers/*.yaml`
and `spec-check` holds the implementation against them.

`docs/ACCEPTANCE.md` §3 forbids a box that judgement alone can tick. A box whose evidence is
machinery nobody built is worse than an open box, because a reader has no reason to doubt it and
no way to check it.

## Decision

Two things, together.

**The two sentences are corrected**, not the tree. §4.1 D now says `docs/reference/` is generated
from the registries and that what every provider advertises is checked against them; §4.7.4 now
says the four §42 claims are *declared in* `docs/contracts/providers/*.yaml` and held against the tree
by `spec-check`. Both boxes stay ticked, because the drift check they were really describing is
real and green.

**And `spec-check` enforces the shape from now on:** a box in `docs/ACCEPTANCE.md` that contains
the phrase "generated from" must name, in backticks and before that phrase, at least one path that
`cargo xtask docs` actually writes. The rule is mechanical, it fires on exactly the two false
claims as they stood, and it leaves an honest claim — "`docs/reference/adapters/` … is generated
from the contracts" — alone.

Building the generator the sentences described was the alternative, and it is the wrong one for a
reason worth writing down: a conformance suite generated from a YAML file asserts what the YAML
says, so it can only ever restate the claim. The value of `ono-spatial-index/tests/conformance.rs`
is that it is written against §42's *semantics* by someone reading §42, and then held against the
declaration by a drift check. That is two independent statements; a generator would leave one.

## Consequences

`xtask::reference::check_generation_claims` runs on every gate, so a future box cannot claim a
generation that does not exist. The parse is deliberately literal — one phrase, one document —
because a cleverer one would be a second thing to maintain.

Encoded by `xtask/tests/reference.rs::should_report_a_box_that_claims_a_generation_nobody_wrote`,
`::should_accept_a_box_that_names_the_page_it_claims_is_generated` and
`::should_find_every_generation_claim_of_this_repositorys_checklist_true`.

## Alternatives considered

- **Generating the provider conformance suites for real** (C-1's reading). Rejected above: the
  generated suite would assert the declaration rather than the semantics, and the drift check
  already gives the coupling the sentence wanted.
- **Deleting the word "generated" from both boxes and checking nothing.** Rejected: the same
  sentence would be written again by the next agent, and this time nothing would notice.
