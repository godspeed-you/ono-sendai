# ADR-0820: The plannable operations are read from the contract

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §4.3, §6.1, §6.2, §7.1, §8.2, §23.1, §47; v0.2 §36.5
- Decided by: agent (autonomous)

## Context

§6.1 fixes what makes an operation plannable: a contract declaring its target scope, its effects,
its reversibility, the privilege it needs and how it is verified. §6.2 makes the absence of one a
refusal. §47 puts the contract in `docs/contracts/change/actions.yaml`, and `ono-change-actions`
loads it — forty-six operations, each with its effects, its idempotency class, its recovery
semantics and its verification contracts.

`ono-cli` did not read it. It carried its own `const OPERATIONS: &[PlannableOperation]` with
eleven rows written out by hand, and every one of the other thirty-five operations was refused
with a sentence saying the shell "knows it as a command and not as a mutation it can describe" —
about operations whose contract described them in full, two directories away.

Nothing caught it. `spec-check` holds the registries against the vocabularies and against each
other; nothing held the shell's table against the registry, because nobody expected there to be
a table.

## Decision

**`ono-cli` reads `ono_change_actions::OperationRegistry::embedded()`, and the hand-written table
is gone.** What the shell adds is only what the registry cannot know, and each of those comes
from the command contract rather than from a second list:

- how the target is frozen (§7.1) — `TargetShape::of(contract.target())`;
- which selector names the object — the contract's first selector;
- whether the selector is a number — the selector's declared type, because a provider holding
  `pid` as an integer answers nothing when asked for the string `4211`;
- whether §43.3's elevation is needed — the contract's `privilege`.

Three things follow from reading the registry rather than a summary of it:

**Every declared effect reaches the plan.** The table carried one effect per operation; the
contract declares as many as the operation has. `kill process` has three, all irreversible, and a
plan that showed the first understated the change — §8.2 asks what the operation does, not what
the most important part of it does. An effect naming its own selector lands on that object, so
`copy file <source> to <destination>` shows the create against the destination.

**Every declared verification contract reaches the plan**, with `{option:<name>}` filled from the
resolved arguments. A contract whose substitution has no value is not emitted: §23.1 asks for
checks that can be answered, and a version check with no version asked is not one.

**`TargetShape` covers every target, not three.** `Service`, `File` and `Package` keep their own
freezing; everything else — a route, a mount, a container, a user, a process — is `Named(word)`
and freezes the way §7.1 describes: the provider's namespace, the value that selected the object,
and the generation a revalidation compares against. The identity is the *selector value* rather
than the display name, because two `sleep` processes have one name and two pids.

## Consequences

- `plan kill process <pid>` produces a plan: three mutation domains, all `UNPROTECTED`, three
  irreversible effects, `HIGH` on irreversibility, and `not recoverable` naming the subject. That
  is §33.1 and §35 reaching an operator, and none of it did before.
- The registry is the single declaration. Adding an operation is a row in
  `docs/contracts/change/actions.yaml`; the shell needs no change, which is what §47 is for.
- `operations()` is built once behind a `OnceLock` and leaks the strings it derives from the
  command registry. They live as long as the process and there is one of each, which is what
  `&'static` here means; the alternative is threading a lifetime through `Resolution` and every
  caller of it for no observable difference.
- A malformed embedded document, or a command registry that will not load, yields an empty list
  and therefore refuses everything. §6.2's default is refusal and §56.3 chooses blocking over
  guessing; both point the same way. The registry's own tests are what keep it from happening.
- `xtask/src/change.rs` should hold the two against each other — a registry row whose command id
  no contract declares is now a silently dropped operation rather than a compile error. That
  check is `check_plannable_operations`, added with this change.

## Alternatives considered

- **Keep the table and grow it to forty-six rows.** It is what the registry already is, kept in
  Rust, and it would drift again the first time a row changed on one side.
- **Generate the table from the registry at build time.** `ono-change-actions` already does that
  — its `build.rs` embeds the YAML as JSON. A second generator over the same input is the same
  duplication with an extra step.
