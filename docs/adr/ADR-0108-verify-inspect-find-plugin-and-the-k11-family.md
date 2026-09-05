# ADR-0108: `verify`, `inspect` and `find plugin`, and the K11 family in the global error model

- Status: accepted
- Date: 2026-08-27
- Spec refs: §31.7, §31.8, §31.9, §31.33, §31.36, §31.62, §31.68, §31.79, §43; ADR-0022, ADR-0040 §3, ADR-0051, ADR-0091, ADR-0107
- Decided by: agent (autonomous)

## Context

Three read-only management commands of `kuang.yaml` had no implementation: `verify plugin`
answers the four questions of spec §31.36, `inspect plugin` the detail view of §31.33, and
`find plugin` searches sources without executing anything (§31.9). And every supervisor error
still crossed into the shell as `provider.unsupported` with the K code in its message
(ADR-0051), so a script could not match `package.incompatible` and a user saw E0402 where the
contract promises `Ono-Sendai-K11002`.

## Decision

### 1. The K11 family is part of `ono_core::ErrorCode`

`ErrorCode` gains one variant per `KuangErrorCode` (`KuangPackageInvalid` … 
`KuangRemotePolicyDenied`), rendered exactly as `docs/contracts/kuang/errors.v1.yaml` renders them
and listed in `docs/contracts/errors.yaml` under their own heading, so `spec-check`'s registry check
covers them. A supervisor error becomes an `ErrorValue` by its code (`ErrorCode::from_code`),
keeping message and help; an unknown code — a plugin built against a newer taxonomy — still
falls back to `provider.unsupported` with the code in the message. `load plugin` of an
incompatible package therefore answers `Ono-Sendai-K11002` on stderr, and `kind` follows
ADR-0022's mapping.

### 2. `verify plugin` runs in the evaluator

Verification is a check whose *failing* result must both be shown and fail the run
(lifecycle.v1: "a blocking check that does not pass prevents install and prevents load"). A
provider stream cannot say that: a failure event beside a value fails the run only for
provider-kind failures (ADR-0085), and `package.incompatible` is a conflict. So `verify plugin`
is claimed by the evaluator like `load plugin`: it builds the `ono.verification-result/1`
record, seeds the rest of the pipeline with it, and when `blocking_failures` is not empty
reports the first blocking check's error and answers a failing status.

What the checks answer for a package that is a directory on this machine (spec §31.36):

- `integrity`: `valid`/`invalid` against the hash `install plugin` recorded in the management
  state; `unknown` for a package that was placed by hand, which is an answer and not a pass.
- `signature: absent`, `publisher`/`key: null`, `trust: unknown`, `transparency: unknown` —
  signing is not implemented, and an unsigned package says so everywhere it appears.
- `compatibility`: `kuang_api` and `platforms` against this host (`Manifest::check_host`);
  `incompatible` is blocking.
- `manifest`: `valid` for a manifest `Manifest::parse` accepted; `invalid` (blocking) for a
  reference whose manifest does not parse, with the other checks `unknown`.
- `runtime`: the isolation tier, reported and never judged.

The selector is an installed package id or a `path:` reference, so a package can be judged
before it is on disk.

### 3. `inspect plugin` is the producer's detail query

`ono.plugin.inspect` binds to `InspectCommand` (ADR-0091): the plugin table answers a query
with `detail: true` with `ono.plugin-inspection/1` — the parsed manifest as a record, the
origin `plugin`, the contributions, every capability request with its class and grant state,
the embedded verification result, and the runtime contract, resource figures and last error
of the instance when one is loaded (null, never zero, when none is).

A package that is not loaded and whose manifest declares no contribution files has exactly one
source of truth for what it contributes: its `hello`. `inspect plugin` then performs a
**discovery handshake** — `Supervisor::load` under the deny-all policy, the contributions are
read from the negotiated handle, and the instance is shut down with `ShutdownReason::Unload`
before the record is built. Nothing is retained; the row stays `installed` and `runtime` stays
null.

### 4. `find plugin` is the `query` selector on the plugin table

`ono.plugin.find` is an ordinary `ProviderProducer`; the contract's selector arrives as
`Selector::field("query", …)`, which the plugin table answers with `ono.plugin-package/1`
records instead of plugin rows: the packages of the configured sources whose id or name
contains the term. The sources are the plugin home and, with `--source path:<dir>`, an unpacked
directory — one package when it holds a `manifest.yaml`, else every package directory under it
(lifecycle.v1's `path:` scheme). `installed` says whether the same id and version is in the
plugin home; `source` is the resolved `path:` reference; nothing is executed.

## Spec deviation

- Section: spec §31.8
- Text: "**Installed**: artifact exists locally and metadata/signature have been validated." and
  "KUANG/11 MUST distinguish package presence from code execution."
- Instead: `inspect plugin` of an installed, unloaded package whose manifest declares no
  contribution files runs the package through the handshake once, under deny-all, and
  discards the instance.
- Why: spec §31.33 requires `inspect plugin` to show the package's contributions, spec §31.68
  requires them to be answerable before the package has ever run, and a runtime package that
  declares them only in its `hello` gives the host no other way to learn them. The handshake
  grants nothing, keeps nothing, and happens on the operator's explicit request to inspect that
  package; a package that declares contribution files is never started.

## Consequences

- `crates/ono-cli/tests/plugins_missing.rs`:
  `should_show_manifest_contributions_and_capability_requests_when_inspected`,
  `should_find_an_installed_package_without_loading_it`,
  `should_find_an_uninstalled_package_in_a_path_source`,
  `should_report_an_unsigned_local_package_as_compatible_when_verified`,
  `should_report_incompatibility_when_the_kuang_api_range_excludes_the_host`,
  `should_refuse_to_load_an_incompatible_package`.
- `ono_core::ErrorCode::ALL` grows by 27; `docs/reference/errors.md` is regenerated.
- `ono-kuang-protocol` keeps its own `KuangErrorCode`: the wire speaks strings, and a plugin
  must be able to name a code this host does not know (spec §31.62).

## Alternatives considered

- **`verify plugin` as a provider query with a failure event.** The record would render and the
  run would succeed, because a conflict is not a provider failure. Rejected.
- **A `--describe` mode in the SDK for inspect.** Would work for SDK-built plugins only; the
  protocol's `hello` is what every plugin already has. Rejected.
- **Refuse `inspect plugin` on an unloaded package.** Contradicts spec §31.33 and §31.68 more
  than a discarded handshake does. Rejected.
