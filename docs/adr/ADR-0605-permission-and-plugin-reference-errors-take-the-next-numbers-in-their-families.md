# ADR-0605: Permission and plugin-reference errors take the next numbers in their families

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §43, §31.79; `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md`
  (K11P) §24; ADR-0006, ADR-0022, ADR-0108
- Decided by: agent (autonomous)

## Context

K11P §24 names ten structured error cases and leaves their numbers to "the project's existing
registry conventions": the taxonomy is closed and additive, a code is never renumbered, and a new
code takes the next free number in its family (`docs/contracts/kuang/errors.v1.yaml`). The K11
families are all taken — 0 package, 1 load, 2 runtime, 3 capability, 4 state, 5 view, 6 model, 7
remote, 8 plugin, 9 contribution — and the shell's E families are numbered by the kind they
answer with.

## Decision

The six permission cases belong to the capability family, because a permission is the human
projection of a capability decision (ADR-0600), and take K11304–K11309:

| code | name | kind |
|---|---|---|
| Ono-Sendai-K11304 | permission.required | permission |
| Ono-Sendai-K11305 | permission.denied | permission |
| Ono-Sendai-K11306 | permission.invalid_profile | resolution |
| Ono-Sendai-K11307 | permission.invalid_mapping | parse |
| Ono-Sendai-K11308 | permission.scope_unavailable | safety |
| Ono-Sendai-K11309 | permission.escalation_requires_confirmation | safety |

The four plugin-reference cases are about resolving a name against installed packages and
catalogs, which is the shell's job and not the runtime's, so they open the shell family E16
"plugin references":

| code | name | kind |
|---|---|---|
| Ono-Sendai-E1601 | plugin.not_found | resolution |
| Ono-Sendai-E1602 | plugin.reference_ambiguous | resolution |
| Ono-Sendai-E1603 | plugin.catalog_unavailable | resolution |
| Ono-Sendai-E1604 | plugin.release_not_compatible | conflict |

`permission.invalid_mapping` is raised by the manifest parser in place of `package.invalid` for
every rule of the `permissions` section (ADR-0600 §3), and is treated by `verify`, `install` and
`load` exactly as `package.invalid` is: the manifest is invalid and nothing runs. Existing codes
are reused where K11P §24.3 says so: a tampered package is `package.integrity_failed`, a revoked
key `publisher.untrusted`, a refused unattended install `safety.confirmation_required`.

A user-facing permission error leads with the permission and the reason and puts the capability,
the scope and the remedy in metadata (K11P §24.4): `permission.required` carries `plugin`,
`permission`, `capability`, `requested_scope`, `reason` and `remedy`; `permission.denied` carries
`plugin`, `permission`, `capability`, `operation` and `remedy`.

## Consequences

- Four registries move together: `docs/contracts/errors.yaml`, `docs/contracts/kuang/errors.v1.yaml`,
  `ono_core::ErrorCode` and `ono_kuang_protocol::KuangErrorCode`; `spec-check` compares them.
- `docs/contracts/hardening/refusals.yaml` is untouched: neither prefix is a hardening refusal
  of v0.4.1 §54, and the census's `covers` list is not widened by this decision.

## Alternatives considered

- **Putting `plugin.not_found` under K118.** That family is the native-process confinement block
  of v0.4.1 §16.3 and every code in it is a hardening refusal the census must cover; a name
  that did not resolve is not one.
- **Reusing `resolve.target_not_found` for a plugin name.** K11P §24.1 asks for a distinct case,
  and a script that installs plugins needs to tell the two apart.
