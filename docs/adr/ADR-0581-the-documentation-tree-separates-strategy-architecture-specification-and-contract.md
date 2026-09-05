# ADR-0581: The documentation tree separates strategy, architecture, specification and contract

- Status: accepted
- Date: 2026-09-05
- Spec refs: §24.2, §27, §36.2, §47; AGENTS.md §2, §5, §5.2
- Decided by: user (documentation migration), executed by agent

## Context

`docs/` had grown four kinds of document into one flat namespace, and two of them shared a word.

`docs/spec/` held the machine-readable registries — verbs, targets, schemas, errors, commands,
capabilities, provider and KUANG/11 contracts. The narrative release specifications sat beside it
in `docs/` root. Both were called "spec", and the ambiguity was load-bearing in the wrong
direction: AGENTS.md had to spend a paragraph saying that `docs/spec/` is *not* where the
specification lives, and every reader had to hold that correction in mind. `docs/decisions/` was
named for a category rather than for the artefact it holds.

Two new documents then arrived that fitted none of the existing directories: a cloud-native
product vision and a CNCF readiness plan, neither of which is normative for any increment, plus a
generic external-system provider architecture specification, which is normative for an extension
boundary but is tied to no numbered release. Putting them in `docs/` root would have made the
flat namespace worse; putting them under `docs/spec/` would have made the word mean a third thing.

## Decision

`docs/` names each kind of document for what it is:

- `docs/specs/` — the immutable numbered narrative release specifications, plus the
  `spec.sha256` manifest that proves them untouched;
- `docs/architecture/` — cross-cutting architecture specifications, normative for the boundary
  they define and tied to no numbered release;
- `docs/strategy/` — product and ecosystem direction: why a direction is worth taking. Nothing
  here is normative and no gate is derived from it;
- `docs/adr/` — decision records, named for the artefact rather than the category;
- `docs/contracts/` — the machine-readable registries, named for what they are;
- `docs/reference/` — generated output, unchanged.

The project vocabulary follows: a **specification** is a narrative document, a **contract** is a
machine-readable file. The two words no longer name the same directory.

The immutable specifications moved by path only. All nine are byte-identical to their pre-move
state, `docs/specs/spec.sha256` carries the same nine digests against rewritten paths, and the
manifest verifies at its new location.

The Kubernetes provider specification is deliberately absent. It is canonical in
[ono-sendai-kubernetes](https://github.com/godspeed-you/ono-sendai-kubernetes), created on
2026-09-05 while this migration was in progress, and a second canonical copy in core is the thing
that repository separation exists to prevent. That repository is licensed Apache-2.0 while core
remains MIT; the core licence transition is a separate decision this ADR does not make.

## Consequences

Easy: a reader can tell from a path whether a document binds an increment. A contributor looking
for the public contract finds `docs/contracts/`; one looking for what the shell promised finds
`docs/specs/`. Direction is separable from delivery, so the cloud-native vision can be read
without being mistaken for a roadmap commitment.

Hard: every path reference in the tree moved at once — build scripts, generators, the gate, the
Dockerfile, the acceptance harness, tests, ADRs and the generated reference. That is a single
irreversible sweep rather than an incremental one, which is why it is recorded here.

Watch: spec immutability now depends on a directory, not only on a checksum. The manifest proves
that a *discovered* specification was not edited; nothing in it proves that a specification is
discovered at all. A discovery root left behind by a future move would find nothing, report
nothing, and leave nine immutable documents unguarded behind a green gate — the failure ADR-0423
records, arriving through a directory rename instead of a file rename. So discovery is pinned:
`xtask/tests/narrative.rs` holds that a specification outside `docs/specs/` is reported as absent
rather than silently ignored.

## Alternatives considered

Keep `docs/spec/` and put the narrative specifications in `docs/narrative/` — rejected: it leaves
the misleading name on the directory that caused the confusion, and AGENTS.md would still need
its correcting paragraph.

Leave `docs/decisions/` alone to minimise churn — rejected: the sweep touched the tree once, and
doing three of the four renames would have left the fourth as a permanent inconsistency for no
saving.

Add the strategy documents to `docs/` root beside the release specifications — rejected: direction
and normative specification would be indistinguishable by path, which is the specific confusion
this decision removes.

Copy the Kubernetes provider specification into core until its repository exists — rejected: a
temporary canonical copy is how two canonical copies begin.
