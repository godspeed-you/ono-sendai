# ADR-0600: A user grants intentions, and KUANG/11 grants capabilities

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.7, §31.16–§31.19, §31.26, §31.62, §31.63; v0.4.1 §15, §17.3;
  `docs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §0.3, §3, §4, §6, §7, §8,
  §9, §18, §19, §29; ADR-0022, ADR-0264, ADR-0265, ADR-0594
- Decided by: agent (autonomous)

## Context

KUANG/11's capability model is precise and the shell exposed it raw: installing the Kubernetes
provider meant typing its reverse-DNS id, a `path:` source, `--confirm`, and then four to six
`grant capability` lines naming `network.connect`, `filesystem.read`, `secret.use`,
`relation.write` and `process.exec` with their scopes. K11P §1.2 names the failure this produces:
capability fatigue is a security problem, because a user trained to approve six identifiers they
do not understand approves the seventh too.

K11P's central rule is one sentence — "A user grants intentions. KUANG/11 grants capabilities."
(§0.3) — and it leaves no product-design question open. What it leaves to the implementation is
where the human layer lives, how a package declares it, what a package without a declaration gets,
and how the host keeps a package's friendly wording from becoming authority. This ADR decides
those.

## Decision

### 1. The permission layer is a projection onto the broker, and lives in `ono-kuang-protocol`

A **permission** is a user-facing statement that resolves to one or more exact capability
requirements (K11P §4.4, §6.1). The types — `PermissionKind`, `PermissionPhase`, `PermissionRisk`,
`PermissionDescriptor`, `AccessProfile`, `PermissionSet` — are in
`ono_kuang_protocol::permission`, beside `Capability`, because the supervisor, the shell and the
test host all read them and none of them may disagree. The broker is unchanged and stays
authoritative: a permission decision is *executed* by minting the capability grants its
`grants:` list names, through the same `Host::grant` and `Policy` every manual grant goes
through. There is no second security engine (K11P §6.1, invariant 5).

### 2. The host classifies every capability, and a package cannot lower it

K11P §7 asks the host to place every family in a consent class. The table is data in
`docs/contracts/kuang/permissions.v1.yaml` and code in `Capability::consent_class()` /
`Capability::minimum_risk()`, and `spec-check` compares the two:

| class | families | minimum risk | phase a descriptor may use |
|---|---|---|---|
| A extension-local | `clock.read`, `state.persist`, `history.write`, `ui.view`, `ui.notify`, `schema.read`; `relation.write` **only when scoped by `relations`** | local | automatic, install |
| B observation | `object.read`, `process.read`, `network.observe`, `service.read`, `container.read`, `remote.read`, `context.read`, `relation.read`, `history.read`, `filesystem.watch`, `network.connect` | observe | install, jit |
| B observation, sensitive | `filesystem.read`, `secret.use` | sensitive-read | install, jit |
| C conditional | `process.exec`, `container.exec` | execute | jit, explicit |
| D explicit mutation | `provider.mutate`, `process.signal`, `service.mutate`, `network.listen`, `remote.mutate`, `plugin.invoke`, `model.infer`, `history.write`-free; unscoped `relation.write` | mutate | explicit |
| E destructive | `filesystem.write` | destructive | explicit, and never unattended |

A descriptor MAY declare a phase later in the row's list (a JIT permission may be made explicit)
and MAY raise the risk; it MUST NOT do the opposite. A descriptor whose phase or risk is lower
than the class permits is `permission.invalid_mapping` and the manifest is refused before any
package byte runs — which is how K11P §6.4's "a malicious package cannot label `filesystem.write`
as harmless" and §34.6's "hidden mutation" are both answered by the parser rather than by the
prompt.

`relation.write` is the one family whose class depends on its scope. ADR-0264 gave the family no
scope key; this ADR adds **`relations`** (`id-list`, broker-enforced): the relation ids the
package may contribute. Scoped to the ids derived from the package's own `contributions.relations`
shapes, the authority is bounded to the package's declared contributions with provenance the host
stamps (K11P §7.1, option 1) and is class A; unscoped, it could name another provider's relation
and is class D. The shell's relation adoption and the `relations.contribute` host call both honour
the scope.

### 3. A manifest declares permissions in a closed `permissions` section, under `kuang-package/2`

```yaml
permissions:
  profiles:
    minimal:     {title: Minimal, permissions: [spatial-relations]}
    recommended: {title: Recommended, permissions: [cluster-access, kubeconfig-read, …]}
    operate:     {title: Observe and change, permissions: [… , cluster-mutation]}
  requests:
    - id: cluster-access
      kind: external-observe
      title: Connect to Kubernetes clusters
      purpose: …                      # optional
      phase: install
      recommended: true
      risk: observe                    # optional; may only raise
      grants:
        - capability: network.connect
          scope: runtime-derived
