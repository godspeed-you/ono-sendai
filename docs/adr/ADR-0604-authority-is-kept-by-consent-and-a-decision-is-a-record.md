# ADR-0604: Authority is kept by consent, and a decision is a record

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.18, §31.19, §31.35, §31.37, §31.70, §31.81;
  `docs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §16, §21, §22, §23, §28,
  §33 Gates K, L, N–P, V, §34.3–§34.5, §34.8; ADR-0265, ADR-0600, ADR-0602
- Decided by: agent (autonomous)

## Context

ADR-0265 keeps `always` grants in `<config>/kuang/policy.yaml` and the audit trail in
`<state>/kuang/audit.jsonl`. K11P §23 asks for the layer above: a record that links a human
decision to the grants it produced, an upgrade that cannot widen authority silently, a removal
that takes its grants with it, and a way to read an old grant back into the new vocabulary
without rewriting it.

## Decision

### 1. Two stores, one authority

Capability grants stay in `policy.yaml`, which remains what the broker enforces. Each stored
decision gains two optional fields, `permission` and `profile`, naming what minted it; a manual
grant has neither. Permission decisions are written beside it to `<config>/kuang/permissions.yaml`
(`kuang-permissions/1`), one record per package and permission id:

```yaml
format: kuang-permissions/1
plugins:
  io.github.godspeed-you.kubernetes:
    cluster-access:
      decision: allow
      duration: always
      profile: recommended
      created_at: …
      updated_at: …
      grants: [<grant ids>]
      package_version: 0.2.0
      publisher: ed25519:…      # or `unsigned`
```

Only `always` decisions are stored; `session` and `once` live in the session, like their grants
(ADR-0265 §2). Both files are rewritten together when either changes, so a decision never points
at a grant that is not there.

### 2. `get permission` and `set permission`

`get permission [<ref>] [--all]` answers `ono.permission/1` records: one per descriptor, with
`state` in `allowed`, `denied`, `ask`, `included`, `custom` or `legacy` and `when` in `always`,
`session`, `once`, `automatic`, `when-needed` or `explicit`. A descriptor whose capabilities are
held by grants that do not match its mapping exactly — wider, narrower, or from `grant capability`
by hand — is `custom` rather than `allowed`, so the manual path never disappears from the human
view (Gate L); a standing grant for a capability no descriptor maps is a `legacy` row named for the
capability (K11P §28.3). `--all` adds the support permissions the compact view hides and the
legacy rows; every record carries the exact capabilities, scopes, enforcement, source and duration
in every view.

`set permission <ref> --profile <name>` applies a profile: every permission it names is decided
`allow`, every explicit one it does not name stays as it is, and the grants are minted or revoked
accordingly. `set permission <ref> <permission> --decision allow|deny|ask [--duration
session|always] [--scope key=value]` decides one permission; `deny` revokes its grants and records
a user deny that the broker honours ahead of any grant; `ask` clears a decision and a remembered
denial. Both run in the evaluator like `grant capability`, follow ADR-0602 §4's unattended rules,
and answer the changed rows. There is no `permissions` verb (K11P §16.4).

### 3. Upgrades keep what was consented to, and nothing more

An upgrade computes `InstallPlanPermissionDelta` between the installed package's decisions and the
new package's descriptors: a permission newly in the selected profile, a permission that gained a
capability, a scope that widened, a risk that rose, a phase that moved toward automatic or
recommended, an enforcement that weakened, or an id whose mapping changed (K11P §21.3). An empty
delta keeps every decision and says `Permissions unchanged.` (Gate N). A non-empty delta shows
the `+` lines, asks `Update and grant the new access? [y/N/details]`, and non-interactively needs
`--confirm` under ADR-0602 §4's rules; declining leaves the installed version untouched (Gate O).
A permission id reused for materially different authority is a delta entry, never a silent
inheritance (K11P §34.5). A `recommended` profile that gained `provider.mutate` is refused by the
parser before any of this runs (ADR-0600 §3).

### 4. Removal takes the decisions with it; reinstall asks again

`remove plugin <ref>` removes the package, its enabled state, its decisions and its grants —
revoked and retained in memory for the trail, gone from both stores — and its catalog pin, unless
`--keep-grants`, which retains both stores' entries for the id. A retained decision applies on a
later install only when the canonical id and the publisher identity match what it recorded; a
mismatch drops it with a notice. A normal reinstall shows the plan and asks (K11P §22.3, §34.8,
Gate P).

### 5. Audit correlation and explainability

`AuditEvent` gains `correlation: Option<String>`. One user action — a profile applied, a JIT
answer, a `set permission` — mints one request id and stamps it on the `permission.*` event and on
every `capability.grant` / `capability.revoke` it produced (K11P §23.2, Gate V). Given any grant,
`source` (`system-policy`, `user-policy`, `session`, `prompt`, `default`), `permission`, `profile`
and `duration` together say whether it came from a permission decision, a profile, a manual grant,
policy, or session-only JIT consent (K11P §23.3).

### 6. Existing grants are projected, never rewritten

A grant written by an earlier release has no `permission` field. On the first read it is kept
exactly as it is and projected: a match against a descriptor's mapping reads as `allowed`, a
partial match as `custom`, no descriptor as `legacy`. A broad `process.exec` grant reads `custom`
and `get permission` recommends narrowing it; nothing narrows it silently (K11P §28.4).

## Consequences

- `ono.capability-grant/1` gains `permission` and `profile` (nullable); `ono.plugin-audit-event/1`
  gains `correlation` (nullable); `ono.permission/1` is new.
- Encoded by `ono-cli/tests/permissions.rs` (get/set, custom and legacy projection, deny ahead of
  grant), `ono-cli/tests/plugin_upgrade.rs` (unchanged, expanded, repurposed, refused key
  lineage), `ono-cli/tests/permissions.rs` (removal and reinstall) and acceptance cases
  `223`, `225`, `226`.

## Alternatives considered

- **One file for grants and decisions.** `policy.yaml`'s shape is ADR-0265's contract and
  hand-editable; a decision record beside it keeps that true.
- **Rewriting a legacy broad grant into the new scope on migration.** K11P §28.4 forbids it;
  migration must not change what a package holds in either direction.
- **Inheriting grants across an upgrade when the capability set is unchanged but scopes widened.**
  A wider scope is more authority (K11P §21.3).
