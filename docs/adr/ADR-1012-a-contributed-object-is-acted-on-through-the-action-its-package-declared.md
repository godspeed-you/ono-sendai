# ADR-1012: A contributed object is acted on through the action its package declared

- Status: accepted
- Date: 2026-11-10
- Spec refs: §11.5, §16.5, §17.4, §31.19, §31.22, §31.23, §31.37;
  `docs/architecture/external-system-provider.md` §18.4, §21.1, §21.5;
  `docs/contracts/kuang/contributions.v1.yaml` (`command.action`, `action.rules`); issue #224 (6);
  ADR-0583, ADR-0587, ADR-0594, ADR-0595, ADR-0992
- Decided by: agent (autonomous)

## Context

Issue #224 (6): `PluginProvider` implemented neither `subscribe` nor `act`. The question for `act`
was whether `provider.mutate` (ADR-0594) and the `action` block of a command contribution
(ADR-0595) were wired to `Provider::act`, so that the shell's generic mutation boundary — the
`ProviderRegistry::act` that `ProviderMutation` (`stop`, `set`, `kill` …), the change executor's
`ProviderAction` and the host API's `process.signal` all call — could reach a contributed object.

They were not. `PluginProvider` inherited the trait's default, and every action on a contributed
object answered `provider.unsupported`, "plugin:<id> answers queries only" — the five tests below
failed with exactly that message before this change. The protocol has `provider.query` and no
`provider.act`; what a package can do to the system it fronts is a contributed command, and the
`action` block is that command's §21.1 contract (accepted target types, mutation, idempotency,
verification), read by the host before any package code runs. `provider.mutate` is a capability,
enforced by the supervisor at `command.invoke` against every capability the command declares.

## Decision

**`PluginProvider::act` carries out the action the package declared: the contributed command
whose `verb` is the action's operation and whose `action.targets` accept the object's schema (or
`*`), invoked through `command.invoke`. No new protocol call, no plugin special case above the
registry.**

- **Selection.** The verb is the generic action vocabulary (`set`, `remove`, `restart` …), the
  same word `ProviderMutation` passes for a built-in provider. The object's schema is the schema
  the target declares. The first declared command that matches wins; none is
  `provider.unsupported`, naming the package, the operation and the target.
- **Authority.** The command is invoked like any contributed command, so the broker checks every
  capability it declares — `provider.mutate` for a mutation — before the handler runs, and audits
  the allowed and the denied attempt alike (§31.37). A denial is returned as the error, because
  the trait reserves errors for "cannot attempt the action at all", and a missing grant is that.
- **Arguments.** The action's arguments go to the command under the option names it declares.
  One it does not declare is refused rather than dropped: an action asked to change a property it
  cannot see would otherwise report success for a change it never made. `confirm` and `provider`
  are the host's words and are not forwarded.
- **Dry run.** `dry-run: true` reaches the command only when it declares a `dry-run` option; a dry
  run of one that does not is refused, because invoking it would make the change for real.
- **The object** arrives as `object: {schema, identity}` — the schema id and the identity fields
  by name, read from the schema the package contributed — and never as a label, which two
  objects can share (the example package's two places called `checkout`).
- **Outcome.** A failure on the invocation's stream (ADR-0992) is that object's failed outcome,
  carrying the package's own error; a stream that ends cleanly is success, `changed` unless it was
  a dry run. Per-object outcomes are what §11.5 and §16.5 require of a bulk action.
- **Risk and confirmation are not decided here**, exactly as no built-in provider decides them in
  its `act`: the command layer above the registry (`ProviderMutation`'s confirmation, or the
  change gates) decides before it calls. Building a second gate here would make a contributed
  object the one kind of object confirmed twice. Confirmation of contributed *commands* typed by a
  user is issue #156's, and it is a different route.

## Consequences

Every generic caller of `ProviderRegistry::act` reaches a contributed object today, with the
package's declared contract and the broker's authority, and needs no change when one starts
naming a contributed target.

**What does not reach it yet, recorded rather than built here:** no command a user types routes a
contributed object to `ProviderRegistry::act`. `ProviderMutation` is bound only for core command
contracts (`ono-command`'s `implementation_of` skips a non-delivered contract, and the command
table is built once, before a lazily loaded package registers its providers); a contributed
`set echo-place …` is dispatched by `eval/pipeline.rs` straight to `command.invoke`; and the change
planner plans only the core's declared operations. Routing a contributed action command through
`ProviderMutation` belongs to the contributed-stage dispatch and the command table, which were not
this increment's to change. There is therefore no acceptance case for this ADR: nothing a user can
type observes it yet.

The example package gains `set-place` (`set echo-place`), declaring `provider.mutate`, `risk:
mutate`, a `dry-run` option and an action on `dev.example.echo.place/1`; it refuses the state
`rejected` with a message naming the object it was handed.

Encoded by `crates/ono-cli/tests/plugin_provider.rs`:
`should_carry_out_an_action_on_a_contributed_object_when_the_package_holds_provider_mutate`,
`should_refuse_an_action_on_a_contributed_object_when_provider_mutate_is_not_granted`,
`should_report_an_action_the_system_refused_as_a_failed_outcome_for_that_object`,
`should_dry_run_an_action_on_a_contributed_object_without_reporting_a_change`,
`should_refuse_an_operation_the_package_declares_no_action_for`. The routing rule is in
`contributions.v1.yaml` → `action.rules`.

## Alternatives considered

**A `provider.act` protocol call.** Rejected: the `action` block already is the declared contract
of §21.1, and a second call would be a second way to invoke the same handler with a second set of
authority checks to keep in step with the first.

**Match on the command's `target` word instead of the action's accepted schemas.** Rejected: the
target word is how a user spells the command; which objects an action accepts is what §21.1 asks
the action to declare, and `*` actions accept more than one target.

**Forward undeclared arguments and let the package ignore them.** Rejected for the reason above: a
silent drop reads as success.