```

Every field of K11P §6.2 and §8.2 is representable. `grants[].scope` is a record of scope keys the
capability declares, or one of three words: `runtime-derived` (the host derives the concrete value
at use, K11P §19.3), `package-contributions` (only for `relation.write`: the package's own
declared shapes) and `provider-instance` (only for `provider.mutate`). Validation, all
`permission.invalid_mapping` and all before code: every `capability` is declared in the manifest's
own `capabilities` lists; every id is a unique kebab slug; every profile names ids that exist;
`minimal` and `recommended` exist whenever `requests` is non-empty; `recommended` contains no
phase `explicit` request, no class D/E capability and no class C capability without a concrete
program scope (K11P §9.1); a `package-contributions` scope names `relation.write` and a
`provider-instance` scope names `provider.mutate`.

The top-level manifest is closed (`deny_unknown_fields`, ADR-0022 §10), so a `kuang-package/1`
reader refuses a `permissions` section as an unknown field. K11P §8.1 says exactly what to do
then: version the schema explicitly. **`kuang-package/2` is `kuang-package/1` plus the optional
`permissions` section**; this host reads both; a `/1` document carrying `permissions` is refused,
so `/1` keeps meaning one thing. The host API moves to **`kuang-host/11.2`** because the wire
gains behaviour a package can rely on — `capabilities.check` answering `ask`, and a
`process.exec` call that may block on consent (ADR-0603) — and a package that needs it writes
`kuang_api: ">=11.2 <12"`. A package declaring `>=11.1` still loads: the minor is additive.

### 4. A package without descriptors gets generated ones

K11P §8.1: a package that declares no permissions "uses generated fallback permissions derived
from its capability declarations". `PermissionSet::derived(&manifest)` makes one permission per
declared capability — id `<family with '.' as '-'>`, a host-owned title per family
(`docs/contracts/kuang/permissions.v1.yaml` → `families[].title`), the scope the manifest asked
for, kind/phase/risk from the class table, `recommended` for classes A and B — and two profiles:
`minimal` (class A) and `recommended` (classes A and B). A class C capability becomes a JIT
permission; D and E become explicit. The existing example, adapter and test packages therefore
install through the new flow unchanged, with less polished wording, which is the outcome §8.1
asks for.

### 5. Package wording is untrusted, and the host renders its own labels beside it

`title` and `purpose` are sanitised before display exactly as `CapabilityRequest::purpose` is:
control characters and ANSI sequences stripped, length bounded, newlines refused (K11P §8.3,
§34.2). The host always renders the kind and the risk it derived, and the labels `Signature`,
`Publisher trust`, `Runtime` and `Isolation` come from verification, never from the package.

### 6. Every permission is inspectable down to its grants

`get permission <ref>` answers `ono.permission/1` records (`docs/contracts/schemas/permission.v1.yaml`)
carrying the permission and, in every record, the exact capabilities, scopes, enforcement,
decision source and duration underneath it; the default view shows the human columns and `--all`
adds the support rows and the legacy rows (ADR-0604). A grant the layer minted carries the
permission id and the profile that selected it on the `ono.capability-grant/1` record, so `get
capability` and `get permission` describe the same fact from two sides (K11P §16.3, invariant 6).

## Consequences

- `Capability::ALL` is unchanged; `relation.write` gains one scope key and the contracts follow.
- `docs/contracts/kuang/permissions.v1.yaml` is a new registry; `manifest.v1.yaml` gains the
  `permissions` section and states the `/2` format; `spec-check` holds the class table, the
  section and the schema against the runtime.
- The Kubernetes provider is the reference: its manifest becomes `kuang-package/2` with the seven
  permissions of K11P §26.1 (ADR in that repository).
- Encoded by `ono-kuang-protocol/tests/permissions.rs` (classification, fallback derivation, every
  `invalid_mapping` rule, sanitisation), `ono-cli/tests/permissions.rs` and the acceptance cases
  `220`–`226`.

## Alternatives considered

- **Keeping the permission layer in `ono-cli` only.** The supervisor must answer
  `capabilities.check` with `ask` and must know a JIT permission when it meets one, and the test
  host must classify a package without a shell; three copies of the table was the second-copy
  failure §52.2 forbids.
- **Allowing `permissions` under `kuang-package/1`.** Fails closed on an old host with "unknown
  field" rather than "this host reads `/1`", and K11P §8.1 says MUST version.
- **Letting a package set its own risk freely.** The friendly-theme attack of K11P §34.1.
- **Making `relation.write` automatic without a scope.** K11P §7.1 forbids automatically
  granting an authority that could impersonate another provider.
