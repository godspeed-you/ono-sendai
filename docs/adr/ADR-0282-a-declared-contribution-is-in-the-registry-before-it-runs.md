# ADR-0282: A declared contribution is in the registry before it runs

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.22, §31.64, §31.65, §31.66, §31.68, `docs/contracts/kuang/contributions.v1.yaml`
- Decided by: agent (autonomous, C4-kuang)

## Context

Spec §31.68 draws the whole path in one line:

```text
installed manifest -> registry placeholders -> first invocation -> runtime load
```

and adds: "`get command trace packet` can therefore work before `packet-eye` is loaded. Invoking
the command triggers policy negotiation and load."

None of that existed. A contributed command was reachable only as `<package>:<command>`, only
after `load plugin`, and was invisible to `get command`, `find command`, `help`, completion and
`explain` before *and* after the load, because it never entered `CommandRegistry` at all
(B-kuang-3). The only way the shell could learn what a package contributed was
`kuang_host::discover`, which starts the package, does the handshake and shuts it down — the
opposite of a placeholder.

Three things had to be decided.

## Decision

### 1. The declaration on disk is the wire contribution, in YAML

`contributions.commands` in the manifest names documents inside the package. Each is a
`commands:` list of exactly the `CommandContribution` shape that crosses the handshake
(`ono_kuang_protocol::CommandDocument`), and `docs/contracts/kuang/contributions.v1.yaml` now records
that under `command.declaration`.

One shape for both boundaries is the point: the placeholder and the entry the loaded instance
registers are built from the same fields, so they cannot disagree about what the package
contributes. `contributions.v1.yaml` says the declaration uses "the *same* metadata schema core
commands use"; the fields it then lists are the contribution's fields, not
`docs/contracts/commands/*.yaml`'s (`stability`, `phase`, `privilege` and `provider_capability` are
the core's own vocabulary, and `capabilities` is the package's). The contribution's field list is
what a package writes.

### 2. What a placeholder claims, and what it does not

`ContributedCommand::into_contract` fills the registry fields a contribution does not carry, and
each value is a statement the shell can defend:

- **`stability: experimental`** — a contributed command is not named by a normative section of
  the specification and is therefore not a compatibility promise of Ono's (spec §36.3). Its
  package's version is where its own promise lives.
- **`privilege: conditional`** — the shell cannot know whether the code inside a package needs
  privilege. `conditional` is spec §17's word for exactly that, and claiming `none` would be a
  claim nobody checked.
- **`phase: I`** — the phase that delivers a contributed command is the extension runtime.
- **`streaming`** is read from the declared output type rather than declared separately, so the
  two cannot contradict each other.
- **`selectors` and `options` stay empty.** The wire contribution has no field for them, so the
  registry invents none. A contributed command receives the words the user typed, through the
  same `--name value` path `<package>:<command>` already used.
- **`provider_capability` stays `None`** and the KUANG/11 capabilities go in
  `CommandContract::required_capabilities`. They are different registers: one names an entry of
  `docs/contracts/capabilities.yaml`, the other what the broker will check at invocation.

### 3. Registration refuses rather than shadows

`CommandRegistry::extended` applies the two rules `contributions.v1.yaml` and §31.65 state:

- a contribution whose spelling a core command already holds is **refused** (`no-core-shadow`);
- when two packages claim one spelling, **neither takes it**; both entries stay in the registry
  under their own ids, so `get command`, `help` and `<package>:<command>` still find them, and
  what is refused is only the bare spelling. Install order is not a resolution policy.

Every refusal is returned and reported through `get plugin`'s failure stream. A declaration the
shell would not register must not become a command that is quietly missing from `get command`.

### 4. Invocation loads, and only a declaration can trigger it

Invoking a placeholder — `get echo-item`, or the qualified `echo:emit` of §31.66 — loads the
package with the same negotiation `load plugin` performs, then invokes. The lazy load is silent:
the operator asked for the command, and the command's answer is the only thing that belongs on
stdout.

A package that declares nothing keeps the behaviour it has: `<package>:<command>` before a load
is a structured refusal, because with no declaration the shell would have to start the package
just to discover whether the name exists — which is the cost lazy loading exists to avoid.

### 5. The placeholders are built once per process

`crate::plugin_registry::registry()` builds the extended registry on first use, from
`ONO_PLUGIN_PATH` (or `~/.config/ono/plugins`) in the process environment, and `native::registry`
returns it. The consequence is explicit: **a package installed during a session reaches the
registry in the next one.** That is the same boundary spec §31.8 already draws between installing
a package and having it, and it keeps the per-keystroke cost of completion at zero — rereading
every manifest on every prompt is what spec §34's budget forbids.

## Consequences

- `get command`, `find command`, `help`, completion and `explain` answer for a contributed
  command with nothing loaded, and name the package (ADR-0281's origin).
- The `discover`-by-handshake path stays for what it is good at — telling the operator what a
  *running* instance actually registered — and is no longer the only way to know.
- A package must now declare its commands to get any of this. The example package's other eight
  commands are undeclared and stay `<package>:<command>`-after-load, which is the honest result
  of the rule rather than an omission.
- Encoded by `ono-cli/tests/plugins.rs::should_answer_get_command_for_a_contributed_command_before_its_package_is_loaded`,
  `::should_show_a_contributed_commands_package_in_its_help_page`,
  `::should_name_the_contributing_package_when_a_contributed_stage_is_explained`,
  `::should_load_the_package_when_a_declared_contribution_is_first_invoked`,
  `::should_refuse_a_contribution_that_would_shadow_a_core_command`,
  `ono-cli/tests/completion.rs::should_complete_a_contributed_target_before_its_package_is_loaded`
  and acceptance case `126-kuang-lazy-contributions`.

## Alternatives considered

- **Discover by running the package once at startup.** Rejected: it is what §31.68 exists to
  avoid, it makes the prompt wait for every installed package, and it means untrusted code runs
  before the operator asked for anything.
- **A session-scoped registry rebuilt per pipeline.** Rejected for now: `&'static
  CommandRegistry` is threaded through fourteen call sites, several with no session in scope, and
  the gain is only that `install plugin` takes effect in the same session. Recorded as the known
  limit above rather than paid for with a refactor nothing else needs.
- **Letting a package take a core spelling and resolving by precedence.** Rejected by
  `contributions.v1.yaml`'s `no-core-shadow` check: Ono's own vocabulary cannot be replaced from
  a package directory.
