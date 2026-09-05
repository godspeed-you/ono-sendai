# ADR-0107: The KUANG/11 tables are answered by the session provider

- Status: accepted
- Date: 2026-08-27
- Spec refs: §31.3, §31.5, §31.8, §31.9, §31.10, §31.31, §31.36; ADR-0051, ADR-0068, ADR-0090
- Decided by: agent (autonomous)

## Context

ADR-0051 made `get plugin` a session command that prints a line per package, so
`get plugin | where state == loaded` — spec §31.8's own composition and `kuang.yaml`'s example —
did not compose, and none of the plugin management commands the contract declares had a place
to put their records. The runtime instances live on the session (`LoadedPlugin` handles the
supervisor owns), the installed set lives on disk, and a provider cannot borrow the session
(ADR-0090, alternatives). The typed road exists already: `ono.shell` answers `get job` from
tables the session shares with it.

## Decision

### 1. The host lives in the shared tables

`SessionTables` carries a `kuang_host::Host`: the plugin path, the state directory, and the
loaded instances (`Arc<LoadedPlugin>`). The session publishes the two directories before every
pipeline (`publish_host`, beside `publish_jobs`) and reaches the instances through the same lock
(`Session::with_kuang`); `SessionProvider` answers the `plugin` target from the host by scanning
the plugin home and overlaying the instances' states. Nothing is published as rows: presence is
read from disk when asked, so a package placed or removed by another process is seen the next
time, and execution state is read from the handles.

`get plugin` is therefore an ordinary `ProviderProducer` over `ono.shell`, and spec §37 phase I
joins `DELIVERED` in `ono-command` so the KUANG/11 contracts bind. A contributed command still
runs through the evaluator (`<package>:command`, ADR-0051), which now takes the handle out of
the host rather than out of a session field.

### 2. What a plugin row says

- `state`: the instance's lifecycle state when one is loaded; `installed` otherwise. Enablement
  is the separate `enabled` bool (spec §31.3), read from the management state on disk, `true`
  when nothing was recorded — an installed package is eligible under default policy, and there
  is no explicit `enable` command in `kuang.yaml`. A disabled package stays `installed` with
  `enabled: false`; `load` refuses it.
- `trust: local` — every package the plugin home holds is an unsigned local development
  package until signing arrives (spec §31.36); `integrity` is the content hash of the manifest
  and the runtime entry as they are on disk now, `sha256:…`, re-read at every query.
- `isolation`: `runtime.kind` mapped to lifecycle.v1's tiers. A declarative package (no
  runtime, or `kind: declarative`) reports `core-built-in`: what runs is the core's interpreter
  of its packs, in process.
- `source`: what `install plugin` recorded, else `path:<directory>`; `installed_at`: the
  manifest's modification time, the closest fact on disk; `jobs: 0` — invocations run in the
  foreground and finish before the next statement reads a table; `memory` and `state_usage`
  null, never zero (spec §35.3).
- `degraded_reason`: the optional capabilities the negotiated contract denied, joined.

### 3. Management state on disk

`<state dir>/kuang/<package id>/management.json` holds `{enabled, installed_from}` — under
spec §31.31's `~/.local/state/ono/kuang/<package-id>/`, beside the package's own persistent
state, and honouring `XDG_STATE_HOME`. It is written by `set plugin` and `install plugin`,
removed by `remove plugin`.

### 4. The default view leads with the id

`ono.plugin/1`'s `default_view` is `[id, version, state, trust, jobs, memory]` rather than spec
§31.8's `NAME`. `name` is mutable and not unique; `id` is what `load plugin` and every other
command takes, so it is what the table must show.

### 5. Schemas embedded

The KUANG/11 schemas (`plugin`, `plugin-package`, `plugin-runtime`, `plugin-inspection`,
`capability-grant`, `verification-result`, `plugin-audit-event`, `assistant`, `assistant-turn`,
`assistant-action`, `model-provider`, `finding`, `evidence`, `recommendation`) are embedded in
`ono-value`'s builtin registry, where the pre-flight field check and the renderers find them.

## Consequences

- `get plugin | where state == loaded | count`, `get plugin <id>`, `get plugin --state degraded`
  compose; `crates/ono-cli/tests/plugins_missing.rs` (`should_emit_plugin_records_when_get_plugin_is_piped`,
  `should_count_loaded_packages_before_and_after_load`,
  `should_report_degraded_when_an_optional_capability_was_denied_at_load`,
  `should_resolve_one_package_by_its_id_selector`) prove it.
- The terminal rendering of `get plugin` is the table renderer's; acceptance case
  `050-kuang-plugin` asserted the interim line format and is corrected in its own `test:`
  commit.
- The other KUANG/11 tables (capabilities, audit, assistants, models, findings) and the
  mutations (`unload`, `set`, `remove`, `revoke`) follow through the same host (ADR-0108–0111).
- A supervisor error still crosses as `provider.unsupported` with the K code in the message
  until ADR-0108 folds the family into `ono_core::ErrorCode`.

## Alternatives considered

- **Publish plugin rows before every pipeline.** Would scan the plugin home for every
  `get process`; reading on demand costs nothing when nobody asks. Rejected.
- **Keep the instances on the session and publish only their states.** Mutations (`unload`,
  `revoke`) need the handles from inside `act`, which runs on the provider. Rejected.
- **A separate `ono.kuang` provider crate.** It would need the same session-owned state and
  could not own it (ADR-0090); the seam that exists is the right one.
