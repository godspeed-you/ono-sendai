# ADR-0602: Installing a plugin is one transaction that ends ready

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §17.4, §31.8, §31.9, §31.36, §31.68; v0.4.1 §15.2, §17.3;
  `docs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §5, §9, §12, §13, §15,
  §17, §18, §20, §27, §33 Gates B–F, M, T, U; ADR-0109, ADR-0282, ADR-0312, ADR-0600, ADR-0601
- Decided by: agent (autonomous)

## Context

ADR-0109 made `install plugin` a confirmed plan followed by a copy, and left enabling, loading
and every grant to separate commands. K11P §12 keeps every one of those internal steps and makes
them one user transaction: resolve, verify, plan, consent, persist, place, enable, register,
report `ready`. The internal lifecycle of ADR-0022 stays exactly as it is — `installed`, `enabled`,
`loaded`, `active`, `degraded`, `quarantined` — and `ready` is a projection over it (K11P §12.2,
§17.2).

## Decision

### 1. The transaction, in order, and where it can still stop

```text
resolve → verify (blocking answers refuse, no prompt) → compatibility → permission plan
→ consent (interactive prompt, or --confirm/--access under the rules of §4)
→ stage the package under <plugin home>/.staging/<id>-<nonce>/
→ rename into place (an upgrade moves the old directory aside first)
→ write management state (enabled, source, integrity, catalog, publisher key)
→ persist permission decisions and the grants they mint
→ refresh the registry so the contributions are placeholders now
→ report ready
```

Everything before the rename writes nothing outside the staging directory. A failure after it
undoes what was done — the new directory is removed, the old one restored, the management state
and any decision or grant written by this transaction removed — so a failed install leaves no
package, no enabled state, no persisted grant and no runtime (K11P §12.4, Gate U). The audit
trail records `plugin.install` with `denied`/`failed` for the attempt. The staging directory sits
on the same filesystem as the plugin home so the rename is atomic.

### 2. `ready` is derived, and lazy stays lazy

A package is `ready` when it is installed, enabled, its contributions are registered, every
install-time permission of its selected profile is decided `allow`, and it is not quarantined.
`ono.plugin/1` gains `readiness`: `ready`, `needs-permission`, `blocked` (quarantined or disabled)
or `running` (loaded or active). Installation never spawns a `startup: lazy` runtime; the first
invocation of a contribution loads it through the path ADR-0282 built (K11P §12.3, Gate C). The
command registry, built once per process from disk, is rebuilt after an install, an upgrade or a
removal, so `install plugin kubernetes; get k8s-pod` works in one session.

### 3. The compact view, and what `details` adds

The interactive prompt is K11P §13.2, on stderr so a pipeline's stdout stays data:

```text
Kubernetes 0.2.0
Publisher: io.github.godspeed-you
Signature: valid
Publisher trust: project-trusted
Runtime: native process — brokered capabilities and process confinement, not filesystem or network isolation

Recommended access:
  - Connect to Kubernetes clusters
  …
Asked only when needed:
  - Run an external login helper
Not granted:
  - Change Kubernetes resources

Install with recommended access? [Y/n/details]
```

`details` prints the full plan — canonical id, source and catalog, version and digest, signature
state, publisher key and trust source, runtime tier and the isolation statement, and for every
permission its kind, risk, phase, exact capabilities, exact or derived scopes, enforcement and the
duration the decision will carry — and asks again. The plan is one `Value`, the same one
`--confirm`'s refusal carries and `explain install plugin` shows, so the prompt is presentation
over the structured plan (K11P §29.2). The native-tier statement is verbatim v0.4.1 §15.2's and
never says "sandboxed" (K11P §27.1, Gate M).

### 4. Unattended rules

`install plugin <ref> [--access <profile>] [--confirm]`, and the same rules for `set permission`:

| what | interactive | non-interactive |
|---|---|---|
| `recommended` (the default) | `[Y/n/details]` | `--confirm` accepts it |
| `minimal` or a package profile with no explicit permission | `[Y/n/details]` | `--confirm` accepts it |
| a profile that adds a class D explicit permission (`operate`) | its own summary, `[y/N/details]` | `--access <profile> --confirm` accepts it: naming the profile is the deliberate choice K11P §7.4 asks for |
| a class E destructive permission | `[y/N/details]` with the risk named | refused with `permission.escalation_requires_confirmation`; no flag enables it (K11P §7.5) |
| a signed native package whose key no trust store enrols, from a catalog | the K11P §13.3 warning and `[y/N]` | refused with `safety.confirmation_required` naming the trust store as the policy that would allow it |
| an unsigned native package, from a local path or a catalog naming a local artifact | the warning and `[Y/n]` | `--confirm` accepts it: local development semantics (K11P §13.4) — there is no publisher identity to enrol — with the warning on stderr |
| a blocking verification answer | refused | refused — there is no "install anyway" (Gate T) |

Without `--confirm` a script gets `safety.confirmation_required` carrying the plan, as before.
`--yes` is not added: `--confirm` is the flag the contract already declares and K11P §20.2 keeps.

### 5. Mutation is opt-in, everywhere

The recommended profile of any package cannot carry `provider.mutate` or any class D/E capability
— the parser refuses such a manifest (ADR-0600 §3), so no install path can grant mutation by
default (Gate F). A mutation attempt before elevation fails at the host's capability check with
`permission.denied` (K11305) that leads with the human permission and names the elevation
(`set permission kubernetes --profile operate` or `… cluster-mutation --decision allow`); the
cluster sees nothing. Enabling it interactively shows K11P §15.2's summary and asks `[y/N/details]`.
Command-level `risk`, dry-run defaults and provider preconditions stay in force on top (K11P §15.3).

## Consequences

- `install plugin` resolves every `PluginRef` of ADR-0601; `path:` and `--confirm` keep working.
- `set plugin <ref> --enabled false` overrides `ready`; an invocation of a disabled package's
  contribution refuses and says so, and no lazy load overrides the disablement (K11P §17.4).
- Encoded by `ono-cli/tests/plugin_install.rs` (transaction, rollback under a read-only home and a
  read-only config directory, readiness, one-session install-and-use, unattended rules) and
  acceptance cases `220`, `221`, `224`.

## Alternatives considered

- **Keeping `install` as copy-only and adding an `install --load` flag.** Leaves the ceremony in
  place under another name; K11P §1.1 rejects exactly that as the primary experience.
- **Spawning the runtime at install to prove readiness.** K11P §12.2 forbids it for `lazy`.
- **Prompting on stdout.** `install plugin … | to json` would carry the prompt text into the
  data (K11P §29.2).
