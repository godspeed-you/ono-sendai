# ADR-0596: A contributed kind of place declares its semantic roles, and a search reaches it by them

- Status: accepted
- Date: 2026-09-07
- Spec refs: v0.4 §3.1, §6.8, §9.3, §32.1, §33.3, §36.1; §31.23, §10.5;
  `docs/contracts/kuang/contributions.v1.yaml` (`target.roles`),
  `docs/contracts/schemas/spatial-place.v1.yaml` (`roles`), `docs/contracts/commands/spatial.yaml`
  (`find place --role`), `docs/contracts/spatial/spatial.yaml` (`contributed_types.roles`);
  `docs/architecture/external-system-provider.md` §15.5, §25, §42.3; ADR-0584, ADR-0587
- Decided by: agent (autonomous)

## Context

The provider contract lets a provider "register native resource types under semantic roles" —
`k8s.apps.Deployment -> workload` — so that a cross-provider question can be asked without a
provider-specific word (§25.1), and asks that place search reach external resources by "semantic
role" among other keys (§15.5). It keeps the roles small (§25.2), keeps the native value
authoritative beside them (§25.1), and reserves the exact registry for later (§42.3).

Nothing in the host could carry a role. The Kubernetes provider's `ROLE_OVERLAY` reached a user
only as `target_roles` on a relationship edge, so `find place --role workload` had nothing to
match, and a Deployment place said what it was and never what it was *for*.

## Decision

**A target contribution declares `roles`; the kind of place the host registers for it carries
them; `find place --role <role>` asks exactly the targets whose kind carries the role; and every
place record says which roles its kind has.**

- `roles` is a list of kebab-case words on the target declaration, on disk and in the handshake.
  The spelling is closed (`is_role_word`), the vocabulary is open: the registry the provider
  contract reserves is not invented here, and the words its §25.1 and the Kubernetes
  specification's §36.2 use — `workload`, `compute-node`, `network-endpoint`, `configuration`,
  `secret`, `identity`, `storage`, `policy` — are the ones a package is expected to choose from.
- The role belongs to the *kind of place*, registered with the contributed type beside its schema,
  its identity and its origin. A declared type of `spatial.yaml` carries no roles yet.
- `find place --role <role>` narrows the target plan to the targets whose kind carries the role,
  and asks them even when enumerating one is `expensive` — a role names the targets as precisely
  as `--type` does, and ADR-0584's reason for not asking an expensive target was that nothing had
  asked for it. A role nobody declares is refused with the roles that exist, exactly as a type
  nobody serves is (`spatial.unsupported`); a search that answered empty for a word about nothing
  would be the failure ADR-0210 closed for fields.
- `ono.spatial-place/1` gains `roles`, nullable: the roles of the kind, or null where the kind
  declares none — which is not "declared to have none" (spec §10.5). `object_type`,
  `canonical_ref`, `identity` and `spatial_type` are untouched: a role is additional semantics,
  never a replacement for what the object is (§25.1, §36.1 of the Kubernetes specification).

## Consequences

`find place --role workload` answers, against the Kubernetes provider, with its Deployments,
StatefulSets, DaemonSets, Jobs and CronJobs, each carrying `roles: [workload]` beside the native
`io.github.godspeed-you.kubernetes.deployment/1` and the `uid` that identifies it. `look` on any
of them shows the roles. A dynamically adapted custom resource gets roles the same way once its
target declares them.

`crates/ono-cli/tests/spatial_contributed_targets.rs`, over the real `ono` binary:

- `should_find_places_by_the_semantic_role_their_kind_declares` — the three places of the kind
  that declares `workload`, each with the role on its record and its native type preserved;
- `should_refuse_a_role_no_loaded_package_declares` — `--role storage` is refused naming
  `workload`;
- `should_exclude_a_kind_that_carries_no_role_from_a_role_search` — a zone never answers a
  `workload` search.

The `roles` word on a target is validated at load (`package.invalid` for a word that is not a
role word), so a package cannot register `Workload` and `workload` as two roles.

## Alternatives considered

**Roles as a field on every object record.** Rejected: it would widen every contributed schema
with a field the provider contract keeps on the *mapping* rather than on the value (§25.1), and
the place record is where a cross-provider search already answers.

**A closed role registry in `spatial.yaml`.** Rejected: §42.3 of the provider contract reserves
"the exact semantic role registry" for a later specification, and closing the list here would be
deciding it casually. The spelling is closed so the words are one vocabulary; the list is open so
the reservation is honoured.

**Match a role by searching the roles a package emits at query time.** Rejected: a role is a
property of a kind the host must know when it *plans* the search, or an expensive target would
never be asked and the search would answer nothing.
