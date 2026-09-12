# ADR-0857: A `--scope` replaces the values it names, and a custom permission shows what it holds

- Status: accepted
- Date: 2026-09-12
- Spec refs: v0.6.1 §5, §30, §32; v0.2 §31.16, §31.18;
  `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §16.2, §16.3,
  §19.1, §19.2, §23; ADR-0264, ADR-0604
- Decided by: agent (autonomous)

## Context

Issue #128. `set permission <ref> <permission> --decision allow --scope key=value[,value]`
(ADR-0604 §2) merges the named keys into the declared scope key by key: a key it names takes
exactly the values written, a key it does not name keeps its declared values. For the install
permission `kubeconfig-read`, `--scope paths=~/elsewhere.yaml` therefore stored a grant for that
one path. Two things were wrong around that, and neither was the storage:

1. nothing on the `set permission` line said the declared paths were gone, so the first sign was a
   refusal from the broker on the next command that read the kubeconfig;
2. `get permission` rendered the permission `custom` and in the same row showed the declared
   scope, both in `scope` and in `grants[].scope`, so the one place a person checks named paths
   the broker no longer enforced. K11P §19.2 wants the exact structured scope beside the friendly
   rendering, and ADR-0604 §2 says every record carries "the exact … scopes".

The issue offers two remedies for the first defect: name the dropped paths, or add an additive form
such as `--scope +paths=…`. v0.6.1 §5 forbids expanding permission semantics unless that restores
intended behaviour. Neither K11P §16.2 nor ADR-0604 nor ADR-0264 (`--scope`'s spelling for
`grant capability`) mentions an additive form, and `grant capability --scope` replaces too.

## Decision

1. **Replacement stays.** A `--scope key=value[,value]` sets the complete list of values for each
   key it names; a key it does not name keeps its declared values. There is no additive form. To
   widen a declared scope, the user writes the whole list:
   `--scope "paths=~/.kube/config,~/.kube/*.yaml,~/elsewhere.yaml"`.
2. **A narrowing is said where it happens.** Before the replacement revokes the permission's
   standing grant, `set permission` collects, per named key, every value that the declared scope
   or that standing grant listed and the new value does not, in first-seen order, compared as
   exact strings. A glob is not checked against the paths it would match. If anything is left
   out, a notice on standard error names the permission and those values and shows how to keep
   them. The decision, the grants and the audit events are the ones ADR-0604 already writes.
3. **A `custom` permission shows what it holds.** Whenever a permission's state is `custom`,
   meaning its capabilities are held by grants that do not match its mapping exactly, both the
   human `scope` column and each `grants[].scope` render the scope of the standing grant, which
   is what `policy.yaml` holds and the broker enforces. Every other state keeps rendering the
   declared scope, which in those states is the same as what is held, or is what an `allow` would
   grant.
4. `help set permission` documents `--scope` for install permissions as well as for just-in-time
   ones (`docs/contracts/commands/kuang.yaml`).

## Consequences

- No change to `policy.yaml`, `permissions.yaml`, the grant model, audit events or the broker:
  v0.6.0 configuration and scripts behave identically (v0.6.1 §32). The visible changes are the
  notice and the scope rendered for `custom` rows.
- `ono.permission/1` keeps its fields; the `scope` and `grants` docs say which scope a `custom`
  row carries.
- Encoded by `ono-cli/tests/permissions.rs`:
  `should_enforce_and_show_the_stored_scope_when_a_scope_replaces_the_declared_paths`, which
  checks the policy store, broker reads and refusals, the notice, and the answered and shown
  rows; `should_keep_every_path_the_scope_names_and_drop_nothing_silently_or_otherwise`; and
  `should_describe_scope_replacement_for_an_install_permission_in_the_help`.
- A later additive spelling remains possible as a feature with its own ADR; this decision does not
  reserve `+key=`.

## Alternatives considered

- **`--scope +paths=…` appending to the declared values.** It expands the command's semantics,
  which v0.6.1 §5 rules out for a stabilisation release, and it has no basis in K11P §16.2 or
  ADR-0604. It would also leave defect 2 in place.
- **Merge by default (the new values are added to the declared ones).** That silently changes what
  existing `--scope` lines grant, in the widening direction: a v0.6.0 script that narrowed a
  scope on purpose would start granting more. This is a compatibility break and a security
  regression (v0.6.1 §32).
- **Refuse a narrowing unless `--confirm` is given.** Narrowing reduces authority; gating it
  behind the flag that guards widening (ADR-0602 §4) would make the safe direction the harder one,
  and scripts that narrow today would start failing.
- **Fix only the rendering.** v0.6.1 §5 asks for the root cause. The stored state was already
  right, and what was wrong was that the transition was invisible and the row misreported it, so
  both are fixed.
