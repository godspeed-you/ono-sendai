# ADR-0546: The boundary inventory is the join key three registries were already writing against

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §6.1, §6.2, §10.2, §20, §51.3, §52.1, §54.1, §65.3; ADR-0472, ADR-0537,
  ADR-0538
- Issues: #118
- Decided by: agent (autonomous)

## Context

§6.1 asks for one file:

> The repository MUST contain a machine-readable or generated boundary inventory that names at
> minimum: … This inventory MUST be derivable into documentation and MUST be referenced by
> security tests.

It did not exist, and it was the last issue of H0 rather than the first, so by the time it was
written twelve of the thirteen phases had already been delivered without it. That is not a
disadvantage. Three of them recorded what its absence cost them, and those three notes are a
better specification than a fresh reading of §6.1 would have been:

- **H2 (ADR-0472)** could not write the gate check `docs/ACCEPTANCE.md` §4.8.3 named. §10.2 —
  *"Every provider/adapter/action dispatch path MUST also validate that the operation is
  permitted"* — was proved by driving the four paths that existed
  (`crates/ono-protocol/tests/authorization.rs::should_refuse_it_on_every_dispatch_path_the_server_exposes`).
  What no test could say is that those four *are* the paths. `docs/STATE.md` carried it under
  *Deferred*: "the §6.1 boundary-inventory gate check has nothing to check against".
- **H12 (ADR-0538)** wrote `remote_trust.yaml` with a `boundary` field per concept and a header
  admitting what it was: *"§6.1's inventory is owed by its own issue; this is the join key."*
- **H12 (ADR-0537)** wrote `refusals.yaml` the same way, and its checker verified the *shape* of a
  boundary name — lowercase, dots, underscores — because membership had nothing to be a member of.

So the inventory was not a new artifact. It was the missing half of three that already existed.

## Decision

**`docs/spec/hardening/security_boundaries.yaml` is §6.1's twelve rows, and it is the vocabulary
every other hardening registry's `boundary` field is checked against.**

### 1. The specification's two columns are copied, not paraphrased

`input_trust` and `required_enforcement` carry §6.1's words character for character, and
`xtask::contracts::REQUIRED_BOUNDARIES` is the same table typed out from the specification. The
test compares the two.

Reading the twelve rows out of the file under test would make the test agree with whatever the
file said, which is exactly the failure §6.1 exists to prevent: an inventory nothing independent
knows the shape of records what somebody remembered. Everything this repository wants to say
about a boundary — which is a lot, and is the useful part — goes in `doc`, where it cannot dilute
the transcription. §6.1 says *"at minimum"*, so a thirteenth boundary is welcome; a missing one of
the twelve is a red gate.

### 2. A boundary names one owner, one module inside it, and the tests §20 accepts it on

§6.2 gives each boundary *"one owning crate/module responsible for enforcing the primary
guarantee"*, and the gate holds all three parts: the crate exists, the module exists **and lives
under that crate** — so §6.2's "higher layers MUST NOT be the only place" has a written-down
answer to "then where is it" — and every `negative_tests` entry resolves to a test that exists and
is not `#[ignore]`d.

The last part is §20 made mechanical:

> A security control is accepted only when there is an automated negative test proving the
> forbidden behavior is refused.

A boundary with no test fails the gate. A boundary whose test was renamed fails the gate. A
boundary whose test was `#[ignore]`d fails the gate, because §65.10's skip-as-pass is the same
defect one level up: a proof that does not run proves nothing. Forty-six negative tests across
seven crates and `xtask` are now load-bearing in a way a rename cannot silently break.

`xtask` is a legitimate owner, for `release.build` and `release.publish`. §43.2 asks that critical
release logic live in first-party repository code rather than in a third-party action, so the
crate that owns those two boundaries is this repository's own automation, and the scanners run on
every gate rather than only on a tag.

### 3. The dispatch paths are declared, and the check runs in both directions

This is H2's owed check, and the reason it needed the inventory. `provider.query` and
`provider.act` each declare their `dispatch_paths`: the method of the served `RemoteService`
trait, the file the handler lives in, the line it opens with, and the guard it must call.

