# ADR-1022: The stronger declaration confirms, and a destructive capability is the declaring command's

- Status: accepted
- Date: 2026-11-10
- Spec refs: §17.4, §31.16, §31.19, §31.22, §31.37, §31.75; K11P §6.6, §15.3;
  `docs/architecture/external-system-provider.md` §21.5; `docs/contracts/kuang/contributions.v1.yaml`
  → `command.fields.risk`; `docs/contracts/kuang/capabilities.v1.yaml` → `grant.precedence.rules`;
  ADR-1000, ADR-1001, ADR-1002; issue #156 (review finding S-F1)
- Decided by: agent (autonomous)

## Context

ADR-1000 sets a floor under a command's declared risk, ADR-1001 confirms a `destructive` command
by the core gate, and ADR-1002 holds a hello-only declaration to the floor. The independent
security review of v0.6.5 showed the guarantee they were meant to give — *no use of a destructive
capability without the confirmation its risk calls for* — was advisory against a package that
authors both of its declarations:

1. **Two declarations, one read.** `plugins::invoke` read the declaration on disk when there was
   one and the hello only when there was not, and `invoke_contributed` gated on the disk entry.
   Nothing compared them. A disk `wipe` declared `capabilities: []`, `risk: read` beside a hello
   declaring it `destructive` ran without `--confirm`; so did one whose hello declared
   `filesystem.write` at `risk: read` once the permission had been enabled.
2. **A command's declaration did not bound its host calls.** The broker evaluates the package's
   grants (`broker_check` → `policy.evaluate`), whichever command is running:
   `echo:sneaky-clock`, declaring nothing, reads the clock under a package-wide grant. For a
   destructive family that means the command whose declaration was confirmed is not necessarily
   the one that uses the capability.

The confirmation is decided by the shell from a declaration; the capability is enforced by the
supervisor from the grants. The guarantee holds only if the two are tied together at both ends.

The protocol's host calls do not name the invocation they belong to (only `views.open` may,
ADR-0991), and an instance may have several open (`max_concurrent_invocations`).

Of the families in `kuang_capabilities`, two are `destructive`: `filesystem.write` and
`recovery.restore`. Only `recovery.restore` is a brokered host call today; `filesystem.write` has
no host call and is checked only as a command's declared capability at `command.invoke`.

The Kubernetes provider (`ono-sendai-kubernetes`, read for this decision) requests
`network.connect`, `provider.mutate`, `secret.use`, `clock.read`, `state.persist`,
`relation.write` and `process.exec` — no destructive family. Its two commands declare
`[network.connect, provider.mutate]` with `risk: mutate` and `risk: destructive` identically on
disk and in the hello. Its targets, which cannot declare capabilities, read clusters through
`network.connect`, `secret.use` and `process.exec`.

## Decision

1. **The stronger declaration decides the confirmation.** Where a command is declared on disk and
   in the running package's hello, the hello is always read: it is held to the registry's floor
   (a hello below it refuses the invocation as `package.invalid`, as ADR-1002 did for a hello-only
   command), and the invocation is refused with `safety.confirmation_required` unless it satisfies
   the confirmation of *both* declarations. This replaces ADR-1001 Decision 4 ("the declaration on
   disk decides; the hello decides where there is none"). Refusing every disagreement outright was the other
   option offered; it was not taken, because a package that declares a *higher* risk on disk than
   in its hello is not trying to escape anything, and the existing fixture of ADR-1001 (disk
   `destructive`, hello `read` with no capability) is exactly that.
2. **A destructive capability is used only by a command that declared it.** The broker refuses a
   host call needing a capability whose risk is `destructive` with `capability.denied`
   (`reason: undeclared-by-command`, the capability named, the invocations named) unless every
   invocation open in the instance is a command that declared that capability. A host call names
   no invocation, so it is attributed to all that are open; requiring all of them to have declared
   it is sound without that attribution, and an honest restore running alone is unaffected. A
   target never declares a capability, and a call made with no invocation open has nothing that
   declared it — both are refused. The check runs before the policy and before a just-in-time
   question, so nobody is asked about a call that is refused anyway, and the refusal is audited.
3. **Only destructive families.** Bounding every family by the command's declaration would break
   every target of the Kubernetes provider (a target cannot declare the `network.connect` it reads
   through) and its commands' credential helpers (`process.exec`, `secret.use` are package-level).
   The guarantee in question is about destructive capabilities, so the bound is drawn there.

Together: a command that uses a destructive capability must declare it; declaring it makes its
floor `destructive` on disk and in its hello; the stronger declaration then asks for `--confirm`.

## Consequences

- No route to a contributed command uses `filesystem.write` or `recovery.restore` without the
  confirmation `destructive` calls for, whichever declaration a package bends.
- A package whose hello understates a command that is also declared on disk now loses that
  command at invocation with the registry's message, where before the disk declaration hid it.
- An honest package that restores in one invocation while another invocation of the same instance
  that did not declare `recovery.restore` is open gets `capability.denied` for the restore. Fail
  closed: the broker cannot tell which invocation made the call. Giving host calls an
  `invocation` parameter, as `views.open` has, would remove the restriction; it is a protocol
  change outside this fix.
- `filesystem.write` has no host call, so the broker rule covers it only once one exists; today a
  command declaring it is held by the invocation-time capability check and by Decision 1.
- Non-destructive families stay package-wide (`sneaky-clock` still reads the clock under a grant).
  This is recorded, not changed.
- Tests: `crates/ono-cli/tests/plugins.rs` —
  `should_confirm_a_command_by_its_hello_where_the_hello_is_destructive_and_the_disk_says_read`
  (RED: ran unconfirmed, empty stderr),
  `should_refuse_a_hello_that_understates_its_risk_even_where_the_disk_declares_the_command`
  (RED: reached the broker, `permission.denied`); `crates/ono-kuang-sdk/tests/conformance.rs` —
  `should_deny_a_destructive_host_call_the_invoking_command_did_not_declare` (RED: the
  `sneaky-restore` fixture completed). Acceptance case 162.

## Alternatives considered

- **Refuse at load any command whose hello and disk declarations differ in risk or capabilities.**
  Sound, and simple to state, but the check belongs to the load path another workstream owns in
  this run, and it would refuse packages that over-declare on disk. The stronger-of-two rule gives
  the same guarantee at the invocation, where both declarations are at hand.
- **Attribute a host call to "the" invocation only when exactly one is open** (the audit label's
  rule). Equally sound, but refuses two honest restores running together; the all-declared rule
  is no weaker and refuses less.
- **Bound every capability by the command's declaration.** Rejected for Decision 3's reason.
