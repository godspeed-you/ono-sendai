# ADR-0935: Every runnable command example is held to its declared output

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §9.1, §10.5, §11.3, §15.2, §27, §35.3, §36.5, §50; v0.5 §10.2; ADR-0012,
  ADR-0331, ADR-0571, ADR-0778, ADR-0813, ADR-0930
- Decided by: agent (autonomous)

## Context

Nothing checked that a command produces the schema its contract declares (issue #149).
`timeline` declared `output: stream<ono.temporal-event/1>` from the day it was registered and
answered with one `ono.temporal-timeline/1` record until ADR-0778: the v0.2 §11.3 pre-flight check
trusts the declaration, and `spec-check` compares the registry with declarations only. A person
reading the specification found it, which does not scale.

The machinery to build on exists. `xtask/src/conformance.rs` generates
`crates/ono-cli/tests/provider_conformance.rs` from `docs/contracts/providers/*.yaml`, a
hand-written harness in `crates/ono-cli/tests/conformance_harness/` asks the questions, and
`spec-check` fails when the committed suite is not what the generator produces (ADR-0331). Every
command contract carries `examples:` (spec §50 makes them executable documentation), so the
evidence was already written down; it was never run.

Two questions needed an answer: how to observe a produced value *with its schema identity* through
a public boundary, and which examples may run in a gate on any developer's machine.

## Decision

### The suite

1. **The conformance generator writes a second file**, `crates/ono-cli/tests/command_conformance.rs`
   (`xtask/src/conformance_examples.rs`), and `cargo xtask conformance` and `check_committed` treat
   both files alike. One test per runnable example names the command, the example as written and
   the declared `output`; the question lives once in the shared harness
   (`conformance_harness/examples.rs`), as ADR-0331 split it for providers.
2. **Observation is `<example> | inspect | to json`**, run by the real `ono` in a scratch directory
   with a cleared environment (`HOME`, every `XDG_*`, `ONO_CONFIG_DIR` and `ONO_PLUGIN_PATH` inside
   it; only `PATH` inherited). `inspect` is the public boundary that reports, per value, its type,
   a record's schema id, and each field's declared type, access (§10.5: known, unknown, absent,
   failed) and value. `to json` alone would drop the schema id (its contract says so); a new
   machine protocol would be a second surface to keep. The `--agent` protocol is the remote link's
   and does not run a local example.
3. **What the harness holds** (mirroring `ono_value::Schema::validate`, the validator the shell
   uses): the declared `output` is read as alternatives, each a value or `stream<…>` of them. A
   declaration without `stream<…>` admits exactly one value (none for `null`); a record must carry
   the declared schema id (`record`, `any` and `value` admit any); a scalar must be the declared
   kind; a record's fields must be the contract's, in its order; a known field's value must be of
   the declared type (`enum` members, `list<…>` items, nested records' declared fields, integers,
   byte sizes, strings…); an unknown or absent field must be one the contract lets be null; a failed
   access is lawful (§10.5). `ref<…>` admits any non-null value, as `FieldType::accepts` does by
   design ("a name, a number, an identity map or the resolved object itself"), and a nested
   record's extension fields are not checked, as `validate` does not check them. A command that
   declares `null` is a sink: its example runs as written and must succeed.
4. **An example runs only when its contracts say it is safe to run hermetically**, judged from the
   contract fields of *every* command the example names — nested blocks and parenthesised pipelines
   included, so `each { restart service @ }` does not run. It is skipped, with the contract's own
   reason written into the suite, when a stage: is not a declared command (an external program or
   shell syntax); has a verb `verbs.yaml` marks `mutating`; has a verb whose `pipeline_role` is
   `stream producer` (a live source that never ends); declares a privilege other than `none`; uses a
   capability whose `risk` is not `read`, or that leaves the host; writes through a redirection;
   refers to `@…`, a result or event of an earlier session; or declares an output schema still
   listed in `schemas/deferred.yaml`. Leaving the host is a new contract field:
   `capabilities.yaml` marks `dns.resolve`, `port.probe` and `host.probe` `network: true`.
5. **What the contracts cannot see is a register**, `docs/contracts/conformance/command_examples.yaml`:
   `command`, `example`, `reason` per entry — an example naming an object a fresh environment does
   not have (plan `a82f`, the unit `nginx`), a command whose value a pipeline cannot observe, a
   program no contract describes. Generation refuses an entry with no reason, a duplicate, one that
   names no documented example, and one the contracts already skip, so the register cannot outlive
   its reasons.
6. **The report is the suite's head**: how many examples run, how many do not, every command none
   of whose examples runs with the reason, and at the end every example not run with its reason.
7. **The harness itself is tested** against seeded drift (`command_conformance_harness.rs`): a
   stream of another schema, a stream where one value is declared, a scalar of another kind — each
   must fail; the honest declaration must pass.

### The deferred schemas

`ono.measure/1` and `ono.execution-plan/1` stay deferred here: other work packages write them.
`ono.type-info/1` and `ono.inspection/1` are written to match what `type` and `inspect` produce,
and both commands then answer records of them (following commits). `ono.help-page/1` stays deferred:
`help` is a shell builtin that writes rendered text and produces no value at all, so there is no
produced shape to write a schema from; making help a value (spec §9.1 "Structured help", output
`HelpPage`) is a feature, not a contract repair, and is reported rather than done.

### Findings, and which side moves

When the suite finds a command and its contract disagreeing, the side the authority order (AGENTS.md
§5: spec > contract > code) says is wrong moves, in its own `fix:` commit that removes the
exemption the finding was parked under, so the generated test is red before and green after. The
commit sequence of this ADR lands the suite with those exemptions marked `known defect`.

## Consequences

- 96 examples of 353 run on the first generation; the suite takes about four seconds, because each
  example is one short process and the tests run in parallel. It lives in `crates/ono-cli/tests`,
  so the gate selects it whenever `ono-cli` or a crate it depends on changes (ADR-0853), and its
  generated file changes — and `spec-check` demands regeneration — whenever a command contract, a
  verb, a capability, `deferred.yaml` or the register does.
- A schema leaving `deferred.yaml` turns its commands' examples on at the next generation. When
  `ono.measure/1` lands, `get process | measure memory` runs. When `ono.execution-plan/1` lands,
  `explain`'s examples run — and `explain` takes the rest of its pipeline as its subject, so
  `explain get process | inspect | to json` explains the `inspect` too: those examples need an
  entry in the register (or a harness rule for a command that consumes its own pipeline) in the
  same merge.
- The classification is conservative: privilege `conditional` skips `get file`, `get socket`,
  `get container` and every command a `conditional` capability serves, although many of their
  examples would run unprivileged. Narrowing that needs a contract field that says which examples
  need no privilege; it is not invented here.
- A host without systemd or dpkg answers some examples with nothing; an empty stream conforms to
  any stream declaration, so the suite stays green there and is evidence only where the providers
  answer. CI's runners have both.

Encoded by `xtask/tests/conformance.rs` (`should_run_an_example_its_contracts_let_run_…`,
`should_skip_an_example_whose_verb_mutates_and_say_why`,
`should_skip_an_example_whose_output_schema_is_still_deferred`,
`should_carry_an_exempted_example_and_its_reason_into_the_suite`,
`should_refuse_an_exemption_naming_an_example_no_command_documents`,
`should_refuse_an_exemption_for_an_example_the_contracts_already_skip`,
`should_refuse_an_exemption_without_a_reason`,
`should_report_a_committed_command_suite_that_drifted_from_the_contracts`,
`should_match_the_committed_suite_of_this_repository`), by
`crates/ono-cli/tests/command_conformance_harness.rs`, and by the generated suite itself.

## Spec deviation

- Section: spec §9.1 (Meta)
- Text: "| `help [topic]` | Structured help/discovery. | `HelpPage` |"
- Instead: `help` stays a shell builtin that renders text, and `ono.help-page/1` stays in
  `schemas/deferred.yaml`; its examples are skipped by the suite with that reason.
- Why: there is no produced value to write the schema from, and turning help into a value — a
  record a pipeline can carry, rendered by the presentation layer — is new behaviour across the
  builtin, the renderer and the PTY suites, outside a contract-repair work package. It is reported
  as a finding.

## Alternatives considered

- **Run every example and accept a structured refusal as "no values".** Vacuous evidence for the
  examples that matter most, and it runs mutating examples on the developer's machine.
- **A hand list of runnable examples.** The drift this issue is about, one level up.
- **Validate `to json` output against the schema in the harness.** Loses the schema id, so the
  `timeline` case — a record of the wrong schema with plausible fields — would pass.
- **A separate framework for commands.** ADR-0331's generator, check and harness already exist;
  a second one would be two places to keep.
