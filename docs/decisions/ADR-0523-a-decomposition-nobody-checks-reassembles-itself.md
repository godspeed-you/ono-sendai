# ADR-0523: A decomposition nobody checks reassembles itself

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §29.2 (parser module boundaries), §30.2 (evaluator responsibility split), §30.4
  (no architecture inversion), §31.2 (session state groups), §52.1 (machine-readable registries),
  §56 (reference crate architecture), §65.12, §66.6; AGENTS.md §11, §15; ADR-0506, ADR-0507,
  ADR-0508 (the H9 decompositions this defends)
- Decided by: agent (autonomous)

## Context

Phase H9 cut `ono-parser`'s parser, `ono-cli`'s evaluator and `Session` into the modules and state
groups §29.2, §30.2 and §31.2 name, under one rule that made the work trustworthy: **no test could
change** (AGENTS.md §11, §65.12). The evidence that the semantics survived is an *unchanged* suite,
and the H9 agent produced it — `git status --short crates/*/tests xtask/tests` was empty.

That rule has a consequence nobody stated at the time. A decomposition whose only evidence is an
unchanged suite has, by construction, **no test of its own**. Nothing in the repository knew that
`crates/ono-parser/src/parser/` was supposed to have nine modules rather than one file, or that
`Session` was supposed to carry eight groups. The H9 agent saw this and declined to close
`docs/ACCEPTANCE.md` §4.8.10's four boxes, because the file they all name —
`xtask/tests/architecture.rs` — did not exist, and creating a new test file is a `test:` increment
rather than part of a refactor. That was the right call under AGENTS.md §15, and it left the
release criterion §66.6 states with no proof.

The failure mode this defends against is not hypothetical and is not dramatic. A responsibility
slides into whichever file was open; the file that holds it is never renamed; and two releases later
the same thousand-line function exists under a different name, with the module list still looking
correct. Nobody decides to undo a decomposition. It erodes.

## Decision

**The architecture is declared as data and checked in both directions, like every other contract
this tranche produced.**

`docs/spec/hardening/module_architecture.yaml` joins the six registries §52.1 names and the ones
the tranche added beside them (`kuang_confinement_controls`, `limits`, `streaming_classification`,
`streaming`, `cost_classes`, `performance_profiles`, `expected_test_skips`, `remote_limits`). It
declares §29.2's parser responsibilities, §30.2's evaluator and native-execution responsibilities,
§31.2's eight state groups, `ono-cli`'s top-level modules, and a five-layer crate assignment for
§56. `xtask::architecture::check` runs in `spec-check`, and `xtask/tests/architecture.rs` holds it.

Four sub-decisions carry the weight.

**1. Both directions, always.** A declared responsibility must have its module, **and** a module
must be a responsibility somebody declared. The first half alone would let the parser grow a tenth
file nobody named — which is exactly the erosion above. The second half is what makes the registry
a boundary rather than an inventory.

**2. A module beyond the specification's list is allowed, with a reason.** `literals.rs` is not in
§29.2. Keeping numbers, units, lists, records and strings inside the expression module would have
made it the largest file in the crate again, which is the condition §29.1 exists against. So
`optional: true` is accepted and `reason:` is required — the gate refuses the first without the
second, so an unlisted module is a decision somebody wrote down.

**3. The composition-root rule is a proxy, and says so in the file.** §30.4 forbids moving domain
logic up into `ono-cli` to shrink a file, and a move like that leaves no failing test behind. No
gate can recognise "domain logic" by reading it. So `composition_root` declares `ono-cli`'s
thirty-two top-level modules and an undeclared one fails: adding a module to the composition root
becomes a decision, not a diff. Naming the limitation in the registry is part of the decision —
a proxy presented as a proof is worse than no check.

The rule beside it is not a proxy: a module of `ono-cli` may not be named for a domain a lower
crate owns. A `parser.rs`, a `pipeline.rs` or a `protocol.rs` in the composition root is the
inversion §30.4 describes, whatever it contains. **This rule fired on its first run**, on
`crates/ono-cli/src/remote.rs`, and the finding was that the rule was too blunt rather than that
the tree was wrong: that file holds §56.3's user commands — `link host` and the four `client-key`
verbs — and no transport, no trust decision and no authorization policy, which live in
`ono-remote` and `ono-protocol` where §56.1 and §56.2 put them. It is excused by name, with that
reason recorded, and the excuse itself requires a reason to be accepted.

**4. The layering is roles, not depth.** The crate graph is acyclic with `ono-cli` at depth ten, so
a layering derived from dependency depth would be **vacuously true**: every edge already points
"down" by construction, and the test would pass forever without ever being capable of failing. The
five layers — foundation, runtime, capability, surface, composition — are roles, assigned by hand
and then *verified* against the real graph rather than asserted over it. The first assignment was
wrong (it placed `ono-command` in the language layer, below the `ono-provider-api`, `ono-graph` and
`ono-adapter` it depends on) and the checker reported three violations, which is how the assignment
came to describe the repository instead of my impression of it.

## Consequences

- §4.8.10's four boxes close, and §66.6's fourth bullet — no cross-crate dependency inversion — has
  a check rather than a reading.
- Adding a module to the parser, the evaluator, native execution or `ono-cli` now requires editing
  a declaration in the same commit. That is friction, and it is the point: it is the same friction
  `docs/spec/errors.yaml` imposes on a new error code.
- A crate the layering does not place fails the gate, so a new crate is placed deliberately. A new
  crate is rare and the placement is one line.
- The composition-root list will need maintenance as `ono-cli` changes. If it becomes noise, the
  answer is to shrink `ono-cli`, which is what §30.4 wants anyway.
- The rule cannot see logic that grows *inside* an already-declared module. A per-module size
  baseline would catch that and is deliberately not built: it would fire on ordinary work and be
  disabled within a month.

## Proof

`xtask/tests/architecture.rs`, eleven tests. Each rule is proved twice — against a fixture that
seeds the defect and must be reported, so the rule is known to bite, and against this repository,
so it is known to hold here. The six names `docs/ACCEPTANCE.md` §4.8.10 asks for are
`::should_find_every_parser_responsibility_in_its_own_module`,
`::should_find_every_evaluator_responsibility_in_its_own_module`,
`::should_find_no_domain_logic_moved_up_into_the_composition_root`,
`::should_find_every_session_state_group_the_specification_names`,
`::should_hold_the_crate_graph_against_the_declared_layering` and
`::should_report_a_new_dependency_edge_that_inverts_a_declared_boundary`.

**On the order of work, stated rather than glossed:** the checker and its tests were written
together, so this increment did not run AGENTS.md §7's RED step in its usual form. The RED that
exists is real and is of two kinds — every fixture test seeds a defect the check must report and
would pass vacuously without it, and the repository-wide run reported `remote.rs` on its first
execution, before any test named it. That is weaker than a failing test written first, and it is
recorded here rather than presented as something it was not.
