# ADR-0601: A plugin is named by its short name and identified by its canonical id

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.5, §31.9, §31.35, §31.36; `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md`
  (K11P) §1.3, §4.1–§4.3, §10, §11, §25.1, §30, §34.3, §34.4; ADR-0051, ADR-0108, ADR-0109,
  ADR-0312
- Decided by: agent (autonomous)

## Context

`install plugin` resolved exactly one reference shape, `path:<directory>`, and every other plugin
command took the reverse-DNS id. K11P §1.3 keeps both — the id is identity, the path is an
explicit source — and adds the normal path: `install plugin kubernetes`. That needs a name to
resolve against something that is not a directory and executes nothing, which is a catalog
(K11P §11), and it needs every command that takes an id to take the name too (Gate R).

## Decision

### 1. `PluginRef` is one grammar, resolved in one order

`ono_kuang_protocol::PluginRef::parse` reads every form of K11P §4.1 and the shell resolves them
in §10.1's order:

1. an explicit path — `./x`, `../x`, `/x`, `~/x` — or the legacy `path:<dir>`;
2. an exact canonical id among installed packages and enabled catalog entries;
3. `<catalog>/<name>`;
4. an exact short name across enabled catalogs.

A bare word is never a filesystem path. For every command other than `install plugin` and
`find plugin`, a short name resolves **first** against the installed set: when exactly one
installed package has that `package.name`, the name means that package (K11P §10.4). Two installed
packages with one name, or two catalog entries with one name and different ids, is
`plugin.reference_ambiguous` (E1602) listing every candidate as a canonical id and a
`<catalog>/<name>` selector; interactively it is a numbered picker. Nothing is chosen by catalog
order or by publisher (K11P §10.3). A name nothing answers to is `plugin.not_found` (E1601) rather
than `resolve.target_not_found`, so a script can tell "no such plugin" from "no such target".

### 2. A catalog is data, and the shell ships one

A catalog is a `kuang-catalog/1` YAML document (`docs/contracts/kuang/catalog.v1.yaml`): a name,
a description, and entries of `{id, name, description, publisher, releases[]}` where a release is
`{version, platforms, kuang_api, ono_language, artifact, digest, signature}`. Reading a catalog
executes nothing, and searching one is `find plugin`.

Three kinds of catalog exist, and every record says which answered (`ono.plugin-package/1` gains
`catalog` and `catalog_verification`):

| catalog | where | verification |
|---|---|---|
| built-in bootstrap | embedded in the `ono` binary from `docs/contracts/kuang/bootstrap-catalog.yaml` | `built-in` — part of the release integrity chain (K11P §11.4) |
| operator | `<config>/kuang/catalogs/*.yaml`, `/etc/ono/kuang/catalogs/*.yaml` | `operator` — placed by the operator, like `trust.yaml` and `policy.yaml` |
| remote, cached | the same directory, written by a refresh | not in this build: no refresh exists, and `find plugin` says `unknown` for a document that claims a remote origin |

The bootstrap catalog names the Kubernetes reference provider, so `install plugin kubernetes`
resolves on a clean installation (Gate A). Catalog trust is reported apart from package
integrity, signature validity and publisher trust (K11P §11.3): a catalog saying a package exists
makes nobody trusted.

### 3. This build fetches nothing; a release is available when it is local

A release's `artifact` is a source reference. `path:` and `file:` are read; an `https:` artifact is
a statement about where the bytes live, and this build has no fetcher — so resolution looks for
the release **locally** first (K11P §30.2): the package cache `<cache>/ono/kuang/packages/<id>/<version>/`
and the local package sources `ONO_PLUGIN_SOURCES` (default `~/.local/share/ono/packages`,
`/usr/share/ono/packages`), each holding unpacked packages by id. A release that is not local and
whose artifact this build cannot reach is `plugin.catalog_unavailable` (E1603) naming the artifact
and the two directories a package can be placed in; a release whose `platforms` or `kuang_api`
excludes this host is `plugin.release_not_compatible` (E1604). Both are deterministic, and neither
fabricates availability.

A digest the catalog carries is checked against the local bytes before the plan is shown; a
catalog that carries none says `integrity: unknown` in the plan, exactly as `ono.plugin-package/1`
already allows, and the package's own signature and publisher trust decide from there.

### 4. Installation pins its lineage

`Management` records `installed_from`, `catalog` (the catalog name, or null), and
`publisher_key` (the signing key's fingerprint, or `unsigned`). An upgrade requires the same
canonical id and an acceptable key lineage: a signed package replacing one signed by a different
key is refused as `publisher.untrusted` naming both keys (K11P §34.4), unless the operator removes
and reinstalls deliberately — which requests permission anew (ADR-0604). A later catalog entry
with the same short name and a different id never redirects an installed package (K11P §34.3),
because upgrades resolve by the pinned id and never by the name.

### 5. Completion offers names first

`install plugin <TAB>` completes short names from the installed set and the catalogs, with the
canonical id as the description (K11P §25.1).

## Consequences

- `Host::resolve` becomes `Host::resolve_ref(&PluginRef, purpose)`; `Host::installed_package`
  accepts a short name that is unique among installed packages, so `load`, `verify`, `inspect`,
  `remove`, `set`, `unload`, `get permission` and `set permission` all take it (Gate R).
- `find plugin` searches installed, bootstrap, operator catalogs and configured local sources, and
  its table leads with `NAME` (K11P §11.6).
- Encoded by `ono-kuang-protocol/tests/catalog.rs`, `ono-cli/tests/plugin_resolution.rs` and
  acceptance case `220-kuang-install-by-name`.

## Alternatives considered

- **A network fetcher now.** Nothing in the workspace speaks HTTP for the shell itself, and a
  fetcher is a supply-chain surface with its own hardening tranche; the honest answer is a
  structured "not available locally" with the two places a package can be put.
- **Resolving a bare word as a directory when one exists.** K11P §10.1 forbids it; `./` is one
  character away and unambiguous.
- **Choosing the first catalog on a collision.** K11P §10.3 forbids choosing by order.
