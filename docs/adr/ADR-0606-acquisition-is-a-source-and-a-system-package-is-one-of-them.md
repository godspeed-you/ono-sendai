# ADR-0606: Acquisition is a source, and a system package is one of them

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.8, §31.9, §31.36; v0.4.1 §15.2, §17.3;
  `docs/specs/kuang11/kuang11-plugin-package-acquisition-system-distribution.md` (K11A) §0.2, §4,
  §5, §8–§11, §14–§16, §18–§21, §23–§25, §27; K11P §10, §11, §12, §21, §22; ADR-0601, ADR-0602,
  ADR-0604
- Decided by: agent (autonomous)

## Context

ADR-0601 decided how a name becomes a package and left one thing deliberately closed: this build
fetched nothing, and a catalog release was "available" when its unpacked directory sat in the
package cache or a local package source. K11A opens exactly that concern — how the payload
physically reaches a host — and adds a third way it can arrive: an operating-system package
(`apt install ono-plugin-kubernetes`) placing a payload under a system source root. Its central
rule is one sentence: *package acquisition may be delegated; trust, permission and activation
decisions may not* (§0.2). Everything ADR-0600 through ADR-0605 decided stays as it is; this ADR
decides where the sources sit in the resolution ADR-0601 built, and ADR-0607 decides the wire.

## Decision

### 1. Three source kinds, one contract, and nothing decided by arriving

`ono_kuang_protocol::SourceKind` is `catalog-network`, `local-path` or `system-package` (K11A
§4.2). A payload from any of them goes through the same verification, trust judgement, permission
plan, consent and transaction as before; acquisition is the step *before* ADR-0602's transaction
and adds no branch to it. `ono.plugin-package/1` carries `source_kind` and, for a system payload,
`system_package`; `find plugin` renders the kind as a column.

### 2. A system package is a versioned source, never Ono's installed store

A distribution package places its payload under `<root>/<id>/<version>/` (K11A §8.2), with an
optional sidecar `<root>/<id>/<version>.origin.yaml` (`kuang-system-origin/1`: the outer package's
name and its manager) beside it. The roots are `ONO_PLUGIN_SYSTEM_SOURCES` or, by default,
`/usr/lib/ono-sendai/plugin-sources`; the ordinary user configures nothing. A payload being there
means one thing — *available as a candidate* — and neither installed, enabled, trusted nor granted
(§9). `install plugin <name>` copies the verified payload into the plugin home through the ordinary
transaction (§8.3): what runs is Ono's copy, pinned to the digest it verified, so `apt upgrade`
replacing the source files changes a candidate and never the active package (§14.1).

**Root policy (§16.1).** A root is searched only when it is a directory not writable by its group
or by everyone, and owned by root or by the user running the shell — the second being what a
scratch root on a developer machine is, and no wider than the environment variable that named
it. A root that fails is set aside with `plugin.source_root_rejected` (E1607) as a warning on
`find plugin` and `install plugin`, and nothing under it is offered.

### 3. Resolution: candidates, preference, lineage, conflict

`Host::locate_for_install(reference, preference)` gathers, for the release a catalog entry names,
every place that has it — system payloads of that id and version, a local copy (cache, package
sources, a `path:` artifact), the network artifact — and chooses in the order **system, local,
network** under the default preference (K11A §10.3). A system package may carry a release the
catalog does not list; under a preference that admits it, a newer system release is the candidate
(§14.2). A payload no catalog lists at all resolves by id or by name from the system roots alone,
which is what an offline host with a distribution package has (§20).

- `--source system|catalog|local` admits one kind (§10.4). A word that admits nothing that has the
  release is `plugin.catalog_unavailable` naming what `--source` excluded.
- **Lineage.** An installed package remembers its origin (§4 below), and an upgrade without
  `--source` admits only that kind (`Preference::Lineage`): a system payload that appears after a
  catalog install does not become its upgrade path by appearing, and vice versa (§18, §27).
  `--source` is the explicit, inspectable change of lineage.
- **Conflict.** The same id and version with two different content digests — among the admitted
  candidates, and against the catalog's own vouched digest under the default preference — is
  `plugin.source_conflict` (E1605), listing every source with its digest and never resolved by
  order (§19.2, invariant 8). An explicit `--source` is the selection §19.2 asks a person to make.
- **Ambiguity is unchanged.** Two ids under one name, one of them a system payload, stay
  `plugin.reference_ambiguous`; being on the disk chooses nothing (§10.3, §19.3).

The content digest is `ono_kuang_protocol::content_digest` — the same `sha256:` over every
artifact file's path and digest that `integrity` records and a catalog release vouches for — so
two sources are compared by one number, and `kuang-sign digest` prints it.

### 4. The origin is remembered, inspectable, and says whether it is still there

`Management` gains `origin` (`kuang_acquire::Origin`): the source kind, the source identity (a
URL, a `path:` reference, or `system:<payload>`), the distribution package and manager when the
sidecar said, the source path, the catalog, and the digest the copy is pinned to (K11A §18, §24).
`inspect plugin` carries it as `acquisition`, with `origin_available` — whether the source still
holds that version, or null for a network artifact this host does not probe (§14.4, §21.4). The
install prompt and the plan show `Source:` as one more fact beside signature, trust and runtime,
and never in place of the permission plan (§11.1, §12).

### 5. Removal removes Ono's copy, and says what remains

`remove plugin` removes the installed copy, its grants and its decisions as ADR-0604 decided, runs
no package manager, and — when a system payload of that id is still under a root — says so on
stderr: *A system-provided source remains available from package …* (K11A §15). Removing the
outer package is the package manager's; it runs no script that touches Ono's state (§11.3, §15.2).

## Consequences

- `install plugin kubernetes` on a machine with `ono-plugin-kubernetes` installed and no network
  installs from the system payload; the prompt says `Source: system package
  (ono-plugin-kubernetes)`. On a machine with neither, the answer names the distribution package
  as one of the ways to obtain it.
- `docs/contracts/errors.yaml` gains E1605–E1607; `ono.plugin-package/1` gains `source_kind` and
  `system_package`; `ono.plugin-inspection/1` gains `acquisition`; `ono.plugin.install` and
  `ono.plugin.find` gain `--source`.
- The Kubernetes provider ships `ono-plugin-kubernetes` as the reference `.deb`/`.rpm` wrapper
  (ADR-0071 in that repository), placing the same signed payload under the standard root.
- Encoded by `ono-cli/tests/acquisition.rs` (every item of K11A §25.3–§25.7 that concerns a
  system source, selection, upgrade isolation and removal), `ono-kuang-protocol/tests/catalog.rs`,
  the unit tests of `kuang_acquire`, and acceptance cases `227` and `228`.

## Alternatives considered

- **Binding the plugin home to the system payload (a symlink) instead of copying.** K11A §8.3
  allows an immutable verified reference only where it guarantees the same upgrade isolation; a
  symlink into files `apt upgrade` rewrites is exactly the mutable reference it forbids.
- **Letting the newest version win across sources.** Would let a public catalog bypass the
  distribution's pin, which §2.5 and §10.3 exist to prevent; the newest wins only inside the
  admitted kind.
- **Treating `root`-owned payloads as trusted.** §11.1 forbids the implication outright; a
  system payload's signature and publisher are judged like any other's.
- **A separate `update plugin` verb for K11A §14.2.** The verb registry deliberately has no
  `update` (ADR-0562: it means the other thing in every package manager), and ADR-0602 already
  made `install plugin <name>` the upgrade of an installed package. The lineage preference above
  is what §14.2 asks of that upgrade; a second word would be the same operation under a name the
  registry refused.
