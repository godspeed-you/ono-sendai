# ADR-0587: A contribution declares its own arguments, and its own refusal

- Status: accepted
- Date: 2026-09-06
- Spec refs: §10.5, §31.5, §31.16, §31.22, §31.23, §31.64, §31.68, §31.75, §31.79, §31.86, §36.5,
  §50; `docs/architecture/external-system-provider.md` §21.1, §21.2, §21.5;
  `docs/contracts/kuang/contributions.v1.yaml`, `docs/contracts/kuang/errors.v1.yaml`,
  `docs/contracts/capabilities.yaml`, `docs/contracts/errors.yaml`;
  ADR-0009, ADR-0012, ADR-0022, ADR-0144, ADR-0282, ADR-0444, ADR-0582, ADR-0583, ADR-0586
- Decided by: agent (autonomous)

## Context

Building a real external-system provider — a package that reads a Kubernetes cluster and now
writes to one — turned up five places where the contribution model could not express something
true. This ADR settles all five: it closes three, closes the visible half of a fourth, and defers
the fifth with its reason.

**1. A contributed command could not declare its options.** `contributions.v1.yaml` says a command
contribution uses "the same metadata schema core commands use", and lists `selectors` and
`options` among its fields. `CommandContribution` carried neither, and `ContributedCommand::into_contract`
said so in a comment while writing `selectors: Vec::new(), options: Vec::new()`. So a package
could write the documented field and have it silently dropped — the worst of the three possible
outcomes, worse than refusing it and worse than reading it.

The cost is not abstract. The Kubernetes package's `dry_run` argument decides whether a cluster is
changed. Undeclared, it had no help line, no type, no completion candidate and no default the host
could apply: a user typing a mutating command got no assistance on the argument that matters most,
and the package had to hope every handler remembered the safe fallback.

**2. A contributed target could not declare its options either.** ADR-0582 made `get <target>
--context prod` *work* — the words reach `provider.query`. Nothing told a user the word existed.

**3. No capability family covers "change state in the external system a provider fronts."** The 29
families of §31.16 gate *host* services. A package that carries its own protocol over a brokered
byte connection is invisible to the broker between a `GET` and a `PATCH`, so its writes are gated
on `network.connect` — truthful and enforceable to the host and port, and unable to separate
reading a cluster from writing to one. An operator cannot grant read-only access to a cluster.

**4. No error code for a refusal by a provider's own safety rule.** A package that declines because
one of *its* preconditions is unmet had to borrow a code that asserts something else.
`safety.policy_denied` says "a *configured* safety policy forbids the operation" when nothing was
configured; `provider.unsupported` claims an inability the package does not have;
`provider.unavailable` claims the external system did not answer when it was never asked.

**5. The provider role has one protocol method and it is a read.** `provider.query`. A provider
mutation must arrive as an opaque `command.invoke`, so §21.1's action contract — accepted target
types, a parameter schema, idempotency semantics, a result schema, a verification hook — has
nowhere to live.

## Decision

### 1 and 2. A contribution declares its arguments, in the vocabulary a core command declares its own

`CommandContribution` gains `selectors` and `options`; `TargetContribution` gains `options`. Both
are lists of a new `ParameterContribution`, which is field-for-field a core command's parameter
entry — `name`, `type`, `doc`, `repeatable`, `optional_value`, `default` (ADR-0012 §7, ADR-0144).
"The same metadata schema core commands use" is meant literally: the declaration becomes
`ParameterSpec`s on the registry entry, and from there `help`, completion, `explain` and the
synopsis are the same code that serves `get process --sort`.

Both fields default to empty on the wire and in the on-disk declaration document, so every package
that exists keeps working unchanged.

Three things a declaration buys, and the third is the one that matters for a mutating package:

- **Help and completion.** `help get echo-item` shows an `OPTIONS` block and the synopsis reads
  `get echo-item [--count <int>]`; `get echo-item --co<TAB>` completes to `--count`. Both hold for
  a target's options as much as a command's.
- **A declared type.** A written word is coerced to the type the declaration gives it, so
  `--count 1` arrives as an integer because it was *declared* one rather than because it happened
  to parse as one. Values cross in the lossless tagged encoding the protocol already uses, so a
  `duration` arrives as a duration rather than as a string somebody has to re-parse.
- **A default the host applies.** When the argument is absent and the declaration names a default,
  the host supplies it. This is what lets a mutating contribution default to not mutating: the
  safe value becomes the shell's guarantee instead of every handler's memory. An absent default
  stays absent — unknown data is null, never zero and never false (spec §10.5).

`protocol.v1.yaml` had already promised the typing half of this and could not deliver it:
`command.invoke.arguments` reads "Selectors and options, already bound and typed by the host's
command layer" and `provider.query.options` reads "Provider options by name, already typed". With
nowhere to declare a parameter there was no type to bind against, so the host forwarded whatever
the words looked like. The promise is now kept for every argument a contribution declares.

