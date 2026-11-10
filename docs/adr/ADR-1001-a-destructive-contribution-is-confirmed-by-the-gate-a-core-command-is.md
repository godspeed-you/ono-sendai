# ADR-1001: A destructive contribution is confirmed by the gate a core command is

- Status: accepted
- Date: 2026-11-10
- Spec refs: §17.4, §18.4, §31.22, §31.66, §31.68; `docs/architecture/external-system-provider.md`
  §21.5; K11P §15.3; `docs/contracts/kuang/contributions.v1.yaml` → `command.fields.risk`;
  ADR-0088 §3, ADR-0587, ADR-0595, ADR-1000; issue #156
- Decided by: agent (autonomous)

## Context

ADR-0587 made a contribution's declared risk visible; it did not act on it. Every contributed entry
was registered `confirmation: bulk`, and the contributed path in the evaluator handed the words to
`crate::plugins::invoke_contributed` without the binding and confirmation a core command goes
through — so `CommandContract::confirmation` was decorative for a contribution whatever it held,
and a contribution with `risk: destructive` ran without a word (#156). §21.5 of the provider
architecture says confirmation belongs to host safety policy and that a provider returns
structured risk so the host can apply consistent rules; K11P §15.3 says permission to mutate does
not remove command-level `risk: destructive` handling.

#156 asks that a destructive contribution "refuses in a non-interactive context
(`safety.confirmation_required`) and prompts at a terminal". The core gate it is to be routed
through does not prompt. A core `confirmation: always` command (`protect`, `stop socket`,
`remove recovery`, …) needs `--confirm` on every run, at a terminal as in a script; without it, it
acts on nothing and names the flag (spec §17.4, ADR-0088 §3, `ProviderMutation::run`). The prompts
in the shell belong to other gates — v0.6 §40's plan gates and K11P's permission questions — which
answer different questions with their own flags.

A contribution has three doors: the bare registry spelling (`remove echo-record`), the qualified
spelling of a loaded package (`echo:wipe`, §31.66), and a background job (`… &`, §18.4), whose
evaluator reaches the first door again.

## Decision

1. **The policy is the registry entry's.** `ContributedCommand::into_contract` maps a declared
   `destructive` to `Confirmation::Always`, and adds a `confirm` bool option where the package
   declared none, so `help` and completion offer the flag as they do for a core command. Every
   other risk stays `Confirmation::Bulk`; a contribution names no set of targets for a bulk
   threshold to count, so `mutate` asks for nothing — as a core `mutate` command under the
   threshold asks for nothing.
2. **The refusal is the core one.** `CommandContract::unconfirmed(confirmed)` is the single place
   the refusal is made: `safety.confirmation_required`, "`<spelling>` is destructive and was not
   confirmed", help "nothing was changed. Write `<spelling> --confirm` to act (spec §17.4)". The
   core mutation and the contributed path both call it.
3. **The gate stands at the package's door, not at the evaluator's.** It runs in
   `crate::plugins`, where every contributed invocation passes: `invoke_contributed` asks before the
   package is even loaded, and `invoke` (the qualified spelling) asks before `command.invoke` is
   sent. A background job runs the line through the same functions, so it is gated too. Nothing of
   the package runs for a refused invocation; the tests prove it through the package's own audit
   entry, which is absent.
4. **The declaration on disk decides; the hello decides where there is none.** For a command the
   package declares only in its hello, the hello's `risk` is read into a registry entry for this
   decision alone, so a package cannot escape the gate by not declaring on disk.
5. **No prompt at a terminal.** The core gate wins over the issue's wording: a contribution is
   confirmed identically to a core destructive command, so at a terminal it refuses and names
   `--confirm` exactly as `protect` does. Adding a prompt for contributions alone would make a
   plugin's destructive command behave differently from Ono's own, which is the inconsistency
   §21.5 exists to prevent.
6. `--confirm` reaches the package among its arguments, as a core mutation forwards its written
   options to the provider.

## Consequences

- A destructive contribution needs `--confirm` on every run, including a run the package itself
  would only dry-run. The Kubernetes provider's `remove k8s-resource` (`risk: destructive`, default
  `dry_run: true`) now needs `--confirm` even for its server-side dry run; `set k8s-resource`
  (`risk: mutate`) is unchanged. Release-note and MIGRATION material.
- The confirmation decision for a hello-only command is the hello's own word; a package that
  under-declares in its hello is held to ADR-1000's floor only on disk. Moving that floor into the
  supervisor's hello validation is the follow-up ADR-1000 names.
- Tests: `crates/ono-command/tests/contributed_risk.rs` (the entry's policy and option, the refusal
  identical to `protect`'s, `read`/`observe`/`mutate` unconfirmed);
  `crates/ono-cli/tests/plugins.rs` — `should_refuse_an_unconfirmed_destructive_contribution_without_reaching_the_package`,
  `should_run_a_destructive_contribution_once_it_is_confirmed`,
  `should_refuse_an_unconfirmed_destructive_contribution_by_its_qualified_name_too`,
  `should_refuse_an_unconfirmed_destructive_command_a_package_declares_only_in_its_hello`,
  `should_refuse_an_unconfirmed_destructive_contribution_run_as_a_background_job`,
  `should_refuse_an_unconfirmed_destructive_contribution_at_a_terminal_as_a_core_command_does`,
  `should_leave_a_reading_contribution_unconfirmed_beside_a_destructive_one`; acceptance case 161.
- The example package gains a `--destructive` mode whose hello declares `remove echo-record`
  (`wipe`) destructive, kept apart from the default so every existing assertion about what the
  package contributes stays true.

## Alternatives considered

- **Prompt at a terminal, refuse in a script** (the issue's wording). Rejected for the reason in
  Decision 5: the brief and §21.5 ask for the core gate, and the core gate for a destructive
  command does not prompt. If Ono ever prompts for `confirmation: always`, it should do so for
  core and contributed commands together, in `CommandContract::unconfirmed`'s callers.
- **Gate in `eval/pipeline.rs` at the contributed-stage dispatch.** Covers the bare spelling only;
  the qualified spelling and any future caller of `crate::plugins::invoke` (a background job's
  instance, #304) would bypass it.
- **Bind a contribution's words with the core binder.** Would give `--confirm` its typed reading
  for free, but closes the argument set ADR-0587 deliberately left open; a larger change than the
  confirmation needs.
