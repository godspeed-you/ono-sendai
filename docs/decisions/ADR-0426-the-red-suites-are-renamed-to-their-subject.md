# ADR-0426: The RED suites are renamed to their subject

- Status: accepted
- Date: 2026-09-01
- Spec refs: AGENTS.md §7 (RED → GREEN → REFACTOR), §11 (a pure refactor leaves the suite green
  and unchanged), §8 (superseding, never editing, an accepted ADR)
- Decided by: agent (autonomous)

## Context

Twenty-three integration suites were written on 2026-08-27 as RED suites: the gap between what
`docs/spec/commands/*.yaml` and the narrative specifications promise and what the binary then did,
written out complete and failing before a line of the implementation existed. They were named
`<area>_missing.rs`, and their module documentation said so — *"the contract declares and this
build does not deliver"*, *"every test here is `#[ignore]`d until the increment that delivers
it"*.

All of it is delivered. The suites are green, every `#[ignore]` was removed by the increment that
earned it, and the repository now holds **no `#[ignore]` attribute at all**. What survived was the
name and the prose, on 21 231 lines carrying 597 tests — 69 % of the `ono-cli` test code —
asserting in the present tense that the shell cannot do what it demonstrably does.

`docs/STATE.md` recorded the debt on 2026-08-29 and deferred it: *"The files keep their `_missing`
names because renaming them would rename 113 proofs the checklist points at, which is a `refactor`
of its own."* This is that refactor.

## Decision

**A test suite is named for its subject, never for the state the product was in when it was
written.** The RED phase is recorded by the commit and by this ADR; it is not encoded in a file
name that outlives it.

Twenty of the suites drop the suffix (`processes_missing.rs` → `processes.rs`). Three would have
collided with a suite that already carried the plain name, and are named for what distinguishes
them instead — they are **not** merged into their neighbour, because merging would make two
same-named local helpers into one and change which implementation a test runs, which §11 forbids
in a rename:

| was | is | why |
| --- | --- | --- |
| `plugins_missing.rs` | `plugin_commands.rs` | `plugins.rs` is the runtime and the loader; this is the KUANG/11 command surface |
| `remote_missing.rs` | `remote_commands.rs` | `remote.rs` is the link frame; this is the host and link command family |
| `completion_missing.rs` | `completion_fields.rs` | `completion.rs` is verb and option completion; this is schema-field completion |

Module documentation is moved to the present tense in the same change.

**Pointers into these files are rewritten where a document is a live index, and left alone where
it is a record.**

- `docs/ACCEPTANCE.md` §4.7, `ADR-0203` and `ADR-0245` carry evidence tables that
  `xtask/tests/spatial_evidence.rs` resolves on every run; its own failure message says to
  *"rename them there in the increment that renames the test"*. Their pointer cells are updated.
  This is not editing the history of an accepted ADR (AGENTS.md §8): the Context, Decision and
  Consequences of ADR-0203 and ADR-0245 are untouched, and only the file half of a
  `file.rs::test_name` pointer the gate must be able to follow changes.
- The other 121 ADRs and the session records of `docs/STATE.md` keep the names they used. They
  say what was true when they were written, the test function names they cite are unchanged and
  still greppable, and no check resolves them — the same reasoning by which
  `check_acceptance_case_references` holds the board's session records out of scope.

## Consequences

Easy: a reader who opens `crates/ono-cli/tests/` sees what each suite is about. The largest
single piece of misinformation in the repository is gone.

Hard: a `git log --follow` across the rename is needed to read a suite's RED-phase history, and
the 121 ADRs that cite an old file name now need the test function name, not the file name, to
locate a proof.

Encoded by: `xtask/tests/spatial_evidence.rs::should_find_every_test_the_v04_checklist_names_as_a_proof`,
`::should_find_every_test_the_spatial_enumeration_review_names` and
`::should_find_every_test_the_threat_model_names`, which fail if any pointer this change rewrote
stops resolving.

## Alternatives considered

**Keep the names, fix only the prose** — the name is what a reader sees first in a directory
listing and in a `cargo test` target list, so the contradiction would survive the fix.

**Merge each of the three pairs into one file** — changes which local helper a test binds to, so
the suite would not be provably unchanged across the refactor (AGENTS.md §11).

**Rewrite every ADR that names an old file** — forbidden by AGENTS.md §8, and wrong on its own
terms: an ADR records what proved a decision at the time it was made.