**A declaration does not close the argument set.** A word the contribution did not declare still
reaches the package. Refusing it would refuse invocations that work today, and nothing about a
package declaring *some* of its arguments says it accepts no others. What a declaration adds is
the four things above; what it does not add is a refusal.

The declaration is read from the package's own `contributions.*` documents before any of its code
runs (spec §31.68), so help, completion and the default are all available at the placeholder
stage — a user finds out what `--context` is for without loading anything.

### 3. No thirtieth capability family. The package states its risk, and the host shows whose statement it is

**A capability family for "mutate the fronted system" is rejected**, and §31.16's own sentence is
the reason: *"A scope that cannot be enforced reliably MUST NOT be offered as if it were a security
boundary."* The broker sees bytes on a connection it already authorised by host and port. It cannot
tell a `GET` from a `PATCH`, it cannot enumerate a cluster's namespaces to scope them, and a
package that held such a grant for its read command could use the same connection inside any other
command. A family whose only enforcement is "the host declines to start a contribution that
declares it" is not a boundary against the package it is meant to bound; it is a label with a
lock's shape. Offering it would teach an operator that `get k8s-pod` is safe because the write
family was not granted, which is precisely the false confidence §31.16 forbids.

**An unenforceable risk marker does have a place — as a statement, attributed.** The honest answer
is the one the finding suggests: only the package can know, and the model should make it say so.
`CommandContribution.risk` already existed on the wire, carrying one of the four `risk_levels` of
`docs/contracts/capabilities.yaml`, and it was read and thrown away — no registry entry held it, so
no help page, no `explain` and no operator ever saw it. It now reaches the registry entry as
`CommandContract::declared_risk`, and every surface that shows it says whose claim it is:

```
SAFETY
  privilege     conditional
  risk          destructive  (declared by the contributing package)
```

A word outside the four is `package.invalid` at registration, reported like every other unreadable
declaration — a closed vocabulary that accepts a fifth word is not a vocabulary. The vocabulary now
has one reader for its two documents, so the capability registry and a contribution cannot come to
disagree about what `destructive` means.

The synthetic `get` the host builds for a contributed target declares `read` for it, because
§21.2 of the external-system-provider architecture forbids a getter from mutating and the host is
the one making that entry.

**What this deliberately does not do:** it does not *require* a risk where a mutating capability is
declared, and it does not turn `destructive` into a confirmation prompt. Requiring it would refuse
packages that install today, which this increment is not allowed to do; the confirmation path for a
contributed stage is host-side work with tests of its own. Both are written into `docs/STATE.md`
under *Found, not yet filed*.

### 4. `contribution.refused` — `Ono-Sendai-K11901`

A new code, in a new 901 block, `kind: safety`. §31.79's families 001 through 701 are the host's
account of the package — its manifest, its load, its instance, its grants, its state, its views,
its models, its links — and 801 (ADR-0444) is the host's account of a launch that never happened.
None of them is the *package's* account of an operation it would not perform. 901 is the next free
block, and the taxonomy stays closed and additive.

Its help says what it is and what it is not, because the three codes it replaces each assert
something false and a reader needs to know which claim is now being made:

> This is the package's rule, not the host's policy and not the external system's answer: a
> precondition the package requires was not met, so it did nothing.

It reaches a user through the path every KUANG error already takes: the package raises it, the
supervisor carries it, and `wire_error_value` resolves it against the one taxonomy `ono-core` and
`errors.yaml` share, so it renders and is caught exactly as `Ono-Sendai-E0102` is.

### 5. A typed provider action is its own tranche, and does not belong in this increment

Deferred, deliberately, and this is the reasoning rather than an omission.

§21.1 asks a provider action to declare an action identity, accepted target types, a parameter
schema, required provider capabilities, required KUANG/11 privileges, whether it mutates, its
idempotency semantics, an expected result schema, a verification hook or the explicit lack of one,
and prospective-change metadata. Four of those ten have no home in this build yet: §21.5's
confirmation contract needs the host's confirmation policy to reach a contributed stage; §21.6's
verification hook needs a result-versus-outcome distinction nothing models; §22 requires the
action to reuse the v0.6 `ChangePlan`, and **v0.6 is not implemented** (AGENTS.md §5.2 lists it as
the tranche after v0.5, itself unstarted); and §21.4's provenance-of-prediction labelling depends
on the same plan object.

Delivering `provider.action` before those exist would mean shipping a method whose declaration
names fields nothing reads — which is exactly the defect this ADR is closing in finding 1. The
sequence is the other way round: the `ChangePlan` first, then the action that produces one. Until
then a provider mutation is an ordinary contributed command, which now declares its own options,
its own default and its own risk — and those three are most of what §21.1 asks for that is
expressible today.

