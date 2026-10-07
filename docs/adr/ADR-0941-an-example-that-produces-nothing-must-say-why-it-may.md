# ADR-0941: An example that produces nothing must say why it may

- Status: accepted
- Date: 2026-10-07
- Spec refs: §35.3, §36.5, §50; ADR-0935
- Decided by: agent (autonomous)

## Context

ADR-0935 runs every documented example its contracts let run and holds each value it produces to
the command's declared `output`. The harness
(`crates/ono-cli/tests/conformance_harness/examples.rs`) admitted any number of values for a
`stream<…>` declaration, zero included, and checked each value it saw. An example that produced
nothing therefore passed without a single value having been held to the contract (review C8).
Measured on 114bf3a1: 39 of the 111 generated cases produced nothing in the hermetic scratch
environment — every `timeline`, `changes` and `find event` example, `get plan`, `get plugin`,
`get job` and others — so a third of the suite was vacuous, invisibly.

## Decision

1. **An empty answer is a failure by default.** When an example's declaration does not itself
   admit nothing (a `null` alternative), producing no value is a conformance problem: "produced no
   value, so nothing was held to the declaration".
2. **Unless the register says why it may be empty.** `docs/contracts/conformance/command_examples.yaml`
   gains a section `may_be_empty`, entries of `command`, `example`, `reason`, in the same form as
   `exemptions`. A listed example still runs and every value it produces is still checked; the
   entry only makes an empty answer acceptable, and the reason states what a fresh environment
   (or a test host) lacks.
3. **Checked like the exemptions.** `cargo xtask conformance` refuses an entry without a reason,
   a duplicate, and an entry that names no runnable example, so a reason cannot outlive its
   example. Each generated case carries `may_be_empty: Some("<reason>")` or `None`, and the
   suite's header states how many examples may be empty.
4. The register lists the 39 examples that are empty in a fresh environment and 13 whose answer
   depends on the test host (CPU load, networking, a login session, a read-only mount, the
   package database), so the suite stays deterministic across machines.

## Consequences

- An example that starts producing nothing where it used to produce values fails, instead of
  passing silently.
- 52 of 111 examples are declared possibly empty: the gap is visible in one file. Giving those
  examples something to find — a recorded history, a fixture plugin, a seeded plan — would turn
  them into real checks; that is coverage work for later, not a reason to hide the gap.
- Tests: `xtask/tests/conformance.rs` —
  `should_carry_an_example_that_may_be_empty_and_its_reason_into_the_suite`,
  `should_refuse_a_may_be_empty_entry_naming_an_example_no_command_documents`,
  `should_refuse_a_may_be_empty_entry_without_a_reason`; the generated
  `crates/ono-cli/tests/command_conformance.rs`.

## Alternatives considered

- **Require at least one value without exception.** Fails on any host that has nothing to show,
  which is a property of the host rather than of the command.
- **Report empty answers as a warning.** A test that passes with a warning is a test that passes;
  nobody reads the warning.
- **Seed fixtures for every empty example now.** Right in the long run, too large for a release
  fix, and independent of making the gap visible.