`check_dispatch_paths` then asks two questions:

- **does every declared path ask?** The handler's block is read from its opening line to its
  closing brace and must contain `require_observe(` or `require_action(`. A handler that takes the
  authorization context and never consults it is §65.3 exactly, and it reads as correct at a
  glance because the parameter is right there in the signature.
- **is every path declared?** The methods of `pub trait RemoteService` are read out of
  `crates/ono-protocol/src/service.rs`, and each must appear as a dispatch path at *every* file
  the inventory names as a dispatch site — today the protocol loop and `ono-remote`'s
  `RegistryService`, which is ADR-0472's two-checks-in-two-crates as a contract rather than as a
  habit.

The second direction is the one a future author meets. A fifth method on that trait is a fifth
dispatch path the moment it compiles, and §10.2 governs it immediately. Now the gate says so
before the reviewer has to notice.

The reading is textual, like the rest of `xtask::contracts`. It cannot prove the guard is reached
on every branch — that is what
`crates/ono-protocol/tests/authorization.rs::should_refuse_it_on_every_dispatch_path_the_server_exposes`
is for, and it drives the paths and asserts no provider code ran. The two are complementary: the
test proves the four paths refuse, the gate proves there are no others.

### 4. The page is derived, and the index points at it

§6.1's *"derivable into documentation"* is `docs/reference/security-boundaries.md`, written by
`cargo xtask docs` and compared against the tree on every gate run like every other generated
page. A reader gets the table, then one section per boundary with what this repository knows about
it, its owner, its module, and the tests that prove the refusal — so "what stops that?" is one
click from the answer.

## Consequences

Easy: `refusals.yaml`'s boundary check became a membership test instead of a character-class test,
and `remote_trust.yaml`'s `boundary` stopped being a note about a file that did not exist. Both
headers now name the file rather than the issue that owed it. Renaming a boundary is now a
three-file change the gate insists on, which is the point.

Hard: the forty-six negative tests are named by path and function, so renaming one of them is a
two-file change. That is the trade §6.1 asks for — a citation that cannot rot is a citation
something has to maintain — and it is the same trade `docs/ACCEPTANCE.md` §4.7 and §4.8 already
made for their proofs.

Also hard: `handler_body` finds the *first* occurrence of an entry line. Two handlers opening with
the same text in one file would read as one. Today no file has that, and the alternative — a Rust
parser in `xtask` for a question this size — is the speculative generality AGENTS.md §4 forbids.

What is deliberately not here: `SECURITY.md` still transcribes §6.1's table by hand. Holding that
page to the inventory is a documentation check, it belongs beside `xtask::terminology`'s document
checks rather than in the increment that creates the registry, and #114's box now says so.

Encoded by: `xtask/tests/contracts.rs::should_name_an_owning_crate_and_a_security_test_for_every_declared_boundary`,
`::should_report_a_boundary_the_specification_requires_and_the_inventory_omits`,
`::should_report_a_boundary_whose_named_security_test_does_not_exist`,
`::should_find_the_authorization_check_on_every_declared_dispatch_path`,
`::should_report_a_dispatch_path_that_reaches_a_provider_without_asking_the_authorization_context`,
`::should_report_a_dispatch_path_the_inventory_does_not_declare`,
`::should_report_a_refusal_whose_boundary_is_not_in_the_inventory`,
`xtask/tests/reference.rs::should_render_a_boundary_page_that_matches_the_inventory`.

## Alternatives considered

**Generate the inventory from the code.** §6.1 permits "machine-readable **or** generated", and a
generated one cannot drift. Rejected: what would generate it is the set of places a guard is
called, which is the thing being checked. An inventory derived from the implementation says the
implementation agrees with itself.

**One `boundary` registry per consuming crate.** Each crate declares its own boundaries and the
gate unions them. Rejected by §6.2: one owning module per boundary is the property, and a union of
self-declarations cannot express a boundary that is owned in one place and enforced in two.

**Skip the dispatch-path completeness check and keep H2's four-path test as the whole proof.**
Rejected because that is the state ADR-0472 recorded as insufficient, and the reason it was
insufficient — nothing enumerates the set — is precisely what an inventory fixes.
