# ADR-1024: A contributed action is held to its command's risk, defaults and dry run

- Status: accepted
- Date: 2026-11-10
- Spec refs: §11.5, §11.6, §16.5, §17.4, §31.22; `docs/architecture/external-system-provider.md`
  §21.1, §21.4, §21.5; `docs/contracts/kuang/contributions.v1.yaml` → `action.rules`; ADR-0587,
  ADR-1000, ADR-1001, ADR-1002, ADR-1012, ADR-1022; issue #224 (review findings R-F7 / S-F4)
- Decided by: agent (autonomous)

## Context

ADR-1012 made `PluginProvider::act` carry out the contributed command whose verb is the action's
operation and whose `action` accepts the object's schema. It decided that risk and confirmation are
the caller's business, "exactly as no built-in provider decides them in its `act`". The review
found that this does not hold for a contributed action:

- A built-in provider's caller (`ProviderMutation`) confirms from the *core* contract of the verb
  it was typed as. A contributed command's own declared `risk` is not in that contract, so a
  `destructive` action would at most be bulk-confirmed by the core verb's policy, and the
  ADR-1000 floor was not checked at all for an action resolved from the hello.
- Declared option defaults were not applied, so a package's safe default was each handler's
  memory again. The Kubernetes provider's `remove k8s-resource` declares `dry_run` (underscore)
  default `true`; through `act` it would have arrived without it — a real deletion reported as
  `changed: true`.
- Only an option spelled `dry-run` counted as the command's dry run.

The path is reachable from `ProviderRegistry::act`, which the change executor's `ProviderAction`
calls; ADR-1012 records that no typed command routes a contributed object there yet.

## Decision

`act` holds the selected command to what the command declared, before any package code runs:

1. **Floor.** The hello declaration, with its options, is read as a registry entry; a hello the
   registry would refuse (ADR-1000's floor) refuses the action as `package.invalid` (ADR-1002).
2. **Confirmation.** If the hello or the disk declaration of the command calls for confirmation
   (`destructive` → `confirmation: always`), the action is refused with
   `safety.confirmation_required` — the core refusal of `CommandContract::unconfirmed` — unless
   it carries `confirm: true`, the argument a confirmed mutation forwards. The stronger
   declaration decides, as ADR-1022 decided for a typed invocation. A dry run of a destructive
   action needs it too, as a typed one does (ADR-1001).
3. **Defaults.** Every option default the hello or the disk declaration states is applied when
   the action did not give that option (ADR-0587's rule, on this path too).
4. **Dry run by declaration.** The command's dry-run option is the `bool` option it declares named
   `dry-run` or `dry_run` — the generic vocabulary's word in the two spellings packages write it
   in. A dry-run action sets that option; one whose command declares none is refused, as before.
   The outcome is `changed` unless that option arrived `true`, whether the action asked for it or
   the declared default supplied it.

This reverses ADR-1012's "Risk and confirmation are not decided here" for contributed actions:
a contributed command carries its own risk, which no caller above the registry can see.

## Consequences

- An unconfirmed destructive action on a contributed object is refused, through any caller of
  `ProviderRegistry::act`. A caller that confirmed passes `confirm: true` with the action, as
  `ProviderMutation` already forwards a written `--confirm`. The change executor carries the
  plan's arguments; a plan that should act on a destructive contributed command will have to
  carry the confirmation it was approved with — recorded for when the planner learns contributed
  operations (ADR-1012's open routing).
- The Kubernetes provider's `remove k8s-resource` through `act`: refused without `confirm`; with
  it, a server dry run unless the action says `dry_run: false`, reported as unchanged.
- "Dry run" is still recognised by name, in two spellings, because the contribution schema has no
  field that marks an option as the dry run. A field in `action` naming it would be the exact
  form; it is a protocol addition, not needed for either package that exists.
- Tests: `crates/ono-cli/tests/plugin_provider.rs` —
  `should_refuse_a_destructive_action_on_a_contributed_object_that_was_not_confirmed` (RED: the
  action was attempted), `should_refuse_an_action_whose_hello_understates_its_risk` (RED:
  attempted), `should_apply_an_actions_declared_defaults_and_honour_its_declared_dry_run_option`
  (RED: the handler received no `dry_run`); the five ADR-1012 tests unchanged. Fixture: the
  example package's `--destructive-action` mode. No acceptance case: no typed command reaches
  `act` for a contributed object yet (ADR-1012).

## Alternatives considered

- **Leave confirmation to the caller (ADR-1012).** The caller confirms from a core contract that
  does not carry the contributed command's risk; the review showed that is a bypass.
- **Recognise only the hyphenated `dry-run`.** Kubernetes, the one real provider, spells it
  `dry_run`; its default would still have to be applied, and an action asked as a dry run of it
  would be refused.
- **Refuse a destructive action through `act` outright.** Safer still, but makes the generic
  boundary useless for the one destructive command a real provider has, where a confirmed caller
  is exactly what the boundary is for.