## Consequences

**What a package author can now declare that they could not:**

- the positional and named arguments of a contributed command, each with a type, a doc line, a
  repeatable flag, an optional-value flag and a default — reaching `help`, completion, `explain`,
  the synopsis, argument typing and the value the host supplies when the user says nothing;
- the options a contributed target's query is narrowed by, with the same five properties and the
  same reach;
- what a contributed command actually does to the world, as one of the four `risk_levels`, shown
  as the package's own claim rather than as a check the host performed;
- that the package itself is refusing, and why, without borrowing a code that describes a
  different situation.

**What is unchanged.** A package that declares none of this behaves exactly as before: its words
reach it unchanged, its help page has no `OPTIONS` block, its risk is unstated, and nothing about
it is refused. The two example packages in the tree, every existing contributed command and every
existing contributed target keep working; the fields are optional at the wire, in the document and
in the registry.

**`contributions.v1.yaml` is now drift-checked.** Its `command.fields`, `target.fields` and the new
`parameter.fields` are compared against serde's own field names on the wire shapes, both
directions, with `provider` excepted as the one documented field the host sets rather than the
package sending. This is the check whose absence let finding 1 exist: every other registry under
`docs/contracts/` was already held against its implementation, and this one reached `spec-check`
only through the generic sweep that proves a file is non-empty valid YAML.

**Which tests encode it:**

- `crates/ono-cli/tests/plugins.rs` — `should_show_a_contributed_commands_declared_option_in_its_help_page`,
  `should_offer_a_contributed_commands_declared_option_when_completing` (a real pseudo-terminal,
  because completion happens nowhere else),
  `should_apply_a_contributed_commands_declared_default_when_the_option_is_absent` (the fixture
  declares `count: 2` while the package's own fallback is 3, so only a default the host applied can
  produce two values),
  `should_show_the_risk_a_contributed_command_declares_in_its_help_page`,
  `should_refuse_a_contributed_command_whose_risk_is_not_a_risk_level`.
- `crates/ono-cli/tests/plugin_targets.rs` — the same three for a target, plus
  `should_name_a_packages_own_refusal_as_its_own_rather_than_a_host_policy`, which stands beside
  the older `should_report_a_refusing_target_rather_than_an_empty_success`: two targets that both
  refuse and emit nothing, and the codes they refuse with are different because the situations are.
- `crates/ono-kuang-sdk/tests/conformance.rs` — `should_surface_the_arguments_a_contribution_declares`
  and `should_refuse_with_its_own_code_when_a_packages_precondition_is_unmet`, at the protocol
  boundary under the deterministic test host.
- `crates/ono-kuang-protocol/src/error.rs` — `should_expose_all_27_codes_of_spec_31_79_when_enumerated`
  counts `contribution.refused` apart from §31.79's proposed list, as it already does
  `runtime.concurrency_limit`, so the specified list stays checkable as the closed thing it is.

## Alternatives considered

**Add a `system.mutate` capability family with resource-kind scopes.** Rejected under §31.16. The
scope keys that would make it meaningful — namespaces, resource kinds, verbs — are all inside a
protocol the host does not speak, so the broker could record them and never check them. One
advisory scope key exists in the whole model today (`model.infer.data_class`) and it is advisory
because the *broker* forwards the request and can see the class; here the broker would be
forwarding bytes.

**Make the risk declaration mandatory when a mutating capability is declared.** This is the
enforcement the `risk-metadata` registration check already publishes, and it is tempting: it would
make every package fronting a cluster state whether each command reads or writes. Rejected for this
increment only, because a package that installs today and declares `network.connect` without a
risk would stop installing, and the increment's constraint is that no existing package breaks. It
is a good next increment and is recorded as one.

**Bind a contributed command's words against its declaration and refuse the undeclared.** Rejected:
it turns an additive declaration into a breaking one. A package that declares two of its five
options would start refusing the other three, and the declaration would become a thing an author
must complete before it helps rather than a thing that helps as far as it goes.

**Put the new error code in the `runtime.*` family.** Rejected: `runtime.*` is the instance
misbehaving or being stopped — a trap, a timeout, a memory ceiling, a protocol violation. A package
declining on purpose is the opposite of the instance going wrong, and a code whose family says
"something broke" would send every reader to the wrong question.

**Name it `provider.refused` in the E04 family instead.** Rejected: E04 is a *core* provider
failing to answer, and its three codes are all about the provider's ability. The refusal here is
specific to a contribution — the thing that has a manifest, a publisher and a rule of its own — and
belongs with the other codes that name one.

**Deliver `provider.action` now, with the six of §21.1's ten fields that are expressible.** Rejected
for the reason given in §5: a declaration whose fields nothing reads is the defect this ADR exists
to close, and shipping four of them unread would reproduce it at a larger scale.
